# System: controllers, audio, menu, files, detection and lifecycle

Everything around the engine and the renderer: what the game expected from Windows and the Switch program (`app/engine/`) does here.

## Controllers

- **What is read:** up to 8 controllers, handheld mode, Pro Controller and Joy-Con. A **single Joy-Con** is read held sideways (the stick is rotated; SL/SR act as shoulders).
- **Buttons → keys:** each button produces a GameMaker key (`gm8-input::ButtonMap`). The defaults and how to change them are in the [README](../../README.en.md#controls); the **Mandos** screen remaps them and saves them to `controls.txt` (format `BUTTON=KEY`, one pair per line; buttons `A B X Y L R ZL ZR PLUS MINUS STICK_L STICK_R`; keys: letters, digits, `SPACE`, `ENTER`, `ESC`, `TAB`, `SHIFT`, `CTRL`, `ALT`, `BACKSPACE`, `DELETE`, `INSERT`, `HOME`, `END`, `PAGEUP`, `PAGEDOWN` and `F1`–`F12`).
- **Key layer:** `LAYER=ZL` (or `ZR`, `L`, `R`, `NONE`) picks the modifier button and `LAYER.A=F5`, `LAYER.UP=F1`… give each button's key while it is held. With the layer on, the modifier sends nothing by itself; a short tap (no other button) sends its normal key for about 70 ms. Defaults: A=F5, B=F6, X=F11, Y=F12, L=F10, R=Delete, +=Home, −=End, D-pad = F1 / F9 / Page Up / Page Down.
- **Second layer (ZR):** `LAYER2=ZR` and `LAYER2.A=1`… work like the first and give the digits and letters no button gave: A, B, X, Y, L, R = 1–6 (party members and field moves), D-pad = Q, P, L, F (missions, trainer, areas and search menus), + = T, − = N, left-stick click = H, right-stick click = K. If both modifiers are held the first layer wins. A `controls.txt` without `LAYER2` lines uses these values.
- **Controller profiles:** *Por defecto* (factory, not editable), *Perfil 1* (`controls.txt`, the original file) and *Perfil 2* (`controls-2.txt`). `profile.txt` (`default`, `1` or `2`) says which one the game uses; in **Mandos**, **ZL/ZR** switch profile and the **Vista** tab draws the controller with each button's key (and, in green, its layer key).
- **D-pad and left stick:** always the arrow keys; the stick uses hysteresis (by default on at 55 %, off at 40 %) so it does not flutter. The **Zona muerta del palo** option (`stickzone.txt`: `small` 35 %, `normal`, `large` 70 %) adjusts it.
- **Exit the game:** `L + R + +` (with a single Joy-Con: `ZL + ZR` + `+` or `−`).
- **No on-screen touch controls.**

## Audio

- Output through `audout`: a ring of 4 buffers of 1024 stereo samples at 48 kHz, on its own thread (core 1, priority `0x2B`).
- The engine's mixer delivers PCM: WAV, MP3 and MIDI. **Without `soundfont.sf2`, MIDI music is silent**; put it in the program's folder.
- The thread pauses while the application loses focus (HOME, suspend).

## Menu

- **Graphical** (`app/engine/gfx.cpp`, `menu.cpp`): a 1280×720 linear libnx *framebuffer* with its own shapes and **the system font** (`stb_truetype`). It does not use OpenGL: it runs before EGL and releases the window when it closes.
- **Text fallback** (`menu_text.cpp`): if the framebuffer or the font fail to start, libnx's text console is used. `boot.log` (line 4) says which one was used.
- **Screens:** Home (tiles), Updates (new version, download, microSD ZIP and backups), Options, Controls (Buttons, Layer 1, Layer 2 and View tabs), Keys (the game's and which button produces each), HOME shortcut (creates the icon on the home screen), Diagnostics and notice cards.
- **PC preview:** `tools/menu_preview` builds `gfx.cpp` and `menu.cpp` with a stand-in `switch.h` and saves PNG captures of each screen. It checks the drawing, not the console.

## Files and import

- **Game updates:** the official blog publishes one post per version with a Mega link. The menu reads its feed, asks Mega for the file name and size, downloads and decrypts the ZIP (AES-CTR, in ranges, resumable) into `downloads/` and installs it with the importer. It needs Wi-Fi, `cacert.pem` in the program folder (it ships in the package) and free space (≈ 2.5 times the ZIP). Mega limits daily transfer for users without an account: if it fails, install a ZIP downloaded by hand.
- **Your data on update or import:** before `game/` changes, a dated copy is made (`backups/copia-NNN`, up to 5) and the new version receives the **saves** (`data/partidas`), the **settings** (`data/text/config.ini` and `data/audio/config.ini`; if the new version's differs it is kept as `config.ini.nuevo`), the **screenshots** and `LogBattle.txt`. Boxes, teams and custom characters are copied without overwriting what the new version ships. That list comes from the game's own file calls.
- **The game's keys:** the game keeps its keyboard layout in variables its code fills in. The menu reads them once (a few seconds; the program must be opened as an application) into `cache/game-keys.txt` and compares them with your buttons and the layers. While you play, the engine notes every couple of seconds the keys the game is using at that moment (`cache/game-keys-live.txt`); if you changed keys inside the game, the screen shows yours and marks the changed ones with «antes» (before).
- **Compatibility per version:** in Updates, «Compatibilidad de esta versión» counts the built-in GameMaker functions the installed version calls and separates those the engine lacks from those it runs with less detail (`cache/game-compat.txt`). It is offered after installing a new version. It is a code analysis, not proof that everything works: it cannot see calls the game computes while running.
- **HOME shortcut from the program:** the HOME shortcut screen creates a small title (a «forwarder») that starts `sdmc:/switch/pokenx/pokenx.nro`: it builds three content files in memory, writes them to the SD card and registers the application (A creates or updates, Y removes it). It needs Atmosphère with *sigpatches*. It has not been tried on a console yet.
- **Import:** a microSD browser; the ZIP is extracted to a temporary folder and **validated before replacing `game/`**: dangerous paths (`..`, absolute), CRC, PE header, `data/` folder and GM8 structure. The previous version is **deleted** when it finishes: there are never two copies of the game (your saves move to the new one and a dated copy stays in `backups/`). **B** or **+** cancel without harming the current installation.
- **Saves:** done by the game itself; the engine writes them atomically and leaves recovery copies `.pokenx-save.bak` and `.pokenx-save.tmp.N` (do not delete them when cleaning up).
- **File cache** (`filecache.txt`): the game re-reads the same data when changing scene; it is kept in memory and emptied on any write.

## Detection and game versions

- The program looks in `game/` (and, if there is nothing at the root, in its immediate subfolders) for an `.exe` that is a **standard GameMaker 8.0 container**: `MZ` header, i386 `PE\0\0` signature, the GM8.0 loader sequence at fixed positions and header version 800. **Name and size play no part.** If more than one valid container exists, the program says so and does not choose: leave only the version you want.
- **Tested version:** 1.9.2.2 (an `.exe` with no DLLs or extensions). With another version these can happen, from most to least likely:
  1. **A GameMaker function the engine does not implement yet** (it fails the first time the script that calls it runs).
  2. **External DLLs or extensions**: they are x86 code and are not executed.
  3. **Another format** (GameMaker 8.1, a UPX-compressed `.exe`, anti-decompilation): it is not detected and the menu says there is no game.
  4. **The GML patches** (`patches.rs`) are applied by script name; if it changes they are not applied and the original defect returns.
  5. **Saved games**: they depend on the game; make a copy before changing version.
- **Checking a new version** (for those who build): `inspect_compatibility` counts the functions the code uses without running anything or showing names (`cargo +nightly-2026-09-29 run --release -p pokenx-core --example inspect_compatibility -- "<path-to-game.exe>"`), and the `real_game_smoke` test opens the game **from a PC folder** with Windows OpenGL, advances frames and reports the first error with the room and frame (see `host_gl_fixture.rs`).

## Lifecycle and memory

- **Startup order** (libnx), useful for reasoning about "there is no log": `_start` → `__nx_dynamic` (relocations and RELRO protection) → `envSetup` → `newlibSetup` → `virtmemSetup` → `__libnx_initheap` → `__appInit` (services, microSD, `userAppInit`: **line 1 of `boot.log`**) → static constructors (**line 2**) → `main` (**line 3**) → menu (**line 4**).
- **Application vs applet mode:** as an application the process has ≈ 3 GiB; from the Album (applet), 380–512 MiB. Below 1 GiB, "Jugar" is blocked.
- **While loading:** `loading_pump` keeps HOME and the controllers alive, lets you **cancel with `+`** and draws a bar.
- **Exceptions:** `crash.log` (type, `pc`, `lr`, `sp`, registers). **`last-session.txt`** stores the reason for the last exit and the menu shows it next time.
- **The window is not reopened after Mesa**: the menu is only shown when the program opens.

## FPS meter, preload and caches

- **FPS meter** (**Opciones → Medidor de FPS**, `fps.txt`): a box in the corner of the game with the presented frames per second and the longest gap between two frames (ms); green at 55 or more, yellow below, red below 40. It is painted with scissored OpenGL clears right before the frame is presented, touching none of the game's programs, textures or buffers (it only changes and restores the scissor, clear colour and colour mask). The **game's own pauses** (GML `sleep()` in dialogs and scenes, 200 to 2000 ms, during which no frame is presented) are left out: they do not count as a drop, and if the window had any, a third line «P» gives their milliseconds.
- **Preload** (**Opciones → Precargar el juego**, `preload.txt`): when the menu opens, the game's data is read and parsed on a thread; **Jugar** skips that work if it has finished. Title mode only. Anything that calls into the engine (import, backups, reading keys) waits for that thread first.
- **Caches:** whole files (`filecache.txt`) and **decoded images** (up to 48 MiB; `POKENX_IMAGE_CACHE_MB`), dropped on any write. Room and instance creation code is compiled when used, like scripts; the result is **kept** between visits (a room already visited is not compiled again).
- **Compiling ahead during pauses:** the engine notes which room usually follows each room (`cache/room-hints.txt`, lines `from to count`) and, when the game pauses itself (`sleep` of 30 ms or more), spends up to half of that pause compiling the code of the rooms that most often come next. The game waits the same time; only the hitch on entering is saved. If the file belongs to another game version, only rooms nobody visits get compiled.
- **Events the game adds at start:** the game calls `object_event_add` and `object_event_clear` over a hundred times when it starts. Each call used to rebuild the lists of every event of every object; it now updates only the list of the affected event (collision events are still rebuilt whole). The result is identical (a test compares it with the full rebuild) and entering the first room went from ≈ 0.97 s to ≈ 0.2 s on a PC.
- **The caches are no longer emptied whole on a write:** writing, renaming, copying or deleting a file evicts only what may have become stale (the same path, what lies under it, or a file with the same name); folders still empty everything. A save no longer forces dozens of files to be read again and the images to be decoded again.

## Clocks and performance

`clkrst` raises CPU, memory and GPU while playing and restores them on exit. Profiles in **Opciones → Rendimiento**: *Máximo* (CPU 1785 MHz; GPU 460.8 MHz handheld / 768 MHz docked), *Equilibrado* (1224 MHz; 384 / 614.4 MHz) and *Sin cambios*.

## Logs and diagnostics

`logs/boot.log`, `engine.log` (millisecond timestamps; `engine.log.1` is the previous run), `crash.log`, `last-session.txt` and `informe.txt` (**Diagnóstico → X**: memory, mode, load times, settings and the tails of the logs, with no game data). What to do with them: [FAQ-ERRORS.md](FAQ-ERRORS.md).

### What `engine.log` records while playing

- `video: host … Hz, … max host work …, late N/M` every 5 s, and right after it `pace: …`: how many milliseconds the game paused itself (`sleep`) and in how many pauses, how many files it read from the card (and KiB), how many room changes happened (engine time, pauses excluded, and the longest) and how many rooms were compiled ahead. It tells «the engine is slow» from «the game paused» or «it changed room».
- `mark: …` lines (written to the card at once): the texture upload to the GPU (`mark: atlas 1/1 …`), the first three presentations (`mark: eglSwapBuffers #1 begins` / `done in N ms`) and the first two frames. If the program stalls inside a graphics-driver call, the last mark says which.
- Watchdog: a low-priority thread checks every second that something moves (log lines, frames, pad polls). If nothing moves for 10 s, 30 s, 1, 2, 3, 5, 10 and 15 min, it writes `mark: WATCHDOG: no progress for N s; …; last checkpoint at … ms: <last mark>` and flushes the log. It does not act on the game; it only leaves a record.
- **Developer file `dev-env.txt`** (not in the menu): `NAME=VALUE` lines applied before EGL starts, only with the prefixes `NVK_`, `MESA_`, `NOUVEAU_`, `ZINK_` or `POKENX_` (for example `POKENX_ATLAS_MAX=2048` for smaller atlases). Each applied line is written to `engine.log`.
- **Zink that never presents a frame** (the menu no longer offers Zink; it only runs with `graphics-driver.txt` set to `zink` **and** a `dev-env.txt` present, even an empty one; a leftover `zink` without that file is ignored): starting a session with the `zink` driver creates `zink-pending.txt`, removed when the game's first frame is presented (or when the session ends by itself). If the program was closed by force before that, the next start sets the driver back to `nvc0` and the menu explains it; Zink can be chosen again in Opciones.

## Safe start of the NRO

An NRO linked with relocations in read-only memory used to die **before showing anything** (black screen, no log). The cause: the homebrew loader maps `.rodata` read-only, but libnx writes every relocation at start, and the non-PIC libstdc++/libc objects from the kit produce them. The fix: `app/engine/switch-pokenx.ld` moves `.rodata` to RELRO's writable area, and `scripts/verify_relocations.py` **stops the build** if any relocation falls outside a writable segment. The linker warning `read-only segment has dynamic relocations` is not silenced: it is resolved.
