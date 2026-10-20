#![no_std]

//! Bounded parser for compressed GameMaker 8.0/8.1 Sprite records.

extern crate alloc;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const SPRITE_VERSION: u32 = 800;
pub const SPRITE_VERSION_81: u32 = 810;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_name_bytes: usize,
    pub max_frames: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub max_frame_bytes: usize,
    pub max_collision_pixels: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 8 * 1024 * 1024,
            max_decompressed_bytes: 64 * 1024 * 1024,
            max_name_bytes: 1024 * 1024,
            max_frames: 4096,
            max_width: 8192,
            max_height: 8192,
            max_frame_bytes: 24 * 1024 * 1024,
            max_collision_pixels: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollisionMap {
    pub width: u32,
    pub height: u32,
    pub bbox_left: u32,
    pub bbox_right: u32,
    pub bbox_bottom: u32,
    pub bbox_top: u32,
    pub data: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprite {
    pub name: Vec<u8>,
    pub version: u32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub frames: Vec<Frame>,
    pub colliders: Vec<CollisionMap>,
    pub per_frame_colliders: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    CompressedInputTooLarge,
    ZeroOutputLimit,
    Inflate(InflateError),
    Truncated,
    DeletedAsset,
    NameTooLong,
    UnsupportedVersion(u32),
    InvalidFrameVersion(u32),
    InvalidCollisionVersion(u32),
    TooManyFrames,
    DimensionsTooLarge,
    PixelLengthOverflow,
    InvalidFrameLength { expected: usize, got: usize },
    FrameDataTooLarge,
    CollisionDataTooLarge,
    TooManyCollisions,
    TrailingBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteTableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_sprite_records: u32,
    pub max_total_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedSprite {
    pub index: u32,
    pub sprite: Sprite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteTable {
    pub directory: Gm80DirectorySummary,
    pub sprites: Vec<IndexedSprite>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteTableError {
    Directory(Gm80DirectoryError),
    TooManySprites,
    Parse { index: u32, error: ParseError },
    TotalOutputLimitExceeded,
}

/// Validate a complete GM8.0 directory and decode its Sprite table.
pub fn load_gm80_sprite_table(payload: &[u8], limits: SpriteTableLimits) -> Result<SpriteTable, SpriteTableError> {
    let mut spans = Vec::new();
    let mut too_many = false;
    let directory = scan_gm80_resource_directory(payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Sprite {
            if spans.len() < limits.max_sprite_records as usize {
                spans.push((index, span));
            } else {
                too_many = true;
            }
        }
    })
    .map_err(SpriteTableError::Directory)?;
    if too_many {
        return Err(SpriteTableError::TooManySprites);
    }

    let mut sprites = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_output_bytes = 0usize;
    for (index, span) in spans {
        let sprite = parse_indexed_sprite_record(payload, span, limits.record)
            .map_err(|error| SpriteTableError::Parse { index, error })?;
        let Some(sprite) = sprite else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        let output_bytes = sprite
            .frames
            .iter()
            .try_fold(sprite.name.len(), |total, frame| total.checked_add(frame.rgba.len()))
            .and_then(|total| {
                sprite.colliders.iter().try_fold(total, |total, collider| total.checked_add(collider.data.len()))
            });
        let output_bytes = output_bytes.ok_or(SpriteTableError::TotalOutputLimitExceeded)?;
        total_output_bytes =
            total_output_bytes.checked_add(output_bytes).ok_or(SpriteTableError::TotalOutputLimitExceeded)?;
        if total_output_bytes > limits.max_total_output_bytes {
            return Err(SpriteTableError::TotalOutputLimitExceeded);
        }
        sprites.push(IndexedSprite { index, sprite });
    }

    Ok(SpriteTable { directory, sprites, deleted_count })
}

/// Inflate and decode one OpenGMK Sprite record.
pub fn parse_compressed_sprite_record(input: &[u8], limits: Limits) -> Result<Sprite, ParseError> {
    if input.len() > limits.max_compressed_bytes {
        return Err(ParseError::CompressedInputTooLarge);
    }
    if limits.max_decompressed_bytes == 0 {
        return Err(ParseError::ZeroOutputLimit);
    }
    let decoded = decompress_zlib_bounded(input, limits.max_decompressed_bytes).map_err(ParseError::Inflate)?;
    let mut cursor = SliceReader::new(&decoded);
    if cursor.read_u32_le().map_err(map_read_error)? == 0 {
        return Err(ParseError::DeletedAsset);
    }
    let name_len = cursor.read_limited_u32_le(limits.max_name_bytes).map_err(map_name_error)?;
    let name = cursor.take(name_len).map_err(map_read_error)?.to_vec();
    let version = cursor.read_u32_le().map_err(map_read_error)?;
    if version != SPRITE_VERSION && version != SPRITE_VERSION_81 {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let origin_x = cursor.read_u32_le().map_err(map_read_error)? as i32;
    let origin_y = cursor.read_u32_le().map_err(map_read_error)? as i32;
    let frame_count = cursor.read_u32_le().map_err(map_read_error)?;
    if frame_count > limits.max_frames {
        return Err(ParseError::TooManyFrames);
    }

    let (frames, colliders, per_frame_colliders) = if frame_count == 0 {
        (Vec::new(), Vec::new(), false)
    } else {
        let mut frames = Vec::new();
        frames.try_reserve_exact(frame_count as usize).map_err(|_| ParseError::FrameDataTooLarge)?;
        for _ in 0..frame_count {
            let frame_version = cursor.read_u32_le().map_err(map_read_error)?;
            if frame_version != SPRITE_VERSION {
                return Err(ParseError::InvalidFrameVersion(frame_version));
            }
            let width = cursor.read_u32_le().map_err(map_read_error)?;
            let height = cursor.read_u32_le().map_err(map_read_error)?;
            if width > limits.max_width || height > limits.max_height {
                return Err(ParseError::DimensionsTooLarge);
            }
            let expected = (width as usize)
                .checked_mul(height as usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or(ParseError::PixelLengthOverflow)?;
            if expected > limits.max_frame_bytes {
                return Err(ParseError::FrameDataTooLarge);
            }
            let length = cursor.read_limited_u32_le(limits.max_frame_bytes).map_err(map_frame_error)?;
            if length != expected {
                return Err(ParseError::InvalidFrameLength { expected, got: length });
            }
            let rgba = cursor.take(length).map_err(map_read_error)?.to_vec();
            frames.push(Frame { width, height, rgba });
        }

        if version == SPRITE_VERSION_81 {
            let _collision_shape = cursor.read_u32_le().map_err(map_read_error)?;
        }
        let per_frame_colliders = cursor.read_u32_le().map_err(map_read_error)? != 0;
        let collider_count = if per_frame_colliders { frame_count } else { 1 };
        let mut colliders = Vec::new();
        colliders.try_reserve_exact(collider_count as usize).map_err(|_| ParseError::TooManyCollisions)?;
        for _ in 0..collider_count {
            let collision_version = cursor.read_u32_le().map_err(map_read_error)?;
            if collision_version != SPRITE_VERSION {
                return Err(ParseError::InvalidCollisionVersion(collision_version));
            }
            let width = cursor.read_u32_le().map_err(map_read_error)?;
            let height = cursor.read_u32_le().map_err(map_read_error)?;
            let bbox_left = cursor.read_u32_le().map_err(map_read_error)?;
            let bbox_right = cursor.read_u32_le().map_err(map_read_error)?;
            let bbox_bottom = cursor.read_u32_le().map_err(map_read_error)?;
            let bbox_top = cursor.read_u32_le().map_err(map_read_error)?;
            let count = (width as usize).checked_mul(height as usize).ok_or(ParseError::PixelLengthOverflow)?;
            if count > limits.max_collision_pixels || width > limits.max_width || height > limits.max_height {
                return Err(ParseError::CollisionDataTooLarge);
            }
            let mut data = Vec::new();
            data.try_reserve_exact(count).map_err(|_| ParseError::CollisionDataTooLarge)?;
            for _ in 0..count {
                data.push(cursor.read_u32_le().map_err(map_read_error)? != 0);
            }
            colliders.push(CollisionMap { width, height, bbox_left, bbox_right, bbox_bottom, bbox_top, data });
        }
        (frames, colliders, per_frame_colliders)
    };

    if cursor.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }
    Ok(Sprite { name, version, origin_x, origin_y, frames, colliders, per_frame_colliders })
}

/// Parse one indexed record and map a deleted table entry to `None`.
pub fn parse_indexed_sprite_record(
    payload: &[u8],
    span: AssetSpan,
    limits: Limits,
) -> Result<Option<Sprite>, ParseError> {
    let start = span.offset as usize;
    let end = start.checked_add(span.length as usize).ok_or(ParseError::Truncated)?;
    let compressed = payload.get(start..end).ok_or(ParseError::Truncated)?;
    match parse_compressed_sprite_record(compressed, limits) {
        Ok(sprite) => Ok(Some(sprite)),
        Err(ParseError::DeletedAsset) => Ok(None),
        Err(error) => Err(error),
    }
}

fn map_read_error(_: ReadError) -> ParseError {
    ParseError::Truncated
}

fn map_name_error(error: ReadError) -> ParseError {
    match error {
        ReadError::LimitExceeded => ParseError::NameTooLong,
        ReadError::OutOfBounds => ParseError::Truncated,
    }
}

fn map_frame_error(error: ReadError) -> ParseError {
    match error {
        ReadError::LimitExceeded => ParseError::FrameDataTooLarge,
        ReadError::OutOfBounds => ParseError::Truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    fn put_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn record(version: u32, frames: &[(u32, u32, &[u8])], per_frame: bool) -> Vec<u8> {
        let mut out = Vec::new();
        put_u32(&mut out, 1);
        put_u32(&mut out, 4);
        out.extend_from_slice(b"test");
        put_u32(&mut out, version);
        put_u32(&mut out, 2);
        put_u32(&mut out, 3);
        put_u32(&mut out, frames.len() as u32);
        for (width, height, pixels) in frames {
            put_u32(&mut out, 800);
            put_u32(&mut out, *width);
            put_u32(&mut out, *height);
            put_u32(&mut out, pixels.len() as u32);
            out.extend_from_slice(pixels);
        }
        if !frames.is_empty() {
            if version == SPRITE_VERSION_81 {
                put_u32(&mut out, 0);
            }
            put_u32(&mut out, u32::from(per_frame));
            let collision_count = if per_frame { frames.len() } else { 1 };
            for _ in 0..collision_count {
                put_u32(&mut out, 800);
                put_u32(&mut out, 1);
                put_u32(&mut out, 1);
                for bbox in [0, 0, 0, 0] {
                    put_u32(&mut out, bbox);
                }
                put_u32(&mut out, 1);
            }
        }
        compress_to_vec_zlib(&out, 6)
    }

    fn limits() -> Limits {
        Limits {
            max_compressed_bytes: 4096,
            max_decompressed_bytes: 4096,
            max_name_bytes: 32,
            max_frames: 4,
            max_width: 16,
            max_height: 16,
            max_frame_bytes: 1024,
            max_collision_pixels: 32,
        }
    }

    #[test]
    fn decodes_frame_and_collision_map() {
        let compressed = record(800, &[(1, 1, &[1, 2, 3, 4])], false);
        let sprite = parse_compressed_sprite_record(&compressed, limits()).unwrap();
        assert_eq!(sprite.name, b"test");
        assert_eq!(sprite.frames[0].rgba, [1, 2, 3, 4]);
        assert_eq!(sprite.colliders[0].data, [true]);
        assert_eq!((sprite.origin_x, sprite.origin_y), (2, 3));
    }

    #[test]
    fn decodes_81_and_per_frame_collision_maps() {
        let compressed = record(810, &[(1, 1, &[0; 4]), (1, 1, &[1; 4])], true);
        let sprite = parse_compressed_sprite_record(&compressed, limits()).unwrap();
        assert_eq!(sprite.version, 810);
        assert!(sprite.per_frame_colliders);
        assert_eq!(sprite.colliders.len(), 2);
    }

    #[test]
    fn allows_zero_frame_sprite_without_collision_tail() {
        let compressed = record(800, &[], false);
        let sprite = parse_compressed_sprite_record(&compressed, limits()).unwrap();
        assert!(sprite.frames.is_empty());
        assert!(sprite.colliders.is_empty());
    }

    #[test]
    fn rejects_mismatched_pixel_length_and_truncation() {
        let mut raw = vec![1, 0, 0, 0, 1, 0, 0, 0, b'x'];
        put_u32(&mut raw, 800);
        put_u32(&mut raw, 0);
        put_u32(&mut raw, 0);
        put_u32(&mut raw, 1);
        put_u32(&mut raw, 800);
        put_u32(&mut raw, 1);
        put_u32(&mut raw, 1);
        put_u32(&mut raw, 3);
        raw.extend_from_slice(&[1, 2, 3]);
        let compressed = compress_to_vec_zlib(&raw, 6);
        assert!(matches!(
            parse_compressed_sprite_record(&compressed, limits()),
            Err(ParseError::InvalidFrameLength { .. })
        ));
        assert!(parse_compressed_sprite_record(&compressed[..compressed.len() - 1], limits()).is_err());
    }
}
