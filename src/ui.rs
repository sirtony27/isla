//! Layout, drawing and hit-testing of every view. All coordinates are DIPs
//! relative to the window; the notch is anchored to the top center.

use std::f32::consts::PI;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1BitmapBrush1, ID2D1GeometrySink, ID2D1PathGeometry1, ID2D1StrokeStyle,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE, D2D1_ARC_SIZE_LARGE,
    D2D1_ARC_SIZE_SMALL, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::focus::{self, Focus, Phase};
use crate::gfx::{Fonts, Gfx};
use crate::i18n::Text;
use crate::media::{Info, ART_SIZE};
use crate::sys::Power;

/// The window is fixed at this size; the notch animates inside it and the
/// window region follows the notch so clicks elsewhere pass through.
pub const WIN_W: f32 = 460.0;
pub const WIN_H: f32 = 236.0;
pub const CX: f32 = WIN_W / 2.0;
/// Concave curve joining the notch to the top edge of the screen.
pub const FLARE: f32 = 8.0;
/// Height of the invisible strip that opens the notch in "alerts only" mode.
pub const HOT_STRIP: f32 = 4.0;

const WHITE: u32 = 0xFFFFFF;
const GREEN: u32 = 0x34C759;
const RED: u32 = 0xFF453A;
const BLUE: u32 = 0x5AC8FA;
const CORAL: u32 = 0xFF7A59;
pub const MIC_COLOR: u32 = 0xFF9F0A;
pub const CAM_COLOR: u32 = 0x30D158;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Hit {
    #[default]
    None,
    Prev,
    Play,
    Next,
    Seek,
    Mute,
    Volume,
    Drink,
    Focus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Power,
    Track,
    Water,
    Volume,
    Brightness,
    Device,
    Privacy,
    Focus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Closed,
    Open,
    Activity(Act),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerEvent {
    #[default]
    Plugged,
    Unplugged,
    Low,
    Full,
}

/// What shapes the closed pill.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    pub playing: bool,
    pub hidden: bool,
    /// Privacy dot color while the microphone or camera is in use.
    pub dot: Option<u32>,
}

/// Target (width, height, corner radius) of the notch for a mode.
pub fn size_for(mode: Mode, look: Look) -> (f32, f32, f32) {
    match mode {
        Mode::Closed if look.hidden && look.dot.is_none() => (160.0, 0.0, 0.0),
        Mode::Closed if look.hidden => (40.0, 22.0, 11.0),
        Mode::Closed => {
            let w = 180.0
                + if look.playing { 40.0 } else { 0.0 }
                + if look.dot.is_some() { 18.0 } else { 0.0 };
            (w, 30.0, 12.0)
        }
        Mode::Open => (420.0, 212.0, 30.0),
        Mode::Activity(Act::Power | Act::Focus) => (300.0, 36.0, 15.0),
        Mode::Activity(Act::Track) => (340.0, 40.0, 17.0),
        Mode::Activity(Act::Water) => (380.0, 64.0, 24.0),
        Mode::Activity(Act::Volume | Act::Brightness) => (280.0, 36.0, 15.0),
        Mode::Activity(Act::Device) => (330.0, 40.0, 17.0),
        Mode::Activity(Act::Privacy) => (310.0, 36.0, 15.0),
    }
}

pub fn phase_color(phase: Phase) -> u32 {
    match phase {
        Phase::Focus => CORAL,
        Phase::Rest => GREEN,
    }
}

#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }
}

struct OpenLayout {
    art: Rect,
    title: Rect,
    artist: Rect,
    prev: Rect,
    play: Rect,
    next: Rect,
    bar: Rect,
    times: Rect,
    mute: Rect,
    volume: Rect,
    volume_text: Rect,
    water: Rect,
    focus: Rect,
    battery: Rect,
}

fn open_layout() -> OpenLayout {
    let (w, _, _) = size_for(Mode::Open, Look::default());
    let (x0, x1, pad) = (CX - w / 2.0, CX + w / 2.0, 18.0);
    let art = Rect::new(x0 + pad, 16.0, 64.0, 64.0);
    let cy = art.cy();
    let next = Rect::new(x1 - pad - 34.0, cy - 17.0, 34.0, 34.0);
    let play = Rect::new(next.x - 4.0 - 40.0, cy - 20.0, 40.0, 40.0);
    let prev = Rect::new(play.x - 4.0 - 34.0, cy - 17.0, 34.0, 34.0);
    let tx = art.right() + 14.0;
    let tw = prev.x - 10.0 - tx;
    let bar = Rect::new(x0 + pad, 94.0, w - 2.0 * pad, 4.0);
    let mute = Rect::new(x0 + pad - 4.0, 122.0, 26.0, 22.0);
    let volume = Rect::new(
        mute.right() + 4.0,
        131.0,
        x1 - pad - 46.0 - (mute.right() + 4.0),
        4.0,
    );
    let card_w = (w - 2.0 * pad - 20.0) / 3.0;
    let water = Rect::new(x0 + pad, 154.0, card_w, 44.0);
    let focus = Rect::new(water.right() + 10.0, water.y, card_w, water.h);
    OpenLayout {
        art,
        title: Rect::new(tx, art.y + 9.0, tw, 22.0),
        artist: Rect::new(tx, art.y + 32.0, tw, 20.0),
        prev,
        play,
        next,
        bar,
        times: Rect::new(bar.x, 101.0, bar.w, 14.0),
        mute,
        volume,
        volume_text: Rect::new(x1 - pad - 40.0, 122.0, 40.0, 22.0),
        water,
        focus,
        battery: Rect::new(focus.right() + 10.0, water.y, card_w, water.h),
    }
}

