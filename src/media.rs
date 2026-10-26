//! "Now playing" through Windows' System Media Transport Controls (SMTC).
//! Works with any player that integrates with the Windows media flyout:
//! Spotify, browsers (YouTube, etc.), Apple Music, Groove, VLC with plugins...
//!
//! All SMTC calls run on one background thread. Windows events only wake that
//! thread through a channel; nothing polls.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use windows::core::Result;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeMedianCut,
    WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IStream, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::WinRT::CreateStreamOverRandomAccessStream;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Album art is decoded to a square of this many pixels (enough for 64 DIP at 200 %).
pub const ART_SIZE: u32 = 128;

/// Bits sent in WPARAM with the media message so the UI only redraws what changed.
pub const CHANGED_TRACK: usize = 1;
pub const CHANGED_PLAYBACK: usize = 2;
pub const CHANGED_TIMELINE: usize = 4;

#[derive(Clone, Default)]
pub struct Info {
    pub active: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub can_prev: bool,
    pub can_next: bool,
    /// Seconds, valid at `sampled`.
    pub position: f64,
    pub duration: f64,
    pub sampled: Option<Instant>,
    pub art_gen: u64,
    /// Average album-art color (0xRRGGBB), used as accent.
    pub accent: u32,
}

impl Info {
    /// Current playback position, extrapolated while playing.
    pub fn position_now(&self) -> f64 {
        let elapsed = match (self.playing, self.sampled) {
            (true, Some(t)) => t.elapsed().as_secs_f64(),
            _ => 0.0,
        };
        (self.position + elapsed).clamp(0.0, self.duration.max(0.0))
    }
}

#[derive(Default)]
pub struct Shared {
    pub info: Info,
    /// Premultiplied BGRA, `ART_SIZE`² pixels.
    pub art: Option<Arc<Vec<u8>>>,
}

enum Cmd {
    Session,
    Properties,
    Playback,
    Timeline,
    Toggle,
    Next,
    Prev,
    Seek(f64),
}

pub struct Media {
    pub shared: Arc<Mutex<Shared>>,
    tx: Sender<Cmd>,
}

impl Media {
    pub fn start(hwnd: HWND, message: u32) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = mpsc::channel();
        let worker_shared = shared.clone();
        let worker_tx = tx.clone();
        let hwnd = hwnd.0 as isize;
        let _ = std::thread::Builder::new()
            .name("media".into())
            .spawn(move || {
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let mut worker = Worker {
                    hwnd,
                    message,
                    shared: worker_shared,
                    tx: worker_tx,
                    manager: None,
                    session: None,
                    wic: None,
                    art_key: String::new(),
                    start_ticks: 0,
                };
                if worker.init().is_ok() {
                    worker.run(rx);
                }
            });
        Self { shared, tx }
    }

    pub fn toggle(&self) {
        let _ = self.tx.send(Cmd::Toggle);
    }
    pub fn next(&self) {
        let _ = self.tx.send(Cmd::Next);
    }
    pub fn prev(&self) {
        let _ = self.tx.send(Cmd::Prev);
    }
    pub fn seek(&self, seconds: f64) {
        let _ = self.tx.send(Cmd::Seek(seconds));
    }
}

struct Worker {
    hwnd: isize,
    message: u32,
    shared: Arc<Mutex<Shared>>,
    tx: Sender<Cmd>,
    manager: Option<Manager>,
    session: Option<(Session, [i64; 3])>,
    wic: Option<IWICImagingFactory>,
    /// Identifies the art currently decoded; empty when the last decode failed.
    art_key: String,
    start_ticks: i64,
}

fn on_event<S, A>(tx: &Sender<Cmd>, make: fn() -> Cmd) -> TypedEventHandler<S, A>
where
    S: windows::core::RuntimeType + 'static,
    A: windows::core::RuntimeType + 'static,
{
    let tx = tx.clone();
    TypedEventHandler::new(move |_, _| {
        let _ = tx.send(make());
        Ok(())
    })
}

impl Worker {
    fn init(&mut self) -> Result<()> {
        let manager = Manager::RequestAsync()?.join()?;
        manager.CurrentSessionChanged(&on_event(&self.tx, || Cmd::Session))?;
        manager.SessionsChanged(&on_event(&self.tx, || Cmd::Session))?;
        self.manager = Some(manager);
        self.rebind();
        Ok(())
    }

