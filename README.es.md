# Isla

Un notch nativo y diminuto para la parte superior de tu pantalla en Windows. Muestra lo
que suena, te recuerda tomar agua y te avisa cuando conectas el cargador. Después se
aparta.

[Read in English](README.md)

- **Lo que suena, de cualquier reproductor.** Spotify, YouTube en cualquier navegador,
  Apple Music y todo lo que aparece en el panel multimedia de Windows. Carátula, título,
  artista, barra de progreso en vivo (clic para adelantar) y anterior / pausa / siguiente.
- **Recordatorio de agua.** Eliges intervalo, meta diaria y tamaño del vaso. Isla te avisa
  durante el día (de 9:00 a 22:00) y lleva la cuenta de hoy.
- **Aviso de carga.** En laptops, un aviso breve al conectar o desconectar el cargador,
  con el nivel de batería.
- **Pasa el mouse para abrir, aléjalo para cerrar.** Nada que configurar. Animación
  con resorte, sincronizada con la frecuencia de tu monitor.
- **Se oculta con apps a pantalla completa** (juegos, videos, presentaciones).

## Liviano por diseño

| | Isla 0.1 |
|---|---|
| Descarga | un `.exe` de ~0,4 MB, sin instalador |
| Memoria | ~13 MB privados (35 MB de working set) |
| CPU en reposo | 0 %, también con música |
| Dependencias | ninguna; Windows 10 1809+ o Windows 11, 64 bits |

- **Win32 nativo + Direct2D**, escrito en Rust. Sin motor web, sin .NET, sin Electron.
- **Espera en vez de consultar.** Windows avisa cuando cambia la canción o se conecta el
  cargador. Entre eventos el proceso duerme.
- **Dibuja solo cuando algo se mueve**, a partir de vistas cacheadas en bitmaps.
- **Sin driver de GPU en el proceso:** dibuja con WARP y Windows compone en la GPU, así
  la GPU dedicada de una laptop no se despierta.
- **Sin hooks de teclado ni mouse, sin red, sin telemetría.**

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