fn water_button() -> Rect {
    let (w, _, _) = size_for(Mode::Activity(Act::Water), Look::default());
    Rect::new(CX + w / 2.0 - 16.0 - 88.0, 17.0, 88.0, 30.0)
}

pub fn hit(mode: Mode, x: f32, y: f32, info: &Info, has_volume: bool) -> Hit {
    match mode {
        Mode::Open => {
            let l = open_layout();
            if info.active {
                if l.prev.contains(x, y) && info.can_prev {
                    return Hit::Prev;
                }
                if l.play.contains(x, y) {
                    return Hit::Play;
                }
                if l.next.contains(x, y) && info.can_next {
                    return Hit::Next;
                }
                if info.duration > 0.0
                    && Rect::new(l.bar.x, l.bar.y - 8.0, l.bar.w, 20.0).contains(x, y)
                {
                    return Hit::Seek;
                }
            }
            if has_volume {
                if l.mute.contains(x, y) {
                    return Hit::Mute;
                }
                if Rect::new(l.volume.x - 6.0, 122.0, l.volume.w + 12.0, 22.0).contains(x, y) {
                    return Hit::Volume;
                }
            }
            if l.water.contains(x, y) {
                return Hit::Drink;
            }
            if l.focus.contains(x, y) {
                return Hit::Focus;
            }
            Hit::None
        }
        Mode::Activity(Act::Water) if water_button().contains(x, y) => Hit::Drink,
        _ => Hit::None,
    }
}

/// Fraction (0..=1) of the track under `x` on the progress bar.
pub fn seek_fraction(x: f32) -> f64 {
    let bar = open_layout().bar;
    ((x - bar.x) / bar.w).clamp(0.0, 1.0) as f64
}

/// Volume (0..=1) under `x` on the volume bar.
pub fn volume_fraction(x: f32) -> f32 {
    let bar = open_layout().volume;
    ((x - bar.x) / bar.w).clamp(0.0, 1.0)
}

/// Everything a frame needs, borrowed from the app for the duration of the draw.
pub struct View<'a> {
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub a_closed: f32,
    pub a_open: f32,
    pub a_act: f32,
    pub act: Act,
    pub look: Look,
    pub info: &'a Info,
    pub art: Option<&'a ID2D1BitmapBrush1>,
    pub clock: &'a str,
    pub date: &'a str,
    pub power: Power,
    pub power_event: PowerEvent,
    pub water_ml: u32,
    pub water_goal: u32,
    pub glass: u32,
    /// Volume (0..=1) and mute, if there is an output device.
    pub volume: Option<(f32, bool)>,
    pub brightness: u8,
    pub device: &'a str,
    /// Camera (true) or microphone (false) and the app using it, for the activity.
    pub privacy: (bool, &'a str),
    pub focus: Focus,
    pub focus_min: u32,
    pub focus_note: &'a str,
    pub hot: Hit,
    pub text: &'a Text,
    pub fonts: &'a Fonts,
}

/// Each view (closed, open, activity) is drawn once into its own cached
/// bitmap and only redrawn when its content changes. Animation frames just
/// compose those bitmaps, which keeps each frame cheap. Parts that change
/// every second (progress, volume, timer) are drawn live on top.
#[derive(Default)]
pub struct Layers {
    slots: [Layer; 3],
}

#[derive(Default)]
struct Layer {
    bitmap: Option<ID2D1Bitmap1>,
    /// Pixel origin and size of the bitmap, plus the activity it shows.
    key: (i32, i32, i32, u8),
    valid: bool,
}

impl Layers {
    pub fn invalidate(&mut self) {
        for slot in &mut self.slots {
            slot.valid = false;
        }
    }
}

const CLOSED: usize = 0;
const OPEN: usize = 1;
// Slot 2: the live activity.

fn layer_alpha(v: &View) -> [f32; 3] {
    [v.a_closed, v.a_open, v.a_act]
}

fn layer_rect(kind: usize, v: &View) -> Rect {
    let mode = match kind {
        CLOSED => Mode::Closed,
        OPEN => Mode::Open,
        _ => Mode::Activity(v.act),
    };
    let (w, h, _) = size_for(mode, v.look);
    Rect::new(CX - w / 2.0, 0.0, w, h)
}

/// Brings every visible layer up to date. Must run before `compose`.
pub fn prepare(g: &Gfx, v: &View, layers: &mut Layers, dpi: f32) -> windows::core::Result<()> {
    let scale = dpi / 96.0;
    for (kind, alpha) in layer_alpha(v).into_iter().enumerate() {
        if alpha <= 0.01 {
            continue;
        }
        // Snap the bitmap to whole pixels so text stays crisp at rest.
        let r = layer_rect(kind, v);
        let ox = (r.x * scale).floor() as i32;
        let pw = ((r.x + r.w) * scale).ceil() as i32 - ox;
        let ph = (r.h * scale).ceil() as i32;
        let slot = &mut layers.slots[kind];
        if pw <= 0 || ph <= 0 {
            slot.valid = false;
            continue;
        }
        let act = if kind == OPEN || kind == CLOSED {
            0
        } else {
            v.act as u8 + 1
        };
        let key = (ox, pw, ph, act);
        if slot.bitmap.is_none() || slot.key != key {
            slot.bitmap = g.layer(pw as u32, ph as u32, dpi);
            slot.key = key;
            slot.valid = false;
        }
        if let (Some(bitmap), false) = (&slot.bitmap, slot.valid) {
            g.render_layer(bitmap, ox as f32 / scale, |g| {
                let p = Painter { g, alpha: 1.0 };
                match kind {
                    CLOSED => draw_closed(&p, v),
                    OPEN => draw_open(&p, v),
                    _ => draw_activity(&p, v),
                }
            })?;
            slot.valid = true;
        }
    }
    Ok(())
}

