# Credits and acknowledgements

This project exists thanks to the work of many people. Thank you.

## Acknowledgements

| To | For |
|---|---|
| **[OpenGMK](https://github.com/OpenGMK/OpenGMK)** | The Rust re-implementation of the GameMaker 8 engine: the base of the whole engine |
| **[Klisee](https://github.com/Klisee/pokemon-reloaded-android)** | Their Android port of Pokémon Reloaded on top of OpenGMK: the game-specific adaptations |
| **F3R Productions** | Authors of **Pokémon Reloaded**, the game this project exists for. The latest version of the game is available on [their blog](https://pokemon-reloaded.blogspot.com/) |
| **[Mesa](https://www.mesa3d.org/)** and the **Nouveau**, **NVK** and **Zink** teams | The OpenGL and Vulkan implementations that drive the graphics |
| **[NaGaa95](https://github.com/NaGaa95/mesa-switch)** and **[danfromtico](https://github.com/danfromtico/mesa-switch)** | The Mesa port to the Nintendo Switch (Horizon) |
| **[switchbrew](https://github.com/switchbrew)** | `libnx`, `nx-hbloader`, `nx-hbmenu` and `switch-tools` |
| **[devkitPro](https://devkitpro.org/)** | The Switch build kit |
| **[Atmosphère](https://github.com/Atmosphere-NX/Atmosphere)** | The custom firmware everything runs on |
| **[Sean Barrett (stb)](https://github.com/nothings/stb)** | `stb_truetype`, used to draw the menu text |
| **[sinshu](https://github.com/sinshu/rustysynth)** | `rustysynth`, the MIDI synthesizer |
| **[kryptan](https://github.com/kryptan/rect_packer)** | `rect_packer`, texture packing |
| Authors of `nanomp3-core`, `miniz_oxide` and the other crates | Exact versions in `Cargo.lock` |

Thanks also to the authors of [halo-ce-universal](https://github.com/thelinkin3000/halo-ce-universal) and [nova3-nx](https://github.com/markaurel13/nova3-nx), whose published measurements of the Switch hardware (clock values and the window behaviour after starting Mesa) were used as a reference. Their code was not copied.

## Licenses

The project is distributed under **GPL-2.0-only** ([LICENSE](LICENSE)) because the engine derives from OpenGMK.

Each project in the table above keeps the license it declares in its own repository or page (linked above): look it up there.

**Mesa:** thanks to Mesa and to everyone who brought it to the Switch. Its license, and those of its components, are in its repository and its site: <https://gitlab.freedesktop.org/mesa/mesa> · <https://www.mesa3d.org/>. If you cannot find it, look there or in the port's repository ([NaGaa95/mesa-switch](https://github.com/NaGaa95/mesa-switch), commit `2330d007712acbd32e396574203339df39bde153`).

## Trademarks

Pokémon is a trademark of Nintendo, Game Freak and Creatures. This project is independent, includes no game assets and is not affiliated with or endorsed by them or by the authors of Pokémon Reloaded.
