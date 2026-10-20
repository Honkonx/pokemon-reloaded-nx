#![no_std]

//! Bounded parser for compressed GameMaker 8.0 Background records.

extern crate alloc;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const BACKGROUND_VERSION_1: u32 = 710;
pub const BACKGROUND_VERSION_2: u32 = 800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_name_bytes: usize,
    pub max_pixel_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 8 * 1024 * 1024,
            max_decompressed_bytes: 32 * 1024 * 1024,
            max_name_bytes: 1024 * 1024,
            max_pixel_bytes: 24 * 1024 * 1024,
            max_width: 8192,
            max_height: 8192,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Background {
    pub name: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// GameMaker stores background pixels as BGRA bytes.
    pub bgra: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    CompressedInputTooLarge,
    ZeroOutputLimit,
    Inflate(InflateError),
    Truncated,
    DeletedAsset,
    InvalidVersion { first: u32, second: u32 },
    NameTooLong,
    DimensionsTooLarge,
    PixelLengthOverflow,
    PixelDataTooLarge,
    InvalidPixelLength { expected: usize, got: usize },
    TrailingBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackgroundTableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_background_records: u32,
    pub max_total_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedBackground {
    pub index: u32,
    pub background: Background,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackgroundTable {
    pub directory: Gm80DirectorySummary,
    pub backgrounds: Vec<IndexedBackground>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundTableError {
    Directory(Gm80DirectoryError),
    TooManyBackgrounds,
    Parse { index: u32, error: ParseError },
    TotalOutputLimitExceeded,
}

/// Validate a complete GM8.0 resource directory and decode its Background table.
pub fn load_gm80_background_table(
    decrypted_payload: &[u8],
    limits: BackgroundTableLimits,
) -> Result<BackgroundTable, BackgroundTableError> {
    let mut spans = Vec::new();
    let mut too_many = false;
    let directory = scan_gm80_resource_directory(decrypted_payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Background {
            if spans.len() < limits.max_background_records as usize {
                spans.push((index, span));
            } else {
                too_many = true;
            }
        }
    })
    .map_err(BackgroundTableError::Directory)?;
    if too_many {
        return Err(BackgroundTableError::TooManyBackgrounds);
    }

    let mut backgrounds = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_output_bytes = 0usize;
    for (index, span) in spans {
        let background = parse_indexed_background_record(decrypted_payload, span, limits.record)
            .map_err(|error| BackgroundTableError::Parse { index, error })?;
        let Some(background) = background else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        let output_bytes = background
            .name
            .len()
            .checked_add(background.bgra.as_ref().map_or(0, Vec::len))
            .ok_or(BackgroundTableError::TotalOutputLimitExceeded)?;
        total_output_bytes =
            total_output_bytes.checked_add(output_bytes).ok_or(BackgroundTableError::TotalOutputLimitExceeded)?;
        if total_output_bytes > limits.max_total_output_bytes {
            return Err(BackgroundTableError::TotalOutputLimitExceeded);
        }
        backgrounds.push(IndexedBackground { index, background });
    }

    Ok(BackgroundTable { directory, backgrounds, deleted_count })
}

/// Inflate the generic asset envelope and decode a version 800 Background record.
pub fn parse_compressed_background_record(input: &[u8], limits: Limits) -> Result<Background, ParseError> {
    if input.len() > limits.max_compressed_bytes {
        return Err(ParseError::CompressedInputTooLarge);
    }
    if limits.max_decompressed_bytes == 0 {
        return Err(ParseError::ZeroOutputLimit);
    }
    let decoded = decompress_zlib_bounded(input, limits.max_decompressed_bytes).map_err(ParseError::Inflate)?;
    let mut cursor = SliceReader::new(&decoded);
    let marker = cursor.read_u32_le().map_err(map_read_error)?;
    if marker == 0 {
        return Err(ParseError::DeletedAsset);
    }

    let name_len = cursor.read_limited_u32_le(limits.max_name_bytes).map_err(map_name_error)?;
    let name = cursor.take(name_len).map_err(map_read_error)?.to_vec();
    let first_version = cursor.read_u32_le().map_err(map_read_error)?;
    let second_version = cursor.read_u32_le().map_err(map_read_error)?;
    if first_version != BACKGROUND_VERSION_1 || second_version != BACKGROUND_VERSION_2 {
        return Err(ParseError::InvalidVersion { first: first_version, second: second_version });
    }

    let width = cursor.read_u32_le().map_err(map_read_error)?;
    let height = cursor.read_u32_le().map_err(map_read_error)?;
    let (width, height, bgra) = if width != 0 && height != 0 {
        if width > limits.max_width || height > limits.max_height {
            return Err(ParseError::DimensionsTooLarge);
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(ParseError::PixelLengthOverflow)?;
        if expected > limits.max_pixel_bytes {
            return Err(ParseError::PixelDataTooLarge);
        }
        let length = cursor.read_limited_u32_le(limits.max_pixel_bytes).map_err(map_pixel_error)?;
        if length != expected {
            return Err(ParseError::InvalidPixelLength { expected, got: length });
        }
        (width, height, Some(cursor.take(length).map_err(map_read_error)?.to_vec()))
    } else {
        (0, 0, None)
    };

    if cursor.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }
    Ok(Background { name, width, height, bgra })
}

/// Parse an indexed Background span and recheck its bounds before inflation.
pub fn parse_indexed_background_record(
    payload: &[u8],
    span: AssetSpan,
    limits: Limits,
) -> Result<Option<Background>, ParseError> {
    let start = span.offset as usize;
    let length = span.length as usize;
    let end = start.checked_add(length).ok_or(ParseError::Truncated)?;
    let compressed = payload.get(start..end).ok_or(ParseError::Truncated)?;
    match parse_compressed_background_record(compressed, limits) {
        Ok(background) => Ok(Some(background)),
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

fn map_pixel_error(error: ReadError) -> ParseError {
    match error {
        ReadError::LimitExceeded => ParseError::PixelDataTooLarge,
        ReadError::OutOfBounds => ParseError::Truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn record(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 6);
        bytes.extend_from_slice(b"forest");
        push_u32(&mut bytes, BACKGROUND_VERSION_1);
        push_u32(&mut bytes, BACKGROUND_VERSION_2);
        push_u32(&mut bytes, width);
        push_u32(&mut bytes, height);
        if width != 0 && height != 0 {
            push_u32(&mut bytes, pixels.len() as u32);
            bytes.extend_from_slice(pixels);
        }
        bytes
    }

    fn limits() -> Limits {
        Limits {
            max_compressed_bytes: 4096,
            max_decompressed_bytes: 4096,
            max_name_bytes: 64,
            max_pixel_bytes: 1024,
            max_width: 16,
            max_height: 16,
        }
    }

    fn compress(bytes: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
    }

    fn synthetic_directory(background_records: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 0); // random dwords
        push_u32(&mut bytes, 1); // Pro
        push_u32(&mut bytes, 0x1234_5678);
        for value in [1, 2, 3, 4] {
            push_u32(&mut bytes, value);
        }
        push_u32(&mut bytes, 700); // extensions
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 800); // triggers
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 800); // constants
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 800); // sounds
        push_u32(&mut bytes, 0);

        for table in 0..8 {
            push_u32(&mut bytes, 800);
            if table == 1 {
                push_u32(&mut bytes, background_records.len() as u32);
                for record in background_records {
                    push_u32(&mut bytes, record.len() as u32);
                    bytes.extend_from_slice(record);
                }
            } else {
                push_u32(&mut bytes, 0);
            }
        }
        push_u32(&mut bytes, 0); // last instance ID
        push_u32(&mut bytes, 0); // last tile ID
        push_u32(&mut bytes, 800); // included files
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 800); // help data
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 500); // init strings
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 700); // room order
        push_u32(&mut bytes, 0);
        bytes
    }

    fn table_limits() -> BackgroundTableLimits {
        BackgroundTableLimits {
            index: Gm80IndexLimits { max_records: 16, max_pascal_string_bytes: 64, max_compressed_asset_bytes: 4096 },
            record: limits(),
            max_background_records: 4,
            max_total_output_bytes: 2048,
        }
    }

    #[test]
    fn parses_bgra_background_pixels() {
        let pixels = [1, 2, 3, 4, 5, 6, 7, 8];
        let bg = parse_compressed_background_record(&compress(&record(2, 1, &pixels)), limits()).unwrap();
        assert_eq!(bg.name, b"forest");
        assert_eq!((bg.width, bg.height), (2, 1));
        assert_eq!(bg.bgra.as_deref(), Some(&pixels[..]));
    }

    #[test]
    fn accepts_empty_background_like_opengmk() {
        let bg = parse_compressed_background_record(&compress(&record(0, 0, &[])), limits()).unwrap();
        assert_eq!((bg.width, bg.height), (0, 0));
        assert_eq!(bg.bgra, None);
    }

    #[test]
    fn rejects_wrong_pixel_length_and_oversized_dimensions() {
        assert_eq!(
            parse_compressed_background_record(&compress(&record(2, 1, &[1, 2, 3, 4])), limits()),
            Err(ParseError::InvalidPixelLength { expected: 8, got: 4 })
        );
        assert_eq!(
            parse_compressed_background_record(&compress(&record(17, 1, &[])), limits()),
            Err(ParseError::DimensionsTooLarge)
        );
    }

    #[test]
    fn enforces_name_and_output_limits() {
        let mut strict = limits();
        strict.max_name_bytes = 2;
        assert_eq!(
            parse_compressed_background_record(&compress(&record(0, 0, &[])), strict),
            Err(ParseError::NameTooLong)
        );
        strict.max_name_bytes = 64;
        strict.max_decompressed_bytes = 4;
        assert!(matches!(
            parse_compressed_background_record(&compress(&record(0, 0, &[])), strict),
            Err(ParseError::Inflate(_))
        ));
    }

    #[test]
    fn rechecks_indexed_span_bounds() {
        let limits = limits();
        assert_eq!(
            parse_indexed_background_record(&[0; 4], AssetSpan { offset: 3, length: 2 }, limits),
            Err(ParseError::Truncated)
        );
    }

    #[test]
    fn loads_backgrounds_from_complete_gm80_directory() {
        let live = compress(&record(2, 1, &[1, 2, 3, 4, 5, 6, 7, 8]));
        let deleted = compress(&[0, 0, 0, 0]);
        let payload = synthetic_directory(&[live, deleted]);
        let table = load_gm80_background_table(&payload, table_limits()).unwrap();
        assert_eq!(table.directory.tail.background_count, 2);
        assert_eq!(table.backgrounds.len(), 1);
        assert_eq!(table.backgrounds[0].index, 0);
        assert_eq!(table.backgrounds[0].background.name, b"forest");
        assert_eq!(table.deleted_count, 1);
    }

    #[test]
    fn background_table_enforces_record_and_aggregate_limits() {
        let live = compress(&record(2, 1, &[1, 2, 3, 4, 5, 6, 7, 8]));
        let deleted = compress(&[0, 0, 0, 0]);
        let payload = synthetic_directory(&[live, deleted]);
        let record_limit = BackgroundTableLimits { max_background_records: 1, ..table_limits() };
        assert_eq!(load_gm80_background_table(&payload, record_limit), Err(BackgroundTableError::TooManyBackgrounds));

        let output_limit = BackgroundTableLimits { max_total_output_bytes: 13, ..table_limits() };
        assert_eq!(
            load_gm80_background_table(&payload, output_limit),
            Err(BackgroundTableError::TotalOutputLimitExceeded)
        );
    }
}
