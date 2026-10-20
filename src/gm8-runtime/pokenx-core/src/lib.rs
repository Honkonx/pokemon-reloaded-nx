// Copyright (C) OpenGMK Project Developers
// Licensed under GPL-2.0-only. See the upstream LICENCE.md.
//
// Headless runtime core derived from OpenGMK's `gm8emulator` (see PROVENANCE.md).
// It builds as `no_std` against `pokenx-std`, which is the real `std` on hosts and a
// newlib/libnx implementation on Horizon. Windowing, audio output, the recording UI and
// Windows-only code of the original are replaced by the `platform` module and host hooks.

#![no_std]
#![feature(prelude_import)]
#![allow(clippy::all)]

#[macro_use]
extern crate pokenx_std as std;
#[prelude_import]
#[allow(unused_imports)]
use std::prelude::rust_2021::*;

extern crate alloc;

mod action;
mod audio_src;
mod asset;
pub mod game;
pub mod gml;
#[cfg(feature = "std")]
pub mod compatibility;
mod glob;
mod handleman;
pub mod image;
mod ini;
pub mod compat_scan;
pub mod keys_report;
pub mod live_keys;
pub mod room_hints;
pub mod stats;
pub mod loader;
pub mod user_data;
pub mod zip_sd;
mod input;
mod instance;
mod instancelist;
mod math;
mod patches;
pub mod platform;
pub mod preload;
mod rect_packer;
mod rustysynth;
mod render;
mod tile;
#[cfg(test)]
mod title_tests;
mod types;
mod util;

pub use encoding_rs;
pub use game::{Game, GameClock, PlayType};
pub use gm8exe;