/// Draws one frame: the notch shape plus the cached layers.
pub fn compose(g: &Gfx, v: &View, layers: &Layers, dpi: f32) {
    let scale = dpi / 96.0;
    let p = Painter { g, alpha: 1.0 };
    if v.h > 0.5 {
        if let Some(shape) = p.geometry(|s| notch_path(s, v.w, v.h, v.r)) {
            p.color(0x000000, 1.0);
            unsafe { g.ctx.FillGeometry(&shape, &g.brush, None) };
        }
    }
    let clip = Rect::new(CX - v.w / 2.0, 0.0, v.w, v.h).d2d();
    unsafe {
        g.ctx
            .PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
    };
    for (kind, alpha) in layer_alpha(v).into_iter().enumerate() {
        let slot = &layers.slots[kind];
        let Some(bitmap) = slot.bitmap.as_ref().filter(|_| slot.valid && alpha > 0.01) else {
            continue;
        };
        // Open views and activities grow from 92 % to 100 % while fading in.
        let s = if kind == CLOSED {
            1.0
        } else {
            0.92 + 0.08 * alpha
        };
        let m = Matrix3x2 {
            M11: s,
            M12: 0.0,
            M21: 0.0,
            M22: s,
            M31: CX * (1.0 - s),
            M32: 0.0,
        };
        let (ox, pw, ph, _) = slot.key;
        let dest = Rect::new(ox as f32 / scale, 0.0, pw as f32 / scale, ph as f32 / scale).d2d();
        unsafe {
            g.ctx.SetTransform(&m);
            g.ctx.DrawBitmap(
                bitmap,
                Some(&dest),
                alpha,
                D2D1_INTERPOLATION_MODE_LINEAR,
                None,
                None,
            );
        }
        let p = Painter { g, alpha };
        match kind {
            CLOSED => {}
            OPEN => draw_open_live(&p, v),
            _ => draw_activity_live(&p, v),
        }
        unsafe { g.ctx.SetTransform(&Matrix3x2::identity()) };
    }
    unsafe { g.ctx.PopAxisAlignedClip() };
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn arc_to(
    s: &ID2D1GeometrySink,
    x: f32,
    y: f32,
    r: f32,
    dir: D2D1_SWEEP_DIRECTION,
    size: D2D1_ARC_SIZE,
) {
    unsafe {
        s.AddArc(&D2D1_ARC_SEGMENT {
            point: v2(x, y),
            size: D2D_SIZE_F {
                width: r,
                height: r,
            },
            rotationAngle: 0.0,
            sweepDirection: dir,
            arcSize: size,
        });
    }
}

fn arc(s: &ID2D1GeometrySink, x: f32, y: f32, r: f32, dir: D2D1_SWEEP_DIRECTION) {
    arc_to(s, x, y, r, dir, D2D1_ARC_SIZE_SMALL);
}

/// Point at `deg` degrees on a circle; 0° is to the right, angles grow clockwise on screen.
fn polar(cx: f32, cy: f32, r: f32, deg: f32) -> (f32, f32) {
    let t = deg * PI / 180.0;
    (cx + r * t.cos(), cy + r * t.sin())
}

/// Rectangle flush with the top edge, rounded bottom corners and concave
/// "flares" at the top so it melts into the screen bezel.
fn notch_path(s: &ID2D1GeometrySink, w: f32, h: f32, r: f32) {
    let (x0, x1) = (CX - w / 2.0, CX + w / 2.0);
    let r = r.min(h / 2.0).min(w / 2.0).max(0.0);
    let f = FLARE.min(h / 2.0);
    unsafe {
        s.BeginFigure(v2(x0 - f, 0.0), D2D1_FIGURE_BEGIN_FILLED);
        s.AddLine(v2(x1 + f, 0.0));
        arc(s, x1, f, f, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE);
        s.AddLine(v2(x1, h - r));
        arc(s, x1 - r, h, r, D2D1_SWEEP_DIRECTION_CLOCKWISE);
        s.AddLine(v2(x0 + r, h));
        arc(s, x0, h - r, r, D2D1_SWEEP_DIRECTION_CLOCKWISE);
        s.AddLine(v2(x0, f));
        arc(s, x0 - f, 0.0, f, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE);
        s.EndFigure(D2D1_FIGURE_END_CLOSED);
    }
}

fn draw_closed(p: &Painter, v: &View) {
    let (w, h, _) = size_for(Mode::Closed, v.look);
    if v.look.hidden {
        if let Some(color) = v.look.dot {
            p.circle(CX, h / 2.0, 4.0, color, 1.0);
        }
        return;
    }
    let (x0, x1) = (CX - w / 2.0, CX + w / 2.0);
    let mut right = x1 - 10.0;
    if let Some(color) = v.look.dot {
        p.circle(right - 4.0, h / 2.0, 4.0, color, 1.0);
        right -= 16.0;
    }
    if v.look.playing {
        p.art(v, Rect::new(x0 + 10.0, 6.0, 18.0, 18.0), 5.0);
        p.bars(Rect::new(right - 14.0, 9.0, 14.0, 12.0), v.info.accent);
    }
    let center = Rect::new(CX - 50.0, 0.0, 100.0, h);
    match (v.focus.phase(), v.focus.remaining()) {
        (Some(phase), Some(left)) => {
            let text = focus::minutes(left);
            p.text(
                &text,
                &v.fonts.clock,
                center,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                phase_color(phase),
                1.0,
            );
        }
        _ => p.text(
            v.clock,
            &v.fonts.clock,
            center,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            WHITE,
            1.0,
        ),
    }
}

fn draw_open(p: &Painter, v: &View) {
    let l = open_layout();
    let info = v.info;
    let f = v.fonts;
    p.art(v, l.art, 14.0);
    let (title, artist) = if info.active && !info.title.is_empty() {
        (info.title.as_str(), info.artist.as_str())
    } else {
        (v.text.nothing_playing, v.text.nothing_hint)
    };
    p.text(
        title,
        &f.title,
        l.title,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        1.0,
    );
    p.text(
        artist,
        &f.body,
        l.artist,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        0.55,
    );

    let on = |enabled: bool| if info.active && enabled { 1.0 } else { 0.25 };
    for (rect, hit, enabled) in [
        (l.prev, Hit::Prev, info.can_prev),
        (l.play, Hit::Play, true),
        (l.next, Hit::Next, info.can_next),
    ] {
        if v.hot == hit {
            p.circle(rect.cx(), rect.cy(), rect.w / 2.0, WHITE, 0.12);
        }
        let a = on(enabled);
        match hit {
            Hit::Prev => p.skip(rect.cx(), rect.cy(), 13.0, false, a),
            Hit::Next => p.skip(rect.cx(), rect.cy(), 13.0, true, a),
            _ if info.playing => p.pause(rect.cx(), rect.cy(), 17.0, a),
            _ => p.play(rect.cx() + 1.5, rect.cy(), 17.0, WHITE, a),
        }
    }

    // Water card: the whole card adds a glass.
    let c = l.water;
    let hot = v.hot == Hit::Drink;
    p.rrect(c, 12.0, WHITE, if hot { 0.12 } else { 0.07 });
    p.drop(c.x + 16.0, c.cy() - 8.0, 16.0, BLUE, 1.0);
    let (tx, tw) = (c.x + 30.0, c.w - 40.0);
    let amount = v.text.liters_of(v.water_ml, v.water_goal);
    p.text(
        &amount,
        &f.label,
        Rect::new(tx, c.y + 5.0, tw, 18.0),
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        1.0,
    );
    if hot {
        let glass = format!("+{} ml", v.glass);
        p.text(
            &glass,
            &f.small,
            Rect::new(tx, c.y + 23.0, tw, 14.0),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            BLUE,
            1.0,
        );
    } else {
        let frac = (v.water_ml as f32 / v.water_goal.max(1) as f32).min(1.0);
        let track = Rect::new(tx, c.y + 29.0, tw, 3.0);
        p.rrect(track, 1.5, WHITE, 0.14);
        p.rrect(Rect::new(tx, track.y, tw * frac, 3.0), 1.5, BLUE, 1.0);
    }

    // Focus card: click to start or stop. The running clock is drawn live.
    let c = l.focus;
    let hot = v.hot == Hit::Focus;
    p.rrect(c, 12.0, WHITE, if hot { 0.12 } else { 0.07 });
    let sub = match v.focus.phase() {
        None => v.text.pomodoro,
        Some(Phase::Focus) => v.text.focus,
        Some(Phase::Rest) => v.text.rest,
    };
    p.text(
        sub,
        &f.small,
        Rect::new(tx_of(c), c.y + 23.0, c.w - 40.0, 14.0),
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        0.5,
    );
    if v.focus.phase().is_none() {
        p.ring(c.x + 16.0, c.cy(), 7.0, 0.0, WHITE);
        p.play(c.x + 16.8, c.cy(), 6.0, WHITE, 0.8);
        let length = format!("{:02}:00", v.focus_min);
        let r = Rect::new(tx_of(c), c.y + 5.0, c.w - 40.0, 18.0);
        p.text(
            &length,
            &f.label,
            r,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            1.0,
        );
    }

    // Battery card (date card on desktops).
    let c = l.battery;
    p.rrect(c, 12.0, WHITE, 0.07);
    if v.power.has_battery {
        p.battery(Rect::new(c.x + 11.0, c.cy() - 5.5, 22.0, 11.0), v.power);
        let (tx, tw) = (c.x + 41.0, c.w - 48.0);
        let pct = format!("{}%", v.power.percent);
        p.text(
            &pct,
            &f.label,
            Rect::new(tx, c.y + 5.0, tw, 18.0),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            1.0,
        );
        let status = power_status(v.text, v.power);
        p.text(
            status,
            &f.small,
            Rect::new(tx, c.y + 23.0, tw, 14.0),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            0.5,
        );
    } else {
        let (tx, tw) = (c.x + 12.0, c.w - 20.0);
        p.text(
            v.date,
            &f.label,
            Rect::new(tx, c.y + 5.0, tw, 18.0),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            1.0,
        );
        p.text(
            v.clock,
            &f.small,
            Rect::new(tx, c.y + 23.0, tw, 14.0),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            0.5,
        );
    }
}

fn tx_of(card: Rect) -> f32 {
    card.x + 30.0
}

fn draw_open_live(p: &Painter, v: &View) {
    let l = open_layout();
    draw_progress(p, v, &l);
    if let Some((level, muted)) = v.volume {
        if v.hot == Hit::Mute {
            p.circle(l.mute.cx(), l.mute.cy(), 11.0, WHITE, 0.12);
        }
        p.speaker(l.mute.cx() - 7.0, l.mute.cy(), level, muted, 0.9);
        let mut bar = l.volume;
        if v.hot == Hit::Volume {
            bar = Rect::new(bar.x, bar.y - 1.5, bar.w, bar.h + 3.0);
        }
        p.level_bar(bar, if muted { 0.0 } else { level }, WHITE);
        let pct = format!("{}%", (level * 100.0).round());
        p.text(
            &pct,
            &v.fonts.small,
            l.volume_text,
            DWRITE_TEXT_ALIGNMENT_TRAILING,
            WHITE,
            0.5,
        );
    }
    if let (Some(phase), Some(left)) = (v.focus.phase(), v.focus.remaining()) {
        let c = l.focus;
        p.ring(
            c.x + 16.0,
            c.cy(),
            7.0,
            v.focus.progress(),
            phase_color(phase),
        );
        let r = Rect::new(tx_of(c), c.y + 5.0, c.w - 40.0, 18.0);
        p.text(
            &focus::clock(left),
            &v.fonts.label,
            r,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            1.0,
        );
    }
}

fn draw_progress(p: &Painter, v: &View, l: &OpenLayout) {
    let info = v.info;
    if !info.active || info.duration <= 0.0 {
        return;
    }
    let pos = info.position_now();
    let mut bar = l.bar;
    if v.hot == Hit::Seek {
        bar = Rect::new(bar.x, bar.y - 1.5, bar.w, bar.h + 3.0);
    }
    p.rrect(bar, bar.h / 2.0, WHITE, 0.16);
    let done = (pos / info.duration).clamp(0.0, 1.0) as f32;
    let fill = Rect::new(bar.x, bar.y, bar.w * done, bar.h);
    p.rrect(fill, bar.h / 2.0, info.accent, 0.95);
    let f = &v.fonts.small;
    p.text(
        &time(pos),
        f,
        l.times,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        0.5,
    );
    let left = format!("-{}", time(info.duration - pos));
    p.text(
        &left,
        f,
        l.times,
        DWRITE_TEXT_ALIGNMENT_TRAILING,
        WHITE,
        0.5,
    );
}

fn power_status(t: &Text, p: Power) -> &'static str {
    match (p.on_ac, p.charging) {
        (_, true) => t.charging,
        (true, false) => t.plugged_in,
        (false, false) => t.on_battery,
    }
}

