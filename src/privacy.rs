//! Microphone / camera "in use" indicator. Windows records every app's use of
//! those devices in the registry (CapabilityAccessManager). A thread sleeps
//! until one of those keys changes, then rescans and tells the UI.

use std::sync::{Arc, Mutex};

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    RRF_RT_REG_QWORD,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const STORE: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\";

/// Friendly name of an app currently using each device, if any.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct InUse {
    pub mic: Option<String>,
    pub cam: Option<String>,
}

pub fn start(hwnd: HWND, message: u32) -> Arc<Mutex<InUse>> {
    let shared = Arc::new(Mutex::new(InUse::default()));
    let out = shared.clone();
    let hwnd = hwnd.0 as isize;
    let _ = std::thread::Builder::new()
        .name("privacy".into())
        .spawn(move || unsafe {
            // A key that does not exist yet (device never used) is simply not watched.
            let keys = [open("microphone"), open("webcam")];
            let watched: Vec<(HKEY, HANDLE)> = keys
                .iter()
                .flatten()
                .filter_map(|&key| {
                    CreateEventW(None, false, false, PCWSTR::null())
                        .ok()
                        .map(|ev| (key, ev))
                })
                .collect();
            if watched.is_empty() {
                return;
            }
            let events: Vec<HANDLE> = watched.iter().map(|&(_, ev)| ev).collect();
            loop {
                // Re-arm before scanning so no change between the two is missed.
                for &(key, ev) in &watched {
                    let filter = REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET;
                    let _ = RegNotifyChangeKeyValue(key, true, filter, Some(ev), true);
                }
                let state = InUse {
                    mic: keys[0].and_then(in_use),
                    cam: keys[1].and_then(in_use),
                };
                let changed = match out.lock() {
                    Ok(mut current) if *current != state => {
                        *current = state;
                        true
                    }
                    _ => false,
                };
                if changed {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), message, WPARAM(0), LPARAM(0));
                }
                WaitForMultipleObjects(&events, false, INFINITE);
            }
        });
    shared
}

unsafe fn open(capability: &str) -> Option<HKEY> {
    let mut key = HKEY::default();
    let path = HSTRING::from(format!("{STORE}{capability}"));
    (unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, KEY_READ, &mut key) } == ERROR_SUCCESS)
        .then_some(key)
}

fn subkeys(key: HKEY) -> Vec<String> {
    let mut names = Vec::new();
    let mut buf = [0u16; 512];
    for index in 0.. {
        let mut len = buf.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        names.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    names
}

fn qword(key: HKEY, subkey: &str, value: &str) -> u64 {
    let mut data = 0u64;
    let mut size = 8u32;
    let status = unsafe {
        RegGetValueW(
            key,
            &HSTRING::from(subkey),
            &HSTRING::from(value),
            RRF_RT_REG_QWORD,
            None,
            Some(&mut data as *mut u64 as _),
            Some(&mut size),
        )
    };
    if status == ERROR_SUCCESS {
        data
    } else {
        0
    }
}

/// An app is using the device while its last use has a start but no stop.
fn using(key: HKEY, app: &str) -> bool {
    qword(key, app, "LastUsedTimeStart") != 0 && qword(key, app, "LastUsedTimeStop") == 0
}

fn in_use(key: HKEY) -> Option<String> {
    for app in subkeys(key) {
        if app == "NonPackaged" {
            let mut desktop = HKEY::default();
            let path = HSTRING::from(app.as_str());
            if unsafe { RegOpenKeyExW(key, &path, None, KEY_READ, &mut desktop) } != ERROR_SUCCESS {
                continue;
            }
            let found = subkeys(desktop).into_iter().find(|exe| using(desktop, exe));
            let _ = unsafe { RegCloseKey(desktop) };
            if let Some(exe) = found {
                return Some(friendly(&exe));
            }
        } else if using(key, &app) {
            return Some(friendly(&app));
        }
    }
    None
}

/// "C:#Program Files#Mozilla Firefox#firefox.exe" -> "Firefox",
/// "Microsoft.WindowsCamera_8wekyb3d8bbwe" -> "WindowsCamera".
fn friendly(key: &str) -> String {
    let name = match key.rsplit_once('#') {
        Some((_, exe)) => exe.trim_end_matches(".exe").trim_end_matches(".EXE"),
        None => {
            let package = key.split('_').next().unwrap_or(key);
            package.rsplit('.').next().unwrap_or(package)
        }
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
