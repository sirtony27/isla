//! Layout, drawing and hit-testing of every view. All coordinates are DIPs
//! relative to the window; the notch is anchored to the top center.

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1BitmapBrush1, ID2D1GeometrySink, ID2D1PathGeometry1, ID2D1StrokeStyle,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_ROUNDED_RECT,
    D2D1_SWEEP_DIRECTION, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::gfx::{Fonts, Gfx};
use crate::i18n::Text;
use crate::media::{Info, ART_SIZE};
use crate::sys::Power;

/// The window is fixed at this size; the notch animates inside it and the
/// window region follows the notch so clicks elsewhere pass through.
pub const WIN_W: f32 = 460.0;
pub const WIN_H: f32 = 196.0;
pub const CX: f32 = WIN_W / 2.0;
/// Concave curve joining the notch to the top edge of the screen.
pub const FLARE: f32 = 8.0;

const WHITE: u32 = 0xFFFFFF;
const GREEN: u32 = 0x34C759;
const RED: u32 = 0xFF453A;
const BLUE: u32 = 0x5AC8FA;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Hit {
    #[default]
    None,
    Prev,
    Play,
    Next,
    Seek,
    Drink,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Power,
    Track,
    Water,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Closed,
    Open,
    Activity(Act),
}

/// Target (width, height, corner radius) of the notch for a mode.
pub fn size_for(mode: Mode, playing: bool) -> (f32, f32, f32) {
    match mode {
        Mode::Closed if playing => (220.0, 30.0, 12.0),
        Mode::Closed => (180.0, 30.0, 12.0),
        Mode::Open => (420.0, 176.0, 30.0),
        Mode::Activity(Act::Power) => (300.0, 36.0, 15.0),
        Mode::Activity(Act::Track) => (340.0, 40.0, 17.0),
        Mode::Activity(Act::Water) => (380.0, 64.0, 24.0),
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
    water: Rect,
    drink: Rect,
    battery: Rect,
}

fn open_layout() -> OpenLayout {
    let (w, _, _) = size_for(Mode::Open, false);
    let (x0, x1, pad) = (CX - w / 2.0, CX + w / 2.0, 18.0);
    let art = Rect::new(x0 + pad, 16.0, 64.0, 64.0);
    let cy = art.cy();
    let next = Rect::new(x1 - pad - 34.0, cy - 17.0, 34.0, 34.0);
    let play = Rect::new(next.x - 4.0 - 40.0, cy - 20.0, 40.0, 40.0);
    let prev = Rect::new(play.x - 4.0 - 34.0, cy - 17.0, 34.0, 34.0);
    let tx = art.right() + 14.0;
    let tw = prev.x - 10.0 - tx;
    let bar = Rect::new(x0 + pad, 94.0, w - 2.0 * pad, 4.0);
    let card_w = (w - 2.0 * pad - 10.0) / 2.0;
    let water = Rect::new(x0 + pad, 124.0, card_w, 38.0);
    OpenLayout {
        art,
        title: Rect::new(tx, art.y + 9.0, tw, 22.0),
        artist: Rect::new(tx, art.y + 32.0, tw, 20.0),
        prev,
        play,
        next,
        bar,
        times: Rect::new(bar.x, 101.0, bar.w, 14.0),
        water,
        drink: Rect::new(water.right() - 8.0 - 58.0, water.y + 7.0, 58.0, 24.0),
        battery: Rect::new(water.right() + 10.0, water.y, card_w, water.h),
    }
}

fn water_button() -> Rect {
    let (w, _, _) = size_for(Mode::Activity(Act::Water), false);
    Rect::new(CX + w / 2.0 - 16.0 - 88.0, 17.0, 88.0, 30.0)
}

pub fn hit(mode: Mode, x: f32, y: f32, info: &Info) -> Hit {
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
            if l.drink.contains(x, y) {
                return Hit::Drink;
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

/// Everything a frame needs, borrowed from the app for the duration of the draw.
pub struct View<'a> {
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub a_closed: f32,
    pub a_open: f32,
    pub a_act: f32,
    pub act: Act,
    pub playing: bool,
    pub info: &'a Info,
    pub art: Option<&'a ID2D1BitmapBrush1>,
    pub clock: &'a str,
    pub date: &'a str,
    pub power: Power,
    pub water_ml: u32,
    pub water_goal: u32,
    pub glass: u32,
    pub hot: Hit,
    pub text: &'a Text,
    pub fonts: &'a Fonts,
}

/// Each view (closed, open, activity) is drawn once into its own cached
/// bitmap and only redrawn when its content changes. Animation frames just
/// compose those bitmaps, which keeps each frame cheap.
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
    let (w, h, _) = size_for(mode, v.playing);
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
        let act = if kind == OPEN || kind == CLOSED {
            0
        } else {
            v.act as u8 + 1
        };
        let key = (ox, pw, ph, act);
        let slot = &mut layers.slots[kind];
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
                    _ => match v.act {
                        Act::Power => draw_power(&p, v),
                        Act::Track => draw_track(&p, v),
                        Act::Water => draw_water(&p, v),
                    },
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
    if let Some(shape) = p.geometry(|s| notch_path(s, v.w, v.h, v.r)) {
        p.color(0x000000, 1.0);
        unsafe { g.ctx.FillGeometry(&shape, &g.brush, None) };
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
        if kind == OPEN {
            // The progress bar changes every second; it is drawn live on top.
            draw_progress(&Painter { g, alpha }, v);
        }
        unsafe { g.ctx.SetTransform(&Matrix3x2::identity()) };
    }
    unsafe { g.ctx.PopAxisAlignedClip() };
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn arc(s: &ID2D1GeometrySink, x: f32, y: f32, r: f32, dir: D2D1_SWEEP_DIRECTION) {
    unsafe {
        s.AddArc(&D2D1_ARC_SEGMENT {
            point: v2(x, y),
            size: D2D_SIZE_F {
                width: r,
                height: r,
            },
            rotationAngle: 0.0,
            sweepDirection: dir,
            arcSize: D2D1_ARC_SIZE_SMALL,
        });
    }
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
    let (w, h, _) = size_for(Mode::Closed, v.playing);
    let x0 = CX - w / 2.0;
    if v.playing {
        p.art(v, Rect::new(x0 + 10.0, 6.0, 18.0, 18.0), 5.0);
        p.bars(
            Rect::new(x0 + w - 10.0 - 14.0, 9.0, 14.0, 12.0),
            v.info.accent,
        );
    }
    p.text(
        v.clock,
        &v.fonts.clock,
        Rect::new(CX - 50.0, 0.0, 100.0, h),
        DWRITE_TEXT_ALIGNMENT_CENTER,
        WHITE,
        1.0,
    );
}

fn draw_open(p: &Painter, v: &View) {
    let l = open_layout();
    let info = v.info;
    p.art(v, l.art, 14.0);
    let (title, artist) = if info.active && !info.title.is_empty() {
        (info.title.as_str(), info.artist.as_str())
    } else {
        (v.text.nothing_playing, v.text.nothing_hint)
    };
    p.text(
        title,
        &v.fonts.title,
        l.title,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        1.0,
    );
    p.text(
        artist,
        &v.fonts.body,
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
            _ => p.play(rect.cx() + 1.5, rect.cy(), 17.0, a),
        }
    }

    // Water card.
    let c = l.water;
    p.rrect(c, 12.0, WHITE, 0.07);
    p.drop(c.x + 18.0, c.cy() - 8.5, 17.0, BLUE, 1.0);
    let tx = c.x + 34.0;
    let tw = l.drink.x - 8.0 - tx;
    let label = format!("{} / {} ml", v.water_ml, v.water_goal);
    p.text(
        &label,
        &v.fonts.label,
        Rect::new(tx, c.y + 4.0, tw, 18.0),
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        1.0,
    );
    let frac = (v.water_ml as f32 / v.water_goal.max(1) as f32).min(1.0);
    let track = Rect::new(tx, c.y + 25.0, tw, 4.0);
    p.rrect(track, 2.0, WHITE, 0.14);
    p.rrect(Rect::new(tx, track.y, tw * frac, 4.0), 2.0, BLUE, 1.0);
    let hot = v.hot == Hit::Drink;
    p.rrect(l.drink, 12.0, BLUE, if hot { 0.38 } else { 0.22 });
    let glass = format!("+{}", v.glass);
    p.text(
        &glass,
        &v.fonts.label,
        l.drink,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        BLUE,
        1.0,
    );

    // Battery card (date card on desktops).
    let c = l.battery;
    p.rrect(c, 12.0, WHITE, 0.07);
    let tx = c.x + 50.0;
    let tw = c.right() - 10.0 - tx;
    if v.power.has_battery {
        p.battery(Rect::new(c.x + 14.0, c.cy() - 6.0, 24.0, 12.0), v.power);
        let pct = format!("{}%", v.power.percent);
        p.text(
            &pct,
            &v.fonts.label,
            Rect::new(tx, c.y + 3.0, tw, 18.0),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            1.0,
        );
        let status = power_status(v.text, v.power);
        p.text(
            status,
            &v.fonts.small,
            Rect::new(tx, c.y + 20.0, tw, 14.0),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            0.5,
        );
    } else {
        let r = Rect::new(c.x + 14.0, c.y + 3.0, c.w - 28.0, 18.0);
        p.text(
            v.date,
            &v.fonts.label,
            r,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            1.0,
        );
        let r = Rect::new(c.x + 14.0, c.y + 20.0, c.w - 28.0, 14.0);
        p.text(
            v.clock,
            &v.fonts.small,
            r,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            WHITE,
            0.5,
        );
    }
}

fn draw_progress(p: &Painter, v: &View) {
    let info = v.info;
    if !info.active || info.duration <= 0.0 {
        return;
    }
    let l = open_layout();
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

fn draw_power(p: &Painter, v: &View) {
    let (w, h, _) = size_for(Mode::Activity(Act::Power), false);
    let (x0, x1) = (CX - w / 2.0, CX + w / 2.0);
    p.battery(Rect::new(x0 + 18.0, h / 2.0 - 6.0, 24.0, 12.0), v.power);
    let label = if v.power.on_ac {
        power_status(v.text, v.power)
    } else {
        v.text.unplugged
    };
    p.text(
        label,
        &v.fonts.label,
        Rect::new(x0 + 54.0, 0.0, 150.0, h),
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        1.0,
    );
    let color = if v.power.charging { GREEN } else { WHITE };
    let pct = format!("{}%", v.power.percent);
    let r = Rect::new(x1 - 18.0 - 70.0, 0.0, 70.0, h);
    p.text(
        &pct,
        &v.fonts.label,
        r,
        DWRITE_TEXT_ALIGNMENT_TRAILING,
        color,
        1.0,
    );
}

fn draw_track(p: &Painter, v: &View) {
    let (w, _, _) = size_for(Mode::Activity(Act::Track), false);
    let (x0, x1) = (CX - w / 2.0, CX + w / 2.0);
    p.art(v, Rect::new(x0 + 9.0, 6.0, 28.0, 28.0), 7.0);
    let tw = x1 - 40.0 - (x0 + 46.0);
    let title = Rect::new(x0 + 46.0, 4.0, tw, 18.0);
    p.text(
        &v.info.title,
        &v.fonts.label,
        title,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        1.0,
    );
    let artist = Rect::new(x0 + 46.0, 21.0, tw, 15.0);
    p.text(
        &v.info.artist,
        &v.fonts.small,
        artist,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        0.55,
    );
    p.bars(Rect::new(x1 - 14.0 - 16.0, 13.0, 16.0, 14.0), v.info.accent);
}

fn draw_water(p: &Painter, v: &View) {
    let (w, _, _) = size_for(Mode::Activity(Act::Water), false);
    let x0 = CX - w / 2.0;
    p.drop(x0 + 30.0, 17.0, 30.0, BLUE, 1.0);
    let b = water_button();
    let tw = b.x - 10.0 - (x0 + 58.0);
    let title = Rect::new(x0 + 58.0, 10.0, tw, 22.0);
    p.text(
        v.text.water_title,
        &v.fonts.title,
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
    p.text(
        &sub,
        &v.fonts.body,
        r,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        WHITE,
        0.55,
    );
    let hot = v.hot == Hit::Drink;
    p.rrect(b, b.h / 2.0, BLUE, if hot { 1.0 } else { 0.88 });
    let label = format!("+{} ml", v.glass);
    p.text(
        &label,
        &v.fonts.label,
        b,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        0x00151C,
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
            unsafe {
                self.g.ctx.FillGeometry(&geo, &self.g.brush, None);
            }
        }
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

    fn play(&self, cx: f32, cy: f32, size: f32, a: f32) {
        let (h, w) = (size / 2.0, size * 0.42);
        self.polygon(
            &[(cx - w, cy - h), (cx + w * 1.1, cy), (cx - w, cy + h)],
            WHITE,
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
            // Lightning bolt with a black outline so it reads over the green fill.
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
