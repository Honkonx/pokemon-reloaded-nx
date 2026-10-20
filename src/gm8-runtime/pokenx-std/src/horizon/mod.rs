//! Horizon implementation of the `std` slice (used when the `std` feature is off).

pub mod collections;
pub mod env;
pub mod ffi;
pub mod float;
pub mod fs;
pub mod host;
pub mod io;
#[cfg(not(feature = "std"))]
pub mod macros;
pub mod path;
pub mod process;
pub mod sync;
pub mod thread;
pub mod time;

#[cfg(test)]
mod tests;

// Everything in `core` and the pure-alloc modules is shared as is.
pub use alloc_crate::{borrow, boxed, rc, slice, str, string, vec};
#[allow(deprecated)]
pub use core::{
    any, array, ascii, cell, char, clone, cmp, convert, default, error, f32, f64, fmt, hash, hint, i128, i16, i32, i64,
    i8, isize, iter, marker, mem, net, num, ops, option, pin, ptr, result, u128, u16, u32, u64, u8, usize,
};
// `alloc_crate::alloc` and `core::alloc` merge here, as in `std::alloc`.
pub mod alloc {
    pub use alloc_crate::alloc::*;
    pub use core::alloc::*;
}
pub mod os {
    pub mod raw {
        pub use core::ffi::{
            c_char, c_double, c_float, c_int, c_long, c_longlong, c_schar, c_short, c_uchar, c_uint, c_ulong,
            c_ulonglong, c_ushort, c_void,
        };
    }
}

pub mod prelude {
    pub mod rust_2021 {
        pub use super::v1::*;
    }
    pub mod v1 {
        pub use alloc_crate::{
            borrow::ToOwned,
            boxed::Box,
            string::{String, ToString},
            vec::Vec,
        };
        pub use super::super::float::{F32Ext, F64Ext};
        pub use core::prelude::rust_2021::*;
    }
}
