# Entorno de Mesa Switch aislado

El payload actual usa la SDK Mesa26.2.4 compilada y enlazada. Mesa20.1 de devkitPro se conserva como variante explícita de desarrollo. Este build no instala sobre `C:/devkitPro/portlibs/switch`.

Origen: `third_party/mesa-switch`, commit `2330d007712acbd32e396574203339df39bde153` (Mesa26.2.4). Se necesita el checkout completo; si es sparse, completar con Git Bash: `git -C third_party/mesa-switch sparse-checkout disable`.

## Herramientas locales

- devkitPro en `C:/devkitPro`, MSYS2 incluido y Clang64 para los compiladores de shaders que se ejecutan en Windows.
- Paquetes MSYS2 Clang64: clang, llvm, cmake, pkgconf, spirv-tools, spirv-llvm-translator, cbindgen; también flex/bison MSYS. Se instalan con pacman y firmas verificadas. `target/mesa-tools/pacman.conf` copia la configuración local y añade el repositorio oficial clang64; no cambia `/etc/pacman.conf`.
- Python venv en `target/mesa-tools/venv`, dependencias fijadas en `requirements.txt`.
- bindgen-cli0.72.1 (se evita0.72.0, rechazado por Mesa) en `target/mesa-tools/cargo/bin`; `cargo +nightly-2026-09-29 install bindgen-cli --version 0.72.1 --locked --root target/mesa-tools/cargo`.
- Rust nightly-2026-09-29 y target `aarch64-unknown-linux-gnu` para el código Rust interno de esta referencia Mesa. El runtime propio conserva el target Switch freestanding. El host Rust de esta PC es MSVC.

## Compilación

Desde PowerShell en la raíz:

```powershell
pwsh scripts/build-mesa-switch.ps1 -Stage ConfigureHost
pwsh scripts/build-mesa-switch.ps1 -Stage BuildHost -Jobs 4
pwsh scripts/build-mesa-switch.ps1 -Stage Configure
pwsh scripts/build-mesa-switch.ps1 -Stage Build -Jobs 4
pwsh scripts/build-mesa-switch.ps1 -Stage Install
```

`-Stage All` recorre las etapas en ese orden. Las herramientas son Windows x64; el producto final es ARM64 Horizon, estático. El script comprueba el commit de origen y adapta las rutas MSYS2 a esta instalación en archivos generados bajo `target/mesa26`. Ajusta los probes/proc macros Rust al host MSVC; no cambia la toolchain Rust global ni el código de referencia. El entorno de proceso se restaura al salir.

Los resultados se guardan en `target/mesa26/host`, `target/mesa26/switch` y `target/mesa26/sdk`. Se usa un prefijo Windows absoluto dentro del workspace: Python3.13 rechaza `/opt/...` como ruta absoluta de Windows. Compilar esta SDK no implica que el NRO la esté enlazando, ni valida GPU/driver en consola. El payload ya ofrece integración explícita; falta comprobar el renderer real en hardware.

La ausencia de libelf no es un bloqueo para Horizon en este commit: `meson.build` excluye Switch de ese error NVK, aunque el README enumere libelf entre los requisitos generales. No sustituir archivos ELF por bibliotecas Windows.

## Enlace del payload

Tras Install, la SDK incluye pokenx-mesa-sdk.json con commit, hashes de archivos y arquitecturas ELF inspeccionadas. El origen local es NaGaa95/mesa-switch2330d007; danfromtico/mesa-switch es su upstream.

`pwsh app/build-package.ps1 -MesaSdk target/mesa26/sdk` solicita el enlace de la SDK. El build verifica sus hashes, usa archivos Mesa explícitos y headers de la misma SDK. La etiqueta Mesa26 del menú se habilita solo junto al payload que enlazó esa ruta. El build por defecto requiere esta SDK local. La variante Mesa20 de devkitPro se solicita explícitamente con -MesaSdk "". La disponibilidad de esas dos variantes no prueba ejecución en hardware.

La composición de bibliotecas Rust requiere la misma versión de rustc. En una copia local de libvulkan se hacen weak solamente los tres símbolos privados de panic/OOM duplicados, conservando handlers fuertes del motor. No se usa allow-multiple-definition; SDK original intacta. compose_rust_archive.py adapta importaciones privadas en objetos Rust y hace weak handlers solo en std/alloc porque objcopy de archivos completos falla en Windows con un miembro que contiene directorios; ar s normaliza ese nombre y reconstruye el índice. Esta técnica fija compiler/ABI y debe revisarse al actualizar Rust.

El payload incluye el puente privado de ABI Rust/Mesa a Horizon, documentado en docs/auditorias/ABI_RUST_MESA_HORIZON_2026-10-16.md. Clocks/errno/flags/stat difieren de newlib; TLS se enlaza en main/workers antes de usar Rust Mesa. Pruebas ampliadas: pwsh scripts/check.ps1 -WithNativeGl -WithNativeAbi. La prueba ABI no ejecuta servicios Horizon.
