//! Rust side of the PokeNX engine payload.
//!
//! The C++/libnx entrypoint owns process startup, the EGL context, HID polling
//! and `audout`; this crate receives them through a narrow C ABI (`abi`). It is
//! `no_std + alloc` so it can be built for `aarch64-nintendo-switch-freestanding`
//! (feature `switch`) and unit-tested on the host.
//!
//! The engine loads a GameMaker data container from the SD card, constructs
//! `pokenx-core::Game`, and drives its simulation and OpenGL renderer from the
//! host's frame loop. If loading fails, it retains a diagnostic frame and tone
//! so the host can report EGL, controller, and audio status.

#![cfg_attr(feature = "switch", no_std)]

extern crate alloc;

pub mod abi;
mod engine;
mod text_input;
mod frame_clock;
mod gl;
mod logging;
pub mod tone;
pub mod zip_ffi;

#[cfg(feature = "switch")]
mod platform_switch;

pub use engine::{clear_color_for, Engine};
