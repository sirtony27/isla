# Estado — Isla

Actualizado: 2026-10-26

## Vigente

- Decisiones: 0001 (Rust + `windows`, nombre Isla), 0002 (dibujo con WARP y capas).
- v0.1 en `main`: alcance v1 completo, auditado (`docs/audits/2026-10-26-v1-core.md`) y con QA
  del dueño aprobado. Merge aprobado por el dueño el 2026-10-26.
- Toolchain en la PC del dueño: rustup `stable-x86_64-pc-windows-gnu`, componentes clippy,
  rustfmt y llvm-tools. PATH del sistema no modificado: para compilar, anteponer
  `%USERPROFILE%\.cargo\bin` y `%USERPROFILE%\.cargo\llvm-dlltool` (ver CONTRIBUTING.md).
- No hay worktrees abiertos.

## Pendientes

- Crear el repo en GitHub y publicar `v0.1.0` (requiere aprobación del dueño). La CI (MSVC)
  corre por primera vez ahí.
- Sin probar: Windows 10, escalas de DPI distintas de 100 %, varios monitores.
- Instalar la plantilla del workflow para habilitar el worker.

## Acuerdos con el dueño

- (ninguno todavía)
