---
name: switch-nro-startup-crash
description: Diagnosticar un NRO que muestra pantalla negra, no crea registros y no responde al botón + en la Switch (un proceso que muere o se cuelga antes de userAppInit); úsala cuando no exista boot.log o al cambiar banderas o scripts de enlace o el arranque de libnx.
---

# NRO que muere antes de `userAppInit`

Síntoma: pantalla negra, ningún `boot.log`, `+` no responde. El código propio no llegó a ejecutarse. Pasos, en orden:

## 1. Relocaciones
- `python scripts/verify_relocations.py app/engine/target/pokenx-engine.elf` debe terminar en `OK`. Si dice `ERROR`, el NRO se cae en `crt0` (libnx escribe cada relocación; el cargador mapea el texto `R+X` y `.rodata` `R`).
- Origen típico: `-Wl,-z,notext` sobre objetos no PIC (libstdc++/libc del kit con vtables en `.rodata`). Solución: `app/engine/switch-pokenx.ld` (`.rodata` dentro de RELRO) mediante el `specs` que genera `build.ps1`.
- Control: compila un `main.cpp` mínimo con `throw std::runtime_error` y `-specs=switch.specs`; si el enlazador dice `read-only segment has dynamic relocations`, la toolchain exige la corrección.

## 2. Instrucciones que el Cortex-A57 no tiene
Desensambla (`aarch64-none-elf-objdump -d`) y busca LSE (`cas`, `swp`, `ldadd`), `sdot`/`udot`, `sqrdmlah`, fp16, SVE. Las LSE existen solo en los `__aarch64_*` protegidos por `getauxval`, que el proyecto deja en 0 (camino seguro).

## 3. Montón y servicios antes de `main`
- `__libnx_initheap` propio (reserva 32 MiB). Si falla `svcSetHeapSize` aborta con `LibnxError_HeapAllocFailed` sin registro: la primera línea de `boot.log` imprime el tamaño del montón.
- `__appInit` aborta (pantalla de error de libnx) si falla `sm`, `applet`, `hid`, `time` o `fs`.

## 4. Si hay `boot.log`
La última línea dice hasta dónde llegó: `1.` libnx listo, `2.` constructores estáticos, `3.` `main`, `4.` menú gráfico (o su fallo). Sigue con `switch-crash-diagnostics`.

## 5. Si no hay `boot.log`
Pide `sdmc:/atmosphere/crash_reports/*.log` (dirección de la falla) y la versión de Atmosphère y firmware. Busca la dirección en `app/engine/target/pokenx-engine.map` y en el desensamblado.

Regla asociada: `.claude/rules/arranque-del-nro.md`.
