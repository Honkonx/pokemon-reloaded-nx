# Procedencia de `pokenx-core`

Núcleo del runtime GameMaker 8 sin ventana ni audio de escritorio, derivado de **OpenGMK `gm8emulator`**.

| Elemento | Origen | Licencia | Cambios |
|---|---|---|---|
| Todo `src/` salvo lo indicado abajo | `OpenGMK/gm8emulator/src` en el commit `9e4282e6` (2026-10-13) | GPL-2.0-only (OpenGMK Project Developers) | Pasa a `#![no_std]` sobre `pokenx-std`; se eliminan `ramen`, `udon`, `imgui`, `clipboard`, `getopts`, grabación/TAS (`recording`, `savestate`, ffmpeg), código de Windows (`win32`, `wow64`, `glx`, `wgl`); `Game::run` se sustituye por `Game::start`/`Game::step` dirigidos por el host; `game_save`/`game_load` usan `postcard`; fechas desde un reloj del host; stubs de imagen y red |
| `src/platform.rs` | Propio, con la forma de la API de `ramen` que usa el emulador | GPL-2.0-only | Estado de ventana y cola de eventos entregada por el host; hooks `host::GlHost` y reloj |
| `src/audio_src.rs`, `src/game/audio/mp3.rs` | Propios, con el contrato `Source` del mezclador de OpenGMK (no se copió `udon`: licencia inconsistente) | GPL-2.0-only | Rechanneler, Resampler (lineal), Cycle, WAV; MP3 con `nanomp3-core` |
| `src/ini.rs`, `src/glob.rs`, `src/image.rs` | Propios | GPL-2.0-only | Sustituyen `rust-ini`, `glob` y `image` (sin decodificar archivos de imagen) |
| `src/rustysynth/` | `rustysynth` 1.3.7 de sinshu (MIT; la crate no trae LICENSE, ver `src/rustysynth/LICENSE`) | MIT | Rutas de crate, módulos de prueba retirados, `MidiFile::get_length` total |
| `src/image/codec.rs` | Propio | GPL-2.0-only | Decodificadores PNG/GIF/BMP y codificadores PNG/BMP sin `std` |
| `src/patches.rs`, partes de `kernel.rs`, `game.rs`, `audio*`, `opengl.rs`, `input.rs`, `asset/sprite.rs` | port Android de Klisee | Ver `CREDITOS.md` | Fusión a tres bandas |
| `src/rect_packer/` | `rect_packer` 0.2.1 de kryptan | MIT (`src/rect_packer/LICENSE`) | Rutas de módulos a edición 2021 |
| `src/render/glsl/*`, `data/arimo.dat`, `data/particles.dat` | OpenGMK `gm8emulator` | GPL-2.0-only (assets embebidos: licencia **sin verificar**) | Sin cambios |
| `build.rs` | OpenGMK `gm8emulator/build.rs` | GPL-2.0-only | Solo bindings GL 3.3 core y macro `_apply_args`; usa `core::hint` |

Aportes del port Android de Klisee integrados el 2026-10-16: tabla origen → destino al final de este documento y crédito en `CREDITOS.md`.

Actualización `50ab5465561459da00c34cd633685e670b63ae8b` frente a `765ae035`: `gml/runtime.rs` (lectura de instancias ausentes devuelve cero, tamaño de sprite sin escala), `asset/sprite.rs` (máscara placeholder sin píxeles: normalización durante carga), `render/opengl.rs` (UV de partes reflejadas, factorizado como helper probado en `render.rs`). El overlay y las APIs Android siguen fuera de la build Switch. El probe de ejecutables GM8 y el sondeo HID desde bucles GML/ABI v3 son adaptaciones propias GPL-2.0-only.

Dependencias externas nuevas con licencia: `nanomp3-core` (MIT OR Apache-2.0), `postcard` (MIT OR Apache-2.0), `hashbrown`/`spin`/`libm` vía `pokenx-std` (MIT/Apache).

Adaptación propia del rustysynth MIT (2026-10-16): validación de cabecera/tempo/ticks MIDI y protección del sequencer frente a bucles vacíos o sin avance temporal; conserva aviso MIT en src/rustysynth/LICENSE. Test PCM opcional con SoundFont externo atribuido, sin datos del juego.

Adaptación propia GPL-2.0-only (2026-10-17) sobre el renderer OpenGMK: render/opengl/shader.rs centraliza compilación/enlace, diagnósticos y ownership; opengl.rs valida cadenas de identidad GL. No incorpora overlay Android al objetivo Switch.

- 2026-10-17: lectura de globales/locales no inicializados devuelve 0 (port Android 24b0671; `gml/runtime.rs`). Pendiente de portar: ventana `show_info`/`show_image` (`info_window.rs`, requiere máquina de estados no bloqueante).

## Aportes del port Android (origen → destino)

Origen: [pokemon-reloaded-android](https://github.com/Klisee/pokemon-reloaded-android) de Klisee (commit `765ae03`, 2026-10-14; después `50ab546` y `f3194f0`).

| Origen (Android) | Destino | Cambio |
|---|---|---|
| `gm8emulator/src/asset/sprite.rs` (comparación del píxel completo al quitar el fondo; máscara vacía = colisión por bbox) | `pokenx-core/src/asset/sprite.rs` | Fusión a tres bandas, sin cambios de lógica |
| `gm8exe/src/asset/sprite.rs` (máscaras de colisión ausentes o truncadas) | `gm8exe/src/asset/sprite.rs` | Fusión a tres bandas |
| `gm8emulator/src/gml/kernel.rs` (funciones GML sin `panic`, `.gmspr`, `sprite_add_sprite`/`replace`/`duplicate`, `sound_replace` con rutas de datos, `room_set_code`, ratón, volumen de sonidos) | `pokenx-core/src/gml/kernel.rs` | Fusión; `flate2` → `miniz_oxide`; partes específicas de Android descartadas |
| `gm8emulator/src/game/audio.rs`, `audio/midi.rs`, `audio/mixer.rs` (MIDI por bloques con SoundFont, WAV con cabecera MP3) | `pokenx-core/src/game/audio*` | Sin `udon`/AAudio; SoundFont desde la microSD |
| `gm8emulator/src/game.rs`, `input.rs`, `render.rs`, `render/opengl.rs`, `gml/runtime.rs`, `gml/string.rs`, `asset/sound.rs`, `render/glsl/*` | `pokenx-core/src/...` | Fusión; EGL/GLES y la capa táctil de Android no se usan |
| `gm8emulator/src/patches.rs` | `pokenx-core/src/patches.rs` | Sin cambios |
| `gm8emulator/src/game/info_window.rs` (ventana de información del juego) | `pokenx-core/src/game/info_window.rs` | Sin la parte táctil |
