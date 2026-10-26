# Reglas de código — Isla

Aplican a cualquier agente que escriba código en este repo.

## Rendimiento (la razón de ser del proyecto)

- El hilo de UI duerme en `GetMessageW`. Prohibido el polling: nada de bucles con `sleep` ni
  timers que consulten estado. Se usan eventos del sistema (SMTC, `WM_POWERBROADCAST`, appbar).
- Se dibuja solo si algo cambió (`dirty`) o durante una animación. Al asentarse, se deja de dibujar.
- Timers permitidos: los listados en `src/app.rs` (`TIMER_*`). Uno nuevo requiere justificar su frecuencia.
- Sin hooks globales de teclado o mouse (`SetWindowsHookEx`). Sin red.

## Presupuesto (no se supera)

Con todas las funciones activadas, en reposo y con música sonando:

- CPU: 0 % durante 10 s.
- Memoria privada: ≤ 20 MB.
- Cada función nueva tiene su interruptor en los ajustes. Apagada, no registra eventos ni carga DLLs.
- Cada PR que agregue una función informa la medición antes/después (ver CONTRIBUTING.md).

## Dependencias

- Única dependencia: el crate `windows` (y sus crates internos). Agregar otra requiere decisión aprobada.
- Features del crate `windows`: solo las que se usan.

## Estilo

- Código, identificadores y comentarios en inglés. Textos de UI en `src/i18n.rs` (es / en).
- `unsafe` acotado a las llamadas Win32/COM; sin `unwrap()` en rutas que dependen del sistema
  (dispositivo gráfico, SMTC, registro): se degrada con gracia.
- `cargo fmt` y `cargo clippy -- -D warnings` en verde.
- Comentarios solo donde el porqué no es obvio.

## Archivos que no se tocan sin pedido explícito

- `CLAUDE.md`, `AGENTS.md`, `agent-workflow.json`, `docs/decisions/*` (gobierno del workflow).
- `.github/workflows/*` (publicación de releases).
