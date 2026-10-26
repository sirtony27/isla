# Estado — Isla

Actualizado: 2026-10-26

## Vigente

- Decisiones: 0001 (Rust + `windows`, nombre Isla), 0002 (dibujo con WARP y capas).
- Toolchain en la PC del dueño: rustup `stable-x86_64-pc-windows-gnu`, componentes clippy,
  rustfmt y llvm-tools. PATH del sistema no modificado: para compilar, anteponer
  `%USERPROFILE%\.cargo\bin` y `%USERPROFILE%\.cargo\llvm-dlltool` (ver CONTRIBUTING.md).
- `feat/v1-core` (worktree `.worktrees/v1-core`): alcance v1 implementado y auditado
  (`docs/audits/2026-10-26-v1-core.md`). Pendiente: QA del dueño y aprobación del merge a `main`.

## Pendientes

- QA del dueño: aviso de carga, hover real, menú de bandeja, pantalla completa.
- Merge de `feat/v1-core` a `main` (requiere aprobación del dueño).
- Crear el repo en GitHub y publicar `v0.1.0` (requiere aprobación del dueño). La CI corre por
  primera vez ahí (MSVC).
- Instalar la plantilla del workflow para habilitar el worker.

## Acuerdos con el dueño

- (ninguno todavía)
