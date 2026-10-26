use std::mem::size_of;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    PostMessageW, SetForegroundWindow, TrackPopupMenu, HICON, HMENU, ICONINFO, MF_CHECKED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, SM_CXSMICON, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WM_NULL,
};

use crate::i18n::Text;

pub const INTERVALS: [u32; 5] = [30, 45, 60, 90, 120];
pub const GOALS: [u32; 5] = [1500, 2000, 2500, 3000, 3500];
pub const GLASSES: [u32; 4] = [200, 250, 330, 500];

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
    callback: u32,
}

impl Tray {
    pub fn new(hwnd: HWND, callback: u32) -> Self {
        let tray = Self {
            hwnd,
            icon: make_icon(),
            callback,
        };
        tray.add();
        tray
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: self.callback,
            hIcon: self.icon,
            ..Default::default()
        };
        for (dst, src) in nid.szTip.iter_mut().zip("Isla".encode_utf16()) {
            *dst = src;
        }
        nid
    }

    /// Also called when Explorer restarts ("TaskbarCreated"), which drops every tray icon.
    pub fn add(&self) {
        let mut nid = self.data();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
        }
    }

    pub fn remove(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
            let _ = DestroyIcon(self.icon);
        }
    }
}

pub const FOCUS_LENGTHS: [u32; 4] = [15, 25, 45, 50];
pub const REST_LENGTHS: [u32; 3] = [5, 10, 15];

#[derive(Clone, Copy)]
pub enum Alert {
    Track,
    Volume,
    Brightness,
    Audio,
    Battery,
    Privacy,
}

#[derive(Clone, Copy)]
pub struct MenuState {
    pub autostart: bool,
    pub hidden: bool,
    pub monitor_active: bool,
    pub hotkey: bool,
    /// Track, Volume, Brightness, Audio, Battery, Privacy (same order as `Alert`).
    pub alerts: [bool; 6],
    pub water_enabled: bool,
    pub interval: u32,
    pub goal: u32,
    pub glass: u32,
    pub focus_running: bool,
    pub focus_min: u32,
    pub rest_min: u32,
    pub text: Text,
}

#[derive(Clone, Copy)]
pub enum MenuCmd {
    None,
    Autostart,
    Hidden,
    Monitor(bool),
    Hotkey,
    Alert(Alert),
    WaterToggle,
    Interval(u32),
    Goal(u32),
    Glass(u32),
    ResetWater,
    FocusToggle,
    FocusLength(u32),
    RestLength(u32),
    Exit,
}

const ALERTS: [Alert; 6] = [
    Alert::Track,
    Alert::Volume,
    Alert::Brightness,
    Alert::Audio,
    Alert::Battery,
    Alert::Privacy,
];

const ID_AUTOSTART: usize = 1;
const ID_HIDDEN: usize = 2;
const ID_HOTKEY: usize = 3;
const ID_WATER: usize = 4;
const ID_RESET: usize = 5;
const ID_FOCUS: usize = 6;
const ID_EXIT: usize = 9;
const ID_MONITOR_PRIMARY: usize = 10;
const ID_MONITOR_ACTIVE: usize = 11;
const ID_ALERT: usize = 20;
const ID_INTERVAL: usize = 100;
const ID_GOAL: usize = 200;
const ID_GLASS: usize = 300;
const ID_FOCUS_LEN: usize = 400;
const ID_REST_LEN: usize = 500;

fn item(menu: HMENU, id: usize, label: &str, checked: bool) {
    let flags = if checked {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    unsafe {
        let _ = AppendMenuW(menu, flags, id, &HSTRING::from(label));
    }
}

fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
}

fn submenu(parent: HMENU, label: &str, items: &[(usize, String, bool)]) {
    unsafe {
        let Ok(sub) = CreatePopupMenu() else { return };
        for (id, text, checked) in items {
            item(sub, *id, text, *checked);
        }
        let _ = AppendMenuW(parent, MF_POPUP, sub.0 as usize, &HSTRING::from(label));
    }
}

/// One submenu entry per value, with the current one checked.
fn choices(
    base: usize,
    values: &[u32],
    current: u32,
    label: impl Fn(u32) -> String,
) -> Vec<(usize, String, bool)> {
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| (base + i, label(v), v == current))
        .collect()
}

/// Maps a menu id back to a value of `values`, if it belongs to that group.
fn pick(id: usize, base: usize, values: &[u32]) -> Option<u32> {
    id.checked_sub(base).and_then(|i| values.get(i).copied())
}

