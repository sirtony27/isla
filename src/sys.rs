//! Thin wrappers over the Win32 calls that are not about drawing.

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE};
use windows::Win32::Globalization::{
    GetDateFormatEx, GetTimeFormatEx, ENUM_DATE_FORMATS_FLAGS, TIME_NOSECONDS,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, MessageBoxW, MB_ICONERROR,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Isla");

/// Returns a handle that must stay alive for the whole run, or `None` if Isla is already running.
pub fn single_instance() -> Option<HANDLE> {
    let handle = unsafe { CreateMutexW(None, false, w!("Local\\Isla.SingleInstance")) }.ok()?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return None;
    }
    Some(handle)
}

pub fn fatal(message: &str) {
    unsafe {
        MessageBoxW(None, &HSTRING::from(message), w!("Isla"), MB_ICONERROR);
    }
}

pub fn autostart_enabled() -> bool {
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
    };
    status == ERROR_SUCCESS
}

pub fn set_autostart(enabled: bool) {
    unsafe {
        if !enabled {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE);
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let command: Vec<u16> = format!("\"{}\"", exe.display())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            REG_SZ.0,
            Some(command.as_ptr().cast()),
            (command.len() * 2) as u32,
        );
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Power {
    pub has_battery: bool,
    pub on_ac: bool,
    pub charging: bool,
    pub percent: u8,
}

pub fn power() -> Power {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return Power::default();
    }
    // BatteryFlag: 128 = no system battery, 255 = unknown, 8 = charging.
    let has_battery =
        s.BatteryFlag & 128 == 0 && s.BatteryFlag != 255 && s.BatteryLifePercent <= 100;
    Power {
        has_battery,
        on_ac: s.ACLineStatus == 1,
        charging: has_battery && s.BatteryFlag & 8 != 0,
        percent: s.BatteryLifePercent.min(100),
    }
}

/// Local time formatted with the user's own clock settings (12/24 h).
pub fn clock_text() -> String {
    let mut buf = [0u16; 32];
    let n = unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            TIME_NOSECONDS,
            None,
            PCWSTR::null(),
            Some(&mut buf),
        )
    };
    String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
}

/// Short localized date, e.g. "Sun 25 Oct" / "dom 25 oct".
pub fn date_text() -> String {
    let mut buf = [0u16; 64];
    let n = unsafe {
        GetDateFormatEx(
            PCWSTR::null(),
            ENUM_DATE_FORMATS_FLAGS(0),
            None,
            w!("ddd d MMM"),
            Some(&mut buf),
            PCWSTR::null(),
        )
    };
    String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
}

/// `YYYY-MM-DD` of the local date, used to reset the water log each day.
pub fn today() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

pub fn local_hour() -> u32 {
    unsafe { GetLocalTime() }.wHour as u32
}

/// Milliseconds until the next minute starts (plus a small margin).
pub fn ms_to_next_minute() -> u32 {
    let t = unsafe { GetLocalTime() };
    60_000 - (t.wSecond as u32 * 1000 + t.wMilliseconds as u32).min(59_999) + 50
}

/// The desktop itself counts as "full screen" for the shell; we don't want to hide for it.
pub fn desktop_is_foreground() -> bool {
    let mut buf = [0u16; 32];
    let n = unsafe { GetClassNameW(GetForegroundWindow(), &mut buf) };
    let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    class == "Progman" || class == "WorkerW"
}
