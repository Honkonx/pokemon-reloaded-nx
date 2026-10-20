# gm8-zlib

Small `no_std` adapter around the pure Rust `miniz_oxide` backend already used by OpenGMK's `flate2` configuration. It decodes zlib-wrapped DEFLATE with a caller-supplied hard output limit and is intended for the Switch runtime's resource-loading path.

The caller must validate each GM8 record's declared uncompressed size and budget it before calling this API. This crate does not parse GM8 resources, validate asset semantics, or run the game. The license for this runtime adapter follows the OpenGMK workspace (`GPL-2.0-only`); `miniz_oxide` is a separate dependency whose license is recorded upstream.

Host synthetic tests:

```powershell
cargo test --manifest-path src/gm8-runtime/gm8-zlib/Cargo.toml
```

Switch library cross-build:

```powershell
cargo +nightly-2026-09-29 build --manifest-path src/gm8-runtime/gm8-zlib/Cargo.toml --lib --target aarch64-nintendo-switch-freestanding -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem
```

This crate is not yet connected to the app or the OpenGMK resource deserializers.
