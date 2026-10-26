# 0002 — Dibujo con WARP y capas cacheadas

Estado: aplicada (2026-10-26), medida en la PC del dueño.
Complementa: 0001.

## Contexto

La primera versión creaba el dispositivo D3D11 en la GPU por defecto. En la PC del dueño
(NVIDIA) el proceso cargaba el driver de usuario de NVIDIA (`nvgpucomp64.dll`, `nvwgf2umx.dll`):

| Medida | GPU (hardware) |
|---|---|
| Memoria privada | 59 MB |
| Working set | 61 MB |
| Hilos | 62 |
| CPU en reposo | 0 % |

Además, en laptops híbridas usar la GPU dedicada la mantiene despierta y gasta batería.

## Decisión

1. **WARP primero, hardware como respaldo.** El rasterizador de Windows dibuja nuestra
   superficie (pequeña, y solo cuando algo cambia); DWM la compone en la GPU igual.
2. **Una capa (bitmap) por vista** (cerrada, abierta, aviso). Se redibuja solo cuando
   cambia su contenido; cada cuadro de animación compone las capas con opacidad y escala.
   La barra de progreso (cambia cada segundo) se dibuja en vivo encima de la capa abierta.
   Las capas se alinean a píxel entero para que el texto quede nítido en reposo.

## Resultado

| Medida | GPU | WARP | WARP + capas |
|---|---|---|---|
| Memoria privada | 59 MB | 9,9 MB | 12,9 MB |
| Working set | 61 MB | 33 MB | 35 MB |
| Hilos | 62 | 18 | 24 |
| CPU en reposo (10 s, con música) | 0 % | 0 % | 0 % |
| CPU de un ciclo abrir + cerrar | ~150 ms | 266 ms | 141 ms |

## Notas de toolchain (local, GNU)

- `windows` 0.62 usa `raw-dylib`: el toolchain GNU necesita un `dlltool` que genere import
  libs. El `dlltool` incluido requiere `as`, que no viene. Solución: `llvm-ar.exe`
  (componente `llvm-tools`) copiado como `dlltool.exe` en el PATH.
- El `ld` de MinGW falla con rutas no ASCII (`C:\Users\Tomás`). Se enlaza con `rust-lld`
  (`.cargo/config.toml`, solo para el target GNU). La CI usa MSVC y no se ve afectada.
