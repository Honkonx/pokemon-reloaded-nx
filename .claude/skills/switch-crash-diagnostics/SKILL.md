---
name: switch-crash-diagnostics
description: Diagnosticar un NRO que se cierra o falla en la Switch con los registros que escribe el proyecto (boot.log, engine.log, crash.log, last-session.txt, informe.txt) y los informes de Atmosphère; úsala cuando alguien reporte un cierre o la ausencia de registros.
---

# Diagnóstico de cierres en la Switch

Quien reporta copia `sdmc:/switch/pokenx/logs/` a un lugar donde puedas leerlo (esa carpeta no debe versionarse: puede contener rutas del juego). Lee **todo** lo que haya antes de suponer una causa.

## Qué escribe cada pieza (orden de arranque)
1. `boot.log` — `1.` libnx inicializado (con memoria total/usada y montón), `2.` constructores estáticos, `3.` `main()`, `4.` menú gráfico listo (o su fallo). **La última marca dice hasta dónde llegó el proceso.**
2. `engine.log` (+ `engine.log.1`, la ejecución anterior) — cada línea con `[ms]`: trazas de EGL, memoria antes/después de cargar, fases de `Game::launch` (`launch: before …`), `game started` o `load failed`, avisos de vídeo.
3. `crash.log` — si hubo una excepción de CPU: tipo, `pc`, `lr`, `sp`, `far`, `esr`, base del módulo y registros.
4. `last-session.txt` — el motivo de la última salida, que el menú muestra la siguiente vez.
5. `informe.txt` — resumen que genera Diagnóstico → X.
6. `sdmc:/atmosphere/crash_reports/*.log` — informe de Atmosphère (pídelo si hay un cierre sin `crash.log`).

## Interpretar
- Sin `boot.log`: el proceso murió antes de `userAppInit` (relocaciones, carga del NRO) o la microSD no se montó. Ver `switch-nro-startup-crash`.
- Memoria: el modo applet deja ≈ 380–512 MiB; el juego necesita más. Pide abrirlo como aplicación (manteniendo **R** sobre un juego).
- Convertir `pc` en símbolo: `offset = pc − module_text_base`; `aarch64-none-elf-addr2line -f -C -e app/engine/target/pokenx-engine.elf 0x<offset>` (usa el ELF, no el NRO).
- En Horizon, escribir un archivo nuevo en la microSD sin modo **append** puede fallar; si un registro sale vacío, sospecha de eso primero.

## Reglas
- Cada hipótesis queda ordenada y con lo que la confirmaría. No inventes causas: si no hay evidencia, dilo.
- No afirmes «funciona» sin la prueba de quien lo probó (`evidencia-por-etapas`).
