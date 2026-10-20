# Gráficos: Mesa, NVK/Zink, OpenGL y el render

Qué se usó para dibujar el juego en la Switch, cómo encaja y cómo se compila.

## Por qué OpenGL

El renderer de OpenGMK usa **OpenGL 3.3 *core***. En la Switch ese OpenGL lo da **Mesa**, enlazado dentro del propio `.nro`. La alternativa nativa, `deko3d`, exigiría reescribir el renderer entero y quedó como plan B.

## La pila gráfica

```text
Juego (GML)
  → pokenx-core: renderer OpenGL 3.3 core (shaders GLSL 330 en src/gm8-runtime/pokenx-core/src/render/glsl)
    → EGL de Mesa (ventana de libnx)
      → NVC0 ........ OpenGL nativo sobre Nouveau   (por defecto)
      → Zink + NVK .. OpenGL traducido a Vulkan      (experimental)
        → GPU Tegra X1
```

- Se elige con `graphics-driver.txt` (`nvc0` o `zink`); el menú **ya no lo ofrece** porque Zink todavía no presenta el primer fotograma, así que el programa usa `nvc0` (un `zink` en el archivo solo se respeta si existe también `dev-env.txt`). Zink sigue en el código para investigarlo. El selector actúa **antes** de iniciar EGL.
- Medido en la consola: `GL_RENDERER: NV12B` y `GL_VERSION: 4.3 (Core Profile) Mesa 26.2.4`. Con Zink/NVK, también medido: crea el dispositivo (`GL_RENDERER: zink Vulkan 1.4 (NVIDIA Tegra X1 (GM20B) (NVK GM20B))`, `GL_VERSION: 4.5 … Mesa 26.2.4`) y el contexto, y la carga de datos termina, pero **al entrar al juego la pantalla queda en negro** (en estudio; el registro lleva marcas `mark: …` y un vigilante para ver dónde se queda, y un arranque Zink que no presenta ni un fotograma vuelve solo a `nvc0`).

## El renderer

- **Atlas de texturas** con empaquetado (`rect_packer`) y subida por lotes: miles de *sprites* en pocas texturas.
- **Salida:** 1280×720 por defecto, 1920×1080 opcional (`resolution.txt`); escalado a pantalla completa 16:9 o 4:3 con bandas (`scaling.txt`).
- **Presentación objetivo de 60 Hz**, separada de la simulación del juego: el motor conserva `room_speed` y repite el último fotograma cuando no toca simular.
- **Errores:** el blit final comprueba errores de GL y los deja en `engine.log` (con un máximo de líneas).

## Memoria y ventana

- El montón por defecto de libnx se queda con casi toda la RAM disponible; el programa define `__libnx_initheap` y **reserva 32 MiB para el driver gráfico**.
- La ventana de la Switch **no se puede reabrir** una vez que Mesa la usó: por eso el menú gráfico (framebuffer propio, sin OpenGL) vive y muere antes de iniciar EGL. Reabrirla abortaba el programa (error `0x2B59`, medido también en otro proyecto).

## Mesa en la Switch

- **Versión:** Mesa 26.2.4, a través de [NaGaa95/mesa-switch](https://github.com/NaGaa95/mesa-switch), commit `2330d007712acbd32e396574203339df39bde153` (upstream [danfromtico/mesa-switch](https://github.com/danfromtico/mesa-switch)), con los controladores `nouveau` (NVC0), `zink` y `nouveau` Vulkan (NVK).
- **Cómo se compila:** `scripts/build-mesa-switch.ps1` genera un **SDK aparte** (`target/mesa26/sdk`, unos 220 MB) en etapas (`ConfigureHost`, `BuildHost`, `Configure`, `Build`, `Install`). Las herramientas de shaders se ejecutan en Windows; el producto es ARM64 estático. No toca las bibliotecas de devkitPro. El SDK lleva un manifiesto (`pokenx-mesa-sdk.json`) con el commit, la versión de `rustc` y el hash de cada biblioteca, y **el build del NRO lo comprueba**. Requisitos: [REQUISITOS.md](../REQUISITOS.md); pasos: [GUIA.md](GUIA.md).
- **Puente Linux → Horizon** (`app/engine/mesa_linux_compat.cpp`): el código Rust de Mesa usa la `std` de Linux; este puente traduce a Horizon `errno`, relojes, `stat`, hilos, futex, aleatoriedad y operaciones básicas de archivos. Los símbolos de pánico duplicados entre Mesa y el motor se dejan débiles **solo** esos tres, con `scripts/mesa/compose_rust_archive.py`.
- **Enlace:** `-mtp=soft` en C++ y un script de enlace propio que coloca `.rodata` en la zona escribible de RELRO (ver [SISTEMA.md](SISTEMA.md#arranque-seguro-del-nro)).

## Medido y sin medir

| Medido (una consola) | Sin medir |
|---|---|
| El driver inicia y crea el contexto 3.3 (nvc0 y también Zink/NVK) | Que Zink/NVK dibuje y presente el juego |
| El juego se ve y responde | 1080p en dock |
| `GL_RENDERER` y `GL_VERSION` | 60 FPS sostenidos en todas las escenas |
