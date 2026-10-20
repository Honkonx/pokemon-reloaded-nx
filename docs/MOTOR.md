# El motor: qué cambiamos en `gm8-runtime`

`src/gm8-runtime/` es el motor que corre el juego. Parte de **OpenGMK** y se adaptó a la Switch. Este documento dice **qué cambiamos, qué añadimos y qué extendimos, por qué y para qué**.

## De dónde viene

| Pieza | Origen | Licencia |
|---|---|---|
| `pokenx-core` | Derivada de `gm8emulator` de [OpenGMK](https://github.com/OpenGMK/OpenGMK) (commit `9e4282e6`) | GPL-2.0-only |
| `gm8exe`, `gml-parser`, `gm8-real` y los `gm8-*` | OpenGMK y los formatos de GameMaker 8.0 | GPL-2.0-only |
| Adaptaciones específicas del juego | [pokemon-reloaded-android](https://github.com/Klisee/pokemon-reloaded-android) de Klisee | Ver [CREDITOS.md](../CREDITOS.md) |
| `pokenx-std`, `pokenx-io`, `pokenx-engine`, `gm8-input`, `gm8-audio-output`, `gm8-format-probe`, `switch-pe-probe` y varios módulos de `pokenx-core` | Propios | GPL-2.0-only |

La tabla archivo por archivo (origen, licencia y cambios) está en [`src/gm8-runtime/pokenx-core/PROVENANCE.md`](../src/gm8-runtime/pokenx-core/PROVENANCE.md).

## Qué cambiamos, añadimos y extendimos

### Para que compile y corra en la Switch

| Cambio | Por qué | Para qué |
|---|---|---|
| **Pasa a `no_std` sobre `pokenx-std`**: una «std» mínima para Horizon, sobre newlib y `app/engine/host_services.cpp` | La Switch no tiene el sistema de un escritorio (ventanas, hilos del SO, rutas de Windows) | Que el código de OpenGMK compile para `aarch64-nintendo-switch-freestanding` sin reescribirlo entero |
| **Se quitan** `ramen`, `udon`, `imgui`, `clipboard`, `getopts`, la grabación y repetición (TAS), `ffmpeg` y el código de Windows (`win32`, `wow64`, `glx`, `wgl`) | Son de escritorio y no tienen equivalente en Horizon | Menos código, menos dependencias y un binario más pequeño |
| **`Game::run` se sustituye por `start` / `step_deferred` / `present_host_frame`**, dirigidos por el anfitrión | En OpenGMK el motor es dueño del bucle; en la Switch lo es el ciclo de vida de la aplicación (HOME, suspensión, foco) | **Presentar a 60 Hz separado de la velocidad de simulación** del juego (`room_speed`), y poder pausar o cancelar |
| **ABI C versión 3** (`pokenx-engine`: `PokenxHost` de 56 bytes, `PokenxPad` de 24) | El programa de la Switch es C++ (libnx) y el motor es Rust | Un contrato estable entre los dos; el motor se compila como biblioteca estática con asignador de newlib y manejador de pánico propios |
| **Fechas desde un reloj del anfitrión**, `game_save`/`game_load` con `postcard`, imagen y red *stubbed* | No hay reloj de pared ni red en el motor | Que el juego pueda guardar y consultar la fecha |

### Para leer y reconocer el juego

| Cambio | Por qué | Para qué |
|---|---|---|
| **Detección por contenido** (`loader.rs`, `gm8-format-probe`) | El nombre y el tamaño del `.exe` no identifican una versión | Reconocer cualquier compilación GM8.0 estándar y rechazar con un mensaje claro lo que no lo es; si hay varias, no elegir al azar |
| **Cargadores de recursos** (`gm8-*`, `gm8exe`) con lectura acotada | El `.exe` es un contenedor cifrado y comprimido con `zlib` | Leer miles de recursos sin ejecutar nada del `.exe` |
| **Importador de ZIP validado** (`zip_import.rs`, `zip_sd.rs`) | Instalar el juego desde la propia consola, sin PC | Validar rutas, CRC, cabecera PE, `data/` y estructura GM8 **antes** de sustituir `game/` (la anterior se borra al terminar; las partidas pasan a la nueva) |

### Para audio, imagen y entrada sin dependencias de escritorio

| Cambio | Por qué | Para qué |
|---|---|---|
| **Mezclador de audio propio** (`audio_src.rs`): repartidor de canales, remuestreo, WAV, MP3 con `nanomp3-core` y MIDI con [`rustysynth`](https://github.com/sinshu/rustysynth) | `udon` (OpenGMK) es de escritorio y su licencia es inconsistente | PCM a 48 kHz para el `audout` de la Switch; MIDI con una SoundFont opcional |
| **Códecs de imagen propios** (PNG/GIF/BMP: decodificar; PNG/BMP: codificar) y **`ini` propio** | Sustituyen a `image` y `rust-ini`, que necesitan `std` real | Cargar imágenes del juego y las funciones `ini_*` de GML |
| **Entrada: `gm8-input`** con `ButtonMap`, `controls.txt`, **dos capas de teclas** y zona muerta del palo | La tabla botón → tecla estaba fija y el juego usa teclas (F1–F12, páginas, Supr…) que ningún botón da solo | Reasignar desde el menú; F1–F12 y páginas con la capa; histéresis del palo analógico y su zona muerta; un Joy-Con solo en horizontal |
| **Datos del jugador al actualizar** (`user_data.rs`) y **lector de teclas del juego** (`keys_report.rs`) | Instalar otra versión no debe costar las partidas; el juego define su teclado en su propio código | Copias fechadas y restauración; pasar partidas y ajustes a la versión nueva; saber qué teclas usa el juego y cuáles no tienen botón |

### Para que cargue rápido y no pierda partidas

| Cambio | Por qué | Para qué |
|---|---|---|
| **Los scripts de GML se compilan al primer uso** (`asset/script.rs`) | Compilar unos 5000 scripts al arrancar tardaba ≈ 2 s en un PC y mucho más en el CPU de la Switch | En un PC con el juego real, `Game::launch` pasó de 2,45 s a 0,42 s |
| **Los archivos que el juego abre para leer se cargan enteros y se sirven de una caché** (`gml/file.rs`), que se vacía con cualquier escritura, borrado, renombrado o copia | Una transición de sala del juego leía 93 archivos / 12,5 MB línea a línea, y cada `seek` de un `BufReader` descarta su búfer y relee de la microSD | Cambios de escena sin esperas de decenas de segundos; la caché se puede desactivar (`filecache.txt`) |
| **Búfer grande de lectura** al abrir archivos (`pokenx_fs_read_buffer`) | newlib lee de 1 KiB en 1 KiB por defecto: leer el `.exe` de 22 MiB eran miles de viajes a la tarjeta | Lectura del `.exe` en bloques |
| **Guardado atómico** (`write_atomic`, copias `.pokenx-save.bak` y `.tmp`) | Un corte de energía a mitad de un guardado | No perder partidas |
| **Registro con marcas de tiempo y desglose de `Game::launch` y `load_room`** | No se podía saber dónde se iba el tiempo en la consola | Medir con registros reales |

### Compatibilidad con el juego

| Cambio | Por qué | Para qué |
|---|---|---|
| **Ventana de información del juego** (`show_info`, `show_image`, `load_info`), `.gmspr`, `sprite_add`/`replace`/`duplicate`, máscaras de colisión, funciones GML que no hacen `panic` | El juego usa funciones que OpenGMK no cubría o trataba con un `panic` | Que el juego no se detenga por funciones ausentes (aportes del port Android de Klisee) |
| **`patches.rs`**: dos correcciones de GML en memoria (el área de clic de los menús del título y de opciones) | Defectos del propio juego: la mitad derecha de cada entrada no respondía | Que los clics funcionen; el archivo del juego **nunca se modifica** |

## Lo que no hay

- Ejecución del código x86 del `.exe` (nunca).
- DLL externas y extensiones de GameMaker: son código nativo de Windows.
- Grabación y repetición, red, ventana de escritorio.

## Límites

- Probado con **un** juego (versión 1.9.2.2) en **una** consola.
- Una función de GameMaker sin implementar falla **al primer uso del script** que la llama (consecuencia de compilar al primer uso).
- El efecto de la caché y de los búferes en la consola está por medir; las cifras de arriba son de un PC con el juego real.
