//! Tray icon, its menu, and the hidden window that receives tray and single-instance messages.

use std::{
    cell::{Cell, RefCell},
    mem::zeroed,
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW},
    UI::{
        Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW},
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, FindWindowW, GetCursorPos, MF_CHECKED, MF_SEPARATOR,
            MF_STRING, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, SetMenuDefaultItem,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
        },
    },
};

use crate::{config, settings, ui, wide, with_switcher};

const CLASS: &str = "eqloadout_tray";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

thread_local! {
    static HWND_: Cell<HWND> = const { Cell::new(null_mut()) };
    static TIP: RefCell<String> = RefCell::new("EQ Loadout".into());
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static ICON: Cell<u16> = const { Cell::new(ui::ICON_APP) };
}

pub fn create() -> Result<(), String> {
    let class = wide(CLASS);
    unsafe {
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: ui::hinstance(), lpszClassName: class.as_ptr(), ..zeroed() };
        RegisterClassW(&wc);
        // Hidden top-level window (not message-only) so it receives the TaskbarCreated broadcast.
        let h = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), ui::hinstance(), null());
        if h.is_null() {
            return Err("could not create the tray window".into());
        }
        HWND_.set(h);
        TASKBAR_CREATED.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
    }
    notify(NIM_ADD);
    Ok(())
}

/// Asks an already running instance to open its settings. Returns false if none was found.
pub fn signal_existing() -> bool {
    unsafe {
        let h = FindWindowW(wide(CLASS).as_ptr(), null());
        !h.is_null() && PostMessageW(h, ui::WM_APP_SHOW_SETTINGS, 0, 0) != 0
    }
}

/// Updates the tooltip and the icon (one of the `ui::ICON_*` resources).
pub fn set_state(tip: &str, icon: u16) {
    if ICON.get() == icon && TIP.with_borrow(|t| t == tip) {
        return;
    }
    TIP.set(tip.to_string());
    ICON.set(icon);
    notify(NIM_MODIFY);
}

pub fn remove() {
    notify(NIM_DELETE);
}

fn notify(op: u32) {
    let hwnd = HWND_.get();
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = ui::WM_APP_TRAY;
        nid.hIcon = ui::icon(ICON.get(), true);
        TIP.with_borrow(|t| {
            let w: Vec<u16> = t.encode_utf16().take(nid.szTip.len() - 1).collect();
            nid.szTip[..w.len()].copy_from_slice(&w);
        });
        Shell_NotifyIconW(op, &nid);
    }
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        ui::WM_APP_TRAY => match lp as u32 {
            WM_RBUTTONUP | WM_CONTEXTMENU => menu(h),
            WM_LBUTTONDBLCLK => settings::open(),
            _ => {}
        },
        ui::WM_APP_SHOW_SETTINGS => settings::open(),
        m if m != 0 && m == TASKBAR_CREATED.get() => notify(NIM_ADD), // Explorer restarted
        _ => return unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
    0
}

fn menu(h: HWND) {
    const SETTINGS: usize = 1;
    const PAUSE: usize = 2;
    const STARTUP: usize = 3;
    const QUIT: usize = 4;
    let paused = with_switcher(|s| s.paused).unwrap_or(false);
    let startup = startup_enabled();
    let cmd = unsafe {
        let m = CreatePopupMenu();
        AppendMenuW(m, MF_STRING, SETTINGS, wide("Settings").as_ptr());
        AppendMenuW(m, MF_STRING, PAUSE, wide(if paused { "Resume" } else { "Pause" }).as_ptr());
        AppendMenuW(m, MF_STRING | if startup { MF_CHECKED } else { 0 }, STARTUP, wide("Start with Windows").as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        AppendMenuW(m, MF_STRING, QUIT, wide("Quit").as_ptr());
        SetMenuDefaultItem(m, SETTINGS as u32, 0);
        let mut pt: POINT = zeroed();
        GetCursorPos(&mut pt);
        SetForegroundWindow(h); // required so the menu closes when clicking elsewhere
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, h, null()) as usize;
        DestroyMenu(m);
        PostMessageW(h, WM_NULL, 0, 0);
        cmd
    };
    match cmd {
        SETTINGS => settings::open(),
        PAUSE => {
            with_switcher(|s| s.set_paused(!paused));
        }
        STARTUP => {
            if let Err(e) = set_startup(!startup) {
                ui::warn(null_mut(), &e);
            }
        }
        QUIT => unsafe { PostQuitMessage(0) },
        _ => {}
    }
}

fn startup_enabled() -> bool {
    config::reg_string(HKEY_CURRENT_USER, RUN_KEY, "eqloadout").is_some()
}

fn set_startup(on: bool) -> Result<(), String> {
    let (key, name) = (wide(RUN_KEY), wide("eqloadout"));
    let rc = unsafe {
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let v = wide(format!("\"{}\"", exe.display()));
            RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32)
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr())
        }
    };
    if rc == 0 { Ok(()) } else { Err(format!("Could not update the startup entry (error {rc}).")) }
}
