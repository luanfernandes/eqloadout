#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// stdout logging in debug builds only; release has no console.
macro_rules! log {
    ($($t:tt)*) => { if cfg!(debug_assertions) { println!($($t)*) } };
}

mod apps;
mod config;
mod eapo;
mod focus;
mod gpu;
// shortcut: only the tests use it until the in-app "Get preset" download lands.
#[cfg_attr(not(test), allow(dead_code))]
mod preset;
mod rule_dialog;
mod rules;
mod settings;
mod tray;
mod ui;

use std::{cell::RefCell, ffi::OsStr, fs, mem::zeroed, os::windows::ffi::OsStrExt, path::PathBuf, ptr::null, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
    System::{
        Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        Threading::CreateMutexW,
    },
    UI::{
        Controls::{ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx},
        WindowsAndMessaging::{DispatchMessageW, GetMessageW, IsDialogMessageW, MSG, TranslateMessage},
    },
};

use config::Config;
use focus::Focus;

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

thread_local! {
    static SWITCHER: RefCell<Option<Switcher>> = const { RefCell::new(None) };
}

/// Runs `f` on the switcher; None if it is already borrowed (re-entrant message) or not created yet.
pub fn with_switcher<R>(f: impl FnOnce(&mut Switcher) -> R) -> Option<R> {
    SWITCHER.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

pub struct Switcher {
    pub cfg: Config,
    pub paused: bool,
    game_txt: PathBuf,
    /// What game.txt currently holds, so unchanged content is never rewritten.
    active: Option<String>,
    /// Exe and title of the last focused window.
    focus: Option<(String, String)>,
    /// e.g. "cs2.txt (cs2.exe)"
    summary: String,
    warning: Option<String>,
    write_error: Option<String>,
    config_problem: Option<String>,
}

impl Switcher {
    fn new(cfg: Config, config_problem: Option<String>) -> Self {
        let game_txt = cfg.eapo_config_dir.join("game.txt");
        let active = fs::read_to_string(&game_txt).ok();
        Switcher {
            cfg,
            paused: false,
            game_txt,
            active,
            focus: None,
            summary: String::new(),
            warning: None,
            write_error: None,
            config_problem,
        }
    }

    fn on_focus(&mut self, f: &Focus) {
        self.focus = Some((f.exe.clone(), f.title.clone()));
        if !self.paused {
            self.evaluate();
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.evaluate();
    }

    pub fn set_config(&mut self, cfg: Config) {
        self.game_txt = cfg.eapo_config_dir.join("game.txt");
        self.active = fs::read_to_string(&self.game_txt).ok();
        self.cfg = cfg;
        self.config_problem = None;
        self.write_error = None;
        self.evaluate();
    }

    fn evaluate(&mut self) {
        let dir = &self.cfg.eapo_config_dir;
        let exists = |p: &str| dir.join(p).is_file();
        let default = self.cfg.default_profile.as_deref();
        let (out, summary) = if self.paused {
            let out = rules::game_txt(None, default, exists);
            let s = format!("paused, {}", out.profile.as_deref().unwrap_or("no profile"));
            (out, s)
        } else {
            let (exe, title) = self.focus.as_ref().map_or(("", ""), |(e, t)| (e.as_str(), t.as_str()));
            let rule = rules::find(&self.cfg.rules, exe, title);
            let out = rules::game_txt(rule.map(|r| r.profile.as_str()), default, exists);
            let s = match (&out.profile, rule) {
                (Some(p), Some(r)) if *p == r.profile => format!("{p} ({})", r.label()),
                (Some(p), _) => format!("{p} (default)"),
                (None, _) => "no profile".into(),
            };
            log!("focus: {exe} \"{title}\" -> {s}");
            (out, s)
        };
        if let Some(w) = &out.warning {
            log!("warning: {w}");
        }
        self.warning = out.warning;
        self.summary = summary;
        self.write(out.content);

        let mut tip = format!("EQ Loadout: {}", self.summary);
        for w in self.write_error.iter().chain(&self.warning) {
            tip = tip + "\n" + w;
        }
        let icon = if self.paused {
            ui::ICON_PAUSED
        } else if self.write_error.is_some() || self.warning.is_some() {
            ui::ICON_WARNING
        } else {
            ui::ICON_APP
        };
        tray::set_state(&tip, icon);
    }

    fn write(&mut self, content: String) {
        if self.active.as_deref() == Some(content.as_str()) {
            return;
        }
        match eapo::write_atomic(&self.game_txt, &content) {
            Ok(()) => {
                log!("wrote game.txt: {}", content.trim_end());
                self.active = Some(content);
                self.write_error = None;
            }
            Err(e) => {
                let msg = format!(
                    "Cannot write game.txt ({e}). You need Modify permission on {}",
                    self.cfg.eapo_config_dir.display()
                );
                log!("error: {msg}");
                self.write_error = Some(msg);
            }
        }
    }

    /// Lines for the settings status area.
    pub fn status_lines(&self) -> Vec<String> {
        let mut v: Vec<String> = self.config_problem.iter().cloned().collect();
        v.extend(self.cfg.problems());
        v.extend(self.write_error.iter().cloned());
        v.extend(self.warning.iter().cloned());
        v.push(format!("Active: {}", self.summary));
        v
    }

    /// Leaves game.txt commented out so no profile stays applied after exit.
    fn shutdown(&mut self) {
        if let Some(c) = self.active.clone() {
            self.write(rules::comment_out(&c));
        }
    }
}

fn main() {
    if std::env::args().any(|a| a == "--list-apps") {
        #[cfg(not(debug_assertions))]
        unsafe {
            use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
        apps::print_ranked();
        return;
    }

    let _mutex = unsafe { CreateMutexW(null(), 0, wide("Local\\eqloadout-single-instance").as_ptr()) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        log!("EQ Loadout is already running, opening its settings");
        tray::signal_existing();
        return;
    }

    unsafe {
        CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32); // for the folder picker
        let icc = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_LISTVIEW_CLASSES };
        InitCommonControlsEx(&icc);
    }

    let (cfg, problem) = config::load();
    for p in problem.iter().cloned().chain(cfg.problems()) {
        log!("config: {p}");
    }
    log!("config file: {:?}\nEqualizer APO folder: {}", config::path(), cfg.eapo_config_dir.display());
    match fs::read_to_string(cfg.eapo_config_dir.join("config.txt")) {
        Ok(s) if !eapo::includes_game_txt(&s) => log!("warning: config.txt has no \"Include: game.txt\" line"),
        Err(e) => log!("warning: cannot read config.txt: {e}"),
        _ => {}
    }

    if let Err(e) = tray::create() {
        ui::warn(null_mut(), &e);
        return;
    }
    let show_settings = problem.is_some();
    SWITCHER.set(Some(Switcher::new(cfg, problem)));
    if let Err(e) = focus::start(|f| {
        with_switcher(|s| s.on_focus(f));
    }) {
        ui::warn(null_mut(), &e);
        tray::remove();
        return;
    }
    if show_settings {
        settings::open(); // surface a broken config right away
    } else {
        ui::trim_memory();
    }
    #[cfg(debug_assertions)]
    debug_ctrl_c::install();

    unsafe {
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let dlg = settings::hwnd();
            if !dlg.is_null() && IsDialogMessageW(dlg, &msg) != 0 {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    with_switcher(|s| s.shutdown());
    tray::remove();
}

/// Ctrl+C in the debug console ends the message loop so shutdown() runs.
#[cfg(debug_assertions)]
mod debug_ctrl_c {
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows_sys::{
        Win32::{
            System::{Console::SetConsoleCtrlHandler, Threading::GetCurrentThreadId},
            UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT},
        },
        core::BOOL,
    };

    static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

    unsafe extern "system" fn handler(_: u32) -> BOOL {
        unsafe { PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_QUIT, 0, 0) };
        1
    }

    pub fn install() {
        MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
        unsafe { SetConsoleCtrlHandler(Some(handler), 1) };
    }
}
