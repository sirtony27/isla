# Estado — Isla

Actualizado: 2026-10-26

## Vigente

- Decisiones: 0001 (Rust + `windows`, nombre Isla), 0002 (dibujo con WARP y capas),
  0003 (v0.2: avisos del sistema y opciones), 0004 (letras sincronizadas con LRCLIB: única excepción a "sin red", apagadas por defecto).
- Presupuesto de rendimiento en `AGENTS.md`: 0 % de CPU en reposo y ≤ 20 MB privados con todo activado.
- Repo público: https://github.com/sirtony27/isla.
- **v0.3.2** en `main` (`5c1e68d`, PR #10, merge aprobado por el dueño) y publicado como release por
  la CI: https://github.com/sirtony27/isla/releases/tag/v0.3.2 — `isla.exe` (490 KB, MSVC) +
  `isla.exe.sha256` con LF. Verificado: `sha256sum -c` OK, solo DLLs de Windows, sin
  `VCRUNTIME140.dll` ni `winhttp.dll` en las importaciones, y se identifica como `Isla/0.3.2`.
  Novedad: si LRCLIB está saturado o no hay red, la letra se reintenta a los 2, 5, 10 y 20 s
  mientras suene la misma canción (PR #9). Auditoría, con la comparación de fuentes gratis (solo
  LRCLIB sirve): `docs/audits/2026-10-26-lyrics-retry.md` (en reposo, igual que v0.3.1: 11,2 MB
  privados y 0 ms de CPU).
- v0.3.1 (`57e862a`, PR #7): cumple la regla "apagada, no registra eventos ni carga DLLs" (PR #6).
  Auditoría: `docs/audits/2026-10-26-budget-toggles.md`.
- v0.3.0 (`9465bea`, PR #4): letras sincronizadas de LRCLIB, apagadas por defecto. Auditoría:
  `docs/audits/2026-10-26-lyrics.md` (10,2 MB privados con letras apagadas; 13,4–14,3 MB con letras).
- v0.3.1, v0.3.0, v0.2.0 y v0.1.0 siguen publicados; el `.sha256` de v0.1.0 tiene CRLF
  (desde v0.2.0, LF).
- Toolchain en la PC del dueño: rustup `stable-x86_64-pc-windows-gnu` con el target
  `x86_64-pc-windows-msvc` (solo para `cargo check`/`clippy`), componentes clippy, rustfmt y
  llvm-tools. PATH del sistema no modificado: para compilar, anteponer `%USERPROFILE%\.cargo\bin`
  y `%USERPROFILE%\.cargo\llvm-dlltool` (ver CONTRIBUTING.md).
- Isla v0.3.2 corre en la PC del dueño desde `target\release\isla.exe` de la carpeta principal, con
  inicio automático. Cada arranque reescribe la ruta de inicio automático con la del exe que
  arrancó: tras probar desde un worktree, relanzar desde la carpeta principal.

## Pendientes

- Letras: el dueño informó que funcionan. Sin confirmar con auriculares Bluetooth, por la latencia;
  para eso está el menú "Sincronía de las letras".
- QA del dueño de v0.2 con eventos reales: mic/cámara (p. ej. una llamada de Discord), teclas de
  brillo, cambio de salida de audio, batería baja, monitor activo (si hay dos monitores).
- QA del dueño de v0.3.1: apagar y volver a encender el aviso de brillo, y apagar el de mic/cámara
  desde el menú (no se probó con un script para no cambiar el brillo ni operar el menú de su PC).
- Letras: si la versión de Spotify dura más de 3 s distinto que las de LRCLIB, no hay letra (p. ej.
  "Soulmate" de Mac Miller: 267 s en Spotify, 273–278 s en LRCLIB). Ampliar la tolerancia puede
  desfasar la letra: lo decide el dueño.
- La decisión 0004 (punto 4) todavía describe el reintento anterior ("una vez a los 1,5 s").
  `docs/decisions/*` no se toca sin pedido explícito del dueño.
- Sin probar: Windows 10, varios monitores. (La PC del dueño usa escala 125 % y 144 Hz.)
- Instalar la plantilla del workflow para habilitar el worker.

## Acuerdos con el dueño

- (ninguno todavía)
