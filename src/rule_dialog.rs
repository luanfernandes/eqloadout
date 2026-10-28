//! Add/Edit rule dialog (modal) with the running-app picker.

use std::{
    cell::RefCell,
    collections::HashMap,
    mem::zeroed,
    path::{Path, PathBuf},
    ptr::null,
    sync::mpsc::{Receiver, channel},
    thread,
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::{Registry::HKEY_CURRENT_USER, Threading::GetCurrentProcessId},
    UI::{
        Controls::{
            Dialogs::{GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW},
            LVIS_SELECTED, LVN_ITEMCHANGED, NMHDR, NMLISTVIEW,
        },
        WindowsAndMessaging::{
            DialogBoxParamW, EndDialog, GetForegroundWindow, GetWindowThreadProcessId, IDCANCEL, IDOK, KillTimer, PostMessageW, SetTimer,
            WM_COMMAND, WM_DESTROY, WM_INITDIALOG, WM_NOTIFY, WM_TIMER,
        },
    },
};

use crate::{
    apps::{self, App},
    config, focus, gpu,
    rules::Rule,
    ui::{self, *},
    wide,
};

const CAPTURE_TIMER: usize = 1;
const CAPTURE_SECONDS: u32 = 5;

struct Dlg {
    rule: Rule,
    profiles: Vec<String>,
    apps: Vec<App>,
    gpu: Option<Receiver<HashMap<u32, f64>>>,
    countdown: u32,
    result: Option<Rule>,
}

thread_local! {
    static DLG: RefCell<Option<Dlg>> = const { RefCell::new(None) };
}

/// Shows the dialog; returns the edited rule on OK. Everything it allocates is dropped on return.
pub fn run(owner: HWND, rule: Rule, mut profiles: Vec<String>) -> Option<Rule> {
    if !rule.profile.is_empty() && !profiles.contains(&rule.profile) {
        profiles.push(rule.profile.clone());
    }
    DLG.set(Some(Dlg { rule, profiles, apps: Vec::new(), gpu: None, countdown: 0, result: None }));
    unsafe { DialogBoxParamW(ui::hinstance(), ui::res(IDD_RULE), owner, Some(proc), 0) };
    DLG.take().and_then(|d| d.result)
}

/// Borrow fails while a control notification re-enters the proc; those events are ignored on purpose.
fn with<R>(f: impl FnOnce(&mut Dlg) -> R) -> Option<R> {
    DLG.with(|d| d.try_borrow_mut().ok()?.as_mut().map(f))
}

unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            init(h);
            1
        }
        WM_COMMAND => {
            command(h, ui::loword(wp));
            1
        }
        WM_NOTIFY => {
            let hdr = unsafe { &*(lp as *const NMHDR) };
            if hdr.idFrom == IDC_APPS as usize && hdr.code == LVN_ITEMCHANGED {
                let nm = unsafe { &*(lp as *const NMLISTVIEW) };
                if nm.uNewState & LVIS_SELECTED != 0 && nm.uOldState & LVIS_SELECTED == 0 {
                    pick_app(h, nm.iItem as usize);
                }
            }
            0
        }
        WM_TIMER if wp == CAPTURE_TIMER => {
            let left = with(|d| {
                d.countdown = d.countdown.saturating_sub(1);
                d.countdown
            })
            .unwrap_or(0);
            if left == 0 {
                unsafe { KillTimer(h, CAPTURE_TIMER) };
                capture(h);
            } else {
                set_text(h, IDC_CAPTURE_STATUS, &format!("Switch to the app now... {left}"));
            }
            1
        }
        WM_APP_GPU => {
            let changed = with(|d| {
                let g = d.gpu.as_ref()?.try_recv().ok()?;
                apps::apply_gpu(&mut d.apps, &g);
                apps::sort(&mut d.apps);
                Some(())
            })
            .flatten();
            if changed.is_some() {
                fill_apps(h);
            }
            1
        }
        WM_DESTROY => {
            unsafe { KillTimer(h, CAPTURE_TIMER) };
            with(|d| d.gpu = None); // a still-running sampler finds the channel closed and exits
            0
        }
        _ => 0,
    }
}

fn init(h: HWND) {
    let Some((rule, profiles)) = with(|d| (d.rule.clone(), d.profiles.clone())) else { return };
    set_text(h, IDC_EXE, &rule.exe);
    set_text(h, IDC_TITLE, &rule.title);
    let sel = profiles.iter().position(|p| *p == rule.profile);
    combo_fill(h, IDC_PROFILE, &profiles, sel);
    lv_columns(item(h, IDC_APPS), &[("Exe", 35), ("Window title", 65)]);
    refresh_apps(h);
}

