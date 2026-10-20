# Frequent errors

Two parts: **using the program** and **building it**. Each entry is *symptom → cause → what to do*.

## What to copy when asking for help

From the microSD, the `sdmc:/switch/pokenx/logs/` folder: **`boot.log`**, **`engine.log`** (and `engine.log.1`, the previous run), **`crash.log`**, **`last-session.txt`** and, if you can, **`informe.txt`** (menu → Diagnóstico → **X**). If the program closed by itself, add `sdmc:/atmosphere/crash_reports/*.log`. None of them contains game data. Also state your Atmosphère version, firmware and game version.

---

# A. Using the program

### Black screen on start, no menu and no logs
The program died before running its own code. If **`boot.log` does not exist**, look in `atmosphere/crash_reports/`. Known cause: an NRO built with relocations in read-only memory (the build's checker exists to prevent it; see [SYSTEM.md](SYSTEM.md#safe-start-of-the-nro)). If you built it yourself, run `scripts/check.ps1` again.

### `boot.log`: how far startup got

| Line | Means |
|---|---|
| `1. libnx initialised…` | libnx and the heap are ready (shows total memory and heap size) |
| `2. static constructors ran` | C++ static constructors executed |
| `3. main() started` | `main` was entered |
| `4. graphical menu ready` | the graphical menu could draw |
| `4. graphical menu FAILED…` | no framebuffer or font: the text menu is used |

### "Jugar" is blocked
You are in **applet mode** (low memory). Open the program as an application: open any game **while holding R** and choose PokeNX, or create a HOME-menu shortcut.

### Pressing "Jugar" leaves a black screen
The first load analyzes about 20,000 resources and can take tens of seconds. While loading you should see a bar and **+** cancels. In `engine.log` every line carries `[ms]`: look for `game started` (load complete) or `load failed` (reason). The menu shows the last exit reason (`last-session.txt`).

### "No game" although you copied it
There must be **one** standard GameMaker 8.0 `.exe` (and its `data/`) in `game/` or in an immediate subfolder. If there are several, leave one. Other formats (GameMaker 8.1, a compressed `.exe`) are not recognized. See [SYSTEM.md](SYSTEM.md#detection-and-game-versions).

### The game stutters or is slow when changing scenes
Look in `engine.log` at the `video: host … late N/M` lines (late frames), `pace: …` (how long the game paused itself, files read and room changes in those 5 s) and `room … loaded in …`. The game's own pauses (dialogs, scenes) are not an engine fault. Try the **file cache** and the **Máximo** profile (Opciones). Attach `informe.txt`.

### With Zink the screen stays black
The menu no longer offers Zink (it does not present frames yet); it only runs with `graphics-driver.txt` set to `zink` and a `dev-env.txt` present. It is experimental. Close the program and open it again: if Zink never presented a single frame, the driver goes back to `nvc0` by itself and the menu says so. To understand where it stalled, attach `engine.log.1`: the `mark: …` lines (texture upload, first presentations) and `mark: WATCHDOG …` give the last point reached.

### The controls do not do what you expect
Menu → **Mandos**: there you see which key the game receives for each button and can reset (**Y**). Exit the game: **L + R + +**.

### Updates: "could not check" or the download fails
Messages and what to do: **"not connected to the internet"** (the console's Wi-Fi); **"cacert.pem is missing"** (copy the package's `cacert.pem` to `switch/pokenx/`); **"could not verify the secure connection"** (check the console clock); **"Mega limited today's download"** (daily quota used up: try later, or download the ZIP on another device and use "Instalar desde un ZIP de la SD"). Downloads resume: what was downloaded stays in `downloads/`.

### My saves are not there after updating
They are kept in `data/partidas` and a copy is in `backups/` (Actualizaciones → Copias de seguridad → restore). The previous version of the game is deleted when updating (a single game on the card); your saves are in the new one and in the dated copy.

### A game key (F5, Page Up…) does nothing
Those keys are sent with the **layer** in Mandos: hold ZL and press the button shown on the Layer tab. The **Teclas** screen marks in red the keys no button produces.

### A game function fails in the middle of a session
A GameMaker function the engine does not implement fails the first time the script that calls it runs. It is more likely with a game version other than the tested one. Open an issue with `engine.log`, the room and the game version.

---

# B. Building

### I do not know what I am missing
Run `pwsh scripts/doctor.ps1`: it lists, by group, what is fine, what is missing (with the command or link) and warnings (for example, devkitPro outside its standard folder). With `-DevkitPro <folder>` you point it to another location.

### The Mesa script says "Review Mesa provenance…"
The Mesa clone is not at commit `2330d007712acbd32e396574203339df39bde153`. Run `git -C third_party/mesa-switch checkout 2330d007712acbd32e396574203339df39bde153`.

### "Required file missing: …" when building Mesa
A piece of the Mesa environment is missing (step 5 of the [guide](GUIDE.md)): the Python environment (`target/mesa-tools/venv`), `bindgen.exe` (`target/mesa-tools/cargo/bin`), `clang.exe`/`llvm-config.exe`/`ninja.exe` (devkitPro's MSYS2 clang64) or `libnx`. Install it and repeat the stage.

### The Mesa environment is not automated
A known limitation: today there are three manual steps (MSYS2 clang64 packages with a copy of `pacman.conf`, the Python environment, `bindgen-cli`). If something fails there, that is the first thing to check on a fresh install; `scripts/doctor.ps1` shows which piece is missing.

### devkitPro is not in `C:\devkitPro`, or Git is not in its standard folder
Some scripts use devkitPro's default location (`C:\devkitPro`) and Git Bash's (`C:\Program Files\Git\bin\bash.exe`). Install there or edit the variables at the top of `app/engine/build.ps1`, `scripts/build-mesa-switch.ps1` and `app/engine/tests/check-mesa-abi.ps1`.

### `Mesa and engine Rust compiler versions must match…`
Use **`nightly-2026-09-29`** for everything (engine and Mesa). The build compares the `rustc` versions and stops if they differ.

### `Mesa SDK archive differs from its build manifest`
The SDK was not generated with this code or is incomplete. Run `-Stage Install` again.

### `Relocation check failed` / `read-only segment has dynamic relocations`
**Do not silence it**: it is the warning that the NRO would not start. Review any change to `app/engine/switch-pokenx.ld` or the linker flags. See [SYSTEM.md](SYSTEM.md#safe-start-of-the-nro).

### `Single-NRO rule violated`
There must be exactly one `.nro` under `pokenx/`; delete the extras.

### `cargo` errors about the Rust version or `Cargo.lock`
Always use `cargo +nightly-2026-09-29 …`. The `gm8-*` crates inherit a different `rust-toolchain` from OpenGMK; the scripts already force the right one.

### `check.ps1` fails building the PC tests
The Visual Studio Build Tools (MSVC target) are required. The menu preview needs MinGW's `g++` and is optional: without it that stage is skipped.

### Windows only
The scripts are PowerShell for Windows; they have not been tested on Linux or macOS.
