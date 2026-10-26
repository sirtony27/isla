# Estado — Isla

Actualizado: 2026-10-26

## Vigente

- Decisiones: 0001 (Rust + `windows`, nombre Isla), 0002 (dibujo con WARP y capas),
  0003 (v0.2: avisos del sistema y opciones), 0004 (letras sincronizadas con LRCLIB: única excepción a "sin red", apagadas por defecto).
- Presupuesto de rendimiento en `AGENTS.md`: 0 % de CPU en reposo y ≤ 20 MB privados con todo activado.
- Repo público: https://github.com/sirtony27/isla.
- **v0.3.0** en `main` (`9465bea`, PR #4: el dueño lo probó y aprobó el merge) y publicado como
  release por la CI: https://github.com/sirtony27/isla/releases/tag/v0.3.0 — `isla.exe` (488 KB, MSVC)
  + `isla.exe.sha256` con LF. Verificado: `sha256sum -c` OK, solo DLLs de Windows, sin
  `VCRUNTIME140.dll` y sin `winhttp.dll` en las importaciones (se carga solo con las letras activadas).
  Novedad: letras sincronizadas de LRCLIB, apagadas por defecto. Auditoría:
  `docs/audits/2026-10-26-lyrics.md` (10,2 MB privados con letras apagadas; 13,4–14,3 MB con letras).
- v0.2.0 y v0.1.0 siguen publicados; el `.sha256` de v0.1.0 tiene CRLF (desde v0.2.0, LF).
- Toolchain en la PC del dueño: rustup `stable-x86_64-pc-windows-gnu` con el target
  `x86_64-pc-windows-msvc` (solo para `cargo check`/`clippy`), componentes clippy, rustfmt y
  llvm-tools. PATH del sistema no modificado: para compilar, anteponer `%USERPROFILE%\.cargo\bin`
  y `%USERPROFILE%\.cargo\llvm-dlltool` (ver CONTRIBUTING.md).
- Isla v0.3 corre en la PC del dueño desde `target\release\isla.exe` de la carpeta principal, con
  inicio automático. Cada arranque reescribe la ruta de inicio automático con la del exe que
  arrancó: tras probar desde un worktree, relanzar desde la carpeta principal.

## Pendientes

- Letras: el dueño informó que funcionan. Sin confirmar con auriculares Bluetooth, por la latencia;
  para eso está el menú "Sincronía de las letras".
- QA del dueño de v0.2 con eventos reales: mic/cámara (p. ej. una llamada de Discord), teclas de
  brillo, cambio de salida de audio, batería baja, monitor activo (si hay dos monitores).
- Alinear con la regla del presupuesto ("apagada, no registra eventos ni carga DLLs"): Core Audio
  se carga siempre (lo usa la barra de volumen del panel) y la notificación de brillo se registra
  aunque el aviso esté apagado. Costo medido dentro del presupuesto, pero la regla no se cumple al pie.
- Sin probar: Windows 10, varios monitores. (La PC del dueño usa escala 125 % y 144 Hz.)
- Instalar la plantilla del workflow para habilitar el worker.

## Acuerdos con el dueño

- (ninguno todavía)
