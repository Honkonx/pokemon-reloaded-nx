# gm8-resource-loader

Portable `no_std`/`alloc` vertical slice for standard GM8.0 Sprite and
Background loading. It validates the resource directory through `gm8-assets`,
retrieves compressed spans from the indexed payload, and delegates decoding to
the bounded `gm8-sprite` and `gm8-background` parsers. Returned records own
their data and preserve the original table indexes. Directory, per-record,
record-count, and aggregate payload-byte budgets are configurable. Half the
remaining aggregate byte budget caps each transient inflated record so the
copied fields fit in the other half. This is not a strict heap ceiling:
allocator metadata and excess `Vec` capacity are outside the accounting.

The parser implementation is derived from the OpenGMK workspace copy at
`src/gm8-runtime/` (upstream checkout commit
`9e4282e6d19f4dd00961811840ddc3e11da6dd5a`), chiefly its `gm8-format-probe`,
`gm8-assets`, `gm8-zlib`, and `gm8-sprite` crates. The runtime copy retains the
upstream GPL-2.0-only license and notices; this crate is also GPL-2.0-only.

This is deliberately not a full `GameAssets` loader: it decodes only Sprite
and Background records from standard GM8.0 tables. It does not decode other
resource kinds, instantiate textures, compile GML, run rooms/events, support
GM8.1 encrypted or UPX/antidec payload variants, or prove compatibility with
Pokémon Reloaded.
Tests use only generated synthetic bytes.

Run host tests with `cargo test --manifest-path Cargo.toml`. Horizon check:
`cargo +nightly check --manifest-path Cargo.toml --target aarch64-nintendo-switch-freestanding -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem`.
