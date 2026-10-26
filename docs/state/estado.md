# Estado — Isla

Actualizado: 2026-10-26

## Vigente

- Decisiones: 0001 (Rust + `windows`, nombre Isla), 0002 (dibujo con WARP y capas).
- Repo público: https://github.com/sirtony27/isla (creado con aprobación del dueño el 2026-10-26).
- Release `v0.1.0` publicado por la CI (MSVC, en verde): `isla.exe` (364 KB) + `isla.exe.sha256`.
  Verificado: el exe coincide con su SHA-256 y no depende de `VCRUNTIME140.dll`.
- El `.sha256` de `v0.1.0` tiene final de línea CRLF (`sha256sum -c` falla en Linux/Git Bash;
  `Get-FileHash` funciona). Corregido en la CI para los próximos releases (`fix/sha256-lf`).
- Toolchain en la PC del dueño: rustup `stable-x86_64-pc-windows-gnu` con el target
  `x86_64-pc-windows-msvc` (solo para `cargo check`/`clippy`), componentes clippy, rustfmt y
  llvm-tools. PATH del sistema no modificado: para compilar, anteponer `%USERPROFILE%\.cargo\bin`
  y `%USERPROFILE%\.cargo\llvm-dlltool` (ver CONTRIBUTING.md).
- Isla corre en la PC del dueño desde `target\release\isla.exe`, con inicio automático activado.

## Pendientes

- Sin probar: Windows 10, escalas de DPI distintas de 100 %, varios monitores.
- Instalar la plantilla del workflow para habilitar el worker.

## Acuerdos con el dueño

- (ninguno todavía)
