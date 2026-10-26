# 0004 — Letras sincronizadas con LRCLIB

Estado: pedida por el dueño (2026-10-26: "usar lrclib, bien sincronizadas, que se puedan
habilitar o deshabilitar"). La aprobación final llega con el merge.
Complementa: 0001, 0002, 0003. Presupuesto: `AGENTS.md`.

## Excepción a "sin red"

Es la única función que usa internet, y por eso viene **apagada por defecto** (`lyrics=0`).
Con la función activada, por cada canción nueva se envían a `lrclib.net` el título, el artista,
el álbum y la duración. No se envía nada más: ni identificadores ni cookies. No se guarda nada en
disco, solo una caché en memoria con las últimas 24 canciones.

- **Cliente:** WinHTTP, cargado con `LoadLibraryExW` (solo desde System32) la primera vez que se
  pide una letra. Con la función apagada, el proceso no carga ninguna DLL de red; verificado con
  la lista de módulos del proceso. Una vez cargado, WinHTTP no se descarga hasta salir, porque
  descargarlo mientras termina su pool de hilos no es seguro.
- **Sin dependencias nuevas:** JSON y LRC con parsers propios (`src/json.rs`, `src/lyrics.rs`).
- **Hilo `lyrics`:** se crea al activar la función y termina al desactivarla. Duerme en un canal
  entre canciones y abre una conexión por canción, que se cierra al terminar.
- **User-Agent:** `Isla/<versión> (https://github.com/sirtony27/isla)`, como pide LRCLIB.

## Búsqueda

1. `GET /api/get` con el título, el artista, la duración y el álbum si lo hay. Coincidencia
   exacta. Si la canción es instrumental, no se muestra letra.
2. Si no hay coincidencia: `GET /api/search` con el título y el artista, y después con el
   título "limpio" (sin "(Official Video)", "feat.", "- Remastered 2011"; también separa
   "Artista - Canción" en los títulos de YouTube).
3. Solo sirven letras **sincronizadas** con una duración a **±3 s** de la canción, porque otra
   versión (radio edit, en vivo) tendría otros tiempos. Sin duración conocida, se toma el
   primer resultado sincronizado.
4. Si LRCLIB responde 429 o 503 ("ocupado"), se reintenta una vez a los 1,5 s. Un error de red
   no se guarda en la caché, así que la canción se vuelve a pedir la próxima vez que suene.

La consulta sale 0,7 s después del cambio de canción, cuando el reproductor ya mandó la duración
nueva. Si la duración tarda más, se espera hasta 3 veces más antes de preguntar sin ella.

## Sincronía

- La línea actual se calcula con la posición de SMTC llevada al momento actual. Cada evento de
  timeline del reproductor (adelantar, pausar o reanudar) vuelve a anclar el cálculo.
- `TIMER_LYRIC` es de un solo disparo y se programa para el inicio de la línea siguiente. No es
  polling: solo corre con la letra en pantalla y la música sonando.
- Las líneas cambian 0,1 s antes de su marca de tiempo, así el deslizamiento termina mientras se
  canta. En el menú hay un ajuste de ±0,25, ±0,5 o ±1 s (`lyrics_offset`, en ms, positivo =
  antes) para auriculares Bluetooth o letras desfasadas.
- Medido con una sesión SMTC de prueba en silencio: el cambio empieza en pantalla entre 31 y 52 ms
  después del momento programado, sin deriva en 30 s. Ese margen viene de la resolución del timer
  de Windows y del vsync.

## Interfaz

- **Píldora:** con la canción sonando y la letra disponible, se ensancha a 360 DIP y muestra la
  línea actual centrada. Las líneas largas usan una fuente más chica y, si aún no entran, se cortan
  con "…". Las pausas instrumentales muestran ♪. Si hay un Pomodoro en curso, sus minutos quedan a
  la derecha.
- **Panel:** botón "Letra" bajo la barra de progreso. Reemplaza la fila de volumen y las tarjetas
  por cuatro líneas: la anterior, la actual y dos más, que suben al cambiar. Un clic en una línea
  salta a ese momento de la canción (`lyrics_view` recuerda la elección).
- Modo solo avisos: la píldora no aparece, así que la letra se ve solo en el panel.

## Costo medido (misma PC, monitor de 144 Hz, escala 125 %)

| | Letras apagadas | Letras activadas, cambiando de línea |
|---|---|---|
| Memoria privada | 10,2 MB | 13,4–14,3 MB |
| DLLs de red cargadas | ninguna | winhttp, webio, schannel, dnsapi, mswsock |
| CPU (10 s) | 0 ms | 31–63 ms (0,02–0,04 % del total, ~0,5 % de un núcleo) |
| Exe | — | 511 KB con GNU (v0.2: 437 KB) |

Cada cambio de línea es una animación de ~195 ms (unos 29 cuadros a 144 Hz). Fuera de eso, el
proceso sigue durmiendo. **Excepción al presupuesto:** con la letra en pantalla, la CPU no es 0
exacto, porque mostrar letras sincronizadas exige redibujar en cada línea. El Administrador de
tareas igual muestra 0 %. Con la función apagada, todo queda exactamente como en v0.2.

## Ajustes nuevos

`lyrics` (0), `lyrics_offset` (0, de −5000 a 5000 ms), `lyrics_view` (0).
