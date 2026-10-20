# Sin datos del juego

- El juego y sus datos son **entradas del usuario final**. No los añadas al código, al repositorio, a la integración continua, a los registros compartidos, a las capturas ni al `.nro`.
- El `.exe` se lee como **contenedor de datos GameMaker**; analizarlo no es ejecutar instrucciones x86. Este proyecto adapta código fuente de OpenGMK a un runtime nativo ARM64; no usa Wine, Box64, FEX ni traducción dinámica.
- Las pruebas automáticas usan **datos sintéticos o libres** (el crate `gm8exe` trae un contenedor GM8 de prueba). Las pruebas con un juego real (`real_game_smoke`, `inspect_compatibility`) corren desde una carpeta local del PC y no copian ni imprimen su contenido.
- Evita nombres internos de scripts o recursos del juego en documentos y mensajes; usa descripciones generales.
- La carpeta de datos canónica en la microSD es `sdmc:/switch/pokenx/` (juego en `game/`).
