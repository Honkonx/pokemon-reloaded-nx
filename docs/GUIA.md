# Guía: clonar y compilar `pokenx.nro`

Pasos para compilar el programa desde cero en **Windows**. Sirve para hacerlo tú o para dárselo a un asistente de IA (ver el [último apartado](#hacerlo-con-ayuda-de-una-ia)). Compilar bien **no prueba** que el programa arranque en una consola. Si algo falla, mira [ERRORES-FRECUENTES.md](ERRORES-FRECUENTES.md).

> [!NOTE]
> Esta guía se verificó compilando desde una copia limpia del código en una máquina que ya tenía las herramientas instaladas. **Instalar el entorno desde cero en otra máquina no se ha probado todavía**, y el paso 5 (entorno de Mesa) es el más delicado: anota cualquier tropiezo.

## 0. Qué obtienes
Un único `pokenx/pokenx.nro` y la carpeta `pokenx/` lista para copiar a la microSD (carpetas de datos, licencias de Mesa y un `README.txt`). **El juego no está incluido.**

## 1. Lee los requisitos
[REQUISITOS.md](../REQUISITOS.md) lista todo lo que hay que instalar, con versiones verificadas y enlaces. Cuando termines de instalar, ejecuta `pwsh scripts/doctor.ps1` (después de clonar) para comprobarlo. Las versiones de **Rust** y del **código de Mesa** están fijadas: no uses otras.

> [!TIP]
> En cualquier momento, `pwsh scripts/doctor.ps1` comprueba qué herramientas y versiones tienes frente a [REQUISITOS.md](../REQUISITOS.md) y dice qué falta y dónde conseguirlo. No instala ni cambia nada.

## 2. Clona el repositorio
```powershell
git clone https://github.com/<tu-usuario>/pokemon-reloaded-nx.git
cd pokemon-reloaded-nx
```

## 3. Instala el kit de la Switch (devkitPro)
1. Instala devkitPro con su instalador ([enlace](https://github.com/devkitPro/installer)) y elige el grupo **Switch**. **Déjalo en `C:\devkitPro` (la carpeta que propone el instalador)**: los scripts de compilación buscan devkitPro exactamente ahí, y Git Bash en `C:\Program Files\Git\bin\bash.exe`. Si lo instalas en otra carpeta, tendrás que editar la variable de la ruta al principio de `app/engine/build.ps1`, `scripts/build-mesa-switch.ps1` y `app/engine/tests/check-mesa-abi.ps1`.
2. Desde la consola MSYS2 de devkitPro, instala las bibliotecas que enlaza Mesa:
   ```bash
   pacman -S switch-zlib switch-libexpat switch-libzstd switch-curl switch-mbedtls
   ```
3. Comprueba con `pwsh scripts/doctor.ps1` (busca `devkitA64`, `libnx`, `elf2nro` y las bibliotecas). `doctor.ps1` te avisa si devkitPro no está en `C:\devkitPro`; con otra carpeta puedes comprobarlo con `-DevkitPro <carpeta>`, pero los scripts de compilación seguirán mirando `C:\devkitPro` hasta que edites esas variables.

## 4. Instala Rust
```powershell
rustup toolchain install nightly-2026-09-29 --profile minimal -c rust-src -t aarch64-unknown-linux-gnu
cargo +nightly-2026-09-29 --version      # debe ser 1.101.0-nightly
```
Necesitas además las Build Tools de Visual Studio (objetivo MSVC de Rust en Windows).

## 5. Prepara el entorno de Mesa (una vez)
Este paso **no está automatizado**: son tres partes manuales.

1. **Paquetes de MSYS2 clang64.** Copia el `pacman.conf` de devkitPro (`<devkitPro>\msys2\etc\pacman.conf`) a `target\mesa-tools\pacman.conf`, añade al final la sección `[clang64]` con `Include = /etc/pacman.d/mirrorlist.clang64` e instala con esa configuración (sin tocar la global) los paquetes de la sección D de [REQUISITOS.md](../REQUISITOS.md): `clang`, `llvm`, `cmake`, `ninja`, `spirv-tools`, `spirv-llvm-translator`, `cbindgen` (prefijo `mingw-w64-clang-x86_64-`) y `flex`, `bison`.
2. **Entorno de Python:**
   ```powershell
   python -m venv target\mesa-tools\venv
   target\mesa-tools\venv\Scripts\pip install -r scripts\mesa\requirements.txt
   ```
3. **`bindgen-cli` 0.72.1:**
   ```powershell
   cargo +nightly-2026-09-29 install bindgen-cli --version 0.72.1 --locked --root target\mesa-tools\cargo
   ```

## 6. Clona el código de Mesa para la Switch
```powershell
git clone https://github.com/NaGaa95/mesa-switch third_party/mesa-switch
git -C third_party/mesa-switch checkout 2330d007712acbd32e396574203339df39bde153
```
El script se niega a compilar otro commit.

## 7. Compila el SDK de Mesa (largo)
```powershell
pwsh scripts/build-mesa-switch.ps1 -Stage ConfigureHost
pwsh scripts/build-mesa-switch.ps1 -Stage BuildHost -Jobs 4
pwsh scripts/build-mesa-switch.ps1 -Stage Configure
pwsh scripts/build-mesa-switch.ps1 -Stage Build -Jobs 4
pwsh scripts/build-mesa-switch.ps1 -Stage Install
```
(`-Stage All` las encadena; ajusta `-Jobs` a tu equipo.) El resultado queda en `target/mesa26/sdk` (≈ 220 MB) con un manifiesto de hashes que el build del NRO verifica.

## 8. Compila el NRO
```powershell
pwsh app/build-package.ps1                      # opcional: -SoundFont <ruta-a-un-soundfont.sf2>
```
Compila el motor Rust, el C++ con Mesa y libnx, **verifica las relocaciones del ELF**, crea el NRO y arma `pokenx/`. Al final imprime el tamaño y el SHA-256 del único `pokenx/pokenx.nro`. Si pasas `-SoundFont`, conserva la licencia que se copia a su lado.

## 9. Comprueba
```powershell
pwsh scripts/check.ps1            # pruebas de los crates, compilación cruzada del motor, relocaciones y vista previa del menú
```
Ninguna etapa prueba el NRO en una consola.

## 10. Llévalo a la consola
Copia `pokenx/` a `sdmc:/switch/` (queda `sdmc:/switch/pokenx/pokenx.nro`), pon tu juego en `pokenx/game/` (o impórtalo desde el menú: descarga el ZIP de la última versión desde el [blog de F3R Productions](https://pokemon-reloaded.blogspot.com/), cópialo a la microSD y el programa lo descomprime en la carpeta administrada) y abre **PokeNX** desde Homebrew Menu. Para tener toda la memoria, ábrelo como aplicación (abriendo cualquier juego con **R** pulsado).

## 11. Crea una *Release*
```powershell
pwsh scripts/make-release.ps1 -Version v1.0.0
```
Genera `release/pokemon-reloaded-nx-v1.0.0-switch.zip` (+ `.sha256`) con `switch/pokenx/…`, `LEEME.txt`, `LICENSE` y las licencias de Mesa. **Nunca** incluye el juego: el script se niega si hay archivos en `pokenx/game`.

## 12. Opcional: el acceso directo en HOME

Desde el propio programa: abre **Acceso HOME** y pulsa **A**; crea el icono sin PC (necesita Atmosphère con *sigpatches*; no se ha probado todavía en una consola). La alternativa, en el PC:

`pwsh scripts/build-nsp.ps1 -Keys <tu prod.keys> -Hacbrewpack <hacbrewpack.exe>` construye `app/forwarder` (un cargador que arranca `sdmc:/switch/pokenx/pokenx.nro`) y lo empaqueta como `nsp-home/PokeNX-HOME.nsp`. Con `-DryRun` solo comprueba la estructura y no necesita claves. Instálalo con DBI, Tinfoil o Goldleaf (hacen falta *sigpatches*). No se ha probado todavía en una consola.

## Hacerlo con ayuda de una IA

El repositorio incluye [AGENTS.md](../AGENTS.md) y [`.claude/`](../.claude) con las reglas y skills técnicas del proyecto. Un buen arranque para un asistente:

> Lee `AGENTS.md`, `REQUISITOS.md` y `docs/GUIA.md`. Comprueba mi entorno contra `REQUISITOS.md` y dime qué falta antes de instalar nada. Ejecuta los pasos en orden y, ante un error, consulta `docs/ERRORES-FRECUENTES.md`. No incluyas datos del juego ni rutas personales en nada que vaya a un commit.

Qué **no** conviene pedirle: instalar herramientas sin avisarte, cambiar el script de enlace o la versión fijada de Rust, ni dar por bueno un NRO por compilar sin errores.
