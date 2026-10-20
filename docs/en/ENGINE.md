# The engine: what we changed in `gm8-runtime`

`src/gm8-runtime/` is the engine that runs the game. It starts from **OpenGMK** and was adapted to the Switch. This document says **what we changed, what we added and what we extended, why, and for what**.

## Where it comes from

| Piece | Origin | License |
|---|---|---|
| `pokenx-core` | Derived from `gm8emulator` in [OpenGMK](https://github.com/OpenGMK/OpenGMK) (commit `9e4282e6`) | GPL-2.0-only |
| `gm8exe`, `gml-parser`, `gm8-real` and the `gm8-*` crates | OpenGMK and the GameMaker 8.0 formats | GPL-2.0-only |
| Game-specific adaptations | [pokemon-reloaded-android](https://github.com/Klisee/pokemon-reloaded-android) by Klisee | See [CREDITS.en.md](../../CREDITS.en.md) |
| `pokenx-std`, `pokenx-io`, `pokenx-engine`, `gm8-input`, `gm8-audio-output`, `gm8-format-probe`, `switch-pe-probe` and several modules of `pokenx-core` | Own work | GPL-2.0-only |

The file-by-file table (origin, license and changes) is in [`src/gm8-runtime/pokenx-core/PROVENANCE.md`](../../src/gm8-runtime/pokenx-core/PROVENANCE.md) (in Spanish).

## What we changed, added and extended

### To compile and run on the Switch

| Change | Why | For what |
|---|---|---|
| **Moves to `no_std` on `pokenx-std`**: a minimal "std" for Horizon over newlib and `app/engine/host_services.cpp` | The Switch has no desktop system (windows, OS threads, Windows paths) | So OpenGMK's code compiles for `aarch64-nintendo-switch-freestanding` without rewriting all of it |
| **Removed** `ramen`, `udon`, `imgui`, `clipboard`, `getopts`, recording and replay (TAS), `ffmpeg` and the Windows code (`win32`, `wow64`, `glx`, `wgl`) | They are desktop-only and have no Horizon equivalent | Less code, fewer dependencies, a smaller binary |
| **`Game::run` is replaced by `start` / `step_deferred` / `present_host_frame`**, driven by the host | In OpenGMK the engine owns the main loop; on the Switch the application lifecycle does (HOME, suspend, focus) | **Present at 60 Hz separately from the game's simulation speed** (`room_speed`), and be able to pause or cancel |
| **C ABI version 3** (`pokenx-engine`: `PokenxHost`, 56 bytes; `PokenxPad`, 24 bytes) | The Switch program is C++ (libnx) and the engine is Rust | A stable contract between them; the engine is built as a static library with a newlib allocator and its own panic handler |
| **Dates from a host clock**, `game_save`/`game_load` with `postcard`, stubbed image and network | The engine has no wall clock or network | The game can save and query the date |

### To read and recognize the game

| Change | Why | For what |
|---|---|---|
| **Detection by content** (`loader.rs`, `gm8-format-probe`) | A `.exe`'s name and size do not identify a version | Recognize any standard GM8.0 build and reject anything else with a clear message; if there are several, do not pick at random |
| **Resource loaders** (`gm8-*`, `gm8exe`) with bounded reads | The `.exe` is an encrypted, `zlib`-compressed container | Read thousands of resources without executing anything from the `.exe` |
| **Validated ZIP importer** (`zip_import.rs`, `zip_sd.rs`) | Install the game from the console itself, with no PC | Validate paths, CRC, PE header, `data/` and GM8 structure **before** replacing `game/` (the previous one is deleted once the new one is in place; the saves carry over) |

### Audio, image and input without desktop dependencies

| Change | Why | For what |
|---|---|---|
| **Own audio mixer** (`audio_src.rs`): channel splitter, resampling, WAV, MP3 with `nanomp3-core` and MIDI with [`rustysynth`](https://github.com/sinshu/rustysynth) | `udon` (OpenGMK) is desktop-only and its licensing is inconsistent | 48 kHz PCM for the Switch's `audout`; MIDI with an optional SoundFont |
| **Own image codecs** (PNG/GIF/BMP: decode; PNG/BMP: encode) and **own `ini`** | They replace `image` and `rust-ini`, which need a real `std` | Load the game's images and GML's `ini_*` functions |
| **Input: `gm8-input`** with `ButtonMap`, `controls.txt`, a **key layer** and the stick dead zone | The button → key table was fixed and the game uses keys (F1–F12, page keys, Delete…) no single button gives | Remap from the menu; F1–F12 and page keys through the layer; analog-stick hysteresis and its dead zone; a single Joy-Con held sideways |
| **Player data on update** (`user_data.rs`) and **the game's key reader** (`keys_report.rs`) | Installing another version must not cost the saves; the game defines its keyboard in its own code | Dated backups and restore; carrying saves and settings to the new version; knowing which keys the game uses and which have no button |

### To load fast and not lose saves

| Change | Why | For what |
|---|---|---|
| **GML scripts are compiled on first use** (`asset/script.rs`) | Compiling about 5000 scripts at start took ≈ 2 s on a PC and much more on the Switch's CPU | On a PC with the real game, `Game::launch` went from 2.45 s to 0.42 s |
| **Files the game opens for reading are loaded whole and served from a cache** (`gml/file.rs`), which is emptied on any write, delete, rename or copy | One of the game's room transitions read 93 files / 12.5 MB line by line, and every `seek` of a `BufReader` drops its buffer and re-reads from the microSD | Scene changes without waits of tens of seconds; the cache can be disabled (`filecache.txt`) |
| **Large read buffer** when opening files (`pokenx_fs_read_buffer`) | newlib reads 1 KiB at a time by default: reading the 22 MiB `.exe` meant thousands of trips to the card | Reading the `.exe` in large blocks |
| **Atomic saves** (`write_atomic`, `.pokenx-save.bak` and `.tmp` copies) | A power cut in the middle of a save | Do not lose saves |
| **Timestamped log and a breakdown of `Game::launch` and `load_room`** | There was no way to know where time went on the console | Measure with real logs |

### Compatibility with the game

| Change | Why | For what |
|---|---|---|
| **The game's information window** (`show_info`, `show_image`, `load_info`), `.gmspr`, `sprite_add`/`replace`/`duplicate`, collision masks, GML functions that do not `panic` | The game uses functions OpenGMK did not cover or handled with a `panic` | The game does not stop on missing functions (contributions from the Android port by Klisee) |
| **`patches.rs`**: two in-memory GML fixes (the click area of the title and options menus) | Defects in the game itself: the right half of each entry did not respond | Clicks work; the game's file is **never modified** |

## What is not there

- Execution of the `.exe`'s x86 code (never).
- External DLLs and GameMaker extensions: they are native Windows code.
- Recording and replay, networking, a desktop window.

## Limits

- Tested with **one** game (version 1.9.2.2) on **one** console.
- A GameMaker function that is not implemented fails **the first time the script that calls it runs** (a consequence of compiling on first use).
- The effect of the cache and the buffers on the console is still to be measured; the figures above come from a PC with the real game.
