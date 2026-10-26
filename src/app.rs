//! App state and the notch state machine (closed / open / live activity).
//! Everything here runs on the UI thread, driven by window messages.

use std::mem::size_of;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use windows::core::{w, Result, GUID};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;
use windows::Win32::Graphics::Gdi::{
    CreateRectRgn, GetMonitorInfoW, SetWindowRgn, HMONITOR, MONITORINFO,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABM_NEW, ABM_REMOVE, ABN_FULLSCREENAPP, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::anim::{self, Spring};
use crate::audio::{self, Audio};
use crate::focus::{Focus, Phase};
use crate::gfx::{Fonts, Gfx};
use crate::i18n::{self, Text};
use crate::media::{self, Info, Media, ART_SIZE};
use crate::privacy::{self, InUse};
use crate::settings::Settings;
use crate::sys::{self, Power};
use crate::tray::{Alert, MenuCmd, MenuState, Tray};
use crate::ui::{
    self, Act, Hit, Layers, Look, Mode, PowerEvent, View, CAM_COLOR, CX, FLARE, HOT_STRIP,
    MIC_COLOR, WIN_H, WIN_W,
};

pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_MEDIA: u32 = WM_APP + 2;
pub const WM_APP_APPBAR: u32 = WM_APP + 3;
pub const WM_APP_AUDIO: u32 = WM_APP + 4;
pub const WM_APP_PRIVACY: u32 = WM_APP + 5;
pub const WM_APP_FOREGROUND: u32 = WM_APP + 6;
const WM_MOUSELEAVE: u32 = 0x02A3;
const NIN_SELECT: u32 = WM_USER;
const NIN_KEYSELECT: u32 = WM_USER + 1;
const GUID_ACDC_POWER_SOURCE: GUID = GUID::from_u128(0x5d3e9a59_e9d5_4b00_a6bd_ff34ff516548);
const GUID_BATTERY_PERCENTAGE_REMAINING: GUID =
    GUID::from_u128(0xa7ad8041_b45a_4cae_87a3_eecbb468a9e1);
const GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS: GUID =
    GUID::from_u128(0x8ffee2c6_2d01_46be_adb9_398addc5b4ff);

// The only timers in the app. None of them polls state.
const TIMER_HOVER: usize = 1; // one-shot: open after the cursor rests on the notch
const TIMER_COLLAPSE: usize = 2; // one-shot: close after the cursor leaves
const TIMER_ACTIVITY: usize = 3; // one-shot: end of a live activity
const TIMER_CLOCK: usize = 4; // once a minute, on the minute
const TIMER_LIVE: usize = 5; // 1 Hz, only while open and something is counting
const TIMER_WATER: usize = 6; // every N minutes, only if the reminder is on
const TIMER_FOCUS: usize = 7; // when the pomodoro's shown minute changes or it ends

const HOVER_DELAY: u32 = 110;
const LEAVE_DELAY: u32 = 280;
const LEVEL_DURATION: u32 = 1500;
const VOLUME_STEP: f32 = 0.02;

pub fn create_window(proc: WNDPROC) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("IslaNotch");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: proc,
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("Isla"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

pub struct App {
    hwnd: HWND,
    text: Text,
    fonts: Fonts,
    gfx: Option<Gfx>,
    settings: Settings,
    dpi: u32,
    monitor: HMONITOR,
    foreground_hook: Option<HWINEVENTHOOK>,

    mode: Mode,
    last_act: Act,
    w: Spring,
    h: Spring,
    r: Spring,
    a_closed: Spring,
    a_open: Spring,
    a_act: Spring,
    animating: bool,
    last_frame: Instant,
    dirty: bool,
    visible: bool,
    region: RECT,

    tracking: bool,
    inside: bool,
    hot: Hit,
    pressed: Hit,
    dragging_volume: bool,

    media: Media,
    info: Info,
    media_ready: bool,
    art: Option<ID2D1BitmapBrush1>,
    art_pixels: Option<Arc<Vec<u8>>>,
    layers: Layers,
    live_timer: bool,

    audio: Option<Audio>,
    brightness: Option<u8>,
    privacy: Option<Arc<Mutex<InUse>>>,
    in_use: InUse,
    privacy_shown: (bool, String),
    focus: Focus,
    focus_note: String,

    clock: String,
    date: String,
    power: Power,
    power_event: PowerEvent,
    /// Lowest battery warning already shown on this discharge (100 = none).
    low_warned: u8,
    full_notified: bool,
    tray: Tray,
    taskbar_created: u32,
}

impl App {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let settings = Settings::load();
        let power = sys::power();
        let look = Look {
            hidden: settings.hidden,
            ..Look::default()
        };
        let (w, h, r) = ui::size_for(Mode::Closed, look);
        let mut app = Self {
            hwnd,
            text: i18n::load(),
            fonts: Fonts::new()?,
            gfx: None,
            dpi: unsafe { GetDpiForWindow(hwnd) }.max(96),
            monitor: sys::primary_monitor(),
            foreground_hook: None,
            mode: Mode::Closed,
            last_act: Act::Track,
            w: anim::size(w),
            h: anim::size(h),
            r: anim::size(r),
            a_closed: anim::fade(if h > 0.0 { 1.0 } else { 0.0 }),
            a_open: anim::fade(0.0),
            a_act: anim::fade(0.0),
            animating: false,
            last_frame: Instant::now(),
            dirty: true,
            visible: true,
            region: RECT::default(),
            tracking: false,
            inside: false,
            hot: Hit::None,
            pressed: Hit::None,
            dragging_volume: false,
            media: Media::start(hwnd, WM_APP_MEDIA),
            info: Info::default(),
            media_ready: false,
            art: None,
            art_pixels: None,
            layers: Layers::default(),
            live_timer: false,
            audio: Audio::new(hwnd, WM_APP_AUDIO).ok(),
            brightness: None,
            privacy: settings
                .notify_privacy
                .then(|| privacy::start(hwnd, WM_APP_PRIVACY)),
            in_use: InUse::default(),
            privacy_shown: (false, String::new()),
            focus: Focus::default(),
            focus_note: String::new(),
            clock: sys::clock_text(),
            date: sys::date_text(),
            power,
            power_event: PowerEvent::Plugged,
            low_warned: 100,
            full_notified: power.on_ac && power.percent >= 100,
            tray: Tray::new(hwnd, WM_APP_TRAY),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            settings,
        };
        app.roll_water_day();
        if app.settings.monitor_active {
            app.foreground_hook = Some(sys::watch_foreground(hwnd, WM_APP_FOREGROUND));
        }
        app.place();
        unsafe {
            let mut abd = app.appbar_data();
            SHAppBarMessage(ABM_NEW, &mut abd);
            for guid in [
                GUID_ACDC_POWER_SOURCE,
                GUID_BATTERY_PERCENTAGE_REMAINING,
                GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS,
            ] {
                let _ = RegisterPowerSettingNotification(
                    HANDLE(hwnd.0),
                    &guid,
                    DEVICE_NOTIFY_WINDOW_HANDLE,
                );
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        sys::set_hotkey(hwnd, app.settings.hotkey);
        app.set_timer(TIMER_CLOCK, sys::ms_to_next_minute());
        app.schedule_water();
        // Keep the autostart entry pointing at this exe if the user moved it.
        if sys::autostart_enabled() {
            sys::set_autostart(true);
        }
        Ok(app)
    }

    pub fn shutdown(&mut self) {
        sys::set_hotkey(self.hwnd, false);
        if let Some(hook) = self.foreground_hook.take() {
            sys::unwatch_foreground(hook);
        }
        self.audio = None;
        self.tray.remove();
        unsafe {
            let mut abd = self.appbar_data();
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
        self.settings.save();
    }

    fn appbar_data(&self) -> APPBARDATA {
        APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uCallbackMessage: WM_APP_APPBAR,
            ..Default::default()
        }
    }

    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    fn set_timer(&self, id: usize, ms: u32) {
        unsafe { SetTimer(Some(self.hwnd), id, ms, None) };
    }

    fn kill_timer(&self, id: usize) {
        let _ = unsafe { KillTimer(Some(self.hwnd), id) };
    }

    /// Centers the window at the top of the chosen monitor and sizes the swap chain.
    fn place(&mut self) {
        if !self.settings.monitor_active {
            self.monitor = sys::primary_monitor();
        }
        for _ in 0..2 {
            let s = self.scale();
            let (pw, ph) = ((WIN_W * s).ceil() as i32, (WIN_H * s).ceil() as i32);
            unsafe {
                let mut mi = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if !GetMonitorInfoW(self.monitor, &mut mi).as_bool() {
                    // The monitor went away: fall back to the primary one.
                    self.monitor = sys::primary_monitor();
                    let _ = GetMonitorInfoW(self.monitor, &mut mi);
                }
                let m = mi.rcMonitor;
                let x = m.left + (m.right - m.left - pw) / 2;
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    x,
                    m.top,
                    pw,
                    ph,
                    SWP_NOACTIVATE,
                );
            }
            // Moving to another monitor may change the DPI; re-place once with the new scale.
            let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
            if dpi == self.dpi {
                break;
            }
            self.dpi = dpi;
        }
        let s = self.scale();
        let (pw, ph) = ((WIN_W * s).ceil() as u32, (WIN_H * s).ceil() as u32);
        let dpi = self.dpi as f32;
        let resized = self
            .gfx
            .as_mut()
            .is_some_and(|g| g.resize(pw, ph, dpi).is_ok());
        if !resized {
            self.gfx = Gfx::new(self.hwnd, pw, ph, dpi).ok();
            self.rebuild_art();
        }
        self.layers = Layers::default();
        self.region = RECT::default();
        self.fit_region();
        self.changed();
    }

    /// Something static on screen changed: redraw, re-rendering the cached layers.
    fn changed(&mut self) {
        self.dirty = true;
        self.layers.invalidate();
    }

    fn rebuild_art(&mut self) {
        self.art = match (&self.gfx, &self.art_pixels) {
            (Some(g), Some(px)) => g.art_brush(ART_SIZE, px),
            _ => None,
        };
    }

    // ---- window region: clicks outside the notch go to the window below ----

    fn region_for(&self, w: f32, h: f32) -> RECT {
        let s = self.scale();
        RECT {
            left: ((CX - w / 2.0 - FLARE) * s).floor() as i32,
            top: 0,
            right: ((CX + w / 2.0 + FLARE) * s).ceil() as i32,
            bottom: (h.max(HOT_STRIP) * s).ceil() as i32,
        }
    }

    fn apply_region(&mut self, r: RECT) {
        if r == self.region {
            return;
        }
        self.region = r;
        unsafe {
            let rgn = CreateRectRgn(r.left, r.top, r.right, r.bottom);
            SetWindowRgn(self.hwnd, Some(rgn), true);
        }
    }

    fn fit_region(&mut self) {
        let r = self.region_for(self.w.value, self.h.value);
        self.apply_region(r);
    }

    /// During an animation the region covers both the current and the target
    /// size (plus room for the spring's overshoot), so it changes only twice.
    fn grow_region(&mut self) {
        let t = self.region_for(self.w.target * 1.04 + 4.0, self.h.target * 1.04 + 4.0);
        let c = self.region;
        let r = RECT {
            left: c.left.min(t.left),
            top: 0,
            right: c.right.max(t.right),
            bottom: c.bottom.max(t.bottom),
        };
        self.apply_region(r);
    }

    // ---- state machine ----

    fn playing(&self) -> bool {
        self.info.active && self.info.playing
    }

    fn look(&self) -> Look {
        let dot = match (&self.in_use, self.settings.notify_privacy) {
            (InUse { cam: Some(_), .. }, true) => Some(CAM_COLOR),
            (InUse { mic: Some(_), .. }, true) => Some(MIC_COLOR),
            _ => None,
        };
        Look {
            playing: self.playing(),
            hidden: self.settings.hidden,
            dot,
        }
    }

    fn retarget(&mut self) {
        let look = self.look();
        let (w, h, r) = ui::size_for(self.mode, look);
        self.w.set(w);
        self.h.set(h);
        self.r.set(r);
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        let closed_visible = ui::size_for(Mode::Closed, look).1 > 0.0;
        self.a_closed
            .set(on(self.mode == Mode::Closed && closed_visible));
        self.a_open.set(on(self.mode == Mode::Open));
        self.a_act.set(on(matches!(self.mode, Mode::Activity(_))));
        self.grow_region();
        if !self.animating {
            self.animating = true;
            self.last_frame = Instant::now();
        }
    }

    fn set_mode(&mut self, mode: Mode) {
        if self.mode == mode {
            return;
        }
        if matches!(self.mode, Mode::Activity(_)) {
            self.kill_timer(TIMER_ACTIVITY);
        }
        if let Mode::Activity(act) = mode {
            self.last_act = act;
        }
        if mode == Mode::Open {
            // Other topmost windows may have covered us since the last time.
            unsafe {
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
            }
        }
        self.mode = mode;
        self.retarget();
        self.update_live_timer();
    }

    fn show_activity(&mut self, act: Act, ms: u32) {
        // While the panel is open its rows and cards already show the same information.
        if !self.visible || self.mode == Mode::Open {
            self.dirty = true;
            return;
        }
        self.set_mode(Mode::Activity(act));
        self.set_timer(TIMER_ACTIVITY, ms);
        self.changed();
    }

    /// Volume and brightness: repeated changes keep the same activity on screen.
    fn show_level(&mut self, act: Act) {
        if self.mode == Mode::Activity(act) {
            self.set_timer(TIMER_ACTIVITY, LEVEL_DURATION);
            self.dirty = true;
        } else {
            self.show_activity(act, LEVEL_DURATION);
        }
    }

    fn update_live_timer(&mut self) {
        let counting = (self.playing() && self.info.duration > 0.0) || self.focus.phase().is_some();
        let want = self.visible && self.mode == Mode::Open && counting;
        if want != self.live_timer {
            if want {
                self.set_timer(TIMER_LIVE, 1000);
            } else {
                self.kill_timer(TIMER_LIVE);
            }
            self.live_timer = want;
        }
    }

    fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if !visible {
            self.mode = Mode::Closed;
            self.retarget();
            for s in self.springs() {
                s.value = s.target;
            }
            self.animating = false;
            self.fit_region();
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        self.changed();
        self.update_live_timer();
    }

    fn springs(&mut self) -> [&mut Spring; 6] {
        [
            &mut self.w,
            &mut self.h,
            &mut self.r,
            &mut self.a_closed,
            &mut self.a_open,
            &mut self.a_act,
        ]
    }

    // ---- frames ----

    pub fn wants_frames(&self) -> bool {
        self.animating && self.visible
    }

    pub fn frame(&mut self) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .min(1.0 / 30.0);
        self.last_frame = now;
        let mut settled = true;
        for s in self.springs() {
            s.step(dt);
            settled &= s.settle();
        }
        if settled {
            self.animating = false;
            self.fit_region();
        }
        self.render();
    }

    pub fn render_if_dirty(&mut self) {
        if self.dirty && !self.animating {
            self.render();
        }
    }

    fn render(&mut self) {
        if !self.visible {
            return;
        }
        self.dirty = false;
        let Some(gfx) = &self.gfx else { return };
        let view = View {
            w: self.w.value,
            h: self.h.value,
            r: self.r.value,
            a_closed: self.a_closed.value,
            a_open: self.a_open.value,
            a_act: self.a_act.value,
            act: self.last_act,
            look: self.look(),
            info: &self.info,
            art: self.art.as_ref(),
            clock: &self.clock,
            date: &self.date,
            power: self.power,
            power_event: self.power_event,
            water_ml: self.settings.water_ml,
            water_goal: self.settings.water_goal,
            glass: self.settings.water_glass,
            volume: self
                .audio
                .as_ref()
                .filter(|a| a.available())
                .map(|a| (a.level, a.muted)),
            brightness: self.brightness.unwrap_or(0),
            device: self.audio.as_ref().map_or("", |a| a.device.as_str()),
            privacy: (self.privacy_shown.0, &self.privacy_shown.1),
            focus: self.focus,
            focus_min: self.settings.focus_min,
            focus_note: &self.focus_note,
            hot: self.hot,
            text: &self.text,
            fonts: &self.fonts,
        };
        let dpi = self.dpi as f32;
        let drawn = ui::prepare(gfx, &view, &mut self.layers, dpi)
            .and_then(|_| gfx.draw(|g| ui::compose(g, &view, &self.layers, dpi)));
        if drawn.is_err() {
            // Device lost (driver update, GPU reset): rebuild it from scratch.
            self.gfx = None;
            self.place();
        }
    }

    // ---- input ----

    fn in_shape(&self, x: f32, y: f32) -> bool {
        (x - CX).abs() <= self.w.value / 2.0 && y <= self.h.value.max(HOT_STRIP)
    }

    fn has_volume(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| a.available())
    }

    fn hit_at(&self, x: f32, y: f32) -> Hit {
        ui::hit(self.mode, x, y, &self.info, self.has_volume())
    }

    fn point(&self, lp: LPARAM) -> (f32, f32) {
        let s = self.scale();
        let x = (lp.0 & 0xFFFF) as i16 as f32 / s;
        let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / s;
        (x, y)
    }

    fn on_mouse_move(&mut self, x: f32, y: f32) {
        if self.dragging_volume {
            self.drag_volume(x);
            return;
        }
        if !self.tracking {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            self.tracking = unsafe { TrackMouseEvent(&mut tme) }.is_ok();
        }
        let inside = self.in_shape(x, y);
        if inside && !self.inside {
            self.kill_timer(TIMER_COLLAPSE);
            if self.mode != Mode::Open {
                self.set_timer(TIMER_HOVER, HOVER_DELAY);
            }
        } else if !inside && self.inside {
            self.leave();
        }
        self.inside = inside;
        let hot = if inside { self.hit_at(x, y) } else { Hit::None };
        if hot != self.hot {
            self.hot = hot;
            self.changed();
        }
    }

    fn leave(&mut self) {
        self.kill_timer(TIMER_HOVER);
        if self.mode == Mode::Open {
            self.set_timer(TIMER_COLLAPSE, LEAVE_DELAY);
        }
        if self.hot != Hit::None {
            self.hot = Hit::None;
            self.changed();
        }
    }

    fn drag_volume(&mut self, x: f32) {
        if let Some(audio) = &mut self.audio {
            audio.set_level(ui::volume_fraction(x));
            self.dirty = true;
        }
    }

    fn on_button_down(&mut self, x: f32, y: f32) {
        self.pressed = self.hit_at(x, y);
        if self.pressed == Hit::Volume {
            self.dragging_volume = true;
            unsafe { SetCapture(self.hwnd) };
            self.drag_volume(x);
        }
    }

    fn on_button_up(&mut self, x: f32, y: f32) {
        if self.dragging_volume {
            self.dragging_volume = false;
            self.pressed = Hit::None;
            let _ = unsafe { ReleaseCapture() };
            return;
        }
        let hit = self.hit_at(x, y);
        if hit != std::mem::take(&mut self.pressed) {
            return;
        }
        match hit {
            Hit::None => {
                if self.mode != Mode::Open && self.in_shape(x, y) {
                    self.kill_timer(TIMER_HOVER);
                    self.set_mode(Mode::Open);
                }
            }
            Hit::Prev => self.media.prev(),
            Hit::Next => self.media.next(),
            Hit::Play => {
                self.media.toggle();
                // Optimistic: flip right away, the SMTC event confirms it shortly.
                self.info.position = self.info.position_now();
                self.info.sampled = Some(Instant::now());
                self.info.playing = !self.info.playing;
                self.update_live_timer();
                self.changed();
            }
            Hit::Seek => {
                let seconds = ui::seek_fraction(x) * self.info.duration;
                self.media.seek(seconds);
                self.info.position = seconds;
                self.info.sampled = Some(Instant::now());
                self.dirty = true;
            }
            Hit::Mute => {
                if let Some(audio) = &mut self.audio {
                    audio.toggle_mute();
                    self.dirty = true;
                }
            }
            Hit::Volume => {}
            Hit::Drink => {
                self.drink();
                if matches!(self.mode, Mode::Activity(Act::Water)) {
                    self.set_timer(TIMER_ACTIVITY, 1200);
                }
            }
            Hit::Focus => self.toggle_focus(),
        }
    }

    fn on_wheel(&mut self, steps: f32) {
        let Some(audio) = self.audio.as_mut().filter(|a| a.available()) else {
            return;
        };
        audio.set_level(audio.level + steps * VOLUME_STEP);
        if self.mode == Mode::Open {
            self.dirty = true;
        } else {
            self.show_level(Act::Volume);
        }
    }

    fn on_tray_click(&mut self) {
        if self.mode == Mode::Open {
            self.set_mode(Mode::Closed);
        } else {
            self.set_mode(Mode::Open);
            if !self.inside {
                self.set_timer(TIMER_COLLAPSE, 4000);
            }
        }
    }

    // ---- events ----

    fn on_timer(&mut self, id: usize) {
        match id {
            TIMER_HOVER => {
                self.kill_timer(TIMER_HOVER);
                if self.inside {
                    self.set_mode(Mode::Open);
                }
            }
            TIMER_COLLAPSE => {
                self.kill_timer(TIMER_COLLAPSE);
                if !self.inside && self.mode == Mode::Open && !self.dragging_volume {
                    self.set_mode(Mode::Closed);
                }
            }
            TIMER_ACTIVITY => {
                self.kill_timer(TIMER_ACTIVITY);
                if matches!(self.mode, Mode::Activity(_)) {
                    self.set_mode(if self.inside {
                        Mode::Open
                    } else {
                        Mode::Closed
                    });
                }
            }
            TIMER_CLOCK => {
                self.clock = sys::clock_text();
                self.date = sys::date_text();
                self.roll_water_day();
                self.set_timer(TIMER_CLOCK, sys::ms_to_next_minute());
                self.changed();
            }
            TIMER_LIVE => self.dirty = true,
            TIMER_WATER => self.on_water_timer(),
            TIMER_FOCUS => self.on_focus_timer(),
            _ => {}
        }
    }

    fn on_media(&mut self, changed: usize) {
        let (info, art) = {
            let Ok(shared) = self.media.shared.lock() else {
                return;
            };
            let art = (shared.info.art_gen != self.info.art_gen).then(|| shared.art.clone());
            (shared.info.clone(), art)
        };
        let track_changed = info.title != self.info.title || info.artist != self.info.artist;
        let was_playing = self.playing();
        if let Some(art) = art {
            self.art_pixels = art;
            self.rebuild_art();
        }
        self.info = info;
        let playing = self.playing();
        let announce = self.media_ready && self.settings.notify_track && track_changed && playing;
        if announce && !self.info.title.is_empty() && self.mode == Mode::Closed {
            self.show_activity(Act::Track, 3000);
        } else if playing != was_playing && self.mode == Mode::Closed {
            self.retarget();
        }
        self.media_ready = true;
        self.update_live_timer();
        // Timeline ticks only matter when the progress bar is on screen.
        if changed & !media::CHANGED_TIMELINE != 0 {
            self.changed();
        } else if self.mode == Mode::Open {
            self.dirty = true;
        }
    }

    fn on_audio(&mut self, kind: usize, data: isize) {
        let Some(audio) = &mut self.audio else { return };
        match kind {
            audio::VOLUME_CHANGED => {
                let (level, muted) = audio::unpack(data);
                if (audio.level - level).abs() < 0.0005 && audio.muted == muted {
                    return; // our own change, already on screen
                }
                audio.level = level;
                audio.muted = muted;
                if self.mode == Mode::Open {
                    self.dirty = true;
                } else if self.settings.notify_volume {
                    self.show_level(Act::Volume);
                }
            }
            audio::DEVICE_CHANGED => {
                let old = std::mem::take(&mut audio.device);
                audio.bind();
                let name = audio.device.clone();
                if self.settings.notify_audio && !name.is_empty() && name != old {
                    self.show_activity(Act::Device, 2500);
                }
                self.changed();
            }
            _ => {}
        }
    }

    fn on_brightness(&mut self, value: u8) {
        // Windows sends the current value right after registering: not a change.
        let previous = self.brightness.replace(value);
        if previous.is_some_and(|p| p != value) && self.settings.notify_brightness {
            self.show_level(Act::Brightness);
        }
    }

    fn on_privacy(&mut self) {
        let Some(now) = self
            .privacy
            .as_ref()
            .and_then(|s| s.lock().ok().map(|s| s.clone()))
        else {
            return;
        };
        let started = match (&now.cam, &now.mic) {
            (Some(app), _) if self.in_use.cam.is_none() => Some((true, app.clone())),
            (_, Some(app)) if self.in_use.mic.is_none() => Some((false, app.clone())),
            _ => None,
        };
        self.in_use = now;
        if !self.settings.notify_privacy {
            return;
        }
        if let Some(shown) = started {
            self.privacy_shown = shown;
            self.show_activity(Act::Privacy, 2500);
        } else if self.mode == Mode::Closed {
            // The dot appeared or went away: the pill changes width.
            self.retarget();
        }
        self.changed();
    }

    fn on_foreground(&mut self) {
        if !self.settings.monitor_active {
            return;
        }
        if let Some(monitor) = sys::active_monitor().filter(|&m| m != self.monitor) {
            self.monitor = monitor;
            self.place();
        }
    }

    fn on_power(&mut self) {
        let old = self.power;
        let p = sys::power();
        self.power = p;
        if p.on_ac {
            self.low_warned = 100;
        } else {
            self.full_notified = false;
        }
        if p.has_battery && self.settings.notify_battery {
            let event = if p.on_ac != old.on_ac {
                // Plugging in an already full battery: "plugged in" says enough.
                self.full_notified |= p.on_ac && p.percent >= 100;
                Some(if p.on_ac {
                    PowerEvent::Plugged
                } else {
                    PowerEvent::Unplugged
                })
            } else if !p.on_ac && p.percent <= 10 && self.low_warned > 10 {
                self.low_warned = 10;
                Some(PowerEvent::Low)
            } else if !p.on_ac && p.percent <= 20 && self.low_warned > 20 {
                self.low_warned = 20;
                Some(PowerEvent::Low)
            } else if p.on_ac && p.percent >= 100 && !self.full_notified {
                self.full_notified = true;
                Some(PowerEvent::Full)
            } else {
                None
            };
            if let Some(event) = event {
                self.power_event = event;
                self.show_activity(Act::Power, 3500);
            }
        }
        if p != old {
            self.changed();
        }
    }

    // ---- pomodoro ----

    fn toggle_focus(&mut self) {
        if self.focus.phase().is_some() {
            self.focus.stop();
            self.kill_timer(TIMER_FOCUS);
        } else {
            self.focus.start(Phase::Focus, self.settings.focus_min);
            self.arm_focus();
        }
        self.update_live_timer();
        self.changed();
    }

    fn arm_focus(&self) {
        if let Some(ms) = self.focus.next_tick_ms() {
            self.set_timer(TIMER_FOCUS, ms + 20);
        }
    }

    fn on_focus_timer(&mut self) {
        self.kill_timer(TIMER_FOCUS);
        match self.focus.remaining() {
            None => {}
            Some(left) if left.as_millis() <= 300 => self.focus_phase_done(),
            Some(_) => {
                self.arm_focus();
                self.changed();
            }
        }
    }

    fn focus_phase_done(&mut self) {
        sys::chime();
        if self.focus.phase() == Some(Phase::Focus) {
            self.focus.start(Phase::Rest, self.settings.break_min);
            self.focus_note = format!("{} · {} min", self.text.focus_done, self.settings.break_min);
            self.arm_focus();
        } else {
            self.focus.stop();
            self.focus_note = self.text.rest_done.to_string();
        }
        self.show_activity(Act::Focus, 5000);
        self.update_live_timer();
        self.changed();
    }

    // ---- water ----

    fn roll_water_day(&mut self) {
        let today = sys::today();
        if self.settings.water_date != today {
            self.settings.water_date = today;
            self.settings.water_ml = 0;
            self.settings.save();
            self.changed();
        }
    }

    fn drink(&mut self) {
        self.roll_water_day();
        self.settings.water_ml = (self.settings.water_ml + self.settings.water_glass).min(20_000);
        self.settings.save();
        // Count the next reminder from the last glass, not from the last alert.
        self.schedule_water();
        self.changed();
    }

    fn schedule_water(&self) {
        if self.settings.water_enabled {
            self.set_timer(TIMER_WATER, self.settings.water_interval * 60_000);
        } else {
            self.kill_timer(TIMER_WATER);
        }
    }

    fn on_water_timer(&mut self) {
        self.roll_water_day();
        let s = &self.settings;
        let hour = sys::local_hour();
        if s.water_enabled
            && (s.water_start..s.water_end).contains(&hour)
            && s.water_ml < s.water_goal
        {
            self.show_activity(Act::Water, 15_000);
        }
    }

    // ---- tray menu ----

    pub fn menu_state(&self) -> MenuState {
        let s = &self.settings;
        MenuState {
            autostart: sys::autostart_enabled(),
            hidden: s.hidden,
            monitor_active: s.monitor_active,
            hotkey: s.hotkey,
            alerts: [
                s.notify_track,
                s.notify_volume,
                s.notify_brightness,
                s.notify_audio,
                s.notify_battery,
                s.notify_privacy,
            ],
            water_enabled: s.water_enabled,
            interval: s.water_interval,
            goal: s.water_goal,
            glass: s.water_glass,
            focus_running: self.focus.phase().is_some(),
            focus_min: s.focus_min,
            rest_min: s.break_min,
            text: self.text,
        }
    }

    pub fn on_menu(&mut self, cmd: MenuCmd) {
        let s = &mut self.settings;
        match cmd {
            MenuCmd::None => return,
            MenuCmd::Autostart => {
                sys::set_autostart(!sys::autostart_enabled());
                return;
            }
            MenuCmd::Exit => {
                unsafe { PostQuitMessage(0) };
                return;
            }
            MenuCmd::FocusToggle => {
                self.toggle_focus();
                return;
            }
            MenuCmd::Hidden => s.hidden = !s.hidden,
            MenuCmd::Monitor(active) => s.monitor_active = active,
            MenuCmd::Hotkey => {
                s.hotkey = !s.hotkey;
                sys::set_hotkey(self.hwnd, s.hotkey);
            }
            MenuCmd::Alert(alert) => {
                let flag = match alert {
                    Alert::Track => &mut s.notify_track,
                    Alert::Volume => &mut s.notify_volume,
                    Alert::Brightness => &mut s.notify_brightness,
                    Alert::Audio => &mut s.notify_audio,
                    Alert::Battery => &mut s.notify_battery,
                    Alert::Privacy => &mut s.notify_privacy,
                };
                *flag = !*flag;
            }
            MenuCmd::WaterToggle => s.water_enabled = !s.water_enabled,
            MenuCmd::Interval(m) => s.water_interval = m,
            MenuCmd::Goal(g) => s.water_goal = g,
            MenuCmd::Glass(g) => s.water_glass = g,
            MenuCmd::ResetWater => s.water_ml = 0,
            MenuCmd::FocusLength(m) => s.focus_min = m,
            MenuCmd::RestLength(m) => s.break_min = m,
        }
        self.settings.save();
        self.apply_settings();
    }

    /// Starts or stops whatever depends on the settings that may have changed.
    fn apply_settings(&mut self) {
        if self.settings.notify_privacy && self.privacy.is_none() {
            self.privacy = Some(privacy::start(self.hwnd, WM_APP_PRIVACY));
        }
        match (self.settings.monitor_active, self.foreground_hook) {
            (true, None) => {
                self.foreground_hook = Some(sys::watch_foreground(self.hwnd, WM_APP_FOREGROUND));
                self.on_foreground();
            }
            (false, Some(hook)) => {
                sys::unwatch_foreground(hook);
                self.foreground_hook = None;
                self.place();
            }
            _ => {}
        }
        self.schedule_water();
        self.retarget();
        self.changed();
    }

    // ---- dispatch ----

    /// Handles a window message; `None` means "let DefWindowProc handle it".
    pub fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEMOVE => {
                let (x, y) = self.point(lp);
                self.on_mouse_move(x, y);
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                if self.inside {
                    self.inside = false;
                    self.leave();
                }
            }
            WM_LBUTTONDOWN => {
                let (x, y) = self.point(lp);
                self.on_button_down(x, y);
            }
            WM_LBUTTONUP => {
                let (x, y) = self.point(lp);
                self.on_button_up(x, y);
            }
            WM_CAPTURECHANGED => self.dragging_volume = false,
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / WHEEL_DELTA as f32;
                self.on_wheel(delta);
            }
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_SETCURSOR if self.hot != Hit::None => {
                unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                return Some(LRESULT(1));
            }
            WM_HOTKEY => self.on_tray_click(),
            WM_TIMER => self.on_timer(wp.0),
            WM_APP_MEDIA => self.on_media(wp.0),
            WM_APP_AUDIO => self.on_audio(wp.0, lp.0),
            WM_APP_PRIVACY => self.on_privacy(),
            WM_APP_FOREGROUND => self.on_foreground(),
            WM_APP_TRAY => {
                let event = lp.0 as u32 & 0xFFFF;
                if event == NIN_SELECT || event == NIN_KEYSELECT {
                    self.on_tray_click();
                }
            }
            WM_APP_APPBAR => {
                if wp.0 as u32 == ABN_FULLSCREENAPP {
                    let full = lp.0 != 0;
                    if !(full && sys::desktop_is_foreground()) {
                        self.set_visible(!full);
                    }
                }
            }
            WM_POWERBROADCAST => {
                match wp.0 as u32 {
                    PBT_APMPOWERSTATUSCHANGE => self.on_power(),
                    PBT_POWERSETTINGCHANGE => {
                        let setting = unsafe { (lp.0 as *const POWERBROADCAST_SETTING).as_ref() };
                        match setting {
                            Some(s)
                                if s.PowerSetting == GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS
                                    && s.DataLength >= 4 =>
                            {
                                let value = unsafe {
                                    std::ptr::read_unaligned(s.Data.as_ptr() as *const u32)
                                };
                                self.on_brightness(value.min(100) as u8);
                            }
                            Some(_) => self.on_power(),
                            None => {}
                        }
                    }
                    _ => {}
                }
                return Some(LRESULT(1));
            }
            WM_DPICHANGED => {
                self.dpi = ((wp.0 >> 16) & 0xFFFF) as u32;
                self.place();
            }
            WM_DISPLAYCHANGE => self.place(),
            m if m == self.taskbar_created && m != 0 => self.tray.add(),
            _ => return None,
        }
        Some(LRESULT(0))
    }
}
