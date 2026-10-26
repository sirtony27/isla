//! Microphone / camera "in use" indicator. Windows records every app's use of
//! those devices in the registry (CapabilityAccessManager). A thread sleeps
//! until one of those keys changes, then rescans and tells the UI. Dropping
//! the `Watcher` ends the thread and its registry notifications.

use std::sync::{Arc, Mutex};

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_SUCCESS, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    RRF_RT_REG_QWORD,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const STORE: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\";

/// Friendly name of an app currently using each device, if any.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct InUse {
    pub mic: Option<String>,
    pub cam: Option<String>,
}

/// An event handle shared by the watcher and its thread; closed when both are done.
struct Event(HANDLE);

// The handle is only signaled and waited on, which Windows allows from any thread.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Drop for Event {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub struct Watcher {
    pub in_use: Arc<Mutex<InUse>>,
    stop: Option<Arc<Event>>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop {
            let _ = unsafe { SetEvent(stop.0) };
        }
    }
}

pub fn start(hwnd: HWND, message: u32) -> Watcher {
    let shared = Arc::new(Mutex::new(InUse::default()));
    let out = shared.clone();
    let hwnd = hwnd.0 as isize;
    let stop = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
        .ok()
        .map(|h| Arc::new(Event(h)));
    let Some(thread_stop) = stop.clone() else {
        return Watcher {
            in_use: shared,
            stop: None,
        };
    };
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
            // The stop event first: index 0 of the wait.
            let events: Vec<HANDLE> = std::iter::once(thread_stop.0)
                .chain(watched.iter().map(|&(_, ev)| ev))
                .collect();
            while !watched.is_empty() {
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
                // Stopped, or the wait itself failed: either way, do not spin.
                let woke = WaitForMultipleObjects(&events, false, INFINITE);
                if woke == WAIT_OBJECT_0 || woke.0 >= WAIT_OBJECT_0.0 + events.len() as u32 {
                    break;
                }
            }
            // Keys first: closing one cancels its notification and signals its event,
            // which must still be open then.
            for key in keys.into_iter().flatten() {
                let _ = RegCloseKey(key);
            }
            for &(_, ev) in &watched {
                let _ = CloseHandle(ev);
            }
        });
    Watcher {
        in_use: shared,
        stop,
    }
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
