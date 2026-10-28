//! Settings window: modeless dialog, created on demand and destroyed on close.

use std::{
    cell::{Cell, RefCell},
    mem::zeroed,
    path::PathBuf,
    ptr::null_mut,
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, MAX_PATH, WPARAM},
    System::{
        Com::CoTaskMemFree,
        Threading::{GetCurrentProcess, SetProcessWorkingSetSize},
    },
    UI::{
        Controls::{NM_DBLCLK, NMHDR},
        Shell::{BFFM_INITIALIZED, BFFM_SETSELECTIONW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW, SHGetPathFromIDListW},
        WindowsAndMessaging::{
            CreateDialogParamW, DestroyWindow, EN_KILLFOCUS, IDCANCEL, IDOK, SW_RESTORE, SW_SHOW, SendMessageW, SetForegroundWindow, ShowWindow,
            WM_COMMAND, WM_DESTROY, WM_INITDIALOG, WM_NOTIFY,
        },
    },
};

use crate::{
    config::{self, Config},
    eapo, rule_dialog,
    rules::Rule,
    ui::{self, *},
    wide, with_switcher,
};

struct Settings {
    rules: Vec<Rule>,
    profiles: Vec<String>,
    /// Items of the default-profile combo after "(none)".
    defaults: Vec<String>,
}

thread_local! {
    static HWND_: Cell<HWND> = const { Cell::new(null_mut()) };
    static STATE: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// The open settings window, for IsDialogMessage in the main loop.
pub fn hwnd() -> HWND {
    HWND_.get()
}

pub fn open() {
    unsafe {
        let h = HWND_.get();
        if !h.is_null() {
            ShowWindow(h, SW_RESTORE);
            SetForegroundWindow(h);
            return;
        }
        let h = CreateDialogParamW(ui::hinstance(), ui::res(IDD_SETTINGS), null_mut(), Some(proc), 0);
        if h.is_null() {
            ui::warn(null_mut(), &format!("Could not open the settings window (error {}).", windows_sys::Win32::Foundation::GetLastError()));
            return;
        }
        ShowWindow(h, SW_SHOW);
        SetForegroundWindow(h);
    }
}

unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            HWND_.set(h);
            init(h);
            1
        }
        WM_COMMAND => {
            command(h, ui::loword(wp), ui::hiword(wp));
            1
        }
        WM_NOTIFY => {
            let hdr = unsafe { &*(lp as *const NMHDR) };
            if hdr.idFrom == IDC_RULES as usize && hdr.code == NM_DBLCLK {
                edit(h);
            }
            0
        }
        WM_DESTROY => {
            HWND_.set(null_mut());
            STATE.set(None);
            // Give back the memory the dialog and common controls touched.
            unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
            0
        }
        _ => 0,
    }
}

fn init(h: HWND) {
    ui::set_dialog_icon(h);
    let cfg = with_switcher(|s| s.cfg.clone()).unwrap_or_default();
    set_text(h, IDC_DIR, &cfg.eapo_config_dir.to_string_lossy());
    lv_columns(item(h, IDC_RULES), &[("Exe", 30), ("Title contains", 35), ("Profile", 35)]);
    STATE.set(Some(Settings { rules: cfg.rules, profiles: Vec::new(), defaults: Vec::new() }));
    refresh_profiles(h, cfg.default_profile);
    fill_rules(h, None);
    refresh_status(h, None);
}

fn command(h: HWND, id: i32, code: u32) {
    match id {
        IDC_BROWSE => {
            if let Some(dir) = browse_folder(h, &text(h, IDC_DIR)) {
                set_text(h, IDC_DIR, &dir);
                refresh_profiles(h, selected_default(h));
                refresh_status(h, None);
            }
        }
        IDC_DIR if code == EN_KILLFOCUS => {
            refresh_profiles(h, selected_default(h));
            refresh_status(h, None);
        }
        IDC_ADD => {
            let profiles = STATE.with_borrow(|s| s.as_ref().map(|s| s.profiles.clone()).unwrap_or_default());
            // No borrow held here: the rule dialog runs a nested message loop.
            if let Some(r) = rule_dialog::run(h, Rule::default(), profiles) {
                let n = update(|s| {
                    s.rules.push(r);
                    s.rules.len() - 1
                });
                fill_rules(h, n);
            }
        }
        IDC_EDIT => edit(h),
        IDC_REMOVE => {
            if let Some(i) = lv_selected(item(h, IDC_RULES)) {
                let next = update(|s| {
                    s.rules.remove(i);
                    i.min(s.rules.len().saturating_sub(1))
                });
                fill_rules(h, next);
            }
        }
        IDC_UP | IDC_DOWN => {
            if let Some(i) = lv_selected(item(h, IDC_RULES)) {
                let moved = update(|s| {
                    let j = if id == IDC_UP { i.checked_sub(1)? } else { i + 1 };
                    (j < s.rules.len()).then(|| s.rules.swap(i, j)).map(|_| j)
                })
                .flatten();
                if let Some(j) = moved {
                    fill_rules(h, Some(j));
                }
            }
        }
        IDOK => save(h),
        IDCANCEL => unsafe {
            DestroyWindow(h);
        },
        _ => {}
    }
}

