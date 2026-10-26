use windows::Win32::Globalization::GetUserDefaultUILanguage;

/// Every user-facing string. Picked once at startup from the Windows UI language.
#[derive(Clone, Copy)]
pub struct Text {
    pub nothing_playing: &'static str,
    pub nothing_hint: &'static str,
    pub charging: &'static str,
    pub plugged_in: &'static str,
    pub on_battery: &'static str,
    pub unplugged: &'static str,
    pub battery_low: &'static str,
    pub battery_full: &'static str,
    pub water_title: &'static str,
    pub water_today: &'static str,
    pub water_done: &'static str,
    pub audio_output: &'static str,
    pub mic_in_use: &'static str,
    pub cam_in_use: &'static str,
    pub pomodoro: &'static str,
    pub focus: &'static str,
    pub rest: &'static str,
    pub focus_done: &'static str,
    pub rest_done: &'static str,
    pub menu_autostart: &'static str,
    pub menu_hidden: &'static str,
    pub menu_monitor: &'static str,
    pub menu_monitor_primary: &'static str,
    pub menu_monitor_active: &'static str,
    pub menu_hotkey: &'static str,
    pub menu_alerts: &'static str,
    pub alert_track: &'static str,
    pub alert_volume: &'static str,
    pub alert_brightness: &'static str,
    pub alert_audio: &'static str,
    pub alert_battery: &'static str,
    pub alert_privacy: &'static str,
    pub menu_water: &'static str,
    pub menu_interval: &'static str,
    pub menu_goal: &'static str,
    pub menu_glass: &'static str,
    pub menu_reset: &'static str,
    pub menu_focus_start: &'static str,
    pub menu_focus_stop: &'static str,
    pub menu_focus_len: &'static str,
    pub menu_rest_len: &'static str,
    pub menu_lyrics: &'static str,
    pub menu_lyrics_timing: &'static str,
    pub lyrics_button: &'static str,
    pub lyrics_exact: &'static str,
    pub lyrics_earlier: &'static str,
    pub lyrics_later: &'static str,
    pub menu_exit: &'static str,
    pub decimal_comma: bool,
}

const EN: Text = Text {
    nothing_playing: "Nothing playing",
    nothing_hint: "Play something in Spotify, YouTube or any player",
    charging: "Charging",
    plugged_in: "Plugged in",
    on_battery: "On battery",
    unplugged: "Unplugged",
    battery_low: "Battery low",
    battery_full: "Fully charged",
    water_title: "Time to drink water",
    water_today: "today",
    water_done: "Goal reached",
    audio_output: "Audio output",
    mic_in_use: "Microphone in use",
    cam_in_use: "Camera in use",
    pomodoro: "Focus timer",
    focus: "Focus",
    rest: "Break",
    focus_done: "Time for a break",
    rest_done: "Back to focus",
    menu_autostart: "Start with Windows",
    menu_hidden: "Hide the pill (alerts only)",
    menu_monitor: "Monitor",
    menu_monitor_primary: "Primary",
    menu_monitor_active: "Where the active window is",
    menu_hotkey: "Shortcut Win+Alt+N",
    menu_alerts: "Alerts",
    alert_track: "New song",
    alert_volume: "Volume",
    alert_brightness: "Brightness",
    alert_audio: "Audio output",
    alert_battery: "Battery",
    alert_privacy: "Microphone and camera",
    menu_water: "Water reminder",
    menu_interval: "Remind every",
    menu_goal: "Daily goal",
    menu_glass: "Glass size",
    menu_reset: "Reset today's water",
    menu_focus_start: "Start focus timer",
    menu_focus_stop: "Stop focus timer",
    menu_focus_len: "Focus length",
    menu_rest_len: "Break length",
    menu_lyrics: "Synced lyrics (from LRCLIB)",
    menu_lyrics_timing: "Lyrics timing",
    lyrics_button: "Lyrics",
    lyrics_exact: "No adjustment",
    lyrics_earlier: "earlier",
    lyrics_later: "later",
    menu_exit: "Exit",
    decimal_comma: false,
};

const ES: Text = Text {
    nothing_playing: "Nada en reproducción",
    nothing_hint: "Reproduce algo en Spotify, YouTube o cualquier reproductor",
    charging: "Cargando",
    plugged_in: "Conectado",
    on_battery: "Con batería",
    unplugged: "Desconectado",
    battery_low: "Batería baja",
    battery_full: "Carga completa",
    water_title: "Hora de tomar agua",
    water_today: "hoy",
    water_done: "Meta cumplida",
    audio_output: "Salida de audio",
    mic_in_use: "Micrófono en uso",
    cam_in_use: "Cámara en uso",
    pomodoro: "Pomodoro",
    focus: "Enfoque",
    rest: "Descanso",
    focus_done: "Hora del descanso",
    rest_done: "A enfocarse",
    menu_autostart: "Iniciar con Windows",
    menu_hidden: "Ocultar la píldora (solo avisos)",
    menu_monitor: "Monitor",
    menu_monitor_primary: "Principal",
    menu_monitor_active: "El de la ventana activa",
    menu_hotkey: "Atajo Win+Alt+N",
    menu_alerts: "Avisos",
    alert_track: "Canción nueva",
    alert_volume: "Volumen",
    alert_brightness: "Brillo",
    alert_audio: "Salida de audio",
    alert_battery: "Batería",
    alert_privacy: "Micrófono y cámara",
    menu_water: "Recordatorio de agua",
    menu_interval: "Recordar cada",
    menu_goal: "Meta diaria",
    menu_glass: "Tamaño del vaso",
    menu_reset: "Reiniciar el agua de hoy",
    menu_focus_start: "Iniciar Pomodoro",
    menu_focus_stop: "Detener Pomodoro",
    menu_focus_len: "Duración del enfoque",
    menu_rest_len: "Duración del descanso",
    menu_lyrics: "Letras sincronizadas (de LRCLIB)",
    menu_lyrics_timing: "Sincronía de las letras",
    lyrics_button: "Letra",
    lyrics_exact: "Sin ajuste",
    lyrics_earlier: "antes",
    lyrics_later: "después",
    menu_exit: "Salir",
    decimal_comma: true,
};

pub fn load() -> Text {
    const LANG_SPANISH: u16 = 0x0A;
    if unsafe { GetUserDefaultUILanguage() } & 0x3FF == LANG_SPANISH {
        ES
    } else {
        EN
    }
}

impl Text {
    fn decimal(&self, s: String) -> String {
        if self.decimal_comma {
            s.replace('.', ",")
        } else {
            s
        }
    }

    /// "1.5 L" or "1,5 L" depending on the language.
    pub fn liters(&self, ml: u32) -> String {
        self.decimal(format!("{:.1} L", ml as f32 / 1000.0))
    }

    /// Compact "0.75 / 2 L" for the water card.
    pub fn liters_of(&self, ml: u32, goal: u32) -> String {
        let short = |v: u32| {
            let s = format!("{:.2}", v as f32 / 1000.0);
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        };
        self.decimal(format!("{} / {} L", short(ml), short(goal)))
    }

    /// "0.5 s earlier", "1 s later" or "No adjustment" for a lyrics offset.
    pub fn lyrics_offset(&self, ms: i32) -> String {
        if ms == 0 {
            return self.lyrics_exact.to_string();
        }
        let side = if ms > 0 {
            self.lyrics_earlier
        } else {
            self.lyrics_later
        };
        let seconds = self.decimal(format!("{}", ms.unsigned_abs() as f32 / 1000.0));
        format!("{seconds} s {side}")
    }
}
