# Contributing to Isla

Thanks for helping. Isla's whole point is to be invisible on a PC's resources, so every
change is judged first by what it costs at idle.

## Toolchain

Any of these works:

- **MSVC (recommended):** [rustup](https://rustup.rs) with the default
  `x86_64-pc-windows-msvc` toolchain and Visual Studio Build Tools. This is what CI uses.
- **GNU (no Visual Studio):** `rustup default stable-x86_64-pc-windows-gnu`. The crate
  `windows` needs a `dlltool` that can write import libraries; the one bundled with the
  GNU toolchain can't on its own. Use LLVM's instead:
  ```powershell
  rustup component add llvm-tools
  $bin = "$env:USERPROFILE\.cargo\llvm-dlltool"
  New-Item -ItemType Directory -Force $bin | Out-Null
  Copy-Item "$(rustc --print sysroot)\lib\rustlib\x86_64-pc-windows-gnu\bin\llvm-ar.exe" "$bin\dlltool.exe"
  $env:PATH = "$bin;$env:PATH"   # add it to your PATH permanently if you like
  ```
  Linking uses the bundled `rust-lld` (set in `.cargo/config.toml`), which also avoids
  MinGW `ld` failing on user folders with non-ASCII names.

## Checks

All three must pass before a PR (CI runs them too):

```powershell
cargo fmt --check
cargo clippy --release -- -D warnings
cargo build --release
```

## Rules

Read [AGENTS.md](AGENTS.md). In short:

- No polling and no new timers without a reason; react to Windows events.
- Draw only when something changed; cache what does not change.
- No new dependencies beyond the official `windows-rs` crates without discussion.
- No keyboard/mouse hooks. No network access, except the opt-in lyrics (decision 0004).
- UI strings go in `src/i18n.rs`, in every language.

Parsers have unit tests: `cargo test --release`. Add `-- --include-ignored` to also run
the test that talks to lrclib.net.

When a change affects resource use, include before/after numbers in the PR: private
memory and CPU over 10 s idle (Task Manager or `Get-Process isla`).

## Layout

| File | What it does |
|---|---|
| `src/main.rs` | Entry point and message loop |
| `src/app.rs` | State machine (closed, open, live activity), input, timers, events |
| `src/ui.rs` | Layout, drawing and hit-testing of every view; layer cache |
| `src/gfx.rs` | Direct2D on a DirectComposition swap chain; fonts |
| `src/anim.rs` | Spring animation |
| `src/media.rs` | Now playing via SMTC, on its own thread |
| `src/audio.rs` | Volume and audio output via Core Audio callbacks |
| `src/privacy.rs` | Microphone / camera in use, from registry change notifications |
| `src/focus.rs` | Pomodoro timer state |
| `src/sys.rs` | Power status, autostart, hotkey, monitors, time formatting, single instance |
| `src/tray.rs` | Tray icon and menu |
| `src/settings.rs` | `settings.ini` load and save |
| `src/i18n.rs` | Translations |
| `src/lyrics.rs` | Synced lyrics: LRCLIB lookup on its own thread, LRC parsing |
| `src/net.rs` | HTTPS GET through WinHTTP, loaded only when lyrics are on |
| `src/json.rs` | Minimal JSON parser for LRCLIB's answers |
