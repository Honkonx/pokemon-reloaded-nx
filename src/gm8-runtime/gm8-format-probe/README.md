# gm8-format-probe

This `no_std` crate recognizes the standard GameMaker 8.0 loading sequence, skips the settings and embedded-DLL prelude, then decrypts the initial payload in place with bounds checks. It validates the first metadata fields (`Pro` and `game_id`) following OpenGMK `gm8exe/src/gamedata/gm80.rs` and `reader.rs`. Keep the OpenGMK GPL-2.0-only license and attribution when adapting or distributing it.

It does not recognize GM8.1, UPX/antidec loader variants, or arbitrary patched loaders. It walks the bounded extension records and all GM8.0 resource section tables through room order; asset records are emitted as offsets/spans and are neither copied nor decompressed. `scan_gm80_resource_prefix` handles metadata through sounds, and `scan_gm80_resource_tail` continues from its end offset through sprites, backgrounds, paths, scripts, fonts, timelines, objects, rooms, included files, help data, init strings, and room order. Callers can use `scan_gm80_resource_directory` to coordinate both scans under one shared record budget; the app's ZIP validation uses this entry point. This is a bounded directory walk for the standard GM8.0 layout, not resource deserialization or a game runner. A negative result means “not recognized by this partial parser,” not “not a GameMaker game.” The PokeNX app accesses the PE and prelude by offset, then reads the encrypted GM8.0 resource block into temporary memory capped at 64 MiB; this is not a game runner.

Host tests use synthetic byte arrays and contain no game data:

```powershell
cargo test --manifest-path src/gm8-runtime/gm8-format-probe/Cargo.toml
```

The crate also cross-compiles to the current Switch target without linking the complete OpenGMK parser:

```powershell
cargo build --manifest-path src/gm8-runtime/gm8-format-probe/Cargo.toml --lib --target aarch64-nintendo-switch-freestanding -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem
```