fn act_bounds(v: &View) -> (f32, f32, f32) {
    let (w, h, _) = size_for(Mode::Activity(v.act), v.look);
    (CX - w / 2.0, CX + w / 2.0, h)
}

fn draw_activity(p: &Painter, v: &View) {
    let (x0, x1, h) = act_bounds(v);
    let f = v.fonts;
    let full = Rect::new(x0, 0.0, x1 - x0, h);
    match v.act {
        Act::Power => {
            p.battery(Rect::new(x0 + 18.0, h / 2.0 - 6.0, 24.0, 12.0), v.power);
            let (label, color) = match v.power_event {
                PowerEvent::Plugged => (
                    power_status(v.text, v.power),
                    if v.power.charging { GREEN } else { WHITE },
                ),
                PowerEvent::Unplugged => (v.text.unplugged, WHITE),
                PowerEvent::Low => (v.text.battery_low, RED),
                PowerEvent::Full => (v.text.battery_full, GREEN),
            };
            let r = Rect::new(x0 + 54.0, 0.0, 170.0, h);
            p.text(
                label,
                &f.label,
                r,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                WHITE,
                1.0,
            );
            let pct = format!("{}%", v.power.percent);
            let r = Rect::new(x1 - 18.0 - 70.0, 0.0, 70.0, h);
            p.text(
                &pct,
                &f.label,
                r,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                color,
                1.0,
            );
        }
        Act::Track => {
            p.art(v, Rect::new(x0 + 9.0, 6.0, 28.0, 28.0), 7.0);
            let tw = x1 - 40.0 - (x0 + 46.0);
            let title = Rect::new(x0 + 46.0, 4.0, tw, 18.0);
            p.text(
                &v.info.title,
                &f.label,
                title,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                WHITE,
                1.0,
            );
            let artist = Rect::new(x0 + 46.0, 21.0, tw, 15.0);
            p.text(
                &v.info.artist,
                &f.small,
                artist,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                WHITE,
                0.55,
            );
            p.bars(Rect::new(x1 - 14.0 - 16.0, 13.0, 16.0, 14.0), v.info.accent);
        }
        Act::Water => {
            p.drop(x0 + 30.0, 17.0, 30.0, BLUE, 1.0);
            let b = water_button();
            let tw = b.x - 10.0 - (x0 + 58.0);
            let title = Rect::new(x0 + 58.0, 10.0, tw, 22.0);
            p.text(
                v.text.water_title,
                &f.title,
                title,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                WHITE,
                1.0,
            );
            let sub = if v.water_ml >= v.water_goal {
                v.text.water_done.to_string()
            } else {
                format!(
                    "{} / {} ml {}",
                    v.water_ml, v.water_goal, v.text.water_today
                )
            };
            let r = Rect::new(x0 + 58.0, 33.0, tw, 18.0);
            p.text(&sub, &f.body, r, DWRITE_TEXT_ALIGNMENT_LEADING, WHITE, 0.55);
            let hot = v.hot == Hit::Drink;
            p.rrect(b, b.h / 2.0, BLUE, if hot { 1.0 } else { 0.88 });
            let label = format!("+{} ml", v.glass);
            p.text(
                &label,
                &f.label,
                b,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                0x00151C,
                1.0,
            );
        }
        Act::Device => {
            p.headphones(x0 + 24.0, h / 2.0, 0.9);
            let tw = x1 - 16.0 - (x0 + 46.0);
            let name = Rect::new(x0 + 46.0, 4.0, tw, 18.0);
            p.text(
                v.device,
                &f.label,
                name,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                WHITE,
                1.0,
            );
            let sub = Rect::new(x0 + 46.0, 21.0, tw, 15.0);
            p.text(
                v.text.audio_output,
                &f.small,
                sub,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                WHITE,
                0.55,
            );
        }
        Act::Privacy => {
            let (camera, app) = v.privacy;
            let (label, color) = if camera {
                (v.text.cam_in_use, CAM_COLOR)
            } else {
                (v.text.mic_in_use, MIC_COLOR)
            };
            p.circle(x0 + 22.0, h / 2.0, 4.5, color, 1.0);
            let r = Rect::new(x0 + 38.0, 0.0, 160.0, h);
            p.text(
                label,
                &f.label,
                r,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                WHITE,
                1.0,
            );
            let r = Rect::new(x1 - 16.0 - 100.0, 0.0, 100.0, h);
            p.text(
                app,
                &f.small,
                r,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                WHITE,
                0.55,
            );
        }
        Act::Focus => {
            let color = match v.focus.phase() {
                Some(Phase::Rest) => GREEN,
                _ => CORAL,
            };
            p.ring(x0 + 24.0, h / 2.0, 7.0, 1.0, color);
            let r = Rect::new(x0 + 42.0, 0.0, full.w - 58.0, h);
            p.text(
                v.focus_note,
                &f.label,
                r,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                WHITE,
                1.0,
            );
        }
        // Drawn entirely live: the level changes while the activity is on screen.
        Act::Volume | Act::Brightness => {}
    }
}

