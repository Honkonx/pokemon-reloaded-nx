# Créditos y agradecimientos

Este proyecto existe gracias al trabajo de mucha gente. Gracias.

## Agradecimientos

| A quién | Por qué |
|---|---|
| **[OpenGMK](https://github.com/OpenGMK/OpenGMK)** | Reimplementación en Rust del motor de GameMaker 8: la base de todo el motor |
| **[Klisee](https://github.com/Klisee/pokemon-reloaded-android)** | Su port de Pokémon Reloaded a Android sobre OpenGMK: las adaptaciones específicas del juego |
| **F3R Productions** | Autores de **Pokémon Reloaded**, el juego para el que existe este proyecto. La última versión del juego se consigue en [su blog](https://pokemon-reloaded.blogspot.com/) |
| **[Mesa](https://www.mesa3d.org/)** y los equipos de **Nouveau**, **NVK** y **Zink** | Las implementaciones de OpenGL y Vulkan que mueven los gráficos |
| **[NaGaa95](https://github.com/NaGaa95/mesa-switch)** y **[danfromtico](https://github.com/danfromtico/mesa-switch)** | El port de Mesa a la Nintendo Switch (Horizon) |
| **[switchbrew](https://github.com/switchbrew)** | `libnx`, `nx-hbloader`, `nx-hbmenu` y `switch-tools` |
| **[devkitPro](https://devkitpro.org/)** | El kit de compilación para la Switch |
| **[Atmosphère](https://github.com/Atmosphere-NX/Atmosphere)** | El firmware personalizado en el que se ejecuta todo |
| **[Sean Barrett (stb)](https://github.com/nothings/stb)** | `stb_truetype`, con el que se dibuja el texto del menú |
| **[sinshu](https://github.com/sinshu/rustysynth)** | `rustysynth`, el sintetizador MIDI |
| **[kryptan](https://github.com/kryptan/rect_packer)** | `rect_packer`, el empaquetado de texturas |
| Autores de `nanomp3-core`, `miniz_oxide` y demás *crates* | Versiones exactas en `Cargo.lock` |

Gracias también a los autores de [halo-ce-universal](https://github.com/thelinkin3000/halo-ce-universal) y [nova3-nx](https://github.com/markaurel13/nova3-nx), cuyas medidas publicadas del hardware de la Switch (valores de relojes y el comportamiento de la ventana tras iniciar Mesa) sirvieron de referencia. No se copió su código.

## Licencias

El proyecto se distribuye bajo **GPL-2.0-only** ([LICENSE](LICENSE)), porque el motor deriva de OpenGMK.

Cada proyecto de la tabla de arriba conserva la licencia que declara en su propio repositorio o página (están enlazados): consúltala allí.

**Mesa:** gracias a Mesa y a quienes lo llevaron a la Switch. Su licencia, y las de sus componentes, están en su repositorio y su página: <https://gitlab.freedesktop.org/mesa/mesa> · <https://www.mesa3d.org/>. Si no la encuentras, mira allí o en el repositorio del port ([NaGaa95/mesa-switch](https://github.com/NaGaa95/mesa-switch), commit `2330d007712acbd32e396574203339df39bde153`).

## Marcas

Pokémon es una marca de Nintendo, Game Freak y Creatures. Este proyecto es independiente, no incluye ningún recurso del juego y no está afiliado ni respaldado por ellos ni por los autores de Pokémon Reloaded.
