//! Minimal RGBA image type: the part of the `image` crate the runtime uses for sprite
//! processing. Decoding and encoding of file formats is not part of this build; the host
//! converts files to raw RGBA before handing them over.

pub mod codec;

use std::ops::{Index, IndexMut};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct Rgba(pub [u8; 4]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Rgb(pub [u8; 3]);

pub trait Pixel {
    fn to_rgb(&self) -> Rgb;
    fn channels(&self) -> &[u8];
}

impl Pixel for Rgba {
    fn to_rgb(&self) -> Rgb {
        Rgb([self.0[0], self.0[1], self.0[2]])
    }
    fn channels(&self) -> &[u8] {
        &self.0
    }
}

impl Index<usize> for Rgba {
    type Output = u8;
    fn index(&self, index: usize) -> &u8 {
        &self.0[index]
    }
}

impl IndexMut<usize> for Rgba {
    fn index_mut(&mut self, index: usize) -> &mut u8 {
        &mut self.0[index]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl RgbaImage {
    /// Takes interleaved RGBA bytes; `None` if the length does not match `width * height * 4`.
    pub fn from_vec(width: u32, height: u32, raw: Vec<u8>) -> Option<RgbaImage> {
        if raw.len() as u64 != width as u64 * height as u64 * 4 {
            return None
        }
        Some(RgbaImage { width, height, data: raw })
    }

    pub fn new(width: u32, height: u32) -> RgbaImage {
        RgbaImage { width, height, data: vec![0; width as usize * height as usize * 4] }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        assert!(x < self.width && y < self.height, "pixel ({x}, {y}) out of bounds");
        (y as usize * self.width as usize + x as usize) * 4
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> &Rgba {
        let at = self.offset(x, y);
        // SAFETY: `Rgba` is `repr(transparent)` over `[u8; 4]` (alignment 1) and the slice has 4 bytes.
        unsafe { &*(self.data[at..at + 4].as_ptr() as *const Rgba) }
    }

    pub fn get_pixel_mut(&mut self, x: u32, y: u32) -> &mut Rgba {
        let at = self.offset(x, y);
        // SAFETY: as in `get_pixel`.
        unsafe { &mut *(self.data[at..at + 4].as_mut_ptr() as *mut Rgba) }
    }

    pub fn put_pixel(&mut self, x: u32, y: u32, pixel: Rgba) {
        *self.get_pixel_mut(x, y) = pixel;
    }

    pub fn pixels(&self) -> impl Iterator<Item = &Rgba> {
        // SAFETY: as in `get_pixel`; every chunk has exactly 4 bytes.
        self.data.chunks_exact(4).map(|c| unsafe { &*(c.as_ptr() as *const Rgba) })
    }

    pub fn pixels_mut(&mut self) -> impl Iterator<Item = &mut Rgba> {
        // SAFETY: as in `get_pixel`.
        self.data.chunks_exact_mut(4).map(|c| unsafe { &mut *(c.as_mut_ptr() as *mut Rgba) })
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }

    pub fn into_raw(self) -> Vec<u8> {
        self.data
    }

    pub fn as_raw(&self) -> &Vec<u8> {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_raw_bytes_and_indexes_pixels() {
        let raw: Vec<u8> = (0..16).collect();
        let mut image = RgbaImage::from_vec(2, 2, raw.clone()).unwrap();
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(image.get_pixel(1, 0)[0], 4);
        image.get_pixel_mut(0, 1)[3] = 99;
        assert_eq!(image.get_pixel(0, 1).to_rgb(), Rgb([8, 9, 10]));
        let mut expected = raw;
        expected[11] = 99;
        assert_eq!(image.into_vec(), expected);
        assert!(RgbaImage::from_vec(2, 2, vec![0; 15]).is_none());
    }
}

/// Failure to read or write an image file (file decoding is not part of this build).
#[derive(Debug)]
pub struct ImageError(pub &'static str);

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ImageError {}