fn update<R>(f: impl FnOnce(&mut Settings) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

fn edit(h: HWND) {
    let Some(i) = lv_selected(item(h, IDC_RULES)) else { return };
    let Some((rule, profiles)) = STATE.with_borrow(|s| s.as_ref().and_then(|s| Some((s.rules.get(i)?.clone(), s.profiles.clone())))) else {
        return;
    };
    if let Some(r) = rule_dialog::run(h, rule, profiles) {
        update(|s| s.rules[i] = r);
        fill_rules(h, Some(i));
    }
}

fn fill_rules(h: HWND, select: Option<usize>) {
    let rows: Vec<Vec<String>> = STATE.with_borrow(|s| {
        s.as_ref().map_or(Vec::new(), |s| s.rules.iter().map(|r| vec![r.exe.clone(), r.title.clone(), r.profile.clone()]).collect())
    });
    let lv = item(h, IDC_RULES);
    lv_fill(lv, &rows);
    if let Some(i) = select.filter(|&i| i < rows.len()) {
        lv_select(lv, i);
    }
}

fn current_dir(h: HWND) -> PathBuf {
    PathBuf::from(text(h, IDC_DIR).trim())
}

/// Re-reads the profile files from the folder in the edit box and refills the default combo.
fn refresh_profiles(h: HWND, keep_default: Option<String>) {
    let profiles = eapo::list_profiles(&current_dir(h));
    let mut defaults = profiles.clone();
    if let Some(d) = &keep_default
        && !defaults.contains(d)
    {
        defaults.push(d.clone()); // keep a configured default even if its file is gone
    }
    let mut items = vec!["(none)".to_string()];
    items.extend(defaults.iter().cloned());
    let sel = keep_default.and_then(|d| defaults.iter().position(|p| *p == d)).map_or(0, |i| i + 1);
    combo_fill(h, IDC_DEFAULT, &items, Some(sel));
    update(|s| {
        s.profiles = profiles;
        s.defaults = defaults;
    });
}

fn selected_default(h: HWND) -> Option<String> {
    let i = combo_selected(h, IDC_DEFAULT)?.checked_sub(1)?;
    STATE.with_borrow(|s| s.as_ref()?.defaults.get(i).cloned())
}

fn save(h: HWND) {
    let dir = current_dir(h);
    if dir.as_os_str().is_empty() {
        refresh_status(h, Some("Choose the Equalizer APO config folder first."));
        return;
    }
    let Some(rules) = STATE.with_borrow(|s| s.as_ref().map(|s| s.rules.clone())) else { return };
    let cfg = Config { eapo_config_dir: dir, default_profile: selected_default(h), rules };
    match config::save(&cfg) {
        Ok(()) => {
            with_switcher(|s| s.set_config(cfg));
            refresh_status(h, Some("Saved."));
        }
        Err(e) => refresh_status(h, Some(&format!("Could not save {}: {e}", config::path().unwrap_or_default().display()))),
    }
}

fn refresh_status(h: HWND, first: Option<&str>) {
    let dir = current_dir(h);
    let mut lines: Vec<String> = first.map(String::from).into_iter().collect();
    if !dir.is_dir() {
        lines.push(format!("Folder not found: {}. Choose the Equalizer APO config folder.", dir.display()));
    } else {
        match std::fs::read_to_string(dir.join("config.txt")) {
            Ok(s) if !eapo::includes_game_txt(&s) => {
                lines.push("config.txt has no \"Include: game.txt\" line, so switching has no effect.".into())
            }
            Err(e) => lines.push(format!("Cannot read config.txt: {e}")),
            _ => {}
        }
    }
    lines.extend(with_switcher(|s| s.status_lines()).unwrap_or_default());
    set_text(h, IDC_STATUS, &lines.join("\n"));
}

fn browse_folder(owner: HWND, initial: &str) -> Option<String> {
    unsafe extern "system" fn cb(h: HWND, msg: u32, _: LPARAM, data: LPARAM) -> i32 {
        if msg == BFFM_INITIALIZED {
            unsafe { SendMessageW(h, BFFM_SETSELECTIONW, 1, data) };
        }
        0
    }
    let init = wide(initial);
    let title = wide("Choose the Equalizer APO config folder");
    unsafe {
        let mut bi: BROWSEINFOW = zeroed();
        bi.hwndOwner = owner;
        bi.lpszTitle = title.as_ptr();
        bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
        bi.lpfn = Some(cb);
        bi.lParam = init.as_ptr() as LPARAM;
        let pidl = SHBrowseForFolderW(&bi);
        if pidl.is_null() {
            return None;
        }
        let mut buf = [0u16; MAX_PATH as usize];
        let ok = SHGetPathFromIDListW(pidl, buf.as_mut_ptr());
        CoTaskMemFree(pidl as _);
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len]))
    }
}
