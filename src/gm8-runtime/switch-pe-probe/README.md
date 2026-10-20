# Switch PE probe (`no_std`)

Isolated feasibility experiment for parsing the outer DOS/PE/COFF header without `std`, heap allocation, or OS services. Its fixture is generated in a unit test and contains no game data.

The probe only checks `MZ`, the PE signature, and the i386 COFF header. It is **not** a GameMaker 8 parser and does not demonstrate that `gm8exe::reader::from_exe` or `gm8emulator` builds for Switch. Upstream `gm8exe` still uses `std::io`, `flate2`, and Rayon; deeper GM8 data discovery/decryption and asset deserialization remain outside this experiment.

Run the host test with `cargo test` in this directory. Cross-compile the library with `cargo build --lib --target aarch64-nintendo-switch-freestanding -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem`. The crate pins the already-installed nightly locally and does not change the repository or global toolchain.
