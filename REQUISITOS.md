# Requisitos para compilar `pokenx.nro`

Lista completa de lo que hay que instalar en **Windows** para compilar todo lo que se usa y el `.nro`. Las versiones son las **verificadas** (con las que se compiló y se probó); otras pueden funcionar, pero no están comprobadas. Los enlaces llevan a los proyectos de origen. **Nada de esto está dentro del repositorio.** Para comprobar tu entorno automáticamente: `pwsh scripts/doctor.ps1`. Los pasos, en [docs/GUIA.md](docs/GUIA.md); si algo falla, en [docs/ERRORES-FRECUENTES.md](docs/ERRORES-FRECUENTES.md).

### A. Sistema y herramientas generales

| Herramienta | Versión verificada | Para qué | Enlace |
|---|---|---|---|
| Windows | 10 (build 19045), 64 bits | Los scripts son PowerShell para Windows | — |
| Git para Windows (incluye Git Bash) | 2.x reciente | Clonar; el script de Mesa usa `bash.exe` de Git | <https://github.com/git-for-windows/git> |
| PowerShell | 7.x | Ejecutar los scripts `.ps1` | <https://github.com/PowerShell/PowerShell> |
| Python | 3.13 | Verificador de relocaciones, composición del archivo de Mesa y entorno de Mesa | <https://www.python.org/downloads/> |
| Build Tools de Visual Studio (carga «C++» con MSVC y Windows SDK) | *sin verificar la versión mínima* | El Rust de Windows usa el objetivo MSVC para sus scripts de compilación y pruebas | <https://visualstudio.microsoft.com/downloads/> |
| g++ de MinGW-w64 (WinLibs UCRT) | 16.1.0 | **Opcional**: vista previa del menú en el PC | <https://github.com/brechtsanders/winlibs_mingw> |

### B. Kit de la Switch (devkitPro)

| Componente | Versión verificada | Para qué | Enlace |
|---|---|---|---|
| devkitPro (instalador/actualizador, incluye su MSYS2 y `pacman`) | actual | Toolchain y bibliotecas de la Switch | <https://github.com/devkitPro/installer> · <https://devkitpro.org/wiki/Getting_Started> |
| `devkitA64` (GCC 16.1.0, newlib 4.6.0, binutils 2.46.0) | 16.1.0-1 | Compilar y enlazar el C++ del programa | <https://github.com/devkitPro/pacman-packages> |
| `libnx` | 4.12.0-1 | Servicios de la Switch (EGL, HID, audio, relojes, ventana) | <https://github.com/switchbrew/libnx> |
| `switch-tools` (`elf2nro`, `nacptool`) | 1.13.1 | Crear el `.nro` | <https://github.com/switchbrew/switch-tools> |
| Portlibs `switch-zlib`, `switch-libexpat`, `switch-libzstd` | 1.3.1 · 2.2.5 · 1.5.7 | Las enlaza Mesa | <https://github.com/devkitPro/pacman-packages> |
| Portlibs `switch-curl`, `switch-mbedtls` | 7.69.1-5 · 2.28.10-1 | Descarga HTTPS y descifrado de las actualizaciones del juego | <https://github.com/devkitPro/pacman-packages> |

### C. Rust

| Componente | Versión verificada | Para qué | Enlace |
|---|---|---|---|
| `rustup` | actual | Instalar el toolchain fijado | <https://github.com/rust-lang/rustup> |
| Toolchain `nightly-2026-09-29` (rustc 1.101.0-nightly) con el componente `rust-src` | exactamente esa fecha | El motor (`-Z build-std` para `aarch64-nintendo-switch-freestanding`) | <https://rust-lang.github.io/rustup/> |
| Target `aarch64-unknown-linux-gnu` instalado en ese toolchain | — | El código Rust interno de Mesa | — |
| `bindgen-cli` | **0.72.1** (0.72.0 lo rechaza Mesa) | Mesa; `cargo +nightly-2026-09-29 install bindgen-cli --version 0.72.1 --locked` | <https://github.com/rust-lang/rust-bindgen> |

### D. Herramientas para compilar Mesa

| Componente | Versión verificada | Dónde | Enlace |
|---|---|---|---|
| MSYS2 clang64 (se instala en el MSYS2 de devkitPro con una copia de `pacman.conf` que habilita `[clang64]`) | — | | <https://www.msys2.org/> |
| `clang`, `clang-libs`, `llvm`, `llvm-libs`, `llvm-tools` | 23.1.3 | MSYS2 clang64 | <https://github.com/llvm/llvm-project> |
| `cmake` | 4.4.4 | MSYS2 clang64 | <https://github.com/Kitware/CMake> |
| `ninja` | 1.13.x | MSYS2 clang64 | <https://github.com/ninja-build/ninja> |
| `spirv-tools` | 1.4.363.0 | MSYS2 clang64 | <https://github.com/KhronosGroup/SPIRV-Tools> |
| `spirv-llvm-translator` | 23.1.2 | MSYS2 clang64 | <https://github.com/KhronosGroup/SPIRV-LLVM-Translator> |
| `cbindgen` | 0.29.4 | MSYS2 clang64 | <https://github.com/mozilla/cbindgen> |
| `flex`, `bison` | 2.6.4 · 3.8.2 | MSYS2 (msys) | <https://github.com/westes/flex> · <https://www.gnu.org/software/bison/> |
| Entorno virtual de Python: `meson` 1.10.1, `mako` 1.4.3, `pyyaml` 6.0.3, `packaging` 26.3 | las indicadas (`scripts/mesa/requirements.txt`) | `target/mesa-tools/venv` | <https://github.com/mesonbuild/meson> · <https://github.com/sqlalchemy/mako> · <https://github.com/yaml/pyyaml> · <https://github.com/pypa/packaging> |
| **Código fuente de Mesa para la Switch** | commit `2330d007712acbd32e396574203339df39bde153` (Mesa 26.2.4) | clon en `third_party/mesa-switch` | <https://github.com/NaGaa95/mesa-switch> (origen: <https://github.com/danfromtico/mesa-switch>; Mesa: <https://gitlab.freedesktop.org/mesa/mesa>) |

### E. Opcional: el acceso directo de HOME (`scripts/build-nsp.ps1`)

| Qué | Para qué | Dónde |
|---|---|---|
| `prod.keys` de tu propia consola | Cifrar las cabeceras del título (solo se lee; nunca se copia ni se registra) | Lo obtienes de tu consola |
| `hacbrewpack` | Empaqueta los NCA y el NSP | <https://github.com/The-4n/hacBrewPack/releases> |
| Un instalador de títulos (DBI, Tinfoil o Goldleaf) y *sigpatches* de Atmosphère | Instalar el NSP en la consola | Proyectos propios; sin ellos el acceso no se puede instalar |

### F. Espacio y tiempo (orientativo)
Mesa tarda mucho en compilar y el SDK resultante ocupa ≈ 220 MB; los directorios `target/` de Rust, varios GB. Los números exactos se anotan en la guía cuando se midan en una instalación limpia.

