# Estado — Isla

Actualizado: 2026-10-26

## Vigente

- Decisiones: 0001 (Rust + `windows`, nombre Isla), 0002 (dibujo con WARP y capas),
  0003 (v0.2: avisos del sistema y opciones), 0004 (letras sincronizadas con LRCLIB: única excepción a "sin red", apagadas por defecto).
- Presupuesto de rendimiento en `AGENTS.md`: 0 % de CPU en reposo y ≤ 20 MB privados con todo activado.
- Repo público: https://github.com/sirtony27/isla.
- **v0.2.0** en `main` (`215c0c2`, merge aprobado por el dueño) y publicado como release por la CI:
  https://github.com/sirtony27/isla/releases/tag/v0.2.0 — `isla.exe` (414 KB, MSVC) + `isla.exe.sha256`
  con LF. Verificado: `sha256sum -c` OK, sin `VCRUNTIME140.dll`, solo DLLs de Windows.
  Auditoría: `docs/audits/2026-10-26-v0.2.md` (12–13 MB privados, 0 % de CPU).
- v0.1.0 sigue publicado; su `.sha256` tiene CRLF (`v0.2.0` en adelante usa LF).
- Toolchain en la PC del dueño: rustup `stable-x86_64-pc-windows-gnu` con el target
  `x86_64-pc-windows-msvc` (solo para `cargo check`/`clippy`), componentes clippy, rustfmt y
  llvm-tools. PATH del sistema no modificado: para compilar, anteponer `%USERPROFILE%\.cargo\bin`
  y `%USERPROFILE%\.cargo\llvm-dlltool` (ver CONTRIBUTING.md).
- Isla v0.2 corre en la PC del dueño desde `target\release\isla.exe` de la carpeta principal, con
  inicio automático. Cada arranque reescribe la ruta de inicio automático con la del exe que
  arrancó: tras probar desde un worktree, relanzar desde la carpeta principal.

## Pendientes

- PR de `feat/lyrics` (v0.3: letras sincronizadas) esperando el QA y la aprobación del dueño. Auditoría:
  `docs/audits/2026-10-26-lyrics.md`. Falta probar con Spotify real, auriculares Bluetooth y el menú.

- QA del dueño de v0.2 con eventos reales: mic/cámara (p. ej. una llamada de Discord), teclas de
  brillo, cambio de salida de audio, batería baja, monitor activo (si hay dos monitores).
- Alinear con la regla del presupuesto ("apagada, no registra eventos ni carga DLLs"): Core Audio
  se carga siempre (lo usa la barra de volumen del panel) y la notificación de brillo se registra
  aunque el aviso esté apagado. Costo medido dentro del presupuesto, pero la regla no se cumple al pie.
- Sin probar: Windows 10, varios monitores. (La PC del dueño usa escala 125 % y 144 Hz.)
- Instalar la plantilla del workflow para habilitar el worker.

## Acuerdos con el dueño

- (ninguno todavía)
