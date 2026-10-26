# Auditoría — fix/budget-toggles

Nivel: YELLOW. Cambia cuándo se crean objetos COM, registros de energía y un hilo; no toca
zonas críticas. Implementó: Claude directo (worker deshabilitado).
Regla: `AGENTS.md`, presupuesto: "Apagada, no registra eventos ni carga DLLs".

## Qué se alineó

| Función | Antes | Ahora |
|---|---|---|
| Core Audio (avisos de volumen y de salida de audio) | se cargaba siempre al iniciar | solo con alguno de esos avisos activo, con el panel abierto (fila de volumen) o con el aviso de la rueda del mouse. Al cerrar el panel se liberan los callbacks |
| Notificación de brillo | registrada siempre | solo con el aviso de brillo activo; se da de baja al apagarlo |
| Hilo de micrófono/cámara | apagar el aviso no lo detenía hasta reiniciar | se detiene al apagarlo: cierra las claves del registro y cancela sus notificaciones |

Revisado y sin cambios: las notificaciones de batería (`GUID_ACDC_POWER_SOURCE`,
`GUID_BATTERY_PERCENTAGE_REMAINING`) también alimentan la tarjeta de batería del panel, que no
tiene interruptor, y no cargan DLLs.

Límite de Windows: después del primer uso, `MMDevApi.dll` y `AUDIOSES.DLL` quedan cargadas
aunque se liberen los objetos. Se probó `CoFreeUnusedLibrariesEx` y no las descarga, así que se
quitó. Los callbacks sí se dan de baja, así que no llegan eventos.

## Checks

- `cargo fmt --check`, `cargo clippy --release -D warnings` (GNU y MSVC), `cargo build --release`: OK
- `cargo test --release`: 4 de 4 OK (1 ignorada: la consulta real a LRCLIB)

## Medición (misma PC, en reposo, con música en Spotify; letras apagadas para comparar)

Avisos de volumen, salida de audio, brillo y micrófono/cámara **apagados**:

| | v0.3 (`main`) | Esta rama |
|---|---|---|
| DLLs de Core Audio cargadas | `MMDevApi.dll`, `AUDIOSES.DLL` | ninguna |
| Memoria privada | 8,7–10,4 MB | 8,0–8,4 MB (una corrida: 11,1 MB) |
| Hilos | 22–24 | 20–21 |
| CPU (10 s) | 0–15,6 ms | 0 ms (una corrida: 203 ms, durante un cambio de canción; repetido 3 veces más + 20 s: 0) |

Con todo **activado**, las dos cargan Core Audio al iniciar, como antes.

## QA

| Caso | Resultado |
|---|---|
| Avisos apagados: Core Audio no se carga al iniciar | OK |
| Abrir el panel con los avisos apagados: se carga y la fila de volumen muestra 100 % | OK |
| Rueda del mouse con los avisos apagados: aviso de volumen 98 % → 100 % (volumen restaurado) | OK |
| Todos los avisos activados: Core Audio se carga al iniciar | OK |
| Aviso de brillo apagado y vuelto a encender; apagar mic/cámara desde el menú | **pendiente (QA del dueño)**: no se cambia el brillo ni el menú de la PC del dueño desde un script |
