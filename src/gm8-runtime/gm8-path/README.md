# gm8-path

Bounded `no_std` parser for GM8.0 Path records (version 530), including zlib envelope, name, interpolation mode, closed flag, precision, and f64 point coordinates/speeds. Unknown interpolation values are preserved for diagnosis. Deleted records remain absent while table indexes stay stable.

It does not implement path interpolation or connect paths to the OpenGMK simulation loop.

```powershell
cargo test --manifest-path src/gm8-runtime/gm8-path/Cargo.toml
```
