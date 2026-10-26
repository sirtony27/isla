# Auditoría — fix/lyrics-retry

Nivel: YELLOW. Cambia cuántas veces se consulta a LRCLIB cuando falla, pero no qué se envía ni a
quién. No toca zonas críticas (`src/sys.rs`, workflows).
Implementó: Claude directo (worker deshabilitado). Pedido del dueño (2026-10-26): "agregá los
reintentos ante la saturación de LRCLIB".

## Por qué

El dueño pidió más letras, sin pagar y sin perder rendimiento. Se compararon fuentes gratis con
46 canciones, en su mayoría argentinas y latinas:

| Fuente | Resultado |
|---|---|
| LRCLIB | 39 de 46 al primer intento; 42 al reintentar los 503 o con el nombre que usa Spotify. Las otras 4 no estaban en ninguna fuente. |
| NetEase | Inutilizable: la búsqueda devuelve el resultado cifrado y, después de unas 10 consultas, responde 405/406 ("demasiadas operaciones"). |
| Kugou | 15 de 46, todas ya en LRCLIB; ninguna argentina. No suma. |
| Musixmatch | Más cobertura (da las letras de Spotify), pero su API gratis no trae letras sincronizadas y la vía gratis usa el token privado de su app de escritorio, que sus términos prohíben. Descartada. |

LRCLIB, en cambio, se satura seguido: `/api/get` respondió 503 ("ServerOverloaded", después de
~1,3 s) a 8 de 15 canciones que sí tiene, y algunas consultas dieron 503 cuatro veces seguidas.
Con un único reintento a los 1,5 s, si todo fallaba la canción quedaba sin letra hasta que
volviera a sonar.

## Cambio (`src/lyrics.rs`)

- Si un pedido falla por 429, 5xx o un error de red, la búsqueda se repite mientras suene la
  misma canción: a los 2, 5, 10 y 20 s (unos 40 s de espera en total).
- Solo se repiten los pedidos que fallaron. Los que respondieron (letra encontrada, 404 u otro
  rechazo) no se vuelven a mandar.
- La espera es sobre el canal de pedidos: un cambio de canción la corta al instante, y apagar las
  letras termina el hilo como antes.
- Una conexión por intento, que se cierra al terminar: nada queda abierto mientras espera.
- Después de dos intentos fallidos, la píldora deja de reservar el lugar de la letra de la canción
  anterior. Si un reintento encuentra la letra, aparece igual.
- Se quitó el reintento inmediato de 1,5 s: si la coincidencia exacta da 503, se pasa directo a la
  búsqueda, que suele responder.
- Peor caso por canción: 5 intentos de hasta 3 pedidos (antes, 6 pedidos en ~6 s). Si LRCLIB
  responde, se hacen los mismos pedidos que antes.

## Checks

- `cargo fmt --check`, `cargo clippy --release -- -D warnings` (GNU y MSVC): OK
- `cargo build --release`: OK, `isla.exe` 525 312 bytes con GNU, igual que `main`.
- `cargo test --release -- --include-ignored`: 8 de 8 OK. Tres pruebas nuevas sin red: repite solo
  lo que falló, las respuestas definitivas no se repiten, y reintentos con corte por canción nueva o
  por cierre. La prueba real contra lrclib.net ahora reintenta si el servidor está ocupado.

## Medición (misma PC, arranque en limpio, Spotify sonando)

| | v0.3.1 (`main`) | fix/lyrics-retry |
|---|---|---|
| Memoria privada | 10,6 y 13,1 MB | 11,2 y 11,2 MB |
| Hilos | 25 y 26 | 26 y 27 |
| CPU (10 s) | 0 y 94 ms | 0 y 0 ms |

Sin diferencia atribuible al cambio: el hilo espera bloqueado en el canal, sin polling.

## QA

| Caso | Resultado |
|---|---|
| Canción con letra (sesión SMTC de prueba, "One More Time"): la píldora se ensancha y muestra la línea | OK |
| Spotify real, "Soulmate" (Mac Miller): sigue sin letra. LRCLIB la tiene, pero en versiones de 273–278 s y Spotify informa 266,97 s, fuera de la tolerancia de ±3 s | sin cambio (ver hallazgos) |
| Reintento ante 503 y corte al cambiar de canción | cubierto por pruebas sin red; no se puede forzar a LRCLIB a responder 503 |
| Apagar las letras durante una espera termina el hilo | cubierto por prueba (canal cerrado) |
| Memoria y CPU iguales a v0.3.1 | OK |

## Hallazgos

| Hallazgo | Severidad | Estado |
|---|---|---|
| La decisión 0004, punto 4, todavía describe el reintento anterior ("una vez a los 1,5 s") | menor | Pendiente: `docs/decisions/*` no se toca sin pedido explícito del dueño |
| Si la versión de Spotify dura más de 3 s distinto que las de LRCLIB, no hay letra (ej. "Soulmate": 267 s contra 273 s) | media | Fuera de este cambio: aceptar versiones más lejanas puede desfasar la letra; lo decide el dueño |
| El peor caso de pedidos por canción sube de 6 a 15, solo si LRCLIB falla | menor | Aceptado: espaciados en ~40 s y cortados al cambiar de canción |
