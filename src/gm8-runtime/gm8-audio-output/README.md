# gm8-audio-output

Portable, `no_std` contract for consuming the runtime's already-mixed interleaved `f32` PCM. The runtime owns decoding, sound playback state, resampling, channel conversion and mixing; a platform adapter owns device setup and pulls samples through `PcmProducer`.

`gm8emulator::game::audio::Mixer` implements this contract. The existing desktop path is preserved by `UdonHostOutput`, a small adapter back to Udon's `Source` interface; host playback still obtains the default SoundIo device and runs Udon's output stream. `AudioManager::capture_audio` also pulls PCM from the same producer interface.

This is an interface boundary only. It does not implement Switch audio and does not make `AudioManager`, codecs or `Game::launch` portable.

## Remaining work for Switch output

- Decide between direct Horizon `audout`/`audren` and SDL2's Switch backend after checking the versions and Rust/C bindings actually available in the selected build.
- Implement a Switch-owned consumer that pulls bounded PCM blocks from the runtime without invoking Udon `Session`/SoundIo. Define its thread or frame scheduling, queue depth, underrun behavior, pause/resume, and shutdown ownership.
- Match the negotiated sample rate, channel layout and sample representation; the mixer currently uses the host device's sample rate and channel count, so Switch format negotiation must be injected instead of calling `Session::default_output_device`.
- Keep decoder and mixer dependencies target-compatible. MP3/WAV parsing, Udon resampling, `std`, synchronization and the current audio lifecycle still need separate Horizon audits.
- Verify synthetic tone audibly on hardware and measure underruns/latency. Host tests prove only interface behavior and deterministic buffers.

Run the portable tests with `cargo test -p gm8-audio-output`; host integration can be checked with `cargo check -p gm8emulator --lib` from this workspace.
