# gm8-assets

`no_std` bounded index for the compressed resource records in an unprotected, standard GM8.0 payload. It walks the whole GM8.0 directory, stores kind/index/span references up to a caller-supplied limit, and returns borrowed slices from the original payload. Index lookup uses the directory's kind-and-index order.

The crate does not decompress resources, parse every asset family, process extensions/constants into OpenGMK types, compile GML, or instantiate a game. It is an input layer for those later steps, not a replacement for `gm8exe::GameAssets` or `gm8emulator::Game`.

Host tests use synthetic payloads only:

```powershell
cargo test --manifest-path src/gm8-runtime/gm8-assets/Cargo.toml
```
