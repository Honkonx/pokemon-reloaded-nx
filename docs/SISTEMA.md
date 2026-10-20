# Sistema: mandos, audio, menú, archivos, detección y ciclo de vida

Todo lo que rodea al motor y al renderer: lo que el juego esperaba de Windows y aquí lo hace el programa de la Switch (`app/engine/`).

## Mandos

- **Qué se lee:** hasta 8 mandos, modo portátil, Pro Controller y Joy-Con. Un **Joy-Con solo** se lee en horizontal (el palo se gira; SL/SR hacen de hombros).
- **Botones → teclas:** cada botón produce una tecla de GameMaker (`gm8-input::ButtonMap`). Los valores por defecto y cómo cambiarlos están en el [README](../README.md#controles); la pantalla **Mandos** los reasigna y los guarda en `controls.txt` (formato `BOTON=TECLA`, un par por línea; botones `A B X Y L R ZL ZR PLUS MINUS STICK_L STICK_R`; teclas: letras, dígitos, `SPACE`, `ENTER`, `ESC`, `TAB`, `SHIFT`, `CTRL`, `ALT`, `BACKSPACE`, `DELETE`, `INSERT`, `HOME`, `END`, `PAGEUP`, `PAGEDOWN` y `F1`–`F12`).
- **Capa de teclas:** `LAYER=ZL` (o `ZR`, `L`, `R`, `NONE`) elige el botón modificador y `LAYER.A=F5`, `LAYER.UP=F1`… dan la tecla de cada botón mientras se mantiene. Con la capa activa el modificador no envía nada por sí mismo; un toque corto (sin pulsar otro botón) envía su tecla normal unos 70 ms. Valores por defecto: A=F5, B=F6, X=F11, Y=F12, L=F10, R=Supr, +=Inicio, −=Fin, cruz = F1 / F9 / Re Pág / Av Pág.
- **Segunda capa (ZR):** `LAYER2=ZR` y `LAYER2.A=1`… funcionan igual que la primera y dan las cifras y letras que ningún botón daba: A, B, X, Y, L, R = 1–6 (compañeros y movimientos de campo), cruz = Q, P, L, F (menús de misiones, entrenador, áreas y búsqueda), + = T, − = N, clic del palo izquierdo = H, clic del derecho = K. Si se mantienen las dos capas, gana la primera. Un archivo `controls.txt` sin líneas `LAYER2` usa estos valores.
- **Perfiles de mandos:** *Por defecto* (de fábrica, no se edita), *Perfil 1* (`controls.txt`, el archivo de siempre) y *Perfil 2* (`controls-2.txt`). `profile.txt` (`default`, `1` o `2`) dice cuál usa el juego; en **Mandos**, **ZL/ZR** cambian de perfil y la pestaña **Vista** dibuja el mando con la tecla de cada botón (y, en verde, la de la capa).
- **Cruz y palo izquierdo:** siempre son las flechas; el palo usa histéresis (por defecto entra al 55 %, sale al 40 %) para no vibrar. La opción **Zona muerta del palo** (`stickzone.txt`: `small` 35 %, `normal`, `large` 70 %) lo ajusta.
- **Salir del juego:** `L + R + +` (con un Joy-Con: `ZL + ZR` + `+` o `−`).
- **Sin controles táctiles** en pantalla.

## Audio

- Salida por `audout`: anillo de 4 búferes de 1024 muestras estéreo a 48 kHz, en un hilo propio (núcleo 1, prioridad `0x2B`).
- El mezclador del motor entrega PCM: WAV, MP3 y MIDI. **Sin `soundfont.sf2`, la música MIDI es silenciosa**; se coloca en la carpeta del programa.
- El hilo se pausa cuando la aplicación pierde el foco (HOME, suspensión).

## Menú

- **Gráfico** (`app/engine/gfx.cpp`, `menu.cpp`): un *framebuffer* lineal de libnx de 1280×720 con formas propias y **la fuente del sistema** (`stb_truetype`). No usa OpenGL: corre antes de EGL y libera la ventana al cerrarse.
- **Respaldo de texto** (`menu_text.cpp`): si el framebuffer o la fuente no arrancan, se usa la consola de texto de libnx. `boot.log` (línea 4) dice cuál de los dos se usó.
- **Pantallas:** Inicio (mosaicos), Actualizaciones (versión nueva, descarga, ZIP de la microSD y copias de seguridad), Opciones, Mandos (pestañas Botones, Capa 1, Capa 2 y Vista), Teclas (las del juego y qué botón las produce), Acceso HOME (crea el icono en la pantalla de inicio), Diagnóstico y tarjetas de aviso.
- **Vista previa en un PC:** `tools/menu_preview` compila `gfx.cpp` y `menu.cpp` con un `switch.h` de sustitución y guarda capturas PNG de cada pantalla. Comprueba el dibujo, no la consola.

## Archivos e importación

- **Importar:** explorador de la microSD; el ZIP se extrae a una carpeta temporal y se **valida antes de sustituir `game/`**: rutas peligrosas (`..`, absolutas), CRC, cabecera PE, carpeta `data/` y estructura GM8. La versión anterior **se borra** al terminar: no quedan dos copias del juego (tus partidas pasan a la nueva y queda una copia fechada en `backups/`). **B** o **+** cancelan sin dañar la instalación actual.
- **Actualizaciones del juego:** el blog oficial publica una entrada por versión con enlace de Mega. El menú lee su *feed*, pregunta a Mega el nombre y el tamaño, descarga y descifra el ZIP (AES-CTR, por tramos, reanudable) a `downloads/` y lo instala con el importador. Necesita Wi-Fi, `cacert.pem` en la carpeta del programa (viene en el paquete) y espacio libre (≈ 2,5 veces el ZIP). Mega limita la descarga diaria a quien no tiene cuenta: si falla, queda instalar un ZIP descargado a mano.
- **Tus datos al actualizar o importar:** antes de cambiar `game/`, se hace una copia fechada (`backups/copia-NNN`, hasta 5) y se pasan a la versión nueva las **partidas** (`data/partidas`), los **ajustes** (`data/text/config.ini` y `data/audio/config.ini`; si el de la versión nueva es distinto se guarda como `config.ini.nuevo`), las **capturas** y `LogBattle.txt`. Cajas, equipos y personajes propios se copian sin pisar lo que traiga la versión nueva. Esa lista sale de las llamadas a archivos del propio juego.
- **Teclas del juego:** el juego guarda su teclado en variables que rellena su código. El menú las lee una vez (unos segundos; hace falta abrir el programa como aplicación) a `cache/game-keys.txt` y las compara con tus botones y las capas. Mientras juegas, el motor anota cada par de segundos las teclas que el juego usa en ese momento (`cache/game-keys-live.txt`); si cambiaste teclas dentro del juego, la pantalla muestra las tuyas y marca las cambiadas con «antes».
- **Compatibilidad por versión:** en Actualizaciones, «Compatibilidad de esta versión» cuenta las funciones integradas de GameMaker que llama la versión instalada y separa las que el motor no tiene y las que ejecuta con menos detalle (`cache/game-compat.txt`). Se ofrece tras instalar una versión nueva. Es un análisis del código, no una prueba de que todo funcione: no ve las llamadas que el juego calcula mientras corre.
- **Acceso en HOME desde el programa:** la pantalla Acceso HOME crea un título pequeño (un «forwarder») que arranca `sdmc:/switch/pokenx/pokenx.nro`: construye en memoria tres archivos de contenido, los escribe en la tarjeta SD y registra la aplicación (A crea o actualiza, Y lo quita). Necesita Atmosphère con *sigpatches*. No se ha probado todavía en una consola.
- **Guardados:** los hace el propio juego; el motor los escribe de forma atómica y deja copias de recuperación `.pokenx-save.bak` y `.pokenx-save.tmp.N` (no las borres al limpiar).
- **Caché de archivos** (`filecache.txt`): el juego relee los mismos datos al cambiar de escena; se guardan en memoria y se vacían ante cualquier escritura.

## Detección y versiones del juego

- Se busca en `game/` (y, si no hay nada en la raíz, en sus subcarpetas inmediatas) un `.exe` que sea un **contenedor GameMaker 8.0 estándar**: cabecera `MZ`, firma `PE\0\0` de i386, la secuencia del cargador de GM8.0 en posiciones fijas y la versión de cabecera 800. **El nombre y el tamaño no intervienen.** Si hay más de un contenedor válido, el programa lo dice y no elige: deja solo la versión que quieras.
- **Versión probada:** 1.9.2.2 (un `.exe` sin DLL ni extensiones). Con otra versión pueden pasar, de más a menos probable:
  1. **Una función de GameMaker que el motor aún no implementa** (falla al primer uso del script que la llama).
  2. **DLL o extensiones externas**: son código x86 y no se ejecutan.
  3. **Otro formato** (GameMaker 8.1, `.exe` comprimido con UPX, anti-descompilación): no se detecta y el menú dice que no hay juego.
  4. **Los parches de GML** (`patches.rs`) se aplican por nombre de script; si cambia, no se aplican y reaparece el defecto original.
  5. **Partidas guardadas**: dependen del juego; haz una copia antes de cambiar de versión.
- **Comprobar una versión nueva** (para quien compila): `inspect_compatibility` cuenta las funciones que usa el código sin ejecutar nada ni mostrar nombres (`cargo +nightly-2026-09-29 run --release -p pokenx-core --example inspect_compatibility -- "<ruta-del-juego.exe>"`), y la prueba `real_game_smoke` abre el juego **desde una carpeta del PC** con OpenGL de Windows, avanza fotogramas y avisa del primer error con la sala y el fotograma (ver `host_gl_fixture.rs`).

## Ciclo de vida y memoria

- **Orden de arranque** (libnx), útil para razonar sobre «no hay registro»: `_start` → `__nx_dynamic` (relocaciones y protección RELRO) → `envSetup` → `newlibSetup` → `virtmemSetup` → `__libnx_initheap` → `__appInit` (servicios, microSD, `userAppInit`: **línea 1 de `boot.log`**) → constructores estáticos (**línea 2**) → `main` (**línea 3**) → menú (**línea 4**).
- **Modo aplicación frente a applet:** como aplicación el proceso tiene ≈ 3 GiB; desde el Álbum (applet), 380–512 MiB. Con menos de 1 GiB, «Jugar» se bloquea.
- **Durante la carga:** `loading_pump` mantiene vivos HOME y los mandos, deja **cancelar con `+`** y dibuja una barra.
- **Excepciones:** `crash.log` (tipo, `pc`, `lr`, `sp`, registros). **`last-session.txt`** guarda el motivo del último cierre y el menú lo muestra la siguiente vez.
- **La ventana no se reabre tras Mesa**: el menú solo se muestra al abrir el programa.

## Medidor de FPS, precarga y cachés

- **Medidor de FPS** (**Opciones → Medidor de FPS**, `fps.txt`): un recuadro en la esquina del juego con los fotogramas por segundo presentados y el hueco más largo entre dos fotogramas (ms); verde si va a 55 o más, amarillo por debajo y rojo por debajo de 40. Se pinta con recortes y borrados de OpenGL justo antes de presentar el fotograma, sin tocar programas, texturas ni búferes del juego (solo cambia y restaura el recorte, el color de borrado y la máscara de color). Las **pausas del propio juego** (`sleep()` en diálogos y escenas, de 200 a 2000 ms, durante las que no se presenta ningún fotograma) se descuentan: no cuentan como caída, y si la ventana tuvo alguna, una tercera línea «P» da sus milisegundos.
- **Precarga** (**Opciones → Precargar el juego**, `preload.txt`): al abrir el menú se leen y analizan los datos del juego en un hilo; **Jugar** se salta ese trabajo si ya terminó. Solo en modo título. Cualquier operación que llame al motor (importar, copias, leer teclas) espera antes a ese hilo.
- **Cachés:** archivos leídos enteros (`filecache.txt`) y **imágenes decodificadas** (hasta 48 MiB; `POKENX_IMAGE_CACHE_MB`), que se vacían ante cualquier escritura. El código de creación de salas e instancias, como el de los scripts, se compila al usarlo; el resultado se **conserva** entre visitas (una sala ya visitada no se recompila).
- **Compilar por adelantado en las pausas:** el motor anota qué sala suele seguir a cada sala (`cache/room-hints.txt`, líneas `origen destino veces`) y, cuando el juego se pausa a sí mismo (`sleep` de 30 ms o más), emplea hasta la mitad de esa pausa en compilar el código de las salas que más a menudo vienen después. El juego espera lo mismo; solo se ahorra el tirón al entrar. Si el archivo es de otra versión del juego, solo se compilan salas que nadie visita.
- **Eventos añadidos por el juego al arrancar:** el juego llama más de cien veces a `object_event_add` y `object_event_clear` al empezar. Cada llamada reconstruía las listas de todos los eventos de todos los objetos; ahora actualiza solo la lista del evento afectado (los eventos de colisión siguen reconstruyéndose enteros). El resultado es idéntico (una prueba lo compara con la reconstrucción completa) y la entrada a la primera sala pasó de ≈ 0,97 s a ≈ 0,2 s en un PC.
- **Las cachés ya no se vacían enteras al escribir:** escribir, renombrar, copiar o borrar un archivo expulsa solo lo que pudo quedar obsoleto (la misma ruta, lo que cuelga de ella o un archivo con el mismo nombre); las carpetas siguen vaciando todo. Así un guardado ya no obliga a releer decenas de archivos ni a decodificar otra vez las imágenes.

## Relojes y rendimiento

`clkrst` sube CPU, memoria y GPU mientras se juega y los restaura al salir. Perfiles en **Opciones → Rendimiento**: *Máximo* (CPU 1785 MHz; GPU 460,8 MHz portátil / 768 MHz dock), *Equilibrado* (1224 MHz; 384 / 614,4 MHz) y *Sin cambios*.

## Registros y diagnóstico

`logs/boot.log`, `engine.log` (con marcas de tiempo en milisegundos; `engine.log.1` es la ejecución anterior), `crash.log`, `last-session.txt` e `informe.txt` (**Diagnóstico → X**: memoria, modo, tiempos de carga, ajustes y finales de los registros, sin datos del juego). Qué hacer con ellos: [ERRORES-FRECUENTES.md](ERRORES-FRECUENTES.md).

### Lo que anota `engine.log` durante el juego

- `video: host … Hz, … max host work …, late N/M` cada 5 s, y justo después `pace: …`: cuántos milisegundos se pausó el propio juego (`sleep`) y en cuántas pausas, cuántos archivos leyó de la tarjeta (y KiB), cuántos cambios de sala hubo (tiempo del motor, sin contar las pausas, y el más largo) y cuántas salas se compilaron por adelantado. Sirve para distinguir «el motor va lento» de «el juego se pausó» o «cambió de sala».
- Líneas `mark: …` (se escriben en la tarjeta al instante): la subida de texturas a la GPU (`mark: atlas 1/1 …`), las tres primeras presentaciones (`mark: eglSwapBuffers #1 begins` / `done in N ms`) y los dos primeros fotogramas. Si el programa se queda dentro de una llamada del controlador gráfico, la última marca dice cuál.
- Vigilante: un hilo de baja prioridad comprueba cada segundo que algo avance (líneas del registro, fotogramas, lectura del mando). Si no avanza nada durante 10 s, 30 s, 1, 2, 3, 5, 10 y 15 min, escribe `mark: WATCHDOG: no progress for N s; …; last checkpoint at … ms: <última marca>` y vacía el registro. No actúa sobre el juego; solo deja constancia.
- **Archivo de desarrollo `dev-env.txt`** (no está en el menú): líneas `NOMBRE=VALOR` que se aplican antes de iniciar EGL, solo con prefijos `NVK_`, `MESA_`, `NOUVEAU_`, `ZINK_` o `POKENX_` (por ejemplo `POKENX_ATLAS_MAX=2048` para atlas más pequeños). Cada línea aplicada queda en `engine.log`.
- **Zink que no llega a mostrar un fotograma** (el menú ya no ofrece Zink; solo se activa con `graphics-driver.txt` en `zink` **y** un `dev-env.txt` presente, aunque sea vacío; un `zink` heredado sin ese archivo se ignora): al empezar una sesión con el controlador `zink` se crea `zink-pending.txt` y se borra al presentar el primer fotograma del juego (o al terminar la sesión por sí sola). Si el programa se cerró a la fuerza antes, al abrirlo de nuevo el controlador vuelve a `nvc0` y el menú lo explica; se puede elegir Zink otra vez en Opciones.

## Arranque seguro del NRO

Un NRO enlazado con relocaciones en memoria de solo lectura moría **antes de mostrar nada** (pantalla negra, ningún registro). La causa: el cargador de homebrew mapea `.rodata` como solo lectura, pero libnx escribe cada relocación al arrancar, y los objetos no PIC de libstdc++/libc del kit las generan. La solución: `app/engine/switch-pokenx.ld` mueve `.rodata` a la zona RELRO escribible, y `scripts/verify_relocations.py` **detiene el build** si alguna relocación queda fuera de un segmento escribible. No se silencia el aviso del enlazador `read-only segment has dynamic relocations`: se resuelve.
