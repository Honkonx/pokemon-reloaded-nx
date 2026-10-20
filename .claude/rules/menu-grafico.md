# Menú gráfico del NRO: framebuffer propio, sin consola ni EGL

- El menú (`app/engine/menu.cpp`) se dibuja con `app/engine/gfx.cpp`: framebuffer lineal de libnx (1280×720 RGBA8888), formas propias y la **fuente compartida del sistema** con `stb_truetype` (`app/engine/third_party/`). Sin OpenGL: corre **antes** de EGL/Mesa y `gfx::shutdown()` libera la ventana antes de que Mesa la tome.
- **Nunca** mezcles `consoleInit`/`printf` con el menú gráfico ni reabras el framebuffer después de que Mesa usó la ventana (aborto `0x2B59` medido).
- El menú de texto (`menu_text.cpp`, `run_text_menu`) es el **respaldo**: `run_menu` lo usa si `gfx::init()` falla y escribe la causa en `boot.log` (línea 4). No lo borres.
- No uses glifos que la fuente del sistema no garantiza (●, →, ▸, •, −, …): puntos y flechas se dibujan con formas; el texto usa Latin-1.
- Lee archivos y registros **una vez** al abrir la pantalla o tras un cambio, nunca por fotograma: a 60 fps serían cientos de accesos por segundo a la microSD.
- Cada pantalla nueva o cambiada se comprueba con la vista previa del PC (`tools/menu_preview`, etapa de `scripts/check.ps1`) y se revisan las capturas. La vista previa **no prueba nada en la consola**.
- Los nombres de tecla de `controls.txt` (`menu.cpp`, `kKeyNames`) y de `gm8_input::Key::from_name` deben coincidir.
- Skill: `switch-menu-ui`.
