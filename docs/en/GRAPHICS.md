# Graphics: Mesa, NVK/Zink, OpenGL and the renderer

What was used to draw the game on the Switch, how it fits together and how it is built.

## Why OpenGL

OpenGMK's renderer uses **OpenGL 3.3 core**. On the Switch that OpenGL comes from **Mesa**, linked inside the `.nro` itself. The native alternative, `deko3d`, would require rewriting the whole renderer and stayed as plan B.

## The graphics stack

```text
Game (GML)
  → pokenx-core: OpenGL 3.3 core renderer (GLSL 330 shaders in src/gm8-runtime/pokenx-core/src/render/glsl)
    → Mesa's EGL (libnx window)
      → NVC0 ........ native OpenGL on Nouveau   (default)
      → Zink + NVK .. OpenGL translated to Vulkan (experimental)
        → Tegra X1 GPU
```

- Chosen with `graphics-driver.txt` (`nvc0` or `zink`); the menu **no longer offers it** because Zink does not present the first frame yet, so the program uses `nvc0` (a `zink` in the file is honored only if `dev-env.txt` also exists). Zink stays in the code for investigation. The selector acts **before** EGL starts.
- Measured on the console: `GL_RENDERER: NV12B` and `GL_VERSION: 4.3 (Core Profile) Mesa 26.2.4`. With Zink/NVK, also measured: it creates the device (`GL_RENDERER: zink Vulkan 1.4 (NVIDIA Tegra X1 (GM20B) (NVK GM20B))`, `GL_VERSION: 4.5 … Mesa 26.2.4`) and the context, and the data load finishes, but **on entering the game the screen stays black** (under study; the log carries `mark: …` lines and a watchdog to see where it stalls, and a Zink start that presents no frame goes back to `nvc0` by itself).

## The renderer

- **Texture atlases** with packing (`rect_packer`) and batched uploads: thousands of sprites in a few textures.
- **Output:** 1280×720 by default, 1920×1080 optional (`resolution.txt`); scaling to full-screen 16:9 or 4:3 with bars (`scaling.txt`).
- **60 Hz presentation target**, separate from the game's simulation: the engine keeps `room_speed` and repeats the last frame when no simulation step is due.
- **Errors:** the final blit checks for GL errors and writes them to `engine.log` (with a cap on lines).

## Memory and window

- libnx's default heap takes almost all the available RAM; the program defines `__libnx_initheap` and **reserves 32 MiB for the graphics driver**.
- The Switch's window **cannot be reopened** once Mesa has used it: that is why the graphical menu (its own framebuffer, no OpenGL) lives and dies before EGL starts. Reopening it aborted the program (error `0x2B59`, also measured in another project).

## Mesa on the Switch

- **Version:** Mesa 26.2.4, through [NaGaa95/mesa-switch](https://github.com/NaGaa95/mesa-switch), commit `2330d007712acbd32e396574203339df39bde153` (upstream [danfromtico/mesa-switch](https://github.com/danfromtico/mesa-switch)), with the `nouveau` (NVC0), `zink` and `nouveau` Vulkan (NVK) drivers.
- **How it is built:** `scripts/build-mesa-switch.ps1` produces a **separate SDK** (`target/mesa26/sdk`, about 220 MB) in stages (`ConfigureHost`, `BuildHost`, `Configure`, `Build`, `Install`). The shader tools run on Windows; the product is static ARM64. It does not touch devkitPro's libraries. The SDK carries a manifest (`pokenx-mesa-sdk.json`) with the commit, the `rustc` version and each library's hash, and **the NRO build checks it**. Requirements: [REQUIREMENTS.en.md](../../REQUIREMENTS.en.md); steps: [GUIDE.md](GUIDE.md).
- **Linux → Horizon bridge** (`app/engine/mesa_linux_compat.cpp`): Mesa's Rust code uses Linux's `std`; this bridge translates `errno`, clocks, `stat`, threads, futex, randomness and basic file operations to Horizon. The panic symbols duplicated between Mesa and the engine are made weak — **only** those three — with `scripts/mesa/compose_rust_archive.py`.
- **Linking:** `-mtp=soft` in C++ and an own linker script that places `.rodata` in RELRO's writable area (see [SYSTEM.md](SYSTEM.md#safe-start-of-the-nro)).

## Measured and not measured

| Measured (one console) | Not measured |
|---|---|
| The driver starts and creates the 3.3 context (nvc0 and also Zink/NVK) | Zink/NVK drawing and presenting the game |
| The game shows and responds | 1080p docked |
| `GL_RENDERER` and `GL_VERSION` | Sustained 60 FPS in every scene |
