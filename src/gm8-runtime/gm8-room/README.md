# gm8-room

Parser `no_std` y acotado para registros Room de GameMaker 8.0 (versión 541). Sigue el esquema de OpenGMK `gm8exe/src/asset/room.rs`; retiene nombre, caption y creation code como bytes, además de backgrounds, views, instances y tiles. No acepta las extensiones de registro Room de GM8.1 (810/811).

El parser limita el tamaño comprimido y descomprimido, cadenas, cantidades de cada tabla y bytes totales de strings. `load_gm80_room_table` usa los spans del índice completo y aplica un límite agregado entre salas. No instancia objetos ni ejecuta creation code.

## Verificación

```powershell
cargo test --manifest-path src/gm8-runtime/gm8-room/Cargo.toml
cargo +nightly-2026-09-29 check --manifest-path src/gm8-runtime/gm8-room/Cargo.toml --target aarch64-nintendo-switch-freestanding -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem
```

Las pruebas sintéticas cubren sala mínima, campos de las cinco tablas, versión 810 rechazada, exceso de vistas, truncamiento, strings y bytes finales. El crate compila para ARM64/Horizon y `app/` lo enlaza en su diagnóstico de recursos; aún no se ha probado en hardware ni se ha conectado a un runner. El schema deriva del código GPL-2.0-only de OpenGMK; mantener atribución y obligaciones de licencia.
