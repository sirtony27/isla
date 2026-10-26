# Isla

A tiny native notch for the top of your Windows screen. It shows what's playing and
the things your PC wants to tell you, then gets out of the way.

[Leer en español](README.es.md)

- **Now playing, from any player.** Spotify, YouTube in any browser, Apple Music, and
  anything else that shows up in the Windows media flyout. Album art, title, artist,
  a live progress bar you can click to seek, and previous / play-pause / next.
- **Volume.** A slider in the panel (click the speaker to mute), the mouse wheel over
  the notch, and a clean overlay whenever the volume changes.
- **System alerts.** Brightness (laptop screens), audio output switched ("AirPods
  connected"), charger plugged or unplugged, battery low (20 %, 10 %) and fully charged.
- **Microphone and camera indicator.** An orange or green dot, like on a phone, while
  any app is using them, and a short alert saying which one.
- **Focus timer.** A Pomodoro card in the panel; the pill counts down the minutes and a
  soft chime marks the break.
- **Water reminder.** Set an interval, a daily goal and your glass size. Isla taps you
  on the shoulder during the day (9:00 to 22:00) and keeps today's count.
- **Hover to open, move away to close**, or press **Win+Alt+N**. A short spring
  animation, synced to your monitor's refresh rate.
- **Your way.** "Alerts only" mode hides the pill until something happens, it can follow
  the monitor of the active window, and every alert can be turned off.
- **Steps aside for full-screen apps** (games, videos, presentations).

## Light by design

| | Isla 0.2, everything on |
|---|---|
| Download | one `.exe`, about 0.45 MB, no installer |
| Memory | about 13 MB private (37 MB working set) |
| CPU when idle | 0 %, also with music playing |
| Dependencies | none at runtime; Windows 10 1809+ or Windows 11, 64-bit |

How it stays that light:

- **Native Win32 + Direct2D**, written in Rust. No web engine, no .NET, no Electron.
- **Waits instead of checking.** Windows tells Isla when a song changes, the volume
  moves, the charger is plugged in or an app opens the microphone. Between events the
  process sleeps; nothing polls.
- **Draws only while something moves.** Each view is cached in a bitmap; an animation
  just composes those bitmaps, and drawing stops the moment it settles.
- **No GPU driver in the process.** Isla rasterizes its small surface with WARP and lets
  Windows compose it on the GPU, so a laptop's discrete GPU is never woken up.
- **No keyboard or mouse hooks, no network, no telemetry.** The Win+Alt+N shortcut is a
  registered hotkey: Windows reports only that combination.

## Install

1. Download `isla.exe` from the [latest release](../../releases/latest).
2. Optional: check that its SHA-256 matches `isla.exe.sha256` from the same release:
   ```powershell
   Get-FileHash .\isla.exe -Algorithm SHA256
   ```
3. Run it. Windows SmartScreen may warn about an unknown publisher, because the exe is
   new and not code-signed: click **More info → Run anyway**. The build is reproducible
   from this repository by GitHub Actions, so you can verify exactly what you run.

Right-click the tray icon for settings: start with Windows, water reminder interval,
daily goal, glass size, or exit. Settings live in `%APPDATA%\Isla\settings.ini`.

To uninstall: uncheck "Start with Windows" if you enabled it, exit from the tray, then
delete the exe and `%APPDATA%\Isla`.

## Privacy

Isla makes no network connections. It reads the media session Windows already exposes
(title, artist, artwork) and the power status. Your water log stays in your user folder.

## Build from source

```powershell
cargo build --release   # target\release\isla.exe
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the toolchain setup and project rules.

## Roadmap

Ideas for after 0.1: synced lyrics, focus timer, volume and brightness overlays,
multi-monitor placement, a minimal "line" mode for laptops with thick bezels.

## License

[MIT](LICENSE)
