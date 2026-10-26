# Isla

Este proyecto utiliza el workflow `agentic-development-workflow`. Lo que sigue es su contexto.

Isla es un "notch" nativo para Windows 10/11 (estilo Dynamic Island): multimedia con controles,
recordatorio de agua y avisos de carga. Open source, mínimo consumo.

## Leer al empezar

1. `docs/state/estado.md` — estado vigente y pendientes.
2. `docs/agent-workflow/project-profile.md` — stack, comandos, riesgos, DONE.
3. `docs/decisions/` — decisiones numeradas (no se reescriben; se agregan nuevas).
4. `AGENTS.md` — reglas de código (aplican a Claude y al worker).

## Reglas del orquestador en este repo

- Cada tarea en su worktree bajo `.worktrees/<tarea>` y su rama `feat|fix|chore/<tarea>`. `main` no se toca directo.
- Checks: `cargo fmt --check && cargo clippy --release -- -D warnings && cargo build --release`.
- QA visual: correr el exe, capturar la franja superior de la pantalla y medir RAM/CPU (ver perfil).
- El worker headless está deshabilitado hasta instalar la plantilla del workflow (`agent-workflow.json`).
  Mientras tanto, Claude implementa directo y audita su propio diff con el mismo criterio.
- Merge a `main`, crear el repo de GitHub, publicar releases: requieren aprobación del dueño.
