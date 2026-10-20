---
name: switch-menu-ui
description: Cambiar o añadir pantallas del menú gráfico del NRO (gfx.cpp/menu.cpp) y comprobarlas con la vista previa del PC; úsala al tocar el menú o los archivos de ajustes (resolution.txt, controls.txt, filecache.txt, performance.txt).
---

# Menú gráfico del NRO (framebuffer + fuente del sistema)

## Piezas
- `app/engine/gfx.{h,cpp}`: `init/shutdown`, `begin/end` (un fotograma), `rrect`, `rrect_gradient`, `circle`, `triangle`, `line`, `text`/`text_center`/`text_right`/`ellipsize`. Texto con `stb_truetype` sobre la fuente compartida; caché de glifos por (código, tamaño).
- `app/engine/menu.cpp`: pantallas (Inicio con 8 mosaicos, Actualizaciones, Opciones, Mandos con pestañas Botones y Capa, Teclas del juego, Acceso HOME, Diagnóstico, tarjeta de aviso, «Preparando» y descarga). Las actualizaciones usan `updater.cpp` (solo consola) y `update_core.h` (lógica pura con pruebas en `tools/update_core_test`). Cada pantalla es una función con su bucle `while (appletMainLoop())` y un `frame(crumb, chips, hints, cuerpo)`.
- `app/engine/menu_text.cpp`: menú de texto de respaldo (`run_text_menu`).

## Flujo para una pantalla nueva
1. Escribe la función en `menu.cpp`: lee archivos **antes** del bucle o tras un cambio, nunca por fotograma; `B` y `+` salen; el pie lista los botones.
2. Añádela a `tools/menu_preview/preview.cpp` (`shot(...)`), ejecuta `tools/menu_preview/run.ps1` y **mira las capturas** (desbordes de texto, solapes, contraste).
3. Ejecuta `scripts/check.ps1` (la etapa del menú compila y dibuja todo) y `app/build-package.ps1`.
4. Documenta el cambio. El nivel de evidencia sigue siendo «NRO generado» hasta que alguien lo pruebe en una consola.

## Trampas
- Sin `consoleInit`/`printf` mientras el menú gráfico está activo; `gfx::shutdown()` antes de iniciar EGL.
- Glifos: solo Latin-1/ASCII; puntos y flechas se dibujan.
- La fuente del PC de la vista previa (Segoe UI) **no** es la de la Switch: los anchos pueden diferir unos píxeles; deja margen.