fn draw_activity_live(p: &Painter, v: &View) {
    let (x0, x1, h) = act_bounds(v);
    let (level, muted) = match v.act {
        Act::Volume => v.volume.unwrap_or((0.0, false)),
        Act::Brightness => (v.brightness as f32 / 100.0, false),
        _ => return,
    };
    if v.act == Act::Volume {
        p.speaker(x0 + 16.0, h / 2.0, level, muted, 1.0);
    } else {
        p.sun(x0 + 24.0, h / 2.0, 1.0);
    }
    let bar = Rect::new(
        x0 + 46.0,
        h / 2.0 - 2.0,
        x1 - 16.0 - 46.0 - (x0 + 46.0),
        4.0,
    );
    p.level_bar(bar, if muted { 0.0 } else { level }, WHITE);
    let pct = format!("{}%", (level * 100.0).round());
    let r = Rect::new(x1 - 16.0 - 40.0, 0.0, 40.0, h);
    p.text(
        &pct,
        &v.fonts.label,
        r,
        DWRITE_TEXT_ALIGNMENT_TRAILING,
        WHITE,
        1.0,
    );
}

fn time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Small drawing helpers with a shared opacity, so a whole view fades as one.
struct Painter<'a> {
    g: &'a Gfx,
    alpha: f32,
}

