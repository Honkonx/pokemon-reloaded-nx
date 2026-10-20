//! `io` and `byteorder` subsets shared by the OpenGMK crates ported to Horizon.
//!
//! With the `std` feature (default) `io` is `std::io`, so host builds behave as
//! upstream. Without it, a small `alloc`-only implementation is used. The
//! little-endian extension traits are generic over whichever `io::Read` /
//! `io::Write` is active and replace the `byteorder` crate (which needs `std`).

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod byteorder;

#[cfg(feature = "std")]
pub use std::io;

#[cfg(not(feature = "std"))]
pub mod io;
