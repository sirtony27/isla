# Estado — Isla

Actualizado: 2026-10-26

## Vigente

- Decisiones: 0001 (Rust + `windows`, nombre Isla), 0002 (dibujo con WARP y capas),
  0003 (v0.2: avisos del sistema y opciones).
- Presupuesto de rendimiento en `AGENTS.md`: 0 % de CPU en reposo y ≤ 20 MB privados con todo activado.
- Repo público: https://github.com/sirtony27/isla. Release `v0.1.0` publicado y verificado.
  Su `.sha256` tiene CRLF; la CI ya escribe LF para los próximos releases.
- `feat/v0.2` (worktree `.worktrees/v0.2`): volumen, brillo, salida de audio, batería baja/completa,
  mic/cámara, Pomodoro, atajo Win+Alt+N, modo solo avisos y monitor activo. Auditado
  (`docs/audits/2026-10-26-v0.2.md`): 12–13 MB privados, 0 % de CPU. PR pendiente de aprobación.
- Toolchain en la PC del dueño: rustup `stable-x86_64-pc-windows-gnu` con el target
  `x86_64-pc-windows-msvc` (solo para `cargo check`/`clippy`), componentes clippy, rustfmt y
  llvm-tools. PATH del sistema no modificado: para compilar, anteponer `%USERPROFILE%\.cargo\bin`
  y `%USERPROFILE%\.cargo\llvm-dlltool` (ver CONTRIBUTING.md).
- Isla corre en la PC del dueño con inicio automático activado. Ojo: cada arranque reescribe la
  ruta de inicio automático con la del exe que arrancó; tras probar desde un worktree, relanzar
  desde `target\release\isla.exe` de la carpeta principal.

## Pendientes

- QA del dueño de v0.2: mic/cámara (p. ej. una llamada de Discord), brillo con las teclas,
  cambiar la salida de audio, monitor activo (si hay dos monitores).
- Merge de `feat/v0.2` y release `v0.2.0` (requieren aprobación del dueño).
- Sin probar: Windows 10, escalas de DPI distintas de 100 %, varios monitores.
- Instalar la plantilla del workflow para habilitar el worker.

## Acuerdos con el dueño

- (ninguno todavía)
