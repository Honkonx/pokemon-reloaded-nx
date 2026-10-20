# app/engine: el programa de la Switch

Todo esto es **un solo binario** (`pokenx.nro`): menú, anfitrión y motor.

| Archivo | Qué hace |
|---|---|
| `main.cpp` | Anfitrión libnx: arranque (`__libnx_initheap` con 32 MiB de reserva para el driver gráfico), relojes (`clkrst`), contexto EGL 3.3 *core* sobre Mesa, mandos (HID, foco, salida con `L + R + +`), hilo de audio (`audout`), manejador de excepciones (`crash.log`), registros con marcas de tiempo y `loading_pump` (HOME y `+` durante la carga) |
| `gfx.cpp`, `gfx.h` | Dibujo del menú: framebuffer lineal, formas y texto con la fuente del sistema (`stb_truetype`) |
| `menu.cpp`, `menu.h` | Menú gráfico: Inicio, Opciones, Mandos, Importar, Acceso HOME, Diagnóstico y avisos |
| `menu_text.cpp` | Menú de texto de respaldo si el framebuffer o la fuente no arrancan |
| `host_services.cpp` | Servicios C que usa la «std» de Rust para Horizon (`stat`, búfer de lectura, etc.) |
| `pokenx_engine.h` | ABI C versión 3 con el motor Rust (`PokenxHost`, `PokenxPad`); debe coincidir con `src/gm8-runtime/pokenx-engine/src/abi.rs` |
| `mesa_linux_compat.cpp`, `mesa_linux_abi.h` | Puente que traduce a Horizon las llamadas de tipo Linux del código Rust de Mesa |
| `switch-pokenx.ld` | Script de enlace: `.rodata` en la zona escribible de RELRO (ver [docs/SISTEMA.md](../../docs/SISTEMA.md#arranque-seguro-del-nro)) |
| `build.ps1` | Compila el motor Rust y el C++, enlaza con Mesa y libnx, **verifica las relocaciones** y crea el NRO |
| `tests/` | Comprobación de la ABI fija de Mesa y de los plazos (en el PC) |
| `third_party/` | `stb_truetype.h` (dominio público / MIT) |

Se compila con `pwsh app/build-package.ps1` (ver [docs/GUIA.md](../../docs/GUIA.md)). Que compile no prueba que arranque en una consola.
