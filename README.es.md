# Isla

Un notch nativo y diminuto para la parte superior de tu pantalla en Windows. Muestra lo
que suena y lo que tu PC quiere contarte. Después se aparta.

[Read in English](README.md)

- **Lo que suena, de cualquier reproductor.** Spotify, YouTube en cualquier navegador,
  Apple Music y todo lo que aparece en el panel multimedia de Windows. Carátula, título,
  artista, barra de progreso en vivo (clic para adelantar) y anterior / pausa / siguiente.
- **Volumen.** Un control en el panel (clic en el parlante para silenciar), la rueda del
  mouse sobre el notch y un aviso limpio cada vez que cambia el volumen.
- **Avisos del sistema.** Brillo (pantalla de laptop), cambio de salida de audio
  ("AirPods conectados"), cargador conectado o desconectado, batería baja (20 %, 10 %) y
  carga completa.
- **Indicador de micrófono y cámara.** Un punto naranja o verde, como en el teléfono,
  mientras alguna app los usa, y un aviso breve que dice cuál.
- **Pomodoro.** Una tarjeta en el panel; la píldora descuenta los minutos y un sonido
  suave marca el descanso.
- **Recordatorio de agua.** Eliges intervalo, meta diaria y tamaño del vaso. Isla te avisa
  durante el día (de 9:00 a 22:00) y lleva la cuenta de hoy.
- **Pasa el mouse para abrir, aléjalo para cerrar**, o pulsa **Win+Alt+N**. Animación con
  resorte, sincronizada con la frecuencia de tu monitor.
- **A tu manera.** El modo "solo avisos" oculta la píldora hasta que pasa algo, puede
  seguir al monitor de la ventana activa y cada aviso se puede apagar.
- **Se oculta con apps a pantalla completa** (juegos, videos, presentaciones).

## Liviano por diseño

| | Isla 0.2, todo activado |
|---|---|
| Descarga | un `.exe` de ~0,45 MB, sin instalador |
| Memoria | ~13 MB privados (37 MB de working set) |
| CPU en reposo | 0 %, también con música |
| Dependencias | ninguna; Windows 10 1809+ o Windows 11, 64 bits |

- **Win32 nativo + Direct2D**, escrito en Rust. Sin motor web, sin .NET, sin Electron.
- **Espera en vez de consultar.** Windows avisa cuando cambia la canción, se mueve el
  volumen, se conecta el cargador o una app abre el micrófono. Entre eventos el proceso
  duerme.
- **Dibuja solo cuando algo se mueve**, a partir de vistas cacheadas en bitmaps.
- **Sin driver de GPU en el proceso:** dibuja con WARP y Windows compone en la GPU, así
  la GPU dedicada de una laptop no se despierta.
- **Sin hooks de teclado ni mouse, sin red, sin telemetría.** El atajo Win+Alt+N es una
  tecla rápida registrada: Windows solo informa esa combinación.

## Instalación

1. Descarga `isla.exe` desde el [último release](../../releases/latest).
2. Opcional: verifica que su SHA-256 coincida con `isla.exe.sha256` del mismo release:
   ```powershell
   Get-FileHash .\isla.exe -Algorithm SHA256
   ```
3. Ejecútalo. SmartScreen puede advertir de un editor desconocido porque el exe es nuevo
   y no está firmado: **Más información → Ejecutar de todas formas**. GitHub Actions lo
   compila desde este repositorio, así que puedes verificar exactamente lo que corres.

Clic derecho en el icono de la bandeja para los ajustes: iniciar con Windows, intervalo
del recordatorio, meta diaria, tamaño del vaso o salir. Los ajustes se guardan en
`%APPDATA%\Isla\settings.ini`.

Para desinstalar: desmarca "Iniciar con Windows" si lo activaste, sal desde la bandeja y
borra el exe y `%APPDATA%\Isla`.

## Privacidad

Isla no se conecta a internet. Lee la sesión multimedia que Windows ya expone (título,
artista, carátula) y el estado de energía. Tu registro de agua queda en tu carpeta.

## Compilar

```powershell
cargo build --release   # target\release\isla.exe
```

Ver [CONTRIBUTING.md](CONTRIBUTING.md).

## Licencia

[MIT](LICENSE)