impl Painter<'_> {
    fn color(&self, rgb: u32, a: f32) {
        let c = D2D1_COLOR_F {
            r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
            g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
            b: (rgb & 0xFF) as f32 / 255.0,
            a: a * self.alpha,
        };
        unsafe { self.g.brush.SetColor(&c) };
    }

    fn geometry(&self, build: impl FnOnce(&ID2D1GeometrySink)) -> Option<ID2D1PathGeometry1> {
        unsafe {
            let geo = self.g.factory.CreatePathGeometry().ok()?;
            let sink = geo.Open().ok()?;
            build(&sink);
            sink.Close().ok()?;
            Some(geo)
        }
    }

    fn fill(&self, geo: Option<ID2D1PathGeometry1>, rgb: u32, a: f32) {
        if let Some(geo) = geo {
            self.color(rgb, a);
            unsafe { self.g.ctx.FillGeometry(&geo, &self.g.brush, None) };
        }
    }

    fn stroke(&self, geo: Option<ID2D1PathGeometry1>, width: f32, rgb: u32, a: f32) {
        if let Some(geo) = geo {
            self.color(rgb, a);
            unsafe {
                self.g
                    .ctx
                    .DrawGeometry(&geo, &self.g.brush, width, &self.g.round)
            };
        }
    }

    fn line(&self, from: (f32, f32), to: (f32, f32), width: f32, rgb: u32, a: f32) {
        self.color(rgb, a);
        let (p0, p1) = (v2(from.0, from.1), v2(to.0, to.1));
        unsafe {
            self.g
                .ctx
                .DrawLine(p0, p1, &self.g.brush, width, &self.g.round)
        };
    }

    fn polygon(&self, pts: &[(f32, f32)], rgb: u32, a: f32) {
        let geo = self.geometry(|s| unsafe {
            s.BeginFigure(v2(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_FILLED);
            for &(x, y) in &pts[1..] {
                s.AddLine(v2(x, y));
            }
            s.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        self.fill(geo, rgb, a);
    }

    fn rrect(&self, r: Rect, radius: f32, rgb: u32, a: f32) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        self.color(rgb, a);
        let rr = D2D1_ROUNDED_RECT {
            rect: r.d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.g.ctx.FillRoundedRectangle(&rr, &self.g.brush) };
    }

    fn circle(&self, cx: f32, cy: f32, radius: f32, rgb: u32, a: f32) {
        self.color(rgb, a);
        let e = D2D1_ELLIPSE {
            point: v2(cx, cy),
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.g.ctx.FillEllipse(&e, &self.g.brush) };
    }

    fn text(
        &self,
        s: &str,
        f: &IDWriteTextFormat,
        r: Rect,
        align: DWRITE_TEXT_ALIGNMENT,
        rgb: u32,
        a: f32,
    ) {
        if s.is_empty() {
            return;
        }
        let wide: Vec<u16> = s.encode_utf16().collect();
        self.color(rgb, a);
        unsafe {
            let _ = f.SetTextAlignment(align);
            self.g.ctx.DrawText(
                &wide,
                f,
                &r.d2d(),
                &self.g.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    fn art(&self, v: &View, r: Rect, radius: f32) {
        let rr = D2D1_ROUNDED_RECT {
            rect: r.d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        match v.art {
            Some(brush) if v.info.active => {
                let s = r.w / ART_SIZE as f32;
                let m = Matrix3x2 {
                    M11: s,
                    M12: 0.0,
                    M21: 0.0,
                    M22: s,
                    M31: r.x,
                    M32: r.y,
                };
                unsafe {
                    brush.SetTransform(&m);
                    brush.SetOpacity(self.alpha);
                    self.g.ctx.FillRoundedRectangle(&rr, brush);
                }
            }
            _ => {
                self.rrect(r, radius, WHITE, 0.10);
                self.note(r.cx(), r.cy(), r.h * 0.42, 0.55);
            }
        }
    }

    /// Eighth note, for the empty album-art placeholder.
    fn note(&self, cx: f32, cy: f32, size: f32, a: f32) {
        let head = size * 0.28;
        let hx = cx - size * 0.12;
        let hy = cy + size * 0.5 - head;
        self.circle(hx, hy, head, WHITE, a);
        let stem = Rect::new(
            hx + head - size * 0.1,
            cy - size * 0.5,
            size * 0.1,
            size - head,
        );
        self.rrect(stem, 0.0, WHITE, a);
        let x = stem.right();
        self.polygon(
            &[
                (x - 0.5, cy - size * 0.5),
                (x + size * 0.32, cy - size * 0.25),
                (x + size * 0.32, cy - size * 0.08),
                (x - 0.5, cy - size * 0.3),
            ],
            WHITE,
            a,
        );
    }

    fn bars(&self, r: Rect, rgb: u32) {
        let bw = r.w / 4.4;
        let gap = (r.w - 3.0 * bw) / 2.0;
        for (i, k) in [0.55, 1.0, 0.72].iter().enumerate() {
            let h = r.h * k;
            let bar = Rect::new(r.x + i as f32 * (bw + gap), r.y + (r.h - h) / 2.0, bw, h);
            self.rrect(bar, bw / 2.0, rgb, 1.0);
        }
    }

    fn level_bar(&self, bar: Rect, level: f32, rgb: u32) {
        self.rrect(bar, bar.h / 2.0, WHITE, 0.16);
        let fill = Rect::new(bar.x, bar.y, bar.w * level.clamp(0.0, 1.0), bar.h);
        self.rrect(fill, bar.h / 2.0, rgb, 0.95);
    }

    fn play(&self, cx: f32, cy: f32, size: f32, rgb: u32, a: f32) {
        let (h, w) = (size / 2.0, size * 0.42);
        self.polygon(
            &[(cx - w, cy - h), (cx + w * 1.1, cy), (cx - w, cy + h)],
            rgb,
            a,
        );
    }

    fn pause(&self, cx: f32, cy: f32, size: f32, a: f32) {
        let bw = size * 0.27;
        for x in [cx - bw * 1.25, cx + bw * 0.25] {
            self.rrect(Rect::new(x, cy - size / 2.0, bw, size), 1.5, WHITE, a);
        }
    }

    fn skip(&self, cx: f32, cy: f32, size: f32, forward: bool, a: f32) {
        let s = if forward { 1.0 } else { -1.0 };
        let h = size / 2.0;
        self.polygon(
            &[
                (cx - s * h * 0.8, cy - h),
                (cx + s * h * 0.55, cy),
                (cx - s * h * 0.8, cy + h),
            ],
            WHITE,
            a,
        );
        let bx = if forward {
            cx + h * 0.55
        } else {
            cx - h * 0.55 - 2.6
        };
        self.rrect(Rect::new(bx, cy - h, 2.6, size), 1.0, WHITE, a);
    }

    /// Speaker with zero, one or two sound waves depending on the level; a cross when muted.
    fn speaker(&self, x: f32, cy: f32, level: f32, muted: bool, a: f32) {
        self.polygon(
            &[
                (x, cy - 3.0),
                (x + 3.5, cy - 3.0),
                (x + 8.0, cy - 7.0),
                (x + 8.0, cy + 7.0),
                (x + 3.5, cy + 3.0),
                (x, cy + 3.0),
            ],
            WHITE,
            a,
        );
        if muted || level <= 0.001 {
            self.line((x + 11.0, cy - 3.0), (x + 16.0, cy + 3.0), 1.6, WHITE, a);
            self.line((x + 16.0, cy - 3.0), (x + 11.0, cy + 3.0), 1.6, WHITE, a);
            return;
        }
        let (cx, waves) = (x + 9.0, if level > 0.5 { 2 } else { 1 });
        for r in [4.0, 7.5].into_iter().take(waves) {
            let start = polar(cx, cy, r, -45.0);
            let end = polar(cx, cy, r, 45.0);
            let geo = self.geometry(|s| unsafe {
                s.BeginFigure(v2(start.0, start.1), D2D1_FIGURE_BEGIN_HOLLOW);
                arc(s, end.0, end.1, r, D2D1_SWEEP_DIRECTION_CLOCKWISE);
                s.EndFigure(D2D1_FIGURE_END_OPEN);
            });
            self.stroke(geo, 1.6, WHITE, a);
        }
    }

    fn sun(&self, cx: f32, cy: f32, a: f32) {
        self.circle(cx, cy, 3.4, WHITE, a);
        for i in 0..8 {
            let deg = i as f32 * 45.0;
            self.line(
                polar(cx, cy, 5.6, deg),
                polar(cx, cy, 7.6, deg),
                1.5,
                WHITE,
                a,
            );
        }
    }

    fn headphones(&self, cx: f32, cy: f32, a: f32) {
        let r = 6.5;
        let base = cy + 1.0;
        let geo = self.geometry(|s| unsafe {
            s.BeginFigure(v2(cx - r, base), D2D1_FIGURE_BEGIN_HOLLOW);
            arc(s, cx, base - r, r, D2D1_SWEEP_DIRECTION_CLOCKWISE);
            arc(s, cx + r, base, r, D2D1_SWEEP_DIRECTION_CLOCKWISE);
            s.EndFigure(D2D1_FIGURE_END_OPEN);
        });
        self.stroke(geo, 1.8, WHITE, a);
        self.rrect(Rect::new(cx - r - 1.6, base, 3.6, 6.5), 1.5, WHITE, a);
        self.rrect(Rect::new(cx + r - 2.0, base, 3.6, 6.5), 1.5, WHITE, a);
    }

    /// Progress ring starting at 12 o'clock, clockwise.
    fn ring(&self, cx: f32, cy: f32, r: f32, progress: f32, rgb: u32) {
        let e = D2D1_ELLIPSE {
            point: v2(cx, cy),
            radiusX: r,
            radiusY: r,
        };
        self.color(WHITE, 0.22);
        unsafe {
            self.g
                .ctx
                .DrawEllipse(&e, &self.g.brush, 2.0, &self.g.round)
        };
        let progress = progress.clamp(0.0, 1.0);
        if progress >= 0.999 {
            self.color(rgb, 1.0);
            unsafe {
                self.g
                    .ctx
                    .DrawEllipse(&e, &self.g.brush, 2.0, &self.g.round)
            };
        } else if progress > 0.001 {
            let start = polar(cx, cy, r, -90.0);
            let end = polar(cx, cy, r, -90.0 + 360.0 * progress);
            let size = if progress > 0.5 {
                D2D1_ARC_SIZE_LARGE
            } else {
                D2D1_ARC_SIZE_SMALL
            };
            let geo = self.geometry(|s| unsafe {
                s.BeginFigure(v2(start.0, start.1), D2D1_FIGURE_BEGIN_HOLLOW);
                arc_to(s, end.0, end.1, r, D2D1_SWEEP_DIRECTION_CLOCKWISE, size);
                s.EndFigure(D2D1_FIGURE_END_OPEN);
            });
            self.stroke(geo, 2.0, rgb, 1.0);
        }
    }

    fn battery(&self, r: Rect, p: Power) {
        self.color(WHITE, 0.45);
        let rr = D2D1_ROUNDED_RECT {
            rect: r.d2d(),
            radiusX: 3.5,
            radiusY: 3.5,
        };
        unsafe {
            self.g
                .ctx
                .DrawRoundedRectangle(&rr, &self.g.brush, 1.2, None::<&ID2D1StrokeStyle>)
        };
        self.rrect(
            Rect::new(r.right() + 1.0, r.cy() - 2.0, 2.0, 4.0),
            1.0,
            WHITE,
            0.45,
        );
        let color = if p.charging {
            GREEN
        } else if p.percent <= 20 {
            RED
        } else {
            WHITE
        };
        let inner = Rect::new(
            r.x + 2.0,
            r.y + 2.0,
            (r.w - 4.0) * p.percent as f32 / 100.0,
            r.h - 4.0,
        );
        self.rrect(inner, 1.8, color, 1.0);
        if p.charging {
            // Lightning bolt over the fill.
            let (bx, by, bw, bh) = (r.cx() - 4.0, r.y - 1.0, 8.0, r.h + 2.0);
            let pts: Vec<(f32, f32)> = [
                (0.6, 0.0),
                (0.05, 0.58),
                (0.45, 0.58),
                (0.38, 1.0),
                (0.95, 0.42),
                (0.55, 0.42),
            ]
            .iter()
            .map(|&(x, y)| (bx + x * bw, by + y * bh))
            .collect();
            self.polygon(&pts, WHITE, 1.0);
        }
    }

    fn drop(&self, cx: f32, top: f32, h: f32, rgb: u32, a: f32) {
        let r = h * 0.36;
        let cy = top + h - r;
        let geo = self.geometry(|s| unsafe {
            s.BeginFigure(v2(cx, top), D2D1_FIGURE_BEGIN_FILLED);
            s.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v2(cx + r * 0.15, top + h * 0.22),
                point2: v2(cx + r, cy - r * 0.6),
                point3: v2(cx + r, cy),
            });
            arc(s, cx, cy + r, r, D2D1_SWEEP_DIRECTION_CLOCKWISE);
            arc(s, cx - r, cy, r, D2D1_SWEEP_DIRECTION_CLOCKWISE);
            s.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v2(cx - r, cy - r * 0.6),
                point2: v2(cx - r * 0.15, top + h * 0.22),
                point3: v2(cx, top),
            });
            s.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        self.fill(geo, rgb, a);
    }
}
