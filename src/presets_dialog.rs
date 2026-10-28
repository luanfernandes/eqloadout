//! "Get presets" dialog: lists community presets from the GitHub Pages index and downloads one
//! into the Equalizer APO config folder. Users then pick it in a rule themselves.

use std::{
    cell::RefCell,
    path::PathBuf,
    ptr::{null, null_mut},
    sync::mpsc::{Receiver, Sender, channel},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Com::Urlmon::URLDownloadToFileW,
    UI::{
        Controls::{LVIS_SELECTED, LVN_ITEMCHANGED, NMHDR, NMLISTVIEW},
        Shell::ShellExecuteW,
        WindowsAndMessaging::{
            DialogBoxParamW, EndDialog, IDCANCEL, IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW, PostMessageW, SW_SHOWNORMAL, WM_COMMAND,
            WM_INITDIALOG, WM_NOTIFY,
        },
    },
};

use crate::{
    eapo,
    preset::{self, IndexEntry},
    ui::{self, *},
    wide,
};

enum Fetched {
    Index(Result<Vec<IndexEntry>, String>),
    File(String, Result<String, String>),
}

struct Dlg {
    dir: PathBuf,
    entries: Vec<IndexEntry>,
    tx: Sender<Fetched>,
    rx: Receiver<Fetched>,
}

thread_local! {
    static DLG: RefCell<Option<Dlg>> = const { RefCell::new(None) };
}

/// Modal; `dir` is the Equalizer APO config folder files are saved into.
pub fn run(owner: HWND, dir: PathBuf) {
    let (tx, rx) = channel();
    DLG.set(Some(Dlg { dir, entries: Vec::new(), tx, rx }));
    unsafe { DialogBoxParamW(ui::hinstance(), ui::res(IDD_PRESETS), owner, Some(proc), 0) };
    DLG.take(); // drops the channel; late downloads find it closed
}

fn with<R>(f: impl FnOnce(&mut Dlg) -> R) -> Option<R> {
    DLG.with(|d| d.try_borrow_mut().ok()?.as_mut().map(f))
}

unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            ui::set_dialog_icon(h);
            lv_columns(item(h, IDC_PRESET_LIST), &[("Game", 42), ("Preset", 43), ("Votes", 15)]);
            set_text(h, IDC_PRESET_STATUS, "Loading presets...");
            fetch(h, None);
            1
        }
        WM_COMMAND => {
            match ui::loword(wp) {
                IDC_PRESET_DOWNLOAD => download(h),
                IDC_PRESET_VOTE => vote(h),
                IDCANCEL => unsafe {
                    EndDialog(h, 0);
                },
                _ => {}
            }
            1
        }
        WM_NOTIFY => {
            let hdr = unsafe { &*(lp as *const NMHDR) };
            if hdr.idFrom == IDC_PRESET_LIST as usize && hdr.code == LVN_ITEMCHANGED {
                let nm = unsafe { &*(lp as *const NMLISTVIEW) };
                if nm.uNewState & LVIS_SELECTED != 0 {
                    let notes = with(|d| d.entries.get(nm.iItem as usize).map(|e| e.notes.clone())).flatten().unwrap_or_default();
                    set_text(h, IDC_PRESET_NOTES, &notes);
                }
            }
            0
        }
        WM_APP_FETCHED => {
            if let Some(Ok(f)) = with(|d| d.rx.try_recv()) {
                fetched(h, f);
            }
            1
        }
        _ => 0,
    }
}

/// Downloads the index (`file` None) or one preset on a worker thread, then posts WM_APP_FETCHED.
fn fetch(h: HWND, file: Option<String>) {
    let Some(tx) = with(|d| d.tx.clone()) else { return };
    let target = h as usize;
    thread::spawn(move || {
        let msg = match file {
            None => Fetched::Index(get(&format!("{}index.toml", preset::BASE_URL)).and_then(|s| preset::parse_index(&s))),
            Some(f) => {
                let r = get(&format!("{}{f}", preset::BASE_URL));
                Fetched::File(f, r)
            }
        };
        if tx.send(msg).is_ok() {
            unsafe { PostMessageW(target as HWND, WM_APP_FETCHED, 0, 0) };
        }
    });
}