/// Shows the tray menu at the cursor and returns the chosen command.
/// Runs a modal loop, so the caller must not hold any borrow of the app state.
pub fn show_menu(hwnd: HWND, s: &MenuState) -> MenuCmd {
    let t = &s.text;
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return MenuCmd::None;
        };
        let version = format!("Isla {}", env!("CARGO_PKG_VERSION"));
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, &HSTRING::from(version));
        separator(menu);
        item(menu, ID_AUTOSTART, t.menu_autostart, s.autostart);
        item(menu, ID_HIDDEN, t.menu_hidden, s.hidden);
        let monitors = [
            (
                ID_MONITOR_PRIMARY,
                t.menu_monitor_primary.to_string(),
                !s.monitor_active,
            ),
            (
                ID_MONITOR_ACTIVE,
                t.menu_monitor_active.to_string(),
                s.monitor_active,
            ),
        ];
        submenu(menu, t.menu_monitor, &monitors);
        item(menu, ID_HOTKEY, t.menu_hotkey, s.hotkey);
        let labels = [
            t.alert_track,
            t.alert_volume,
            t.alert_brightness,
            t.alert_audio,
            t.alert_battery,
            t.alert_privacy,
        ];
        let alerts: Vec<_> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| (ID_ALERT + i, label.to_string(), s.alerts[i]))
            .collect();
        submenu(menu, t.menu_alerts, &alerts);
        separator(menu);

        item(menu, ID_WATER, t.menu_water, s.water_enabled);
        submenu(
            menu,
            t.menu_interval,
            &choices(ID_INTERVAL, &INTERVALS, s.interval, |m| format!("{m} min")),
        );
        submenu(
            menu,
            t.menu_goal,
            &choices(ID_GOAL, &GOALS, s.goal, |g| t.liters(g)),
        );
        submenu(
            menu,
            t.menu_glass,
            &choices(ID_GLASS, &GLASSES, s.glass, |g| format!("{g} ml")),
        );
        item(menu, ID_RESET, t.menu_reset, false);
        separator(menu);

        let focus = if s.focus_running {
            t.menu_focus_stop
        } else {
            t.menu_focus_start
        };
        item(menu, ID_FOCUS, focus, false);
        let minutes = |m: u32| format!("{m} min");
        submenu(
            menu,
            t.menu_focus_len,
            &choices(ID_FOCUS_LEN, &FOCUS_LENGTHS, s.focus_min, minutes),
        );
        submenu(
            menu,
            t.menu_rest_len,
            &choices(ID_REST_LEN, &REST_LENGTHS, s.rest_min, minutes),
        );
        separator(menu);
        item(menu, ID_EXIT, t.menu_exit, false);

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Without this the menu would not close when clicking elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON;
        let id = TrackPopupMenu(menu, flags, pt.x, pt.y, None, hwnd, None).0 as usize;
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        match id {
            ID_AUTOSTART => MenuCmd::Autostart,
            ID_HIDDEN => MenuCmd::Hidden,
            ID_HOTKEY => MenuCmd::Hotkey,
            ID_WATER => MenuCmd::WaterToggle,
            ID_RESET => MenuCmd::ResetWater,
            ID_FOCUS => MenuCmd::FocusToggle,
            ID_EXIT => MenuCmd::Exit,
            ID_MONITOR_PRIMARY => MenuCmd::Monitor(false),
            ID_MONITOR_ACTIVE => MenuCmd::Monitor(true),
            i if (ID_ALERT..ID_ALERT + ALERTS.len()).contains(&i) => {
                MenuCmd::Alert(ALERTS[i - ID_ALERT])
            }
            i => pick(i, ID_INTERVAL, &INTERVALS)
                .map(MenuCmd::Interval)
                .or_else(|| pick(i, ID_GOAL, &GOALS).map(MenuCmd::Goal))
                .or_else(|| pick(i, ID_GLASS, &GLASSES).map(MenuCmd::Glass))
                .or_else(|| pick(i, ID_FOCUS_LEN, &FOCUS_LENGTHS).map(MenuCmd::FocusLength))
                .or_else(|| pick(i, ID_REST_LEN, &REST_LENGTHS).map(MenuCmd::RestLength))
                .unwrap_or(MenuCmd::None),
        }
    }
}

/// Draws the tray icon at runtime (a small island pill), so the exe needs no resource file.
fn make_icon() -> HICON {
    unsafe {
        let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()).max(16);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            return HICON::default();
        };
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        let s = size as f32;
        let (cx, cy) = (s / 2.0, s / 2.0);
        let (hw, hh) = (s * 0.46, s * 0.24);
        let border = (s / 16.0).max(1.0);
        let dot = (cx + hw - hh, cy, hh * 0.42);
        for y in 0..size {
            for x in 0..size {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                // Signed distance to a stadium (pill) shape.
                let qx = ((fx - cx).abs() - (hw - hh)).max(0.0);
                let d = (qx * qx + (fy - cy) * (fy - cy)).sqrt() - hh;
                let alpha = (0.5 - d).clamp(0.0, 1.0);
                let inner = (0.5 - (d + border)).clamp(0.0, 1.0);
                let dd = ((fx - dot.0).powi(2) + (fy - dot.1).powi(2)).sqrt() - dot.2;
                let dot_a = (0.5 - dd).clamp(0.0, 1.0) * inner;
                // Light border, near-black body: visible on dark and light taskbars.
                let body = 238.0 * (1.0 - inner) + 12.0 * inner;
                let v = body * (1.0 - dot_a) + 150.0 * dot_a;
                let c = v as u32;
                px[(y * size + x) as usize] = ((alpha * 255.0) as u32) << 24 | c << 16 | c << 8 | c;
            }
        }
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            hbmMask: mask,
            hbmColor: color,
            ..Default::default()
        };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}
