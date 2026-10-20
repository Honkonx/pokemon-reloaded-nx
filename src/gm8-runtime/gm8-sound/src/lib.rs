#![no_std]

//! Bounded parser for one compressed GM8.0 Sound asset record.
//!
//! The record envelope follows OpenGMK `gm8exe::reader::get_assets`: a zlib
//! stream whose first little-endian dword is zero for a deleted asset and
//! nonzero for a live asset. The remainder follows
//! `gm8exe/src/asset/sound.rs::Sound::deserialize_exe`.

extern crate alloc;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const SOUND_VERSION: u32 = 800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum compressed zlib record size accepted by the parser.
    pub max_compressed_bytes: usize,
    /// Hard cap passed to the inflater for the complete decoded record.
    pub max_decompressed_bytes: usize,
    /// Maximum byte length of each Pascal string.
    pub max_string_bytes: usize,
    /// Maximum embedded sound file size.
    pub max_sound_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 8 * 1024 * 1024,
            max_decompressed_bytes: 16 * 1024 * 1024,
            max_string_bytes: 1024 * 1024,
            max_sound_bytes: 12 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundKind {
    Normal,
    BackgroundMusic,
    ThreeDimensional,
    Multimedia,
    /// OpenGMK maps unknown values to Normal. Preserve the raw value so the
    /// caller can diagnose nonstandard files while retaining compatible behavior.
    Unknown(u32),
}

