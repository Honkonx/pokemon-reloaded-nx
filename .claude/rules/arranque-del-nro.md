# Arranque del NRO: nada de relocaciones en memoria de solo lectura

Causa raíz ya vista: un NRO enlazado con `-Wl,-z,notext` llevaba cientos de relocaciones en `.rodata`; libnx las escribe al arrancar y el cargador de homebrew mapea ese segmento como solo lectura, así que el programa moría **antes de `userAppInit`**: pantalla negra, ningún registro y `+` sin respuesta.

- **Nunca** enlaces un NRO con `-z notext` sin la comprobación posterior. `app/engine/build.ps1` usa el script de enlace propio `app/engine/switch-pokenx.ld` (mueve `.rodata` a la zona RELRO escribible) y ejecuta `scripts/verify_relocations.py`; si falla, el build se detiene.
- `scripts/check.ps1` incluye la etapa de relocaciones. No des un paquete por válido si falla.
- Cualquier binario nuevo para la Switch con enlace propio debe pasar por el mismo script y el mismo verificador.
- El mensaje del enlazador `read-only segment has dynamic relocations` **no se silencia**: se resuelve.
- Orden de arranque de libnx para razonar sobre «no hay registro»: `_start` → `__nx_dynamic` → `envSetup` → `newlibSetup` → `virtmemSetup` → `__libnx_initheap` → `__appInit` (servicios, microSD, `userAppInit`) → constructores estáticos → `main`. Un `boot.log` ausente sitúa el fallo antes de `userAppInit`.
- Montón: `app/engine/main.cpp` define `__libnx_initheap` con 32 MiB de reserva para el driver gráfico. No reabras la consola ni el framebuffer después de que Mesa usó la ventana.
- Skill: `switch-nro-startup-crash`.
