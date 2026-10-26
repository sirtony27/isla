# Perfil del proyecto — Isla

- **Propósito:** notch nativo para Windows que muestra lo que suena, recuerda tomar agua y avisa la carga, sin gastar recursos.
- **Stack:** Rust (edición 2021) + crate `windows`. Win32, Direct2D, DirectWrite, DirectComposition, WinRT SMTC. Ver `docs/decisions/0001-stack-y-arquitectura.md`.
- **Arquitectura:** un hilo de UI (ventana, dibujo, estado), un hilo de multimedia (SMTC) y,
  solo con las letras activadas, un hilo de letras (WinHTTP → lrclib.net, decisión 0004). Se comunican por `PostMessageW` y un canal. Persistencia: `%APPDATA%\Isla\settings.ini`.

## Comandos

| Acción | Comando |
|---|---|
| Compilar (debug) | `cargo build` |
| Compilar (release) | `cargo build --release` → `target/release/isla.exe` |
| Formato | `cargo fmt --check` |
| Lint | `cargo clippy --release -- -D warnings` |
| Tests | `cargo test --release` (parsers de JSON y LRC); `-- --include-ignored` suma la consulta real a LRCLIB. La UI se valida con QA |

Toolchain local: `stable-x86_64-pc-windows-gnu` (sin Visual Studio) + `llvm-ar` como `dlltool` y `rust-lld` (ver CONTRIBUTING.md y decisión 0002). CI: `windows-latest` con MSVC.

## Entornos

- **Local:** el exe corre en la sesión del dueño. Escribe solo en `%APPDATA%\Isla` y, si el usuario lo activa, en `HKCU\...\Run`.
- **CI:** GitHub Actions compila en cada push y publica release en tags `v*`.
- **Producción:** el release de GitHub. No hay servidores.

## Zonas críticas (RED)

- `src/sys.rs`: registro (inicio con Windows).
- `.github/workflows/`: lo que se publica para otros.

## Información sensible

No hay secretos. No se leen datos del usuario más allá de lo que expone SMTC (título, artista, carátula) y el estado de energía.
Con las letras activadas (opt-in), el título, el artista, el álbum y la duración de cada canción salen hacia lrclib.net.

## Riesgos conocidos

- Antivirus/SmartScreen marcan exes nuevos sin firma. Mitigación: código abierto, build reproducible en CI, SHA-256 publicado, sin hooks; red solo con las letras activadas (opt-in).
- Reproductores que no publican posición (timeline) en SMTC: la barra de progreso se oculta.

## QA (DONE del proyecto)

Además de la definición general de DONE:
- Checks en verde.
- El exe arranca, se ve la píldora arriba al centro y se expande al pasar el mouse.
- Con música: título, artista, carátula y controles funcionan.
- Medición: RAM (working set) < 30 MB, CPU 0 % en reposo durante 10 s.

## Dueño

Aprueba merge a `main`, creación del repo público y releases. Prefiere informes cortos en español.
