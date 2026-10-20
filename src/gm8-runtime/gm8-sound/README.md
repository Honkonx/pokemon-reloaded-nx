# gm8-sound

Bounded `no_std` parser for compressed GM8.0 Sound asset records. `load_gm80_sound_table` validates a complete GM8.0 resource directory with `gm8-format-probe`, extracts bounded Sound spans, and deserializes those records under per-record and aggregate output limits. `parse_indexed_sound_record` also accepts one `AssetSpan` and rechecks its bounds. The record envelope mirrors OpenGMK `gm8exe/src/reader.rs` (`get_assets`): zlib-decompress, consume the generic nonzero asset marker, then parse the Sound record implemented in `gm8exe/src/asset/sound.rs`. Deleted entries are omitted from `sounds` and counted in `deleted_count`, matching OpenGMK's `AssetList`. Other resource kinds remain spans; this is not a complete game loader or audio output backend. The Sound schema is GPL-2.0-only, from the OpenGMK project. This parser uses the sibling `gm8-zlib` adapter.

Pascal strings are retained as byte vectors, matching OpenGMK's `PascalString` representation and preserving legacy encodings. The parser rejects trailing decompressed bytes; upstream's stream reader does not explicitly check this.

`gm8-zlib` and `gm8-sound` follow OpenGMK's GPL-2.0-only licensing. The inflater uses `miniz_oxide` 0.8.9, which is separately licensed MIT OR Apache-2.0 (with `adler2` similarly dual-licensed); retain their upstream notices when distributing binaries. No title files or game-derived fixtures are included. The parser has not been called by the app or the complete GM8 loader and does not play sound.

```powershell
cargo test --manifest-path src/gm8-runtime/gm8-sound/Cargo.toml
cargo +nightly-2026-09-29 build --manifest-path src/gm8-runtime/gm8-sound/Cargo.toml --lib --target aarch64-nintendo-switch-freestanding -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem
```