fn fetched(h: HWND, f: Fetched) {
    match f {
        Fetched::Index(Ok(entries)) => {
            let rows: Vec<Vec<String>> =
                entries.iter().map(|e| vec![e.game.clone().unwrap_or_else(|| "Any game".into()), e.name.clone(), e.votes.to_string()]).collect();
            let n = rows.len();
            with(|d| d.entries = entries);
            lv_fill(item(h, IDC_PRESET_LIST), &rows);
            set_text(h, IDC_PRESET_STATUS, &format!("{n} presets. Select one and click Download."));
        }
        Fetched::Index(Err(e)) => set_text(h, IDC_PRESET_STATUS, &format!("Could not load presets: {e}")),
        Fetched::File(name, Ok(text)) => save(h, &name, &text),
        Fetched::File(name, Err(e)) => set_text(h, IDC_PRESET_STATUS, &format!("Could not download {name}: {e}")),
    }
}

fn selected(h: HWND) -> Option<(String, Option<String>)> {
    let i = lv_selected(item(h, IDC_PRESET_LIST))?;
    with(|d| d.entries.get(i).map(|e| (e.file.clone(), e.vote_url.clone()))).flatten()
}

fn download(h: HWND) {
    match selected(h) {
        Some((file, _)) => {
            set_text(h, IDC_PRESET_STATUS, &format!("Downloading {file}..."));
            fetch(h, Some(file));
        }
        None => set_text(h, IDC_PRESET_STATUS, "Select a preset first."),
    }
}

fn vote(h: HWND) {
    match selected(h) {
        // vote_url was checked against the repo's issue URL prefix when the index was parsed
        Some((_, Some(url))) => unsafe {
            ShellExecuteW(h, wide("open").as_ptr(), wide(&url).as_ptr(), null(), null(), SW_SHOWNORMAL);
        },
        Some((_, None)) => set_text(h, IDC_PRESET_STATUS, "This preset has no voting page yet."),
        None => set_text(h, IDC_PRESET_STATUS, "Select a preset first."),
    }
}

/// Validates the downloaded text and writes it into the config folder.
fn save(h: HWND, name: &str, text: &str) {
    if let Err(e) = preset::parse(text) {
        return set_text(h, IDC_PRESET_STATUS, &format!("Rejected {name}: {e}"));
    }
    let Some(dir) = with(|d| d.dir.clone()) else { return };
    let target = dir.join(name);
    if let Ok(existing) = std::fs::read_to_string(&target) {
        if existing == text {
            return set_text(h, IDC_PRESET_STATUS, &format!("{name} is already up to date."));
        }
        let q = format!("{name} already exists in the config folder and is different. Replace it?");
        if unsafe { MessageBoxW(h, wide(&q).as_ptr(), wide("EQ Loadout").as_ptr(), MB_YESNO | MB_ICONQUESTION) } != IDYES {
            return set_text(h, IDC_PRESET_STATUS, "Kept your version.");
        }
    }
    let msg = match eapo::write_atomic(&target, text) {
        Ok(()) => format!("Saved {name}. Pick it as the profile of a rule."),
        Err(e) => format!("Could not save {name} ({e}). You need Modify permission on {}", dir.display()),
    };
    set_text(h, IDC_PRESET_STATUS, &msg);
}

/// Downloads `url` to a temp file and returns its text. A unique query string skips the WinINet cache.
fn get(url: &str) -> Result<String, String> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let tmp = std::env::temp_dir().join(format!("eqloadout-{}-{stamp}.tmp", std::process::id()));
    let url = format!("{url}?t={stamp}");
    let hr = unsafe { URLDownloadToFileW(null_mut(), wide(&url).as_ptr(), wide(&tmp).as_ptr(), 0, null_mut()) };
    if hr != 0 {
        return Err(format!("download failed (0x{hr:08X}), check your connection"));
    }
    let text = std::fs::read(&tmp).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&tmp);
    let bytes = text?;
    if bytes.len() > 1024 * 1024 {
        return Err("response too large".into());
    }
    String::from_utf8(bytes).map_err(|_| "response is not UTF-8 text".into())
}
