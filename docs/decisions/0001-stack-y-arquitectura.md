# 0001 — Stack y arquitectura

Estado: aprobada por el dueño (2026-10-25). Nombre del proyecto: **Isla**.
Fecha: 2026-10-25

## Contexto

Referencia: NUSA (https://rama2555.github.io/nusa/), un "notch" estilo Dynamic Island para
Windows 10/11. No es open source y su instalador fue marcado por el antivirus del dueño, así que
no se instaló. Lo que hace, según su página:

- Una píldora negra en el borde superior de la pantalla. Al pasar el mouse se expande; al salir, se cierra.
- Música de cualquier reproductor (Spotify, navegador, etc.) con controles y barra de progreso.
- Avisos breves ("live activities"): cargador conectado, auriculares, Wi‑Fi, volumen, brillo, Bloq Mayús.
- Widgets: Pomodoro, recordatorio de agua con meta diaria, estante de archivos, lista de tareas, letras sincronizadas (LRCLIB).
- Icono en la bandeja con ajustes (iniciar con Windows, modo mínimo, salir). Se oculta con apps a pantalla completa.
- Nativo en Rust, dibujado por GPU, ~30 MB de RAM, 0 % de CPU en reposo.

Requisitos del dueño: mínimo consumo, arranque rápido, casi sin dependencias, animaciones fluidas,
uso sencillo, open source en GitHub. Funciones pedidas: multimedia (ver y controlar), panel de
control, recordatorio de agua y aviso de carga en laptops.

## Decisión

**Rust + crate `windows` (oficial de Microsoft), Win32 puro, Direct2D/DirectWrite sobre DirectComposition.**

- Una sola dependencia de compilación (`windows`). Sin runtime, sin motor web, sin instalador.
- Resultado: un `.exe` portable de ~1–2 MB. Corre en Windows 10 1809+ y 11, x64.
- Compilación local con el toolchain GNU de Rust (no requiere Visual Studio ni admin). La CI de
  GitHub compila los releases con MSVC.

### Alternativas descartadas

| Opción | Por qué no |
|---|---|
| Electron / Tauri | Motor web embebido: 80–200 MB de RAM, CPU en reposo |
| C# WPF / WinUI | Runtime .NET, 60–150 MB de RAM, arranque lento |
| C++ Win32 | Igual de liviano, pero exige Visual Studio Build Tools (~5 GB) y es menos seguro |
| C# .NET Framework (csc incluido en Windows) | Cero instalación, pero dibujo por CPU (GDI+), C# 5 y peor experiencia para colaboradores |

## Principios de rendimiento (no negociables)

1. **Esperar, no consultar.** El hilo de UI duerme en `GetMessage`. Windows avisa los cambios:
   eventos SMTC (multimedia), `WM_POWERBROADCAST` (cargador), `ABN_FULLSCREENAPP` (pantalla completa).
2. **Dibujar solo cuando algo se mueve.** Durante una animación se dibuja sincronizado con el
   refresco del monitor; al terminar, se deja de dibujar.
3. **Timers solo cuando aportan.** Barra de progreso: 1 Hz, solo con el panel abierto y música
   sonando. Agua: un timer de minutos. Nada más.
4. **Sin hooks globales de teclado ni mouse.** Evita falsos positivos de antivirus y latencia del sistema.
5. **Sin red.** Nada sale del equipo.

Metas medibles: RAM < 30 MB, CPU 0 % en reposo (con o sin música), arranque < 200 ms, exe < 2 MB.

## Arquitectura

```
src/
  main.rs        entrada: instancia única, COM, ventana, bucle de mensajes
  app.rs         estado de la app y máquina de estados del notch (cerrado / abierto / aviso)
  anim.rs        resortes críticamente amortiguados para tamaño y opacidad
  render.rs      D3D11 + DXGI swap chain de composición + D2D + DirectWrite + DComp
  draw.rs        dibujo de cada vista (píldora, panel, avisos)
  media.rs       GlobalSystemMediaTransportControlsSessionManager (SMTC), eventos → mensajes
  power.rs       estado de batería y notificaciones de energía
  water.rs       lógica del recordatorio y registro diario
  settings.rs    ajustes en %APPDATA%\<app>\settings.ini (clave=valor, sin serde)
  tray.rs        icono de bandeja y menú
  i18n.rs        textos es / en según el idioma del sistema
```

- Ventana `WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP`, centrada arriba en el
  monitor principal, sin entrada en la barra de tareas. La región de la ventana sigue el tamaño
  animado del notch para que los clics fuera de él pasen a la ventana de abajo.
- Los eventos WinRT llegan en hilos de fondo y se reenvían al hilo de UI con `PostMessage`.
- DPI por monitor (v2) desde el manifiesto.

## Alcance v1

- Píldora cerrada: hora, o carátula + indicador si hay música.
- Panel abierto al pasar el mouse: carátula, título, artista, progreso, anterior / pausa / siguiente;
  tarjeta de agua (consumo del día, meta, botón +vaso); tarjeta de batería (si hay batería).
- Avisos: cargador conectado / desconectado con porcentaje; recordatorio de agua con "Listo".
- Bandeja: iniciar con Windows, recordatorio de agua (on/off, intervalo, meta, vaso), salir.
- Se oculta con apps a pantalla completa.

Fuera de v1 (posibles siguientes): letras, Pomodoro, estante de archivos, tareas, overlays de
volumen/brillo, mascota.

## Distribución

GitHub Actions compila el `.exe` en cada tag `v*` y publica el release con su SHA-256. Código
abierto y compilación reproducible: cualquiera puede verificar lo que corre. Licencia MIT.