fn command(h: HWND, id: i32) {
    match id {
        IDC_REFRESH => refresh_apps(h),
        IDC_CAPTURE => {
            with(|d| d.countdown = CAPTURE_SECONDS);
            set_text(h, IDC_CAPTURE_STATUS, &format!("Switch to the app now... {CAPTURE_SECONDS}"));
            unsafe { SetTimer(h, CAPTURE_TIMER, 1000, None) };
        }
        IDC_PICKEXE => {
            if let Some(name) = pick_exe(h) {
                set_text(h, IDC_EXE, &name);
            }
        }
        IDOK => ok(h),
        IDCANCEL => unsafe {
            EndDialog(h, 0);
        },
        _ => {}
    }
}

fn ok(h: HWND) {
    let exe = text(h, IDC_EXE).trim().to_string();
    let title = text(h, IDC_TITLE).trim().to_string();
    let profile = combo_selected(h, IDC_PROFILE).and_then(|i| with(|d| d.profiles.get(i).cloned()).flatten());
    if exe.is_empty() && title.is_empty() {
        return ui::warn(h, "Set an exe name, a window title, or both.");
    }
    let Some(profile) = profile else {
        return ui::warn(h, "Choose a profile. Profiles are the .txt files in the Equalizer APO config folder.");
    };
    with(|d| d.result = Some(Rule { exe, title, profile }));
    unsafe { EndDialog(h, 1) };
}

/// Lists windows now and starts GPU sampling in the background; scores fill in when it reports back.
fn refresh_apps(h: HWND) {
    let mut list = apps::list();
    apps::sort(&mut list);
    let (tx, rx) = channel();
    let target = h as usize;
    thread::spawn(move || match gpu::sample_3d() {
        Ok(g) => {
            if tx.send(g).is_ok() {
                unsafe { PostMessageW(target as HWND, WM_APP_GPU, 0, 0) };
            }
        }
        Err(e) => log!("gpu: {e}"),
    });
    with(|d| {
        d.apps = list;
        d.gpu = Some(rx);
    });
    fill_apps(h);
}

fn fill_apps(h: HWND) {
    let rows = with(|d| d.apps.iter().map(|a| vec![a.exe.clone(), a.title.clone()]).collect::<Vec<_>>()).unwrap_or_default();
    lv_fill(item(h, IDC_APPS), &rows);
}

fn pick_app(h: HWND, i: usize) {
    if let Some((exe, title)) = with(|d| d.apps.get(i).map(|a| (a.exe.clone(), a.title.clone()))).flatten() {
        set_text(h, IDC_EXE, &exe);
        set_text(h, IDC_TITLE, &title);
    }
}

fn capture(h: HWND) {
    let fg = unsafe { GetForegroundWindow() };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(fg, &mut pid) };
    let msg = if fg.is_null() {
        "No window had focus. Try again.".to_string()
    } else if pid == unsafe { GetCurrentProcessId() } {
        "eqswitch itself had focus. Click Capture, then switch to the app.".to_string()
    } else {
        match focus::resolve(fg) {
            Some(f) if !f.exe.is_empty() => {
                set_text(h, IDC_EXE, &f.exe);
                set_text(h, IDC_TITLE, &f.title);
                format!("Captured {}", f.exe)
            }
            _ => "Could not read the focused window.".to_string(),
        }
    };
    set_text(h, IDC_CAPTURE_STATUS, &msg);
}

fn pick_exe(owner: HWND) -> Option<String> {
    let filter = wide("Programs (*.exe)\0*.exe\0"); // wide() adds the final terminator
    let title = wide("Select the game's exe");
    let initial = steam_common_dir().map(wide);
    let mut buf = [0u16; 1024];
    unsafe {
        let mut ofn: OPENFILENAMEW = zeroed();
        ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = owner;
        ofn.lpstrFilter = filter.as_ptr();
        ofn.lpstrFile = buf.as_mut_ptr();
        ofn.nMaxFile = buf.len() as u32;
        ofn.lpstrTitle = title.as_ptr();
        ofn.lpstrInitialDir = initial.as_ref().map_or(null(), |w| w.as_ptr());
        ofn.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut ofn) == 0 {
            return None;
        }
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let path = String::from_utf16_lossy(&buf[..len]);
    Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned())
}

/// steamapps\common of the first Steam library, if Steam is installed.
fn steam_common_dir() -> Option<PathBuf> {
    let steam = PathBuf::from(config::reg_string(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath")?);
    let vdf = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")).ok();
    let lib = vdf.as_deref().and_then(first_library_path).map(PathBuf::from).unwrap_or(steam);
    let common = lib.join("steamapps").join("common");
    common.is_dir().then_some(common)
}

/// First `"path"  "D:\\SteamLibrary"` value in libraryfolders.vdf, unescaped.
fn first_library_path(vdf: &str) -> Option<String> {
    vdf.lines().find_map(|l| {
        let rest = l.trim().strip_prefix("\"path\"")?;
        let v = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
        Some(v.replace("\\\\", "\\"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_steam_library() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"label\"\t\t\"\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}";
        assert_eq!(first_library_path(vdf).as_deref(), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(first_library_path("nothing"), None);
    }
}
