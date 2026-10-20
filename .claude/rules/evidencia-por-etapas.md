# Evidencia por etapas: «funciona» lo declara quien lo prueba

El mínimo para decir que algo **funciona a medias** es, en una Switch real: el juego **abre, renderiza, suena, responde a los mandos y se puede jugar**. Aun así faltan cosas (compatibilidad completa, estabilidad, rendimiento sostenido, guardado, suspensión). Que el NRO abra es una etapa, no una validación.

Informa cada resultado con su nombre exacto, en este orden: **compila en el PC → compila para la Switch → NRO generado → abre → renderiza → suena → controles → se juega**. Por debajo de «se juega» no uses «funciona», «listo» ni «compatible»: di el nivel concreto y lista lo que falta.

- Marca algo como probado en hardware solo con evidencia de quien lo probó (su palabra explícita, un registro o una captura). No la deduzcas de un build.
- Un código de salida 0, `command -v`, una instalación declarada o un build exitoso no demuestran por sí solos que el binario corra.
- Las cifras de rendimiento solo valen con su fuente (un registro con marcas de tiempo o la salida de una prueba). Las estimaciones se etiquetan como estimaciones.
- Al cerrar una tarea, di qué etapas se verificaron y cuáles no.
