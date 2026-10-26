//! Volume and audio output through Core Audio. Windows calls us on its own
//! threads when something changes; the callbacks only forward a message to
//! the UI thread, where everything else happens.

use windows::core::{implement, Result, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{HWND, LPARAM, PROPERTYKEY, WPARAM};
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, CLSCTX_INPROC_SERVER, STGM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// WPARAM values of the audio message.
pub const VOLUME_CHANGED: usize = 1;
pub const DEVICE_CHANGED: usize = 2;

fn post(hwnd: isize, message: u32, kind: usize, data: isize) {
    let _ = unsafe { PostMessageW(Some(HWND(hwnd as _)), message, WPARAM(kind), LPARAM(data)) };
}

/// Packs volume (0..=1000) and mute into the LPARAM of a `VOLUME_CHANGED` message.
pub fn unpack(lp: isize) -> (f32, bool) {
    ((lp & 0xFFFF) as f32 / 1000.0, (lp >> 16) & 1 == 1)
}

#[implement(IMMNotificationClient)]
struct DeviceEvents {
    hwnd: isize,
    message: u32,
}

impl IMMNotificationClient_Impl for DeviceEvents_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> Result<()> {
        // Fired once per role and direction; the default playback device is enough.
        if flow == eRender && role == eConsole {
            post(self.hwnd, self.message, DEVICE_CHANGED, 0);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeEvents {
    hwnd: isize,
    message: u32,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeEvents_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if let Some(d) = unsafe { data.as_ref() } {
            let level = (d.fMasterVolume.clamp(0.0, 1.0) * 1000.0).round() as isize;
            let muted = d.bMuted.as_bool() as isize;
            post(self.hwnd, self.message, VOLUME_CHANGED, level | muted << 16);
        }
        Ok(())
    }
}

pub struct Audio {
    enumerator: IMMDeviceEnumerator,
    device_events: IMMNotificationClient,
    volume_events: IAudioEndpointVolumeCallback,
    endpoint: Option<IAudioEndpointVolume>,
    pub level: f32,
    pub muted: bool,
    pub device: String,
}

impl Audio {
    pub fn new(hwnd: HWND, message: u32) -> Result<Self> {
        let hwnd = hwnd.0 as isize;
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)?;
            let device_events: IMMNotificationClient = DeviceEvents { hwnd, message }.into();
            enumerator.RegisterEndpointNotificationCallback(&device_events)?;
            let mut audio = Self {
                enumerator,
                device_events,
                volume_events: VolumeEvents { hwnd, message }.into(),
                endpoint: None,
                level: 0.0,
                muted: false,
                device: String::new(),
            };
            audio.bind();
            Ok(audio)
        }
    }

    pub fn available(&self) -> bool {
        self.endpoint.is_some()
    }

    /// Attaches to the current default output device (again, after it changes).
    pub fn bind(&mut self) {
        unsafe {
            if let Some(old) = self.endpoint.take() {
                let _ = old.UnregisterControlChangeNotify(&self.volume_events);
            }
            let Ok(device) = self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
                self.device.clear();
                return;
            };
            self.device = friendly_name(&device).unwrap_or_default();
            if let Ok(endpoint) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                let _ = endpoint.RegisterControlChangeNotify(&self.volume_events);
                self.level = endpoint.GetMasterVolumeLevelScalar().unwrap_or(0.0);
                self.muted = endpoint.GetMute().is_ok_and(|m| m.as_bool());
                self.endpoint = Some(endpoint);
            }
        }
    }

    pub fn set_level(&mut self, level: f32) {
        let Some(endpoint) = &self.endpoint else {
            return;
        };
        let level = level.clamp(0.0, 1.0);
        unsafe {
            let _ = endpoint.SetMasterVolumeLevelScalar(level, std::ptr::null());
            if self.muted && level > 0.0 {
                let _ = endpoint.SetMute(false, std::ptr::null());
                self.muted = false;
            }
        }
        self.level = level;
    }

    pub fn toggle_mute(&mut self) {
        let Some(endpoint) = &self.endpoint else {
            return;
        };
        let _ = unsafe { endpoint.SetMute(!self.muted, std::ptr::null()) };
        self.muted = !self.muted;
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        unsafe {
            if let Some(endpoint) = &self.endpoint {
                let _ = endpoint.UnregisterControlChangeNotify(&self.volume_events);
            }
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.device_events);
        }
    }
}

fn friendly_name(device: &IMMDevice) -> Option<String> {
    unsafe {
        let value = device
            .OpenPropertyStore(STGM_READ)
            .ok()?
            .GetValue(&PKEY_Device_FriendlyName)
            .ok()?;
        let text = PropVariantToStringAlloc(&value).ok()?;
        let name = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as _));
        name
    }
}
