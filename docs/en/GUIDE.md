# Guide: clone and build `pokenx.nro`

Steps to build the program from scratch on **Windows**. Use it yourself or hand it to an AI assistant (see the [last section](#doing-it-with-the-help-of-an-ai)). A successful build does **not** prove that the program starts on a console. If something fails, see [FAQ-ERRORS.md](FAQ-ERRORS.md).

> [!NOTE]
> This guide was verified by building from a clean copy of the code on a machine that already had the tools installed. **Installing the environment from scratch on another machine has not been tested yet**, and step 5 (the Mesa environment) is the most delicate: note any stumble.

> [!TIP]
> At any time, `pwsh scripts/doctor.ps1` compares the tools and versions you have against [REQUIREMENTS.en.md](../../REQUIREMENTS.en.md) and tells you what is missing and where to get it. It installs and changes nothing.

## 0. What you get
A single `pokenx/pokenx.nro` and the `pokenx/` folder ready to copy to the microSD (data folders, Mesa licenses and a `README.txt`). **The game is not included.**

## 1. Read the requirements
[REQUIREMENTS.en.md](../../REQUIREMENTS.en.md) lists everything to install, with verified versions and links. The **Rust** and **Mesa source** versions are pinned: do not use others. When you finish installing (after cloning), run `pwsh scripts/doctor.ps1` to check.

## 2. Clone the repository
```powershell
git clone https://github.com/<your-user>/pokemon-reloaded-nx.git
cd pokemon-reloaded-nx
```

## 3. Install the Switch kit (devkitPro)
1. Install devkitPro with its installer ([link](https://github.com/devkitPro/installer)) and choose the **Switch** group. **Leave it in `C:\devkitPro` (the folder the installer proposes)**: the build scripts look for devkitPro exactly there, and for Git Bash in `C:\Program Files\Git\bin\bash.exe`. If you install it elsewhere you will have to edit the path variable at the top of `app/engine/build.ps1`, `scripts/build-mesa-switch.ps1` and `app/engine/tests/check-mesa-abi.ps1`.
2. From devkitPro's MSYS2 console, install the libraries Mesa links:
   ```bash
   pacman -S switch-zlib switch-libexpat switch-libzstd switch-curl switch-mbedtls
   ```
3. Check with `pwsh scripts/doctor.ps1` (it looks for `devkitA64`, `libnx`, `elf2nro` and the libraries). `doctor.ps1` warns you if devkitPro is not in `C:\devkitPro`; with another folder you can check it with `-DevkitPro <folder>`, but the build scripts will keep looking in `C:\devkitPro` until you edit those variables.

## 4. Install Rust
```powershell
rustup toolchain install nightly-2026-09-29 --profile minimal -c rust-src -t aarch64-unknown-linux-gnu
cargo +nightly-2026-09-29 --version      # must be 1.101.0-nightly
```
You also need the Visual Studio Build Tools (Rust's MSVC target on Windows).

## 5. Prepare the Mesa environment (once)
This step is **not automated**: there are three manual parts.

1. **MSYS2 clang64 packages.** Copy devkitPro's `pacman.conf` (`<devkitPro>\msys2\etc\pacman.conf`) to `target\mesa-tools\pacman.conf`, add the `[clang64]` section at the end with `Include = /etc/pacman.d/mirrorlist.clang64`, and install with that configuration (leaving the global one alone) the packages from section D of [REQUIREMENTS.en.md](../../REQUIREMENTS.en.md): `clang`, `llvm`, `cmake`, `ninja`, `spirv-tools`, `spirv-llvm-translator`, `cbindgen` (prefix `mingw-w64-clang-x86_64-`) and `flex`, `bison`.
2. **Python environment:**
   ```powershell
   python -m venv target\mesa-tools\venv
   target\mesa-tools\venv\Scripts\pip install -r scripts\mesa\requirements.txt
   ```
3. **`bindgen-cli` 0.72.1:**
   ```powershell
   cargo +nightly-2026-09-29 install bindgen-cli --version 0.72.1 --locked --root target\mesa-tools\cargo
   ```

## 6. Clone Mesa's source for the Switch
```powershell
git clone https://github.com/NaGaa95/mesa-switch third_party/mesa-switch
git -C third_party/mesa-switch checkout 2330d007712acbd32e396574203339df39bde153
```
The script refuses to build any other commit.

## 7. Build the Mesa SDK (long)
```powershell
pwsh scripts/build-mesa-switch.ps1 -Stage ConfigureHost
pwsh scripts/build-mesa-switch.ps1 -Stage BuildHost -Jobs 4
pwsh scripts/build-mesa-switch.ps1 -Stage Configure
pwsh scripts/build-mesa-switch.ps1 -Stage Build -Jobs 4
pwsh scripts/build-mesa-switch.ps1 -Stage Install
```
(`-Stage All` chains them; adjust `-Jobs` to your machine.) The result lands in `target/mesa26/sdk` (≈ 220 MB) with a hash manifest that the NRO build checks.

## 8. Build the NRO
```powershell
pwsh app/build-package.ps1                      # optional: -SoundFont <path-to-a-soundfont.sf2>
```
It builds the Rust engine and the C++ with Mesa and libnx, **verifies the ELF relocations**, creates the NRO and lays out `pokenx/`. At the end it prints the size and SHA-256 of the single `pokenx/pokenx.nro`. If you pass `-SoundFont`, keep the license that is copied next to it.

## 9. Check
```powershell
pwsh scripts/check.ps1            # crate tests, cross-compilation of the engine, relocations and the menu preview
```
No stage proves the NRO on a console.

## 10. Take it to the console
Copy `pokenx/` to `sdmc:/switch/` (giving `sdmc:/switch/pokenx/pokenx.nro`), put your game in `pokenx/game/` (or import it from the menu: download the ZIP of the latest version from the [F3R Productions blog](https://pokemon-reloaded.blogspot.com/), copy it to the microSD and the program unzips it into the managed folder) and open **PokeNX** from the Homebrew Menu. For the full memory, open it as an application (open any game while holding **R**).

## 11. Make a release
```powershell
pwsh scripts/make-release.ps1 -Version v1.0.0
```
It produces `release/pokemon-reloaded-nx-v1.0.0-switch.zip` (+ `.sha256`) with `switch/pokenx/…`, `LEEME.txt`, `LICENSE` and Mesa's licenses. It **never** includes the game: the script refuses if there are files in `pokenx/game`.

## 12. Optional: the HOME shortcut

From the program itself: open **Acceso HOME** and press **A**; it creates the icon without a PC (it needs Atmosphère with *sigpatches*; it has not been tried on a console yet). The alternative, on the PC:

`pwsh scripts/build-nsp.ps1 -Keys <your prod.keys> -Hacbrewpack <hacbrewpack.exe>` builds `app/forwarder` (a loader that starts `sdmc:/switch/pokenx/pokenx.nro`) and packs it as `nsp-home/PokeNX-HOME.nsp`. With `-DryRun` it only checks the layout and needs no keys. Install it with DBI, Tinfoil or Goldleaf (*sigpatches* are required). It has not been tried on a console yet.

## Doing it with the help of an AI

The repository includes [AGENTS.md](../../AGENTS.md) and [`.claude/`](../../.claude) with the project's technical rules and skills. A good starting prompt for an assistant:

> Read `AGENTS.md`, `REQUIREMENTS.en.md` and `docs/en/GUIDE.md`. Run `scripts/doctor.ps1` and tell me what is missing before installing anything. Run the steps in order and, on an error, consult `docs/en/FAQ-ERRORS.md`. Do not put game data or personal paths in anything that goes into a commit.

What **not** to ask it: to install tools without telling you, to change the linker script or the pinned Rust version, or to take an NRO as good just because it built without errors.
