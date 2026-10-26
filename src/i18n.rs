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
    pub water_title: &'static str,
    pub water_today: &'static str,
    pub water_done: &'static str,
    pub menu_autostart: &'static str,
    pub menu_water: &'static str,
    pub menu_interval: &'static str,
    pub menu_goal: &'static str,
    pub menu_glass: &'static str,
    pub menu_reset: &'static str,
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
    water_title: "Time to drink water",
    water_today: "today",
    water_done: "Goal reached",
    menu_autostart: "Start with Windows",
    menu_water: "Water reminder",
    menu_interval: "Remind every",
    menu_goal: "Daily goal",
    menu_glass: "Glass size",
    menu_reset: "Reset today's water",
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
    water_title: "Hora de tomar agua",
    water_today: "hoy",
    water_done: "Meta cumplida",
    menu_autostart: "Iniciar con Windows",
    menu_water: "Recordatorio de agua",
    menu_interval: "Recordar cada",
    menu_goal: "Meta diaria",
    menu_glass: "Tamaño del vaso",
    menu_reset: "Reiniciar el agua de hoy",
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
    /// "1.5 L" or "1,5 L" depending on the language.
    pub fn liters(&self, ml: u32) -> String {
        let s = format!("{:.1} L", ml as f32 / 1000.0);
        if self.decimal_comma {
            s.replace('.', ",")
        } else {
            s
        }
    }
}
