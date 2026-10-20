# Errores frecuentes

Dos partes: **al usar el programa** y **al compilar**. Cada entrada es *síntoma → causa → qué hacer*.

## Qué copiar para pedir ayuda

De la microSD, la carpeta `sdmc:/switch/pokenx/logs/`: **`boot.log`**, **`engine.log`** (y `engine.log.1`, la ejecución anterior), **`crash.log`**, **`last-session.txt`** y, si puedes, **`informe.txt`** (menú → Diagnóstico → **X**). Si el programa se cerró solo, añade `sdmc:/atmosphere/crash_reports/*.log`. Ninguno contiene datos del juego. Indica también la versión de Atmosphère, el firmware y la versión del juego.

---

# A. Al usar el programa

### Pantalla negra al abrir, sin menú y sin registros
El programa murió antes de ejecutar código propio. Si **no existe `boot.log`**, mira `atmosphere/crash_reports/`. Causa conocida: un NRO compilado con relocaciones en memoria de solo lectura (el verificador del build existe para impedirlo; ver [SISTEMA.md](SISTEMA.md#arranque-seguro-del-nro)). Si compilaste tú, vuelve a ejecutar `scripts/check.ps1`.

### `boot.log`: hasta dónde llegó el arranque

| Línea | Significa |
|---|---|
| `1. libnx initialised…` | libnx y el montón están listos (indica memoria total y tamaño del montón) |
| `2. static constructors ran` | constructores estáticos de C++ ejecutados |
| `3. main() started` | entró en `main` |
| `4. graphical menu ready` | el menú gráfico pudo dibujar |
| `4. graphical menu FAILED…` | no hubo framebuffer o fuente: se usa el menú de texto |

### «Jugar» está bloqueado
Estás en **modo applet** (poca memoria). Abre el programa como aplicación: abre cualquier juego **manteniendo R** y elige PokeNX, o crea un acceso directo en HOME.

### Al pulsar «Jugar» se queda en negro
La primera carga analiza unos 20 000 recursos y puede tardar decenas de segundos. Mientras carga debería verse una barra y **+** cancela. En `engine.log` cada línea lleva `[ms]`: busca `game started` (carga completa) o `load failed` (motivo). El menú muestra el último motivo de cierre (`last-session.txt`).

### «No hay juego» aunque lo copiaste
Debe haber **un** `.exe` de GameMaker 8.0 estándar (y su `data/`) en `game/` o en una subcarpeta inmediata. Si hay varios, deja uno. Otros formatos (GameMaker 8.1, `.exe` comprimido) no se reconocen. Ver [SISTEMA.md](SISTEMA.md#detección-y-versiones-del-juego).

### El juego va a tirones o tarda al cambiar de escena
Mira en `engine.log` las líneas `video: host … late N/M` (fotogramas tardíos), `pace: …` (cuánto se pausó el propio juego, archivos leídos y cambios de sala en esos 5 s) y `room … loaded in …`. Las pausas del propio juego (diálogos, escenas) no son un fallo del motor. Prueba la **caché de archivos** y el perfil **Máximo** (Opciones). Adjunta `informe.txt`.

### Con Zink la pantalla se queda en negro
El menú ya no ofrece Zink (todavía no presenta fotogramas); solo se activa con `graphics-driver.txt` en `zink` y un `dev-env.txt` presente. Es experimental. Cierra el programa y vuelve a abrirlo: si Zink no llegó a presentar ni un fotograma, el controlador vuelve solo a `nvc0` y el menú lo avisa. Para entender dónde se quedó, adjunta `engine.log.1`: las líneas `mark: …` (subida de texturas, primeras presentaciones) y `mark: WATCHDOG …` dicen el último punto alcanzado.

### Los controles no hacen lo esperado
Menú → **Mandos**: ahí ves qué tecla recibe el juego por cada botón y puedes restablecer (**Y**). Salir del juego: **L + R + +**.

### Actualizaciones: «no se pudo consultar» o la descarga falla
Mensajes y qué hacer: **«no está conectada a internet»** (Wi-Fi de la consola); **«Falta cacert.pem»** (copia el `cacert.pem` del paquete a `switch/pokenx/`); **«no se pudo verificar la conexión segura»** (revisa que el reloj de la consola sea correcto); **«Mega limitó la descarga de hoy»** (cuota diaria agotada: prueba más tarde, o descarga el ZIP en otro dispositivo y usa «Instalar desde un ZIP de la SD»). La descarga se puede reanudar: lo descargado queda en `downloads/`.

### Mis partidas no aparecen tras actualizar
Se conservan en `data/partidas` y hay una copia en `backups/` (Actualizaciones → Copias de seguridad → restaurar). La versión anterior del juego se borra al actualizar (un solo juego en la tarjeta); tus partidas están en la nueva y en la copia fechada.

### Una tecla del juego (F5, Re Pág…) no hace nada
Esas teclas se envían con la **capa** de Mandos: mantén ZL y pulsa el botón indicado en la pestaña Capa. Las cifras y letras de menús salen con ZR (pestaña Capa 2). La pantalla **Teclas** marca en rojo las que ningún botón produce.

### Falla una función del juego a mitad de partida
Una función de GameMaker que el motor no implementa falla al primer uso del script que la llama. Es más probable con una versión del juego distinta de la probada. Abre una incidencia con `engine.log`, la sala y la versión del juego.

---

# B. Al compilar

### No sé qué me falta
Ejecuta `pwsh scripts/doctor.ps1`: lista, por grupos, lo que está bien, lo que falta (con el comando o el enlace) y los avisos (por ejemplo, devkitPro fuera de su carpeta estándar). Con `-DevkitPro <carpeta>` indicas otra ubicación.

### El script de Mesa dice «Review Mesa provenance…»
El clon de Mesa no está en el commit `2330d007712acbd32e396574203339df39bde153`. Haz `git -C third_party/mesa-switch checkout 2330d007712acbd32e396574203339df39bde153`.

### «Required file missing: …» al compilar Mesa
Falta una pieza del entorno de Mesa (paso 5 de la [guía](GUIA.md)): el entorno de Python (`target/mesa-tools/venv`), `bindgen.exe` (`target/mesa-tools/cargo/bin`), `clang.exe`/`llvm-config.exe`/`ninja.exe` (MSYS2 clang64 de devkitPro) o `libnx`. Instálala y repite la etapa.

### El entorno de Mesa no está automatizado
Es una limitación conocida: hoy son tres pasos manuales (paquetes de MSYS2 clang64 con una copia de `pacman.conf`, entorno de Python, `bindgen-cli`). Si algo falla ahí, es lo primero que hay que revisar en una instalación nueva.

### devkitPro no está en `C:\devkitPro`, o Git no está en su carpeta estándar
Algunos scripts usan la ubicación por defecto de devkitPro (`C:\devkitPro`) y la de Git Bash (`C:\Program Files\Git\bin\bash.exe`). Instala en esas rutas o edita las variables al principio de `app/engine/build.ps1`, `scripts/build-mesa-switch.ps1` y `app/engine/tests/check-mesa-abi.ps1`.

### `Mesa and engine Rust compiler versions must match…`
Usa **`nightly-2026-09-29`** para todo (motor y Mesa). El build compara las versiones de `rustc` y se detiene si no coinciden.

### `Mesa SDK archive differs from its build manifest`
El SDK no se generó con este código o está incompleto. Vuelve a ejecutar `-Stage Install`.

### `Relocation check failed` / `read-only segment has dynamic relocations`
**No lo silencies**: es el aviso de que el NRO no arrancaría. Revisa cualquier cambio en `app/engine/switch-pokenx.ld` o en las banderas del enlazador. Ver [SISTEMA.md](SISTEMA.md#arranque-seguro-del-nro).

### `Single-NRO rule violated`
Debe haber un único `.nro` bajo `pokenx/`; borra los sobrantes.

### Errores de `cargo` por la versión de Rust o `Cargo.lock`
Usa siempre `cargo +nightly-2026-09-29 …`. Los crates `gm8-*` heredan un `rust-toolchain` de OpenGMK distinto; los scripts ya fuerzan el correcto.

### `check.ps1` falla al compilar pruebas del PC
Hacen falta las Build Tools de Visual Studio (objetivo MSVC). La vista previa del menú necesita `g++` de MinGW y es opcional: sin él, esa etapa se omite.

### Solo Windows
Los scripts son PowerShell para Windows; no se han probado en Linux ni macOS.
