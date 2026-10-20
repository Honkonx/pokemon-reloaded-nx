//! Pack small rectangles into a larger one (texture atlases).
//!
//! Vendored from `rect_packer` 0.2.1 by kryptan (MIT, see `LICENSE` in this folder) so the
//! runtime core builds without `std`. Only the module paths were adapted to edition 2021.

pub use rect::Rect;
pub use packer::Packer;
pub use packer::DensePacker;

mod rect;
mod packer;

/// Describes size and padding requirements of rectangle packing.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Config {
    /// Width of the encompassing rectangle.
    pub width: i32,
    /// Height of the encompassing rectangle.
    pub height: i32,

    /// Minimum spacing between border and rectangles.
    pub border_padding: i32,
    /// Minimum spacing between rectangles.
    pub rectangle_padding: i32,
}
