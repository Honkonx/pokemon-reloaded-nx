# Pokémon Reloaded on Nintendo Switch

Run **Pokémon Reloaded** on a Nintendo Switch running Atmosphère, **with no Windows emulator in between**. An open project, for the community. Spanish is the main language of this repository: [README.md](README.md).

> [!IMPORTANT]
> **This repository contains no game files or game data.** You need your own copy (the `.exe` and its `data/` folder). This is an independent project, **not affiliated with or endorsed by Nintendo, Game Freak, The Pokémon Company or the authors of Pokémon Reloaded.**

> [!NOTE]
> **A hobby project, built with AI help.** This is an experiment and a pastime, not a product: I developed it with the help of [Claude Code](https://claude.com/claude-code) (an AI coding assistant), which wrote a good part of the code and the documentation under my direction. The launcher, the engine and the game may have bugs nobody has seen, it has been tested on a single console, and there are no guarantees or promised support. Reports and improvements are welcome.

## What this really is

Pokémon Reloaded is a GameMaker 8 game for Windows. **Nothing of the game is rewritten or recompiled here.** A single Switch program (`pokenx.nro`) reads the original `.exe` **only as a data file** and runs its GML code with an engine derived from [OpenGMK](https://github.com/OpenGMK/OpenGMK) (the Rust re-implementation of the GameMaker 8 engine) and from [Klisee's Android port](https://github.com/Klisee/pokemon-reloaded-android). **The `.exe`'s x86 code is never executed.**

This project provides everything the game expected from Windows that the Switch does not have: OpenGL graphics (Mesa), controllers, audio, memory management, a menu, game import and logs.

## Status

**Works partially.** Tested by one person, on one console (Atmosphère 1.12.1, firmware 23.0.0), with game version 1.9.2.2: it opens, renders, plays sound, responds to the controllers, and was played for a while without problems. Everything else is **unverified** without a report from you: other game versions, other consoles and firmwares, sustained 60 FPS, the Zink/NVK driver, 1080p docked.

## What you need

- A **Nintendo Switch with [Atmosphère](https://github.com/Atmosphere-NX/Atmosphere)** and the [Homebrew Menu](https://github.com/switchbrew/nx-hbmenu).
- **Your own copy of the game**: the latest version is available on the [blog of F3R Productions](https://pokemon-reloaded.blogspot.com/), the game's authors (tested: version 1.9.2.2).
- A microSD card with free space.
- For the full memory, launch the program **as an application** (for example, open any game while holding **R**). From the Album (applet mode) the menu blocks "Jugar" because of low memory.

## How to use it

1. Download the ZIP from the [latest release](../../releases) and extract it at the **root** of the microSD (creates `switch/pokenx/`).
2. Put your game in `switch/pokenx/game/` (the `.exe` and the `data/` folder), or use **Actualizaciones**: it downloads the version from the official blog or installs a ZIP already on the microSD.
3. Open **PokeNX** from the Homebrew Menu → **Jugar**.

The first load analyzes about 20,000 resources and takes a while; a progress bar is shown, HOME keeps working and **+** cancels.

## Controls

Defaults: A/B/X/Y → Z/X/C/V, L/R → M/B, ZL/ZR → S/O, +/− → Space/Enter, stick clicks → E/Esc, D-pad and left stick → arrow keys. Remap everything in **Mandos**. **Exit the game: L + R + +.** No on-screen touch controls.

The game uses keys no single button gives (F1–F12, Delete, Insert, Home, End, Page Up/Down). A **layer** covers them: hold **ZL** and press another button (ZL + A = F5, ZL + B = F6, ZL + D-pad = F1, F9, Page Up, Page Down…); a short tap of ZL still sends its usual key. There are three controller profiles (**ZL/ZR** switch between them): *Por defecto*, *Perfil 1* and *Perfil 2*; the **Vista** tab draws the controller with the key of each button. A second layer on **ZR** gives digits 1–6 and menu letters (ZR + A = 1, ZR + D-pad = Q, P, L, F…). The **Teclas** screen lists the keys the game uses (yours, if you changed them while playing), which button produces each one and which have none.

## FPS meter and preload

**Opciones → Medidor de FPS** shows the frames per second and the longest gap between two frames in a corner. When the menu opens, the game data is parsed in the background (**Opciones → Precargar el juego**) so **Jugar** skips that wait. To contribute, see [CONTRIBUTING.en.md](CONTRIBUTING.en.md).

## If something fails

Copy `switch/pokenx/logs/` (and `atmosphere/crash_reports/` if the program closed). **Diagnóstico → X** writes `informe.txt`. See [docs/en/FAQ-ERRORS.md](docs/en/FAQ-ERRORS.md) first.

## Building

You need Windows, the Switch kit (devkitPro), Rust and the Mesa SDK for the Switch. Everything to install is in [REQUIREMENTS.en.md](REQUIREMENTS.en.md) and the steps are in [docs/en/GUIDE.md](docs/en/GUIDE.md) (for you or for an AI assistant; see [AGENTS.md](AGENTS.md) and [.claude/](.claude)).

```powershell
pwsh scripts/doctor.ps1          # checks you have everything needed (REQUIREMENTS.en.md)
pwsh app/build-package.ps1     # builds pokenx.nro and creates the microSD folder pokenx/
pwsh scripts/check.ps1         # checks that need no console
```

## Documentation

[Engine changes](docs/en/ENGINE.md) · [Graphics (Mesa, NVK, OpenGL)](docs/en/GRAPHICS.md) · [System (controllers, audio, menu, files, detection)](docs/en/SYSTEM.md) · [Build guide](docs/en/GUIDE.md) · [Frequent errors](docs/en/FAQ-ERRORS.md)

## Credits and license

GPL-2.0-only ([LICENSE](LICENSE)). Thanks to OpenGMK, Klisee, Mesa and the Switch homebrew projects: [CREDITS.en.md](CREDITS.en.md). Pokémon is a trademark of Nintendo, Game Freak and Creatures; this project is not affiliated with or endorsed by them.
