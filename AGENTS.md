# Guía para asistentes de IA y colaboradores

Este archivo da el contexto técnico mínimo para trabajar en el repositorio, a una persona o a un asistente de IA (por ejemplo, para compilar un fork en otra máquina).

## Qué es el proyecto
Un único programa homebrew (`pokenx.nro`) para Nintendo Switch que ejecuta juegos hechos con **GameMaker 8.0** (pensado para Pokémon Reloaded). Lee el `.exe` del juego **solo como archivo de datos**; **nunca** ejecuta su código x86. El motor deriva de OpenGMK; los gráficos usan Mesa (OpenGL; NVC0 o Zink/NVK).

## Mapa
- `app/engine/`: programa de la Switch en C++/libnx: menú gráfico (`gfx.cpp`, `menu.cpp`; respaldo de texto `menu_text.cpp`), anfitrión (`main.cpp`: arranque, memoria, relojes, EGL, mandos, audio), script de enlace (`switch-pokenx.ld`).
- `src/gm8-runtime/`: el motor en Rust (24 crates): `pokenx-core` (runtime de GameMaker 8), `pokenx-engine` (ABI C v3), `pokenx-std`/`pokenx-io` (una «std» para Horizon), `gm8-*` (lectura de datos).
- `app/forwarder/`: el acceso directo de HOME (un cargador derivado de nx-hbloader que arranca `pokenx.nro`); `scripts/build-nsp.ps1` lo empaqueta como NSP en el PC.
- `.github/`: integración continua (`scripts/check.ps1` en Windows) y plantillas; guía en [CONTRIBUTING.md](CONTRIBUTING.md).
- `scripts/`: `doctor.ps1`, `check.ps1`, `build-mesa-switch.ps1`, `verify_relocations.py`, `make-release.ps1`. `tools/menu_preview/`: vista previa del menú en un PC.
- `docs/`: [MOTOR](docs/MOTOR.md), [GRAFICOS](docs/GRAFICOS.md), [SISTEMA](docs/SISTEMA.md), [GUIA](docs/GUIA.md), [ERRORES-FRECUENTES](docs/ERRORES-FRECUENTES.md). Requisitos: [REQUISITOS.md](REQUISITOS.md).

## Comandos
```powershell
pwsh scripts/doctor.ps1          # comprueba el entorno frente a REQUISITOS.md (no instala nada)
pwsh app/build-package.ps1     # compila pokenx.nro y crea la carpeta pokenx/ (necesita el SDK de Mesa; ver docs/GUIA.md)
pwsh scripts/check.ps1         # pruebas del PC, compilación cruzada del motor, relocaciones, vista previa del menú
cargo +nightly-2026-09-29 test --manifest-path src/gm8-runtime/<crate>/Cargo.toml
```
Usa siempre el toolchain `nightly-2026-09-29`. Los scripts son PowerShell para Windows.

## Reglas (detalle en `.claude/rules/`)
1. **Ni el juego ni sus datos** en el repositorio, incidencias, registros o capturas. Pruebas automáticas solo con datos sintéticos o libres.
2. **Evidencia por etapas:** compila → NRO generado → abre → renderiza → suena → controles → se juega. Informa cada etapa con su nombre; compilar no es «funciona».
3. **Licencias:** el proyecto es GPL-2.0-only. Antes de copiar código comprueba su licencia y conserva sus avisos; no copies código GPL-3.0.
4. **Arranque del NRO:** nunca enlaces con `-z notext` sin la comprobación de relocaciones (`scripts/verify_relocations.py`); no silencies `read-only segment has dynamic relocations`.
5. **Menú gráfico:** sin `consoleInit`/`printf` mientras está activo, y sin reabrir la ventana tras iniciar Mesa.
6. **Un solo NRO** como entregable; `pokenx/` se genera, no se versiona.
7. **Sin rutas locales ni datos personales** en nada que se vaya a commitear.

## Antes de dar algo por terminado
Ejecuta `scripts/check.ps1`; si cambió el NRO, `app/build-package.ps1` y anota el tamaño y el SHA-256. Di qué etapas se verificaron y cuáles no.
