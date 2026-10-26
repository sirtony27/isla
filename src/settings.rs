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
            match key.trim() {
                "water_enabled" => s.water_enabled = value != "0",
                "water_interval" => s.water_interval = num(15, 240, s.water_interval),
                "water_goal" => s.water_goal = num(500, 6000, s.water_goal),
                "water_glass" => s.water_glass = num(50, 1500, s.water_glass),
                "water_start" => s.water_start = num(0, 23, s.water_start),
                "water_end" => s.water_end = num(1, 24, s.water_end),
                "water_date" => s.water_date = value.to_string(),
                "water_ml" => s.water_ml = num(0, 20000, 0),
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
        let text = format!(
            "water_enabled={}\nwater_interval={}\nwater_goal={}\nwater_glass={}\nwater_start={}\nwater_end={}\nwater_date={}\nwater_ml={}\n",
            self.water_enabled as u8,
            self.water_interval,
            self.water_goal,
            self.water_glass,
            self.water_start,
            self.water_end,
            self.water_date,
            self.water_ml,
        );
        // Write-then-rename so a crash never leaves a half-written file.
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}
