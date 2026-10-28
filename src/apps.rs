//! Running-app list for the rule picker, ranked by how likely each entry is a game.

use std::{collections::HashMap, mem::zeroed};

use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::{
            Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
            Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
        },
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{EnumWindows, GWL_EXSTYLE, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW},
    },
    core::BOOL,
};

use crate::{focus, gpu};

/// Penalised (never hidden): common apps that are clearly not games.
const NON_GAMES: &[&str] = &[
    "explorer.exe", "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "zen.exe", "discord.exe",
    "steam.exe", "steamwebhelper.exe", "obs64.exe", "obs32.exe", "editor.exe", "deviceselector.exe", "benchmark.exe", "peace.exe",
];

pub struct App {
    pub exe: String,
    pub title: String,
    pub pids: Vec<u32>,
    pub fullscreen: bool,
    pub non_game: bool,
    pub gpu: f64,
}

impl App {
    pub fn score(&self) -> f64 {
        self.gpu.min(100.0) + if self.fullscreen { 50.0 } else { 0.0 } - if self.non_game { 30.0 } else { 0.0 }
    }

    pub fn tags(&self) -> String {
        let mut t = Vec::new();
        if self.gpu >= 1.0 {
            t.push(format!("GPU {:.0}%", self.gpu));
        }
        if self.fullscreen {
            t.push("Fullscreen".into());
        }
        t.join(", ")
    }
}

/// Visible, titled, uncloaked top-level windows of other processes, grouped by exe (topmost window's title).
pub fn list() -> Vec<App> {
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        unsafe { (*(lp as *mut Vec<HWND>)).push(hwnd) };
        1
    }
    let mut hwnds: Vec<HWND> = Vec::new();
    unsafe { EnumWindows(Some(cb), &mut hwnds as *mut _ as LPARAM) };

    let names = focus::process_names();
    let own = unsafe { GetCurrentProcessId() };
    let mut apps: Vec<App> = Vec::new();
    for hwnd in hwnds {
        if !candidate(hwnd) {
            continue;
        }
        let title = focus::window_text(hwnd);
        if title.is_empty() {
            continue;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == own || pid == 0 {
            continue;
        }
        let Some(mut exe) = names.get(&pid).cloned() else { continue };
        if exe.eq_ignore_ascii_case("ApplicationFrameHost.exe")
            && let Some(child) = focus::uwp_child_pid(hwnd, pid)
            && let Some(name) = names.get(&child)
        {
            (exe, pid) = (name.clone(), child);
        }
        let fullscreen = covers_monitor(hwnd);
        match apps.iter_mut().find(|a| a.exe.eq_ignore_ascii_case(&exe)) {
            Some(a) => {
                a.fullscreen |= fullscreen;
                if !a.pids.contains(&pid) {
                    a.pids.push(pid);
                }
            }
            None => apps.push(App {
                non_game: NON_GAMES.contains(&exe.to_ascii_lowercase().as_str()),
                exe,
                title,
                pids: vec![pid],
                fullscreen,
                gpu: 0.0,
            }),
        }
    }
    apps
}

pub fn apply_gpu(apps: &mut [App], gpu: &HashMap<u32, f64>) {
    for a in apps {
        a.gpu = a.pids.iter().filter_map(|p| gpu.get(p)).sum();
    }
}

pub fn sort(apps: &mut [App]) {
    apps.sort_by(|a, b| b.score().total_cmp(&a.score()).then_with(|| a.exe.to_lowercase().cmp(&b.exe.to_lowercase())));
}

/// `eqswitch --list-apps`: prints the ranked list so the heuristics can be checked.
pub fn print_ranked() {
    let mut apps = list();
    match gpu::sample_3d() {
        Ok(g) => apply_gpu(&mut apps, &g),
        Err(e) => println!("GPU sampling failed: {e}"),
    }
    sort(&mut apps);
    println!("{:>6}  {:<20} {:<28} title", "score", "tags", "exe");
    for a in &apps {
        println!("{:>6.1}  {:<20} {:<28} {}", a.score(), a.tags(), a.exe, a.title);
    }
}

fn candidate(hwnd: HWND) -> bool {
    unsafe {
        if IsWindowVisible(hwnd) == 0 || GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let hr = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut cloaked as *mut _ as _, 4);
        hr != 0 || cloaked == 0
    }
}

/// Fullscreen and borderless-fullscreen windows cover their whole monitor, taskbar included.
fn covers_monitor(hwnd: HWND) -> bool {
    unsafe {
        let mut r: RECT = zeroed();
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        if GetWindowRect(hwnd, &mut r) == 0 || GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, gpu: f64, fullscreen: bool) -> App {
        App { non_game: NON_GAMES.contains(&exe), exe: exe.into(), title: String::new(), pids: vec![], fullscreen, gpu }
    }

    #[test]
    fn ranks_games_first() {
        let mut v = vec![app("chrome.exe", 20.0, false), app("notepad.exe", 0.0, false), app("cs2.exe", 60.0, true), app("explorer.exe", 0.0, true)];
        sort(&mut v);
        let order: Vec<_> = v.iter().map(|a| a.exe.as_str()).collect();
        assert_eq!(order, ["cs2.exe", "explorer.exe", "notepad.exe", "chrome.exe"]);
        assert_eq!(v[0].tags(), "GPU 60%, Fullscreen");
    }
}
