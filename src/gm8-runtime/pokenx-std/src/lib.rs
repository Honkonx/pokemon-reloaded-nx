//! `pokenx-std`: the slice of `std` that the OpenGMK runtime uses, usable on
//! Horizon where Rust has no `std`.
//!
//! Crates adopt it with
//!
//! ```ignore
//! #![no_std]
//! #![feature(prelude_import)]
//! #[macro_use]
//! extern crate pokenx_std as std;
//! #[prelude_import]
//! use std::prelude::v1::*;
//! ```
//!
//! so their existing `use std::...` paths keep working.
//!
//! * With the `std` feature (host builds and tests) everything is the real
//!   `std`, so behaviour is identical to upstream.
//! * Without it, `core`/`alloc` are re-exported and the OS-facing modules (`fs`,
//!   `path`, `env`, `time`, `thread`, `sync`, `process`) are implemented over
//!   newlib/libnx through the C symbols declared in `host`.

#![no_std]

#[cfg(any(not(feature = "std"), test))]
extern crate alloc as alloc_crate;

// Used by the exported `format!`/`vec!` macros through `$crate::__alloc`.
#[cfg(not(feature = "std"))]
#[doc(hidden)]
pub extern crate alloc as __alloc;

#[cfg(any(feature = "std", test))]
extern crate std as real_std;

#[cfg(feature = "std")]
pub use real_std::*;

#[cfg(any(not(feature = "std"), test))]
mod horizon;
#[cfg(not(feature = "std"))]
pub use horizon::*;
