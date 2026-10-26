//! App state and the notch state machine (closed / open / live activity).
//! Everything here runs on the UI thread, driven by window messages.

use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

use windows::core::{w, Result, GUID};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;
use windows::Win32::Graphics::Gdi::{
    CreateRectRgn, GetMonitorInfoW, MonitorFromPoint, SetWindowRgn, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::RegisterPowerSettingNotification;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABM_NEW, ABM_REMOVE, ABN_FULLSCREENAPP, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::anim::{self, Spring};
use crate::gfx::{Fonts, Gfx};
use crate::i18n::{self, Text};
use crate::media::{self, Info, Media, ART_SIZE};
use crate::settings::Settings;
use crate::sys::{self, Power};
use crate::tray::{MenuCmd, MenuState, Tray};
use crate::ui::{self, Act, Hit, Layers, Mode, View, CX, FLARE, WIN_H, WIN_W};

pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_MEDIA: u32 = WM_APP + 2;
pub const WM_APP_APPBAR: u32 = WM_APP + 3;
const WM_MOUSELEAVE: u32 = 0x02A3;
const NIN_SELECT: u32 = WM_USER;
const NIN_KEYSELECT: u32 = WM_USER + 1;
const GUID_ACDC_POWER_SOURCE: GUID = GUID::from_u128(0x5d3e9a59_e9d5_4b00_a6bd_ff34ff516548);

// The only timers in the app. None of them polls state.
const TIMER_HOVER: usize = 1; // one-shot: open after the cursor rests on the notch
const TIMER_COLLAPSE: usize = 2; // one-shot: close after the cursor leaves
const TIMER_ACTIVITY: usize = 3; // one-shot: end of a live activity
const TIMER_CLOCK: usize = 4; // once a minute, on the minute
const TIMER_PROGRESS: usize = 5; // 1 Hz, only while open and playing
const TIMER_WATER: usize = 6; // every N minutes, only if the reminder is on

const HOVER_DELAY: u32 = 110;
const LEAVE_DELAY: u32 = 280;

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

    media: Media,
    info: Info,
    media_ready: bool,
    art: Option<ID2D1BitmapBrush1>,
    art_pixels: Option<Arc<Vec<u8>>>,
    layers: Layers,
    progress_timer: bool,

    clock: String,
    date: String,
    power: Power,
    tray: Tray,
    taskbar_created: u32,
}

impl App {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let (w, h, r) = ui::size_for(Mode::Closed, false);
        let mut app = Self {
            hwnd,
            text: i18n::load(),
            fonts: Fonts::new()?,
            gfx: None,
            settings: Settings::load(),
            dpi: unsafe { GetDpiForWindow(hwnd) }.max(96),
            mode: Mode::Closed,
            last_act: Act::Track,
            w: anim::size(w),
            h: anim::size(h),
            r: anim::size(r),
            a_closed: anim::fade(1.0),
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
            media: Media::start(hwnd, WM_APP_MEDIA),
            info: Info::default(),
            media_ready: false,
            art: None,
            art_pixels: None,
            layers: Layers::default(),
            progress_timer: false,
            clock: sys::clock_text(),
            date: sys::date_text(),
            power: sys::power(),
            tray: Tray::new(hwnd, WM_APP_TRAY),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        };
        app.roll_water_day();
        app.place();
        unsafe {
            let mut abd = app.appbar_data();
            SHAppBarMessage(ABM_NEW, &mut abd);
            let _ = RegisterPowerSettingNotification(
                HANDLE(hwnd.0),
                &GUID_ACDC_POWER_SOURCE,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            );
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        app.set_timer(TIMER_CLOCK, sys::ms_to_next_minute());
        app.schedule_water();
        // Keep the autostart entry pointing at this exe if the user moved it.
        if sys::autostart_enabled() {
            sys::set_autostart(true);
        }
        Ok(app)
    }

