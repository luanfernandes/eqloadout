//! Foreground tracking. Only window APIs and Toolhelp snapshots: never a handle to another process.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    mem::{size_of, zeroed},
    ptr::null_mut,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM},
    System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS},
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND, EnumChildWindows, GetClassNameW, GetForegroundWindow, GetWindowTextW,
            GetWindowThreadProcessId, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
        },
    },
};
use windows_sys::core::BOOL;

pub struct Focus {
    /// Process that owns the window (the frame host for UWP apps).
    pub window_pid: u32,
    /// Resolved app: the UWP child for ApplicationFrameHost windows.
    pub exe: String,
    pub title: String,
}

/// Shell windows that take focus for a moment during Alt+Tab / Task View.
/// Reacting to them would switch the EQ away and back on every Alt+Tab.
const TRANSIENT_CLASSES: &[&str] = &["XamlExplorerHostIslandWindow", "MultitaskingViewFrame", "TaskSwitcherWnd", "ForegroundStaging"];

type Handler = Box<dyn FnMut(&Focus)>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
    static FG: Cell<HWND> = const { Cell::new(null_mut()) };
    static NAME_HOOK: Cell<HWINEVENTHOOK> = const { Cell::new(null_mut()) };
}

/// Installs the hooks on the calling thread, which must run a message loop, and reports the current window.
pub fn start(handler: impl FnMut(&Focus) + 'static) -> Result<(), String> {
    HANDLER.set(Some(Box::new(handler)));
    let h = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            null_mut(),
            Some(on_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if h.is_null() {
        return Err("SetWinEventHook(EVENT_SYSTEM_FOREGROUND) failed".into());
    }
    on_foreground(unsafe { GetForegroundWindow() });
    Ok(())
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, _: i32, _: u32, _: u32) {
    match event {
        EVENT_SYSTEM_FOREGROUND => on_foreground(hwnd),
        EVENT_OBJECT_NAMECHANGE if id_object == OBJID_WINDOW && !hwnd.is_null() && hwnd == FG.get() => {
            if let Some(f) = resolve(hwnd) {
                report(&f);
            }
        }
        _ => {}
    }
}

fn on_foreground(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let class = class_name(hwnd);
    if TRANSIENT_CLASSES.contains(&class.as_str()) {
        log!("focus: ignoring transient {class}");
        return;
    }
    let Some(f) = resolve(hwnd) else { return };
    FG.set(hwnd);
    // Title-change hook scoped to the focused process, replacing the previous one.
    let hook = unsafe { SetWinEventHook(EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE, null_mut(), Some(on_event), f.window_pid, 0, WINEVENT_OUTOFCONTEXT) };
    let old = NAME_HOOK.replace(hook);
    if !old.is_null() {
        unsafe { UnhookWinEvent(old) };
    }
    report(&f);
}

fn report(f: &Focus) {
    // try_borrow: a modal loop inside the handler could deliver another event re-entrantly.
    HANDLER.with(|h| {
        if let Ok(mut h) = h.try_borrow_mut()
            && let Some(h) = h.as_mut()
        {
            h(f)
        }
    });
}

/// Exe and title for a window, resolving UWP frame windows to the hosted app.
pub fn resolve(hwnd: HWND) -> Option<Focus> {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }
    let mut exe = exe_name(pid).unwrap_or_default();
    if exe.eq_ignore_ascii_case("ApplicationFrameHost.exe")
        && let Some(app) = uwp_child_pid(hwnd, pid).and_then(exe_name)
    {
        exe = app;
    }
    Some(Focus { window_pid: pid, exe, title: window_text(hwnd) })
}

fn exe_name(pid: u32) -> Option<String> {
    process_names().remove(&pid)
}

/// PID -> exe file name for every process, from one Toolhelp snapshot.
pub fn process_names() -> HashMap<u32, String> {
    let mut out = HashMap::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = zeroed();
        e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            out.insert(e.th32ProcessID, from_wide(&e.szExeFile));
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}

/// First child window owned by a process other than the frame host.
pub fn uwp_child_pid(hwnd: HWND, host_pid: u32) -> Option<u32> {
    unsafe extern "system" fn cb(child: HWND, lp: LPARAM) -> BOOL {
        let (host, found) = unsafe { &mut *(lp as *mut (u32, u32)) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(child, &mut pid) };
        if pid != 0 && pid != *host {
            *found = pid;
            return 0;
        }
        1
    }
    let mut data = (host_pid, 0u32);
    unsafe { EnumChildWindows(hwnd, Some(cb), &mut data as *mut _ as LPARAM) };
    (data.1 != 0).then_some(data.1)
}

pub fn window_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn from_wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}
