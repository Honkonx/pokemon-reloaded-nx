# gm8-real

Reusable extraction of OpenGMK's GameMaker `Real` type. The desktop crate keeps
the default `std` math implementation; `--no-default-features` switches math
operations to `libm`, so the same numeric semantics can compile on the Switch
freestanding target.

This crate is only a portable numeric primitive. It does not make the
`gm8emulator` crate, GML VM, renderer, input, audio or window loop compatible
with Horizon by itself. The NRO references this crate as a cross-target build
check while the upstream desktop runtime retains its normal dependency graph.

Tests:

```powershell
cargo test --manifest-path src/gm8-runtime/gm8-real/Cargo.toml
cargo test --manifest-path src/gm8-runtime/gm8-real/Cargo.toml --no-default-features
```
