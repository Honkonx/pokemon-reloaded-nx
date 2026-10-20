---
name: gm8-load-performance
description: Medir y reducir los tiempos de carga del juego GM8 en la Switch (arranque, cambio de sala, lecturas de archivos) con el registro con marcas de tiempo y la prueba real_game_smoke del PC; úsala ante «tarda mucho en cargar».
---

# Tiempos de carga del juego

## Medir (sin consola)
1. Con el juego en una carpeta local del PC (nunca copiarlo al repositorio), desde `src/gm8-runtime`: `POKENX_REAL_GAME=<ruta-del-juego> POKENX_REAL_FRAMES=3000 POKENX_REAL_FUZZ=7 [POKENX_REAL_PROFILE=1] cargo +nightly-2026-09-29 test -p pokenx-core --release real_game_smoke -- --ignored --nocapture`.
2. Salida útil: `launch: before …` (fases de `Game::launch`), `SLOW frame N room R: <t> (files read: n, bytes)` (fotogramas de más de 100 ms y archivos que leyeron), `room R loaded in …` (desglose de `load_room` si pasa de 150 ms) y, con `POKENX_REAL_PROFILE=1`, el muestreo de scripts y funciones.
3. En la consola, `logs/engine.log` lleva `[ms]` por línea: de `Memory (before load_game)` a `game started` es la carga; `video: … max host work N us` y `simulation clock reset after N ms` delatan fotogramas largos.

## Palancas ya aplicadas (no repetir)
- Scripts de GML compilados en el primer uso (`asset/script.rs`).
- Archivos de texto y binarios del juego leídos enteros a memoria y servidos de una caché con invalidación al escribir (`gml/file.rs`, `POKENX_FILE_CACHE_MB`; `filecache.txt`).
- Búfer de stdio grande en la lectura del `.exe` (`pokenx_fs_read_buffer`): newlib lee de 1 KiB en 1 KiB por defecto.
- Volcado del registro limitado a una escritura cada 100 ms.
- CPU: `-C target-cpu=cortex-a57` y `-mtune=cortex-a57`.

## Pendiente
Precarga durante el menú, análisis GM8 en paralelo, código de creación de salas perezoso, ruta nativa de la lectura de texto del juego, caché de recursos decodificados y asignador rápido.

## Reglas
- Una cifra solo vale con su fuente. Las estimaciones se etiquetan como estimaciones.
- No guardes píxeles decodificados en la microSD como caché: leerlos cuesta lo mismo que reanalizar el `.exe`.
- Toda caché de lectura debe invalidarse en cualquier escritura, borrado, renombrado o copia.