impl SoundKind {
    fn from_raw(value: u32) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::BackgroundMusic,
            2 => Self::ThreeDimensional,
            3 => Self::Multimedia,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundEffects {
    pub chorus: bool,
    pub echo: bool,
    pub flanger: bool,
    pub gargle: bool,
    pub reverb: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    /// Record marker consumed by OpenGMK before its asset deserializer.
    pub record_marker: u32,
    pub name: Vec<u8>,
    pub kind: SoundKind,
    pub extension: Vec<u8>,
    pub source: Vec<u8>,
    pub data: Option<Vec<u8>>,
    pub effects: SoundEffects,
    pub volume: f64,
    pub pan: f64,
    pub preload: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    CompressedInputTooLarge,
    ZeroOutputLimit,
    Inflate(InflateError),
    Truncated,
    DeletedAsset,
    InvalidSoundVersion(u32),
    StringTooLong,
    SoundDataTooLarge,
    TrailingBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundTableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_sound_records: u32,
    /// Aggregate cap for returned sound bytes and names/extensions/sources.
    pub max_total_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexedSound {
    pub index: u32,
    pub sound: Sound,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SoundTable {
    pub directory: Gm80DirectorySummary,
    pub sounds: Vec<IndexedSound>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundTableError {
    Directory(Gm80DirectoryError),
    TooManySounds,
    Parse { index: u32, error: ParseError },
    TotalOutputLimitExceeded,
}

/// Validate the complete standard GM8.0 directory and deserialize its Sound
/// table under per-record and aggregate output limits. This is a resource
/// loading slice, not a complete game loader: other asset kinds remain spans.
pub fn load_gm80_sound_table(
    decrypted_payload: &[u8],
    limits: SoundTableLimits,
) -> Result<SoundTable, SoundTableError> {
    let mut sound_spans = Vec::new();
    let directory = scan_gm80_resource_directory(decrypted_payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Sound {
            sound_spans.push((index, span));
        }
    })
    .map_err(SoundTableError::Directory)?;
    if sound_spans.len() > limits.max_sound_records as usize {
        return Err(SoundTableError::TooManySounds);
    }

    let mut sounds = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_output_bytes = 0usize;
    for (index, span) in sound_spans {
        let sound = parse_indexed_sound_record(decrypted_payload, span, limits.record)
            .map_err(|error| SoundTableError::Parse { index, error })?;
        let Some(sound) = sound else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        let sound_output_bytes = sound
            .name
            .len()
            .checked_add(sound.extension.len())
            .and_then(|n| n.checked_add(sound.source.len()))
            .and_then(|n| n.checked_add(sound.data.as_ref().map_or(0, Vec::len)))
            .ok_or(SoundTableError::TotalOutputLimitExceeded)?;
        total_output_bytes =
            total_output_bytes.checked_add(sound_output_bytes).ok_or(SoundTableError::TotalOutputLimitExceeded)?;
        if total_output_bytes > limits.max_total_output_bytes {
            return Err(SoundTableError::TotalOutputLimitExceeded);
        }
        sounds.push(IndexedSound { index, sound });
    }

    Ok(SoundTable { directory, sounds, deleted_count })
}

/// Inflate and parse one complete compressed asset table entry.
///
/// As in OpenGMK's generic asset loader, the first decoded dword is an
/// envelope marker and is consumed before `Sound::deserialize_exe`; zero marks
/// a deleted entry. Sound record version 800 follows its `name` Pascal string.
pub fn parse_compressed_sound_record(input: &[u8], limits: Limits) -> Result<Sound, ParseError> {
    if input.len() > limits.max_compressed_bytes {
        return Err(ParseError::CompressedInputTooLarge);
    }
    if limits.max_decompressed_bytes == 0 {
        return Err(ParseError::ZeroOutputLimit);
    }
    let decoded = decompress_zlib_bounded(input, limits.max_decompressed_bytes).map_err(ParseError::Inflate)?;
    let mut cursor = SliceReader::new(&decoded);
    let record_marker = cursor.read_u32_le().map_err(map_read_error)?;
    if record_marker == 0 {
        return Err(ParseError::DeletedAsset);
    }

    let name = read_pascal_string(&mut cursor, limits.max_string_bytes)?;
    let version = cursor.read_u32_le().map_err(map_read_error)?;
    if version != SOUND_VERSION {
        return Err(ParseError::InvalidSoundVersion(version));
    }
    let kind = SoundKind::from_raw(cursor.read_u32_le().map_err(map_read_error)?);
    let extension = read_pascal_string(&mut cursor, limits.max_string_bytes)?;
    let source = read_pascal_string(&mut cursor, limits.max_string_bytes)?;
    let data = if cursor.read_u32_le().map_err(map_read_error)? != 0 {
        let length =
            usize::try_from(cursor.read_u32_le().map_err(map_read_error)?).map_err(|_| ParseError::Truncated)?;
        if length > limits.max_sound_bytes {
            return Err(ParseError::SoundDataTooLarge);
        }
        Some(cursor.take(length).map_err(map_read_error)?.to_vec())
    } else {
        None
    };

    let effect_bits = cursor.read_u32_le().map_err(map_read_error)?;
    let effects = SoundEffects {
        chorus: effect_bits & 1 != 0,
        echo: effect_bits & 2 != 0,
        flanger: effect_bits & 4 != 0,
        gargle: effect_bits & 8 != 0,
        reverb: effect_bits & 16 != 0,
    };
    let volume = f64::from_bits(cursor.read_u64_le().map_err(map_read_error)?);
    let pan = f64::from_bits(cursor.read_u64_le().map_err(map_read_error)?);
    let preload = cursor.read_u32_le().map_err(map_read_error)? != 0;
    if cursor.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }

    Ok(Sound { record_marker, name, kind, extension, source, data, effects, volume, pan, preload })
}

/// Parse one Sound record referenced by the bounded GM8.0 resource index.
///
/// `payload` is the decrypted resource block and `span` must come from the
/// index scanner (or an equivalent checked index). The range is checked again
/// here because spans can be supplied by any caller. Deleted table entries
/// are returned as `Ok(None)` to match OpenGMK's `AssetList` behavior.
pub fn parse_indexed_sound_record(
    payload: &[u8],
    span: AssetSpan,
    limits: Limits,
) -> Result<Option<Sound>, ParseError> {
    let start = span.offset as usize;
    let length = span.length as usize;
    let end = start.checked_add(length).ok_or(ParseError::Truncated)?;
    let compressed = payload.get(start..end).ok_or(ParseError::Truncated)?;
    match parse_compressed_sound_record(compressed, limits) {
        Ok(sound) => Ok(Some(sound)),
        Err(ParseError::DeletedAsset) => Ok(None),
        Err(error) => Err(error),
    }
}

fn map_read_error(error: ReadError) -> ParseError {
    match error {
        ReadError::OutOfBounds => ParseError::Truncated,
        ReadError::LimitExceeded => ParseError::StringTooLong,
    }
}

fn read_pascal_string(cursor: &mut SliceReader<'_>, max_length: usize) -> Result<Vec<u8>, ParseError> {
    let length = cursor.read_limited_u32_le(max_length).map_err(map_read_error)?;
    Ok(cursor.take(length).map_err(map_read_error)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_pascal(out: &mut Vec<u8>, value: &[u8]) {
        push_u32(out, value.len() as u32);
        out.extend_from_slice(value);
    }

    fn synthetic_record(with_data: bool) -> Vec<u8> {
        let mut out = Vec::new();
        push_u32(&mut out, 1); // generic OpenGMK asset marker
        push_pascal(&mut out, b"menu_theme");
        push_u32(&mut out, 800);
        push_u32(&mut out, 1); // background music
        push_pascal(&mut out, b"ogg");
        push_pascal(&mut out, b"theme.ogg");
        if with_data {
            push_u32(&mut out, 1);
            push_u32(&mut out, 4);
            out.extend_from_slice(b"OggS");
        } else {
            push_u32(&mut out, 0);
        }
        push_u32(&mut out, 0b1_0101);
        out.extend_from_slice(&0.75f64.to_le_bytes());
        out.extend_from_slice(&(-0.25f64).to_le_bytes());
        push_u32(&mut out, 1);
        out
    }

    fn compress(record: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(record, 6)
    }

    fn limits() -> Limits {
        Limits { max_compressed_bytes: 4096, max_decompressed_bytes: 4096, max_string_bytes: 32, max_sound_bytes: 64 }
    }

    #[test]
    fn parses_sound_fields_and_embedded_data() {
        let compressed = compress(&synthetic_record(true));
        let sound = parse_compressed_sound_record(&compressed, limits()).unwrap();
        assert_eq!(sound.name, b"menu_theme");
        assert_eq!(sound.kind, SoundKind::BackgroundMusic);
        assert_eq!(sound.extension, b"ogg");
        assert_eq!(sound.source, b"theme.ogg");
        assert_eq!(sound.data.as_deref(), Some(&b"OggS"[..]));
        assert_eq!(sound.effects, SoundEffects {
            chorus: true,
            echo: false,
            flanger: true,
            gargle: false,
            reverb: true
        });
        assert_eq!(sound.volume, 0.75);
        assert_eq!(sound.pan, -0.25);
        assert!(sound.preload);
    }

    #[test]
    fn parses_absent_sound_bytes() {
        let sound = parse_compressed_sound_record(&compress(&synthetic_record(false)), limits()).unwrap();
        assert_eq!(sound.data, None);
    }

    #[test]
    fn preserves_arbitrary_pascal_bytes_and_enforces_length_budgets() {
        let mut bad_version = synthetic_record(false);
        bad_version[4 + 4 + b"menu_theme".len()..][..4].copy_from_slice(&801u32.to_le_bytes());
        assert_eq!(
            parse_compressed_sound_record(&compress(&bad_version), limits()),
            Err(ParseError::InvalidSoundVersion(801))
        );

        let mut invalid_utf8 = synthetic_record(false);
        invalid_utf8[8] = 0xff;
        let sound = parse_compressed_sound_record(&compress(&invalid_utf8), limits()).unwrap();
        assert_eq!(sound.name, [0xff, b'e', b'n', b'u', b'_', b't', b'h', b'e', b'm', b'e']);

        let strict_strings = Limits { max_string_bytes: 4, ..limits() };
        assert_eq!(
            parse_compressed_sound_record(&compress(&synthetic_record(false)), strict_strings),
            Err(ParseError::StringTooLong)
        );

        let strict_sound = Limits { max_sound_bytes: 3, ..limits() };
        assert_eq!(
            parse_compressed_sound_record(&compress(&synthetic_record(true)), strict_sound),
            Err(ParseError::SoundDataTooLarge)
        );
    }

    #[test]
    fn rejects_truncated_deleted_and_trailing_records() {
        let compressed = compress(&synthetic_record(false));
        let decoded = gm8_zlib::decompress_zlib_bounded(&compressed, 4096).unwrap();
        let truncated = compress(&decoded[..decoded.len() - 1]);
        assert_eq!(parse_compressed_sound_record(&truncated, limits()), Err(ParseError::Truncated));

        let deleted = compress(&[0, 0, 0, 0]);
        assert_eq!(parse_compressed_sound_record(&deleted, limits()), Err(ParseError::DeletedAsset));

        let mut with_tail = synthetic_record(false);
        with_tail.push(0);
        assert_eq!(parse_compressed_sound_record(&compress(&with_tail), limits()), Err(ParseError::TrailingBytes));
    }

    #[test]
    fn enforces_compressed_and_inflated_caps() {
        let compressed = compress(&synthetic_record(false));
        let tiny_compressed = Limits { max_compressed_bytes: compressed.len() - 1, ..limits() };
        assert_eq!(
            parse_compressed_sound_record(&compressed, tiny_compressed),
            Err(ParseError::CompressedInputTooLarge)
        );

        let tiny_output = Limits { max_decompressed_bytes: 8, ..limits() };
        assert_eq!(
            parse_compressed_sound_record(&compressed, tiny_output),
            Err(ParseError::Inflate(InflateError::OutputLimitExceeded))
        );
    }

    fn synthetic_directory(live: &[u8], deleted: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        push_u32(&mut data, 0); // random dwords
        push_u32(&mut data, 1); // Pro
        push_u32(&mut data, 0x1234_5678);
        for value in [1, 2, 3, 4] {
            push_u32(&mut data, value);
        }
        push_u32(&mut data, 700); // extensions
        push_u32(&mut data, 0);
        push_u32(&mut data, 800); // triggers
        push_u32(&mut data, 0);
        push_u32(&mut data, 800); // constants
        push_u32(&mut data, 0);
        push_u32(&mut data, 800); // sounds
        push_u32(&mut data, 2);
        for record in [live, deleted] {
            push_u32(&mut data, record.len() as u32);
            data.extend_from_slice(record);
        }
        for _ in 0..8 {
            push_u32(&mut data, 800);
            push_u32(&mut data, 0);
        }
        push_u32(&mut data, 0); // last instance ID
        push_u32(&mut data, 0); // last tile ID
        push_u32(&mut data, 800); // included files
        push_u32(&mut data, 0);
        push_u32(&mut data, 800); // help data
        push_u32(&mut data, 0);
        push_u32(&mut data, 500); // initialization strings
        push_u32(&mut data, 0);
        push_u32(&mut data, 700); // room order
        push_u32(&mut data, 0);
        data
    }

    fn table_limits() -> SoundTableLimits {
        SoundTableLimits {
            index: Gm80IndexLimits { max_records: 8, max_pascal_string_bytes: 32, max_compressed_asset_bytes: 4096 },
            record: limits(),
            max_sound_records: 4,
            max_total_output_bytes: 128,
        }
    }

    #[test]
    fn parses_sound_records_from_bounded_gm8_asset_table_spans() {
        let live = compress(&synthetic_record(true));
        let deleted = compress(&[0, 0, 0, 0]);
        let mut table = Vec::new();
        push_u32(&mut table, 2);
        push_u32(&mut table, live.len() as u32);
        table.extend_from_slice(&live);
        push_u32(&mut table, deleted.len() as u32);
        table.extend_from_slice(&deleted);

        let mut spans = [AssetSpan::default(); 2];
        let (count, end) = gm8_format_probe::index_asset_table(&table, 0, &mut spans).unwrap();
        assert_eq!(count, 2);
        assert_eq!(end, table.len());
        let sound = parse_indexed_sound_record(&table, spans[0], limits()).unwrap().unwrap();
        assert_eq!(sound.name, b"menu_theme");
        assert_eq!(parse_indexed_sound_record(&table, spans[1], limits()), Ok(None));
    }

    #[test]
    fn indexed_sound_parser_rechecks_caller_supplied_span_bounds() {
        assert_eq!(
            parse_indexed_sound_record(&[0; 4], AssetSpan { offset: u32::MAX, length: u32::MAX }, limits(),),
            Err(ParseError::Truncated)
        );
    }

    #[test]
    fn loads_sound_table_from_complete_bounded_gm80_directory() {
        let live = compress(&synthetic_record(true));
        let deleted = compress(&[0, 0, 0, 0]);
        let payload = synthetic_directory(&live, &deleted);
        let table = load_gm80_sound_table(&payload, table_limits()).unwrap();
        assert_eq!(table.directory.prefix.sound_count, 2);
        assert_eq!(table.directory.tail.room_order_count, 0);
        assert_eq!(table.sounds.len(), 1);
        assert_eq!(table.sounds[0].index, 0);
        assert_eq!(table.sounds[0].sound.name, b"menu_theme");
        assert_eq!(table.deleted_count, 1);
    }

    #[test]
    fn sound_table_enforces_count_and_aggregate_memory_limits() {
        let live = compress(&synthetic_record(true));
        let deleted = compress(&[0, 0, 0, 0]);
        let payload = synthetic_directory(&live, &deleted);
        let count_limit = SoundTableLimits { max_sound_records: 1, ..table_limits() };
        assert_eq!(load_gm80_sound_table(&payload, count_limit), Err(SoundTableError::TooManySounds));

        let output_limit = SoundTableLimits { max_total_output_bytes: 25, ..table_limits() };
        assert_eq!(load_gm80_sound_table(&payload, output_limit), Err(SoundTableError::TotalOutputLimitExceeded));
    }
}
