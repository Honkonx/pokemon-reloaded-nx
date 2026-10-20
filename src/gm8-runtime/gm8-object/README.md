# gm8-object

Parser `no_std` acotado para registros Object y CodeAction de GameMaker 8.0. Sigue el esquema de OpenGMK (`gm8exe/src/asset/object.rs` y `code_action.rs`) y conserva las cadenas como bytes para no imponer una codificación.

Decodifica el sobre zlib de cada recurso, el marker genérico, los valores por defecto del objeto, sus 12 listas de eventos y las acciones DnD/GML. El helper de tabla consume los spans producidos por `gm8-format-probe` y comparte el escaneo completo del directorio GM8.0. Rechaza versiones inesperadas, datos truncados, cadenas, subeventos, acciones y salidas que exceden los límites configurados. No compila el GML, no ejecuta scripts ni instancia objetos.

## Verificación

```powershell
cargo test --manifest-path src/gm8-runtime/gm8-object/Cargo.toml
cargo +nightly-2026-09-29 check --manifest-path src/gm8-runtime/gm8-object/Cargo.toml --target aarch64-nintendo-switch-freestanding -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem
```

Las pruebas usan registros sintéticos. El crate compila para ARM64/Horizon, pero aún no está integrado en `app/` ni se ha ejecutado en Switch. La cobertura actual valida objetos con listas vacías, una acción GML, marker borrado, versiones/cadenas inválidas y registros truncados. El schema deriva del código GPL-2.0-only de OpenGMK; mantener atribución y obligaciones de licencia.