    pub fn shutdown(&mut self) {
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

    /// Centers the window at the top of the primary monitor and sizes the swap chain.
    fn place(&mut self) {
        for _ in 0..2 {
            let s = self.scale();
            let (pw, ph) = ((WIN_W * s).ceil() as i32, (WIN_H * s).ceil() as i32);
            unsafe {
                let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
                let mut mi = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ = GetMonitorInfoW(monitor, &mut mi);
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

    /// Something on screen changed: redraw, re-rendering the cached layers.
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
            bottom: (h * s).ceil() as i32,
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

    fn retarget(&mut self) {
        let (w, h, r) = ui::size_for(self.mode, self.playing());
        self.w.set(w);
        self.h.set(h);
        self.r.set(r);
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        self.a_closed.set(on(self.mode == Mode::Closed));
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
        self.update_progress_timer();
    }

    fn show_activity(&mut self, act: Act, ms: u32) {
        // While the panel is open its cards already show the same information.
        if !self.visible || self.mode == Mode::Open {
            self.changed();
            return;
        }
        self.set_mode(Mode::Activity(act));
        self.set_timer(TIMER_ACTIVITY, ms);
    }

    fn update_progress_timer(&mut self) {
        let want =
            self.visible && self.mode == Mode::Open && self.playing() && self.info.duration > 0.0;
        if want != self.progress_timer {
            if want {
                self.set_timer(TIMER_PROGRESS, 1000);
            } else {
                self.kill_timer(TIMER_PROGRESS);
            }
            self.progress_timer = want;
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
        self.update_progress_timer();
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
            playing: self.playing(),
            info: &self.info,
            art: self.art.as_ref(),
            clock: &self.clock,
            date: &self.date,
            power: self.power,
            water_ml: self.settings.water_ml,
            water_goal: self.settings.water_goal,
            glass: self.settings.water_glass,
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
        (x - CX).abs() <= self.w.value / 2.0 && y <= self.h.value
    }

    fn hit_at(&self, x: f32, y: f32) -> Hit {
        ui::hit(self.mode, x, y, &self.info)
    }

    fn point(&self, lp: LPARAM) -> (f32, f32) {
        let s = self.scale();
        let x = (lp.0 & 0xFFFF) as i16 as f32 / s;
        let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / s;
        (x, y)
    }

    fn on_mouse_move(&mut self, x: f32, y: f32) {
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

    fn on_click(&mut self, x: f32, y: f32) {
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
                self.update_progress_timer();
                self.changed();
            }
            Hit::Seek => {
                let seconds = ui::seek_fraction(x) * self.info.duration;
                self.media.seek(seconds);
                self.info.position = seconds;
                self.info.sampled = Some(Instant::now());
                self.changed();
            }
            Hit::Drink => {
                self.drink();
                if matches!(self.mode, Mode::Activity(Act::Water)) {
                    self.set_timer(TIMER_ACTIVITY, 1200);
                }
            }
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
                if !self.inside && self.mode == Mode::Open {
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
            TIMER_PROGRESS => self.dirty = true,
            TIMER_WATER => self.on_water_timer(),
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
        if self.media_ready
            && track_changed
            && playing
            && !self.info.title.is_empty()
            && self.mode == Mode::Closed
        {
            self.show_activity(Act::Track, 3000);
        } else if playing != was_playing && self.mode == Mode::Closed {
            self.retarget();
        }
        self.media_ready = true;
        self.update_progress_timer();
        // Timeline ticks only matter when the progress bar is on screen.
        if changed & !media::CHANGED_TIMELINE != 0 || self.mode == Mode::Open {
            self.changed();
        }
    }

    fn on_power(&mut self) {
        let old = self.power;
        self.power = sys::power();
        if self.power.has_battery && self.power.on_ac != old.on_ac {
            self.show_activity(Act::Power, 3500);
        }
        if self.power != old {
            self.changed();
        }
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
        MenuState {
            autostart: sys::autostart_enabled(),
            water_enabled: self.settings.water_enabled,
            interval: self.settings.water_interval,
            goal: self.settings.water_goal,
            glass: self.settings.water_glass,
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
            MenuCmd::WaterToggle => s.water_enabled = !s.water_enabled,
            MenuCmd::Interval(m) => s.water_interval = m,
            MenuCmd::Goal(g) => s.water_goal = g,
            MenuCmd::Glass(g) => s.water_glass = g,
            MenuCmd::ResetWater => s.water_ml = 0,
        }
        self.settings.save();
        self.schedule_water();
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
                self.pressed = self.hit_at(x, y);
            }
            WM_LBUTTONUP => {
                let (x, y) = self.point(lp);
                self.on_click(x, y);
            }
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_SETCURSOR if self.hot != Hit::None => {
                unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                return Some(LRESULT(1));
            }
            WM_TIMER => self.on_timer(wp.0),
            WM_APP_MEDIA => self.on_media(wp.0),
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
                if matches!(
                    wp.0 as u32,
                    PBT_APMPOWERSTATUSCHANGE | PBT_POWERSETTINGCHANGE
                ) {
                    self.on_power();
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
