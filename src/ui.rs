//! Shared Win32 helpers for the dialogs. Control IDs mirror res/ids.h.

use std::{mem::zeroed, ptr::null};

use windows_sys::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                LIM_LARGE, LIM_SMALL, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW,
                LVM_DELETEALLITEMS, LVM_ENSUREVISIBLE, LVM_GETITEMCOUNT, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW,
                LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMSTATE, LVM_SETITEMTEXTW, LVNI_SELECTED, LVS_EX_FULLROWSELECT, LoadIconMetric,
            },
            WindowsAndMessaging::{
                CB_ADDSTRING, CB_ERR, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, GetClientRect, GetDlgItem, GetDlgItemTextW, GetSystemMetrics,
                HICON, ICON_BIG, ICON_SMALL, MB_ICONWARNING, MB_OK, MessageBoxW, SM_CXVSCROLL, SendMessageW, SetDlgItemTextW, WM_APP,
                WM_SETICON,
            },
        },
    },
    core::PCWSTR,
};

use crate::wide;

pub const IDD_SETTINGS: u16 = 100;
pub const IDD_RULE: u16 = 101;

pub const IDC_DIR: i32 = 1001;
pub const IDC_BROWSE: i32 = 1002;
pub const IDC_RULES: i32 = 1003;
pub const IDC_ADD: i32 = 1004;
pub const IDC_EDIT: i32 = 1005;
pub const IDC_REMOVE: i32 = 1006;
pub const IDC_UP: i32 = 1007;
pub const IDC_DOWN: i32 = 1008;
pub const IDC_DEFAULT: i32 = 1009;
pub const IDC_STATUS: i32 = 1010;

pub const IDC_EXE: i32 = 1101;
pub const IDC_PICKEXE: i32 = 1102;
pub const IDC_TITLE: i32 = 1103;
pub const IDC_PROFILE: i32 = 1104;
pub const IDC_APPS: i32 = 1105;
pub const IDC_REFRESH: i32 = 1106;
pub const IDC_CAPTURE: i32 = 1107;
pub const IDC_CAPTURE_STATUS: i32 = 1108;

pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_SHOW_SETTINGS: u32 = WM_APP + 2;
pub const WM_APP_GPU: u32 = WM_APP + 3;

pub fn hinstance() -> HINSTANCE {
    unsafe { GetModuleHandleW(null()) }
}

/// MAKEINTRESOURCE
pub fn res(id: u16) -> PCWSTR {
    id as usize as PCWSTR
}

pub fn icon(small: bool) -> HICON {
    let mut h = std::ptr::null_mut();
    unsafe { LoadIconMetric(hinstance(), res(1), if small { LIM_SMALL } else { LIM_LARGE }, &mut h) };
    h
}

pub fn set_dialog_icon(dlg: HWND) {
    unsafe {
        SendMessageW(dlg, WM_SETICON, ICON_SMALL as WPARAM, icon(true) as LPARAM);
        SendMessageW(dlg, WM_SETICON, ICON_BIG as WPARAM, icon(false) as LPARAM);
    }
}

pub fn item(dlg: HWND, id: i32) -> HWND {
    unsafe { GetDlgItem(dlg, id) }
}

pub fn text(dlg: HWND, id: i32) -> String {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetDlgItemTextW(dlg, id, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n as usize])
}

pub fn set_text(dlg: HWND, id: i32, s: &str) {
    unsafe { SetDlgItemTextW(dlg, id, wide(s).as_ptr()) };
}

pub fn warn(owner: HWND, msg: &str) {
    unsafe { MessageBoxW(owner, wide(msg).as_ptr(), wide("eqswitch").as_ptr(), MB_OK | MB_ICONWARNING) };
}

/// Fills a drop-down list and selects `select` (by index into `items`).
pub fn combo_fill(dlg: HWND, id: i32, items: &[String], select: Option<usize>) {
    let c = item(dlg, id);
    unsafe {
        SendMessageW(c, CB_RESETCONTENT, 0, 0);
        for s in items {
            SendMessageW(c, CB_ADDSTRING, 0, wide(s).as_ptr() as LPARAM);
        }
        SendMessageW(c, CB_SETCURSEL, select.unwrap_or(usize::MAX), 0);
    }
}

pub fn combo_selected(dlg: HWND, id: i32) -> Option<usize> {
    let i = unsafe { SendMessageW(item(dlg, id), CB_GETCURSEL, 0, 0) };
    (i != CB_ERR as isize).then_some(i as usize)
}

pub fn lv_columns(lv: HWND, cols: &[(&str, i32)]) {
    unsafe {
        SendMessageW(lv, LVM_SETEXTENDEDLISTVIEWSTYLE, LVS_EX_FULLROWSELECT as WPARAM, LVS_EX_FULLROWSELECT as LPARAM);
        let mut rc: RECT = zeroed();
        GetClientRect(lv, &mut rc);
        let width = rc.right - GetSystemMetrics(SM_CXVSCROLL);
        for (i, (name, percent)) in cols.iter().enumerate() {
            let mut t = wide(name);
            let mut c: LVCOLUMNW = zeroed();
            c.mask = LVCF_TEXT | LVCF_WIDTH;
            c.cx = width * percent / 100;
            c.pszText = t.as_mut_ptr();
            SendMessageW(lv, LVM_INSERTCOLUMNW, i, &c as *const _ as LPARAM);
        }
    }
}

pub fn lv_fill(lv: HWND, rows: &[Vec<String>]) {
    unsafe {
        SendMessageW(lv, LVM_DELETEALLITEMS, 0, 0);
        for row in rows {
            let idx = SendMessageW(lv, LVM_GETITEMCOUNT, 0, 0) as i32;
            for (col, cell) in row.iter().enumerate() {
                let mut t = wide(cell);
                let mut it: LVITEMW = zeroed();
                it.mask = LVIF_TEXT;
                it.iItem = idx;
                it.iSubItem = col as i32;
                it.pszText = t.as_mut_ptr();
                let msg = if col == 0 { LVM_INSERTITEMW } else { LVM_SETITEMTEXTW };
                SendMessageW(lv, msg, idx as WPARAM, &it as *const _ as LPARAM);
            }
        }
    }
}

pub fn lv_selected(lv: HWND) -> Option<usize> {
    let i = unsafe { SendMessageW(lv, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as LPARAM) };
    (i >= 0).then_some(i as usize)
}

pub fn lv_select(lv: HWND, i: usize) {
    unsafe {
        let mut it: LVITEMW = zeroed();
        it.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
        it.state = LVIS_SELECTED | LVIS_FOCUSED;
        SendMessageW(lv, LVM_SETITEMSTATE, i, &it as *const _ as LPARAM);
        SendMessageW(lv, LVM_ENSUREVISIBLE, i, 0);
    }
}

pub fn loword(w: WPARAM) -> i32 {
    (w & 0xffff) as i32
}

pub fn hiword(w: WPARAM) -> u32 {
    ((w >> 16) & 0xffff) as u32
}
