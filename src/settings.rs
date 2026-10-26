use std::fs;
use std::path::PathBuf;

/// User settings and today's water log, stored as `key=value` lines in
/// `%APPDATA%\Isla\settings.ini`. Unknown keys are ignored, bad values fall back to defaults.
#[derive(Clone)]
pub struct Settings {
    pub water_enabled: bool,
    pub water_interval: u32,
    pub water_goal: u32,
    pub water_glass: u32,
    pub water_start: u32,
    pub water_end: u32,
    pub water_date: String,
    pub water_ml: u32,
    /// "Alerts only": no pill at rest, the notch appears only for alerts or on hover.
    pub hidden: bool,
    /// Follow the monitor of the active window instead of the primary one.
    pub monitor_active: bool,
    pub hotkey: bool,
    pub notify_track: bool,
    pub notify_volume: bool,
    pub notify_brightness: bool,
    pub notify_audio: bool,
    pub notify_battery: bool,
    pub notify_privacy: bool,
    pub focus_min: u32,
    pub break_min: u32,
    /// Synced lyrics from LRCLIB. Off by default: it is the only feature that uses the network.
    pub lyrics: bool,
    /// Milliseconds the lyrics are shown ahead of the player's position (negative: later).
    pub lyrics_offset: i32,
    /// The open panel shows the lyrics instead of the volume row and the cards.
    pub lyrics_view: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            water_enabled: true,
            water_interval: 60,
            water_goal: 2000,
            water_glass: 250,
            water_start: 9,
            water_end: 22,
            water_date: String::new(),
            water_ml: 0,
            hidden: false,
            monitor_active: false,
            hotkey: true,
            notify_track: true,
            notify_volume: true,
            notify_brightness: true,
            notify_audio: true,
            notify_battery: true,
            notify_privacy: true,
            focus_min: 25,
            break_min: 5,
            lyrics: false,
            lyrics_offset: 0,
            lyrics_view: false,
        }
    }
}

fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("Isla").join("settings.ini"))
}

impl Settings {
    pub fn load() -> Self {
        let mut s = Self::default();
        let Some(text) = path().and_then(|p| fs::read_to_string(p).ok()) else {
            return s;
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            let num = |lo: u32, hi: u32, fallback: u32| {
                value
                    .parse::<u32>()
                    .ok()
                    .filter(|v| (lo..=hi).contains(v))
                    .unwrap_or(fallback)
            };
            let on = value != "0";
            match key.trim() {
                "water_enabled" => s.water_enabled = on,
                "water_interval" => s.water_interval = num(15, 240, s.water_interval),
                "water_goal" => s.water_goal = num(500, 6000, s.water_goal),
                "water_glass" => s.water_glass = num(50, 1500, s.water_glass),
                "water_start" => s.water_start = num(0, 23, s.water_start),
                "water_end" => s.water_end = num(1, 24, s.water_end),
                "water_date" => s.water_date = value.to_string(),
                "water_ml" => s.water_ml = num(0, 20000, 0),
                "hidden" => s.hidden = on,
                "monitor" => s.monitor_active = value == "active",
                "hotkey" => s.hotkey = on,
                "notify_track" => s.notify_track = on,
                "notify_volume" => s.notify_volume = on,
                "notify_brightness" => s.notify_brightness = on,
                "notify_audio" => s.notify_audio = on,
                "notify_battery" => s.notify_battery = on,
                "notify_privacy" => s.notify_privacy = on,
                "focus_min" => s.focus_min = num(1, 120, s.focus_min),
                "break_min" => s.break_min = num(1, 60, s.break_min),
                "lyrics" => s.lyrics = on,
                "lyrics_offset" => {
                    s.lyrics_offset = value
                        .parse::<i32>()
                        .ok()
                        .filter(|v| (-5000..=5000).contains(v))
                        .unwrap_or(0)
                }
                "lyrics_view" => s.lyrics_view = on,
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let b = |v: bool| v as u8;
        let text = [
            format!("water_enabled={}", b(self.water_enabled)),
            format!("water_interval={}", self.water_interval),
            format!("water_goal={}", self.water_goal),
            format!("water_glass={}", self.water_glass),
            format!("water_start={}", self.water_start),
            format!("water_end={}", self.water_end),
            format!("water_date={}", self.water_date),
            format!("water_ml={}", self.water_ml),
            format!("hidden={}", b(self.hidden)),
            format!(
                "monitor={}",
                if self.monitor_active {
                    "active"
                } else {
                    "primary"
                }
            ),
            format!("hotkey={}", b(self.hotkey)),
            format!("notify_track={}", b(self.notify_track)),
            format!("notify_volume={}", b(self.notify_volume)),
            format!("notify_brightness={}", b(self.notify_brightness)),
            format!("notify_audio={}", b(self.notify_audio)),
            format!("notify_battery={}", b(self.notify_battery)),
            format!("notify_privacy={}", b(self.notify_privacy)),
            format!("focus_min={}", self.focus_min),
            format!("break_min={}", self.break_min),
            format!("lyrics={}", b(self.lyrics)),
            format!("lyrics_offset={}", self.lyrics_offset),
            format!("lyrics_view={}", b(self.lyrics_view)),
            String::new(),
        ]
        .join("\n");
        // Write-then-rename so a crash never leaves a half-written file.
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}
