#![no_std]

//! Bounded reader for compressed GameMaker 8 font records.
//! The glyph atlas is retained as alpha bytes; the Switch renderer can upload it
//! later without depending on desktop font APIs.

extern crate alloc;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const FONT_VERSION: u32 = 800;
const GLYPH_MAP_WORDS: usize = 0x600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_name_bytes: usize,
    pub max_atlas_bytes: usize,
    pub max_atlas_dimension: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 8 * 1024 * 1024,
            max_decompressed_bytes: 8 * 1024 * 1024,
            max_name_bytes: 1024 * 1024,
            max_atlas_bytes: 8 * 1024 * 1024,
            max_atlas_dimension: 8192,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Font {
    pub name: Vec<u8>,
    pub system_name: Vec<u8>,
    pub size: u32,
    pub bold: bool,
    pub italic: bool,
    pub range_start: u32,
    pub range_end: u32,
    pub charset: u32,
    pub antialias_level: u32,
    /// Six little-endian u32 values for each of 256 character slots.
    pub glyph_map: Vec<u32>,
    pub atlas_width: u32,
    pub atlas_height: u32,
    pub alpha_atlas: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    CompressedInputTooLarge,
    ZeroOutputLimit,
    Inflate(InflateError),
    Truncated,
    DeletedAsset,
    NameTooLong,
    AtlasTooLarge,
    InvalidAtlasDimensions,
    UnsupportedVersion(u32),
    TrailingBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_records: u32,
    pub max_total_atlas_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedFont {
    pub index: u32,
    pub font: Font,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontTable {
    pub directory: Gm80DirectorySummary,
    pub fonts: Vec<IndexedFont>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    Directory(Gm80DirectoryError),
    TooManyRecords,
    Parse { index: u32, error: ParseError },
    TotalOutputLimitExceeded,
}

/// Decompress and parse the GM8.0 font table with per-record and aggregate bounds.
pub fn load_gm80_font_table(payload: &[u8], limits: TableLimits) -> Result<FontTable, TableError> {
    let mut spans = Vec::new();
    let mut overflow = false;
    let directory = scan_gm80_resource_directory(payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Font {
            if spans.len() < limits.max_records as usize {
                spans.push((index, span));
            } else {
                overflow = true;
            }
        }
    })
    .map_err(TableError::Directory)?;
    if overflow {
        return Err(TableError::TooManyRecords);
    }

    let mut fonts = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_atlas_bytes = 0usize;
    for (index, span) in spans {
        let font =
            parse_indexed_font(payload, span, limits.record).map_err(|error| TableError::Parse { index, error })?;
        let Some(font) = font else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        total_atlas_bytes =
            total_atlas_bytes.checked_add(font.alpha_atlas.len()).ok_or(TableError::TotalOutputLimitExceeded)?;
        if total_atlas_bytes > limits.max_total_atlas_bytes {
            return Err(TableError::TotalOutputLimitExceeded);
        }
        fonts.push(IndexedFont { index, font });
    }
    Ok(FontTable { directory, fonts, deleted_count })
}

pub fn parse_indexed_font(payload: &[u8], span: AssetSpan, limits: Limits) -> Result<Option<Font>, ParseError> {
    let start = usize::try_from(span.offset).map_err(|_| ParseError::Truncated)?;
    let length = usize::try_from(span.length).map_err(|_| ParseError::Truncated)?;
    let end = start.checked_add(length).ok_or(ParseError::Truncated)?;
    let compressed = payload.get(start..end).ok_or(ParseError::Truncated)?;
    parse_compressed_font_record(compressed, limits)
}

pub fn parse_compressed_font_record(input: &[u8], limits: Limits) -> Result<Option<Font>, ParseError> {
    if input.len() > limits.max_compressed_bytes {
        return Err(ParseError::CompressedInputTooLarge);
    }
    if limits.max_decompressed_bytes == 0 {
        return Err(ParseError::ZeroOutputLimit);
    }
    let decoded = decompress_zlib_bounded(input, limits.max_decompressed_bytes).map_err(ParseError::Inflate)?;
    let mut reader = SliceReader::new(&decoded);
    let present = reader.read_u32_le().map_err(map_read_error)?;
    if present == 0 {
        if reader.remaining() == 0 {
            return Ok(None);
        }
        return Err(ParseError::TrailingBytes);
    }
    let name = read_pascal(&mut reader, limits.max_name_bytes)?;
    let version = reader.read_u32_le().map_err(map_read_error)?;
    if version != FONT_VERSION {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let system_name = read_pascal(&mut reader, limits.max_name_bytes)?;
    let size = reader.read_u32_le().map_err(map_read_error)?;
    let bold = reader.read_u32_le().map_err(map_read_error)? != 0;
    let italic = reader.read_u32_le().map_err(map_read_error)? != 0;
    let packed_range_start = reader.read_u32_le().map_err(map_read_error)?;
    let range_end = reader.read_u32_le().map_err(map_read_error)?;
    // OpenGMK's reader treats these as plain values for GM8.0. GM8.1 packs
    // charset and antialias level into the upper bytes of range_start.
    let (range_start, charset, antialias_level) = (packed_range_start, 0, 0);

    let mut glyph_map = Vec::with_capacity(GLYPH_MAP_WORDS);
    for _ in 0..GLYPH_MAP_WORDS {
        glyph_map.push(reader.read_u32_le().map_err(map_read_error)?);
    }
    let atlas_width = reader.read_u32_le().map_err(map_read_error)?;
    let atlas_height = reader.read_u32_le().map_err(map_read_error)?;
    if atlas_width > limits.max_atlas_dimension || atlas_height > limits.max_atlas_dimension {
        return Err(ParseError::InvalidAtlasDimensions);
    }
    let atlas_len = reader.read_limited_u32_le(limits.max_atlas_bytes).map_err(map_atlas_error)?;
    let expected =
        (atlas_width as usize).checked_mul(atlas_height as usize).ok_or(ParseError::InvalidAtlasDimensions)?;
    if atlas_len != expected {
        return Err(ParseError::InvalidAtlasDimensions);
    }
    let alpha_atlas = reader.take(atlas_len).map_err(map_read_error)?.to_vec();
    if reader.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }
    Ok(Some(Font {
        name,
        system_name,
        size,
        bold,
        italic,
        range_start,
        range_end,
        charset,
        antialias_level,
        glyph_map,
        atlas_width,
        atlas_height,
        alpha_atlas,
    }))
}

fn read_pascal(reader: &mut SliceReader<'_>, limit: usize) -> Result<Vec<u8>, ParseError> {
    let length = reader.read_limited_u32_le(limit).map_err(|error| match error {
        ReadError::LimitExceeded => ParseError::NameTooLong,
        ReadError::OutOfBounds => ParseError::Truncated,
    })?;
    Ok(reader.take(length).map_err(map_read_error)?.to_vec())
}

fn map_read_error(_: ReadError) -> ParseError {
    ParseError::Truncated
}

fn map_atlas_error(error: ReadError) -> ParseError {
    match error {
        ReadError::LimitExceeded => ParseError::AtlasTooLarge,
        ReadError::OutOfBounds => ParseError::Truncated,
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    fn record(version: u32, atlas: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&4u32.to_le_bytes());
        raw.extend_from_slice(b"font");
        raw.extend_from_slice(&version.to_le_bytes());
        raw.extend_from_slice(&3u32.to_le_bytes());
        raw.extend_from_slice(b"sys");
        raw.extend_from_slice(&16u32.to_le_bytes());
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&32u32.to_le_bytes());
        raw.extend_from_slice(&255u32.to_le_bytes());
        for index in 0..GLYPH_MAP_WORDS {
            raw.extend_from_slice(&(index as u32).to_le_bytes());
        }
        raw.extend_from_slice(&4u32.to_le_bytes());
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&(atlas.len() as u32).to_le_bytes());
        raw.extend_from_slice(atlas);
        compress_to_vec_zlib(&raw, 6)
    }

    fn limits() -> Limits {
        Limits {
            max_compressed_bytes: 16 * 1024,
            max_decompressed_bytes: 16 * 1024,
            max_name_bytes: 32,
            max_atlas_bytes: 256,
            max_atlas_dimension: 32,
        }
    }

    #[test]
    fn parses_gm80_font_and_alpha_atlas() {
        let compressed = record(FONT_VERSION, &[0x00, 0x7f, 0xff, 0x40]);
        let font = parse_compressed_font_record(&compressed, limits()).unwrap().unwrap();
        assert_eq!(font.name, b"font");
        assert_eq!(font.system_name, b"sys");
        assert_eq!(font.size, 16);
        assert!(font.bold);
        assert_eq!((font.atlas_width, font.atlas_height), (4, 1));
        assert_eq!(font.alpha_atlas, [0x00, 0x7f, 0xff, 0x40]);
        assert_eq!(font.glyph_map[17], 17);
    }

    #[test]
    fn rejects_bad_version_and_inconsistent_atlas_dimensions() {
        assert_eq!(
            parse_compressed_font_record(&record(801, &[1, 2, 3, 4]), limits()),
            Err(ParseError::UnsupportedVersion(801))
        );
        assert_eq!(
            parse_compressed_font_record(&record(FONT_VERSION, &[1, 2, 3]), limits()),
            Err(ParseError::InvalidAtlasDimensions)
        );
    }

    #[test]
    fn rejects_oversized_and_truncated_records() {
        let mut small = limits();
        small.max_atlas_bytes = 2;
        assert_eq!(
            parse_compressed_font_record(&record(FONT_VERSION, &[0, 1, 2, 3]), small),
            Err(ParseError::AtlasTooLarge)
        );
        let mut raw = vec![1, 0, 0, 0];
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.push(b'x');
        let compressed = compress_to_vec_zlib(&raw, 6);
        assert_eq!(parse_compressed_font_record(&compressed, limits()), Err(ParseError::Truncated));
    }
}