    fn run(&mut self, rx: Receiver<Cmd>) {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                Cmd::Session => self.rebind(),
                Cmd::Properties => {
                    let changed = self.read_properties().unwrap_or(0);
                    self.notify(changed);
                }
                Cmd::Playback => {
                    let changed = self.read_playback().unwrap_or(0);
                    self.notify(changed);
                }
                Cmd::Timeline => {
                    let changed = self.read_timeline().unwrap_or(0);
                    self.notify(changed);
                }
                Cmd::Toggle => self.control(|s| s.TryTogglePlayPauseAsync()),
                Cmd::Next => self.control(|s| s.TrySkipNextAsync()),
                Cmd::Prev => self.control(|s| s.TrySkipPreviousAsync()),
                Cmd::Seek(seconds) => {
                    let ticks = self.start_ticks + (seconds * 1e7) as i64;
                    self.control(|s| s.TryChangePlaybackPositionAsync(ticks));
                }
            }
        }
    }

    fn control(&self, f: impl Fn(&Session) -> Result<windows_future::IAsyncOperation<bool>>) {
        if let Some((session, _)) = &self.session {
            if let Ok(op) = f(session) {
                let _ = op.join();
            }
        }
    }

    fn notify(&self, changed: usize) {
        if changed != 0 {
            let hwnd = HWND(self.hwnd as _);
            let _ = unsafe { PostMessageW(Some(hwnd), self.message, WPARAM(changed), LPARAM(0)) };
        }
    }

    fn rebind(&mut self) {
        if let Some((old, tokens)) = self.session.take() {
            let _ = old.RemoveMediaPropertiesChanged(tokens[0]);
            let _ = old.RemovePlaybackInfoChanged(tokens[1]);
            let _ = old.RemoveTimelinePropertiesChanged(tokens[2]);
        }
        let session = self
            .manager
            .as_ref()
            .and_then(|m| m.GetCurrentSession().ok());
        let Some(session) = session else {
            let mut shared = self.shared.lock().unwrap();
            shared.info = Info {
                art_gen: shared.info.art_gen + 1,
                ..Info::default()
            };
            shared.art = None;
            self.art_key.clear();
            drop(shared);
            self.notify(CHANGED_TRACK | CHANGED_PLAYBACK | CHANGED_TIMELINE);
            return;
        };
        let tokens = [
            session
                .MediaPropertiesChanged(&on_event(&self.tx, || Cmd::Properties))
                .unwrap_or_default(),
            session
                .PlaybackInfoChanged(&on_event(&self.tx, || Cmd::Playback))
                .unwrap_or_default(),
            session
                .TimelinePropertiesChanged(&on_event(&self.tx, || Cmd::Timeline))
                .unwrap_or_default(),
        ];
        self.session = Some((session, tokens));
        self.shared.lock().unwrap().info.active = true;
        let changed = self.read_properties().unwrap_or(0)
            | self.read_playback().unwrap_or(0)
            | self.read_timeline().unwrap_or(0);
        self.notify(changed | CHANGED_TRACK);
    }

    fn read_properties(&mut self) -> Result<usize> {
        let Some((session, _)) = &self.session else {
            return Ok(0);
        };
        let props = session.TryGetMediaPropertiesAsync()?.join()?;
        let title = props.Title()?.to_string();
        let artist = props.Artist()?.to_string();
        let album = props
            .AlbumTitle()
            .map(|a| a.to_string())
            .unwrap_or_default();
        let key = format!(
            "{}|{title}|{artist}",
            session.SourceAppUserModelId().unwrap_or_default()
        );

        // Players often send the title first and the thumbnail a moment later,
        // so only skip decoding when the same track already has its art.
        let art = if key == self.art_key {
            None
        } else {
            let art = self.decode_art(&props);
            self.art_key = if art.is_some() { key } else { String::new() };
            Some(art)
        };

        let mut shared = self.shared.lock().unwrap();
        let changed = shared.info.title != title || shared.info.artist != artist || art.is_some();
        shared.info.title = title;
        shared.info.artist = artist;
        shared.info.album = album;
        if let Some(art) = art {
            shared.info.accent = art.as_ref().map_or(0xFFFFFF, |px| accent(px));
            shared.art = art.map(Arc::new);
            shared.info.art_gen += 1;
        }
        Ok(if changed { CHANGED_TRACK } else { 0 })
    }

    fn read_playback(&mut self) -> Result<usize> {
        let Some((session, _)) = &self.session else {
            return Ok(0);
        };
        let info = session.GetPlaybackInfo()?;
        let playing = info.PlaybackStatus()? == Status::Playing;
        let controls = info.Controls()?;
        let can_next = controls.IsNextEnabled().unwrap_or(false);
        let can_prev = controls.IsPreviousEnabled().unwrap_or(false);
        let mut shared = self.shared.lock().unwrap();
        let i = &mut shared.info;
        let changed = i.playing != playing || i.can_next != can_next || i.can_prev != can_prev;
        if i.playing != playing {
            // Freeze or resume the extrapolated position at the moment of the change.
            i.position = i.position_now();
            i.sampled = Some(Instant::now());
        }
        i.playing = playing;
        i.can_next = can_next;
        i.can_prev = can_prev;
        Ok(if changed { CHANGED_PLAYBACK } else { 0 })
    }

    fn read_timeline(&mut self) -> Result<usize> {
        let Some((session, _)) = &self.session else {
            return Ok(0);
        };
        let t = session.GetTimelineProperties()?;
        let start = t.StartTime()?.Duration;
        let end = t.EndTime()?.Duration;
        let position = t.Position()?.Duration;
        let updated = t.LastUpdatedTime()?.UniversalTime;
        self.start_ticks = start;

        let mut shared = self.shared.lock().unwrap();
        let i = &mut shared.info;
        i.duration = ((end - start) as f64 / 1e7).max(0.0);
        let mut seconds = (position - start) as f64 / 1e7;
        if i.playing {
            // SMTC reports the position as of `updated`; bring it to "now".
            let ft = unsafe { GetSystemTimeAsFileTime() };
            let now = (ft.dwHighDateTime as i64) << 32 | ft.dwLowDateTime as i64;
            let elapsed = (now - updated) as f64 / 1e7;
            if (0.0..i.duration.max(1.0)).contains(&elapsed) {
                seconds += elapsed;
            }
        }
        i.position = seconds.clamp(0.0, i.duration);
        i.sampled = Some(Instant::now());
        Ok(CHANGED_TIMELINE)
    }

    fn decode_art(&mut self, props: &Properties) -> Option<Vec<u8>> {
        let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
        unsafe {
            let istream: IStream = CreateStreamOverRandomAccessStream(&stream).ok()?;
            if self.wic.is_none() {
                self.wic =
                    CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok();
            }
            let wic = self.wic.as_ref()?;
            let decoder = wic
                .CreateDecoderFromStream(&istream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .ok()?;
            let frame = decoder.GetFrame(0).ok()?;
            let (mut w, mut h) = (0u32, 0u32);
            frame.GetSize(&mut w, &mut h).ok()?;
            let side = w.min(h);
            if side == 0 {
                return None;
            }
            // Center-crop to a square: video thumbnails are 16:9.
            let clipper = wic.CreateBitmapClipper().ok()?;
            let rect = WICRect {
                X: ((w - side) / 2) as i32,
                Y: ((h - side) / 2) as i32,
                Width: side as i32,
                Height: side as i32,
            };
            clipper.Initialize(&frame, &rect).ok()?;
            let scaler = wic.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&clipper, ART_SIZE, ART_SIZE, WICBitmapInterpolationModeFant)
                .ok()?;
            let converter = wic.CreateFormatConverter().ok()?;
            converter
                .Initialize(
                    &scaler,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeMedianCut,
                )
                .ok()?;
            let mut pixels = vec![0u8; (ART_SIZE * ART_SIZE * 4) as usize];
            converter
                .CopyPixels(std::ptr::null(), ART_SIZE * 4, &mut pixels)
                .ok()?;
            Some(pixels)
        }
    }
}

/// Average color of the art, lifted so it reads well on black.
fn accent(bgra: &[u8]) -> u32 {
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for px in bgra.as_chunks::<4>().0.iter().step_by(7) {
        b += px[0] as u64;
        g += px[1] as u64;
        r += px[2] as u64;
        n += 1;
    }
    let n = n.max(1);
    let (mut r, mut g, mut b) = ((r / n) as f32, (g / n) as f32, (b / n) as f32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    // Near-black or gray art has no hue to borrow: plain white reads best on black.
    if max < 40.0 || max - min < 12.0 {
        return 0xFFFFFF;
    }
    // Normalize brightness to a fixed level, then mix a little white in.
    let k = 235.0 / max;
    r = (r * k) * 0.8 + 255.0 * 0.2;
    g = (g * k) * 0.8 + 255.0 * 0.2;
    b = (b * k) * 0.8 + 255.0 * 0.2;
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}
