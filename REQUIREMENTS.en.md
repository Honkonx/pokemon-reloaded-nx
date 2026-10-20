# Requirements to build `pokenx.nro`

The complete list of what to install on **Windows** to build everything this project uses and the `.nro`. Versions are the **verified** ones (the ones used to build and test); others may work but are unchecked. Links go to the upstream projects. **None of this is inside the repository.** To check your environment automatically: `pwsh scripts/doctor.ps1`. Steps: [docs/en/GUIDE.md](docs/en/GUIDE.md); if something fails: [docs/en/FAQ-ERRORS.md](docs/en/FAQ-ERRORS.md).

## A. System and general tools

| Tool | Verified version | Used for | Link |
|---|---|---|---|
| Windows | 10 (build 19045), 64-bit | The scripts are PowerShell for Windows | — |
| Git for Windows (includes Git Bash) | recent 2.x | Cloning; the Mesa script uses Git's `bash.exe` | <https://github.com/git-for-windows/git> |
| PowerShell | 7.x | Running the `.ps1` scripts | <https://github.com/PowerShell/PowerShell> |
| Python | 3.13 | Relocation checker, Mesa archive composition and the Mesa environment | <https://www.python.org/downloads/> |
| Visual Studio Build Tools ("C++" workload with MSVC and Windows SDK) | *minimum version unverified* | Windows Rust uses the MSVC target for its build scripts and tests | <https://visualstudio.microsoft.com/downloads/> |
| MinGW-w64 g++ (WinLibs UCRT) | 16.1.0 | **Optional**: PC preview of the menu | <https://github.com/brechtsanders/winlibs_mingw> |

## B. Switch kit (devkitPro)

| Component | Verified version | Used for | Link |
|---|---|---|---|
| devkitPro (installer/updater, ships its own MSYS2 and `pacman`) | current | Switch toolchain and libraries | <https://github.com/devkitPro/installer> · <https://devkitpro.org/wiki/Getting_Started> |
| `devkitA64` (GCC 16.1.0, newlib 4.6.0, binutils 2.46.0) | 16.1.0-1 | Compile and link the program's C++ | <https://github.com/devkitPro/pacman-packages> |
| `libnx` | 4.12.0-1 | Switch services (EGL, HID, audio, clocks, window) | <https://github.com/switchbrew/libnx> |
| `switch-tools` (`elf2nro`, `nacptool`) | 1.13.1 | Create the `.nro` | <https://github.com/switchbrew/switch-tools> |
| Portlibs `switch-zlib`, `switch-libexpat`, `switch-libzstd` | 1.3.1 · 2.2.5 · 1.5.7 | Linked by Mesa | <https://github.com/devkitPro/pacman-packages> |
| Portlibs `switch-curl`, `switch-mbedtls` | 7.69.1-5 · 2.28.10-1 | HTTPS download and decryption of the game updates | <https://github.com/devkitPro/pacman-packages> |

## C. Rust

| Component | Verified version | Used for | Link |
|---|---|---|---|
| `rustup` | current | Installing the pinned toolchain | <https://github.com/rust-lang/rustup> |
| Toolchain `nightly-2026-09-29` (rustc 1.101.0-nightly) with the `rust-src` component | exactly that date | The engine (`-Z build-std` for `aarch64-nintendo-switch-freestanding`) | <https://rust-lang.github.io/rustup/> |
| Target `aarch64-unknown-linux-gnu` installed in that toolchain | — | Mesa's internal Rust code | — |
| `bindgen-cli` | **0.72.1** (Mesa rejects 0.72.0) | Mesa; `cargo +nightly-2026-09-29 install bindgen-cli --version 0.72.1 --locked` | <https://github.com/rust-lang/rust-bindgen> |

## D. Tools to build Mesa

| Component | Verified version | Where | Link |
|---|---|---|---|
| MSYS2 clang64 (installed into devkitPro's MSYS2 with a copy of `pacman.conf` that enables `[clang64]`) | — | | <https://www.msys2.org/> |
| `clang`, `clang-libs`, `llvm`, `llvm-libs`, `llvm-tools` | 23.1.3 | MSYS2 clang64 | <https://github.com/llvm/llvm-project> |
| `cmake` | 4.4.4 | MSYS2 clang64 | <https://github.com/Kitware/CMake> |
| `ninja` | 1.13.x | MSYS2 clang64 | <https://github.com/ninja-build/ninja> |
| `spirv-tools` | 1.4.363.0 | MSYS2 clang64 | <https://github.com/KhronosGroup/SPIRV-Tools> |
| `spirv-llvm-translator` | 23.1.2 | MSYS2 clang64 | <https://github.com/KhronosGroup/SPIRV-LLVM-Translator> |
| `cbindgen` | 0.29.4 | MSYS2 clang64 | <https://github.com/mozilla/cbindgen> |
| `flex`, `bison` | 2.6.4 · 3.8.2 | MSYS2 (msys) | <https://github.com/westes/flex> · <https://www.gnu.org/software/bison/> |
| Python virtual environment: `meson` 1.10.1, `mako` 1.4.3, `pyyaml` 6.0.3, `packaging` 26.3 | as listed (`scripts/mesa/requirements.txt`) | `target/mesa-tools/venv` | <https://github.com/mesonbuild/meson> · <https://github.com/sqlalchemy/mako> · <https://github.com/yaml/pyyaml> · <https://github.com/pypa/packaging> |
| **Mesa source for the Switch** | commit `2330d007712acbd32e396574203339df39bde153` (Mesa 26.2.4) | clone into `third_party/mesa-switch` | <https://github.com/NaGaa95/mesa-switch> (origin: <https://github.com/danfromtico/mesa-switch>; Mesa: <https://gitlab.freedesktop.org/mesa/mesa>) |

## E. Optional: the HOME shortcut (`scripts/build-nsp.ps1`)

| What | What for | Where |
|---|---|---|
| `prod.keys` of your own console | Encrypting the title headers (only read; never copied or logged) | From your console |
| `hacbrewpack` | Packs the NCAs and the NSP | <https://github.com/The-4n/hacBrewPack/releases> |
| A title installer (DBI, Tinfoil or Goldleaf) and Atmosphere *sigpatches* | Installing the NSP on the console | Their own projects; without them the shortcut cannot be installed |

## F. Space and time (rough)

Mesa takes a long time to build and the resulting SDK is about 220 MB; Rust's `target/` directories take several GB. Exact numbers will be recorded in the guide once measured on a clean install.
