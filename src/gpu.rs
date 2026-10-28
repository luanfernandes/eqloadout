//! Per-process GPU 3D usage from system performance counters (PDH). No process handles.

use std::{collections::HashMap, ptr::null, ptr::null_mut, slice, thread, time::Duration};

use windows_sys::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};

use crate::wide;

/// "pid_1234_luid_0x0_0x0_phys_0_eng_0_engtype_3D" -> Some(1234). Other engine types -> None.
pub fn parse_3d_instance(name: &str) -> Option<u32> {
    if !name.contains("engtype_3D") {
        return None;
    }
    let rest = name.strip_prefix("pid_")?;
    rest[..rest.find('_')?].parse().ok()
}

fn check(rc: u32, what: &str) -> Result<(), String> {
    if rc == 0 { Ok(()) } else { Err(format!("{what} failed: {rc:#x}")) }
}

/// Summed 3D engine utilization per PID. Blocks ~500 ms: the first PDH sample is always zero.
pub fn sample_3d() -> Result<HashMap<u32, f64>, String> {
    unsafe {
        let mut q = null_mut();
        check(PdhOpenQueryW(null(), 0, &mut q), "PdhOpenQuery")?;
        let result = (|| {
            let mut c = null_mut();
            let path = wide(r"\GPU Engine(*)\Utilization Percentage");
            check(PdhAddEnglishCounterW(q, path.as_ptr(), 0, &mut c), "PdhAddEnglishCounter")?;
            check(PdhCollectQueryData(q), "PdhCollectQueryData")?;
            thread::sleep(Duration::from_millis(500));
            check(PdhCollectQueryData(q), "PdhCollectQueryData")?;

            let (mut size, mut count) = (0u32, 0u32);
            let rc = PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, null_mut());
            if rc != PDH_MORE_DATA {
                return check(rc, "PdhGetFormattedCounterArray").map(|_| HashMap::new());
            }
            let mut buf = vec![0u64; size as usize / 8 + 1]; // u64 for alignment
            check(PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, buf.as_mut_ptr().cast()), "PdhGetFormattedCounterArray")?;
            let items = slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize);

            let mut out = HashMap::new();
            for it in items {
                if it.FmtValue.CStatus > 1 {
                    continue; // not PDH_CSTATUS_VALID_DATA / NEW_DATA
                }
                if let Some(pid) = parse_3d_instance(&pwstr(it.szName)) {
                    *out.entry(pid).or_insert(0.0) += it.FmtValue.Anonymous.doubleValue;
                }
            }
            Ok(out)
        })();
        PdhCloseQuery(q);
        result
    }
}

unsafe fn pwstr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let len = (0..).take_while(|&i| unsafe { *p.add(i) } != 0).count();
    String::from_utf16_lossy(unsafe { slice::from_raw_parts(p, len) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_instance_names() {
        assert_eq!(parse_3d_instance("pid_1234_luid_0x00000000_0x0000D1E5_phys_0_eng_0_engtype_3D"), Some(1234));
        assert_eq!(parse_3d_instance("pid_99_luid_0x00000000_0x0000D1E5_phys_0_eng_3_engtype_VideoDecode"), None);
        assert_eq!(parse_3d_instance("luid_0x0_engtype_3D"), None);
        assert_eq!(parse_3d_instance("pid_x_engtype_3D"), None);
    }
}
