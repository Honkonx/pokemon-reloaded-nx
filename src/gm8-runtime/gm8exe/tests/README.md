# Fixture GM8 libre

`support/fixture.rs` crea datos originales GPL-2.0-only; no contiene assets de juegos ni runner Windows. El contenedor `.exe` es exclusivamente datos GM8: firmas del formato, sala, sprite, objeto/eventos GML, script y tono WAV generado. No ejecutarlo como un programa Windows.

- `full_fixture.rs` prueba carga estricta y truncamiento con el parser real, también con `--no-default-features`.
- `pokenx-core/src/title_tests.rs` prueba detector y compilación GML del contenedor completo.
- `pokenx-core/src/host_gl_fixture.rs` ejecuta el núcleo en un contexto WGL nativo oculto (opcional, solo tests Windows): VM, entrada, framebuffer, PCM y cierre. No prueba un NRO ni usa emuladores.

Desde la raíz: `pwsh scripts/check.ps1 -WithNativeGl` (necesita driver OpenGL3.3). El check normal omite la prueba GPU.

Para generar archivos en un directorio NUEVO bajo target: `cargo +nightly-2026-09-29 run --manifest-path src/gm8-runtime/gm8exe/Cargo.toml --example make_fixture -- target/mi-fixture-gm8`. Nunca se instala automáticamente sobre el juego del usuario ni se incorpora al paquete SD.

Detalles y límites: `docs/auditorias/FIXTURE_GM8_Y_RUNTIME_HOST_2026-10-16.md` en la raíz del proyecto. Esta prueba no acredita funcionamiento de Switch, cadencia60FPS ni compatibilidad del título.
