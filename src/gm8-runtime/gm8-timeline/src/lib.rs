#![no_std]

//! Bounded parser for compressed GameMaker 8.0 Timeline records.

extern crate alloc;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_object::{CodeAction, ParseError as ActionParseError, parse_code_action};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const TIMELINE_VERSION: u32 = 500;
pub const MOMENT_VERSION: u32 = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_name_bytes: usize,
    pub max_moments: usize,
    pub max_actions: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 16 * 1024 * 1024,
            max_decompressed_bytes: 16 * 1024 * 1024,
            max_name_bytes: 1024 * 1024,
            max_moments: 65_536,
            max_actions: 65_536,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moment {
    pub moment: u32,
    pub actions: Vec<CodeAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timeline {
    pub name: Vec<u8>,
    pub moments: Vec<Moment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    CompressedInputTooLarge,
    ZeroOutputLimit,
    Inflate(InflateError),
    Truncated,
    DeletedAsset,
    NameTooLong,
    TooManyMoments,
    TooManyActions,
    UnsupportedVersion(u32),
    UnsupportedMomentVersion(u32),
    InvalidAction(ActionParseError),
    TrailingBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_timelines: u32,
    pub max_total_actions: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedTimeline {
    pub index: u32,
    pub timeline: Timeline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineTable {
    pub directory: Gm80DirectorySummary,
    pub timelines: Vec<IndexedTimeline>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    Directory(Gm80DirectoryError),
    TooManyTimelines,
    Parse { index: u32, error: ParseError },
    TotalActionLimitExceeded,
}

pub fn parse_compressed_timeline_record(input: &[u8], limits: Limits) -> Result<Timeline, ParseError> {
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
    let name = read_string(&mut cursor, limits.max_name_bytes)?;
    let version = cursor.read_u32_le().map_err(map_read_error)?;
    if version != TIMELINE_VERSION {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let moment_count = cursor.read_limited_u32_le(limits.max_moments).map_err(map_moment_error)?;
    let mut remaining_actions = limits.max_actions;
    let mut moments = Vec::with_capacity(moment_count);
    for _ in 0..moment_count {
        let moment = cursor.read_u32_le().map_err(map_read_error)?;
        let moment_version = cursor.read_u32_le().map_err(map_read_error)?;
        if moment_version != MOMENT_VERSION {
            return Err(ParseError::UnsupportedMomentVersion(moment_version));
        }
        let action_count = cursor.read_limited_u32_le(remaining_actions).map_err(map_action_count_error)?;
        remaining_actions -= action_count;
        let mut actions = Vec::with_capacity(action_count);
        for _ in 0..action_count {
            actions.push(parse_code_action(&mut cursor, limits.max_name_bytes).map_err(ParseError::InvalidAction)?);
        }
        moments.push(Moment { moment, actions });
    }
    if cursor.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }
    Ok(Timeline { name, moments })
}

pub fn parse_indexed_timeline(payload: &[u8], span: AssetSpan, limits: Limits) -> Result<Option<Timeline>, ParseError> {
    let start = usize::try_from(span.offset).map_err(|_| ParseError::Truncated)?;
    let length = usize::try_from(span.length).map_err(|_| ParseError::Truncated)?;
    let end = start.checked_add(length).ok_or(ParseError::Truncated)?;
    let input = payload.get(start..end).ok_or(ParseError::Truncated)?;
    match parse_compressed_timeline_record(input, limits) {
        Ok(timeline) => Ok(Some(timeline)),
        Err(ParseError::DeletedAsset) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn load_gm80_timeline_table(payload: &[u8], limits: TableLimits) -> Result<TimelineTable, TableError> {
    let mut spans = Vec::new();
    let mut overflow = false;
    let directory = scan_gm80_resource_directory(payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Timeline {
            if spans.len() < limits.max_timelines as usize {
                spans.push((index, span));
            } else {
                overflow = true;
            }
        }
    })
    .map_err(TableError::Directory)?;
    if overflow {
        return Err(TableError::TooManyTimelines);
    }
    let mut timelines = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_actions = 0usize;
    for (index, span) in spans {
        let timeline =
            parse_indexed_timeline(payload, span, limits.record).map_err(|error| TableError::Parse { index, error })?;
        let Some(timeline) = timeline else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        let count = timeline
            .moments
            .iter()
            .try_fold(0usize, |sum, moment| sum.checked_add(moment.actions.len()))
            .ok_or(TableError::TotalActionLimitExceeded)?;
        total_actions = total_actions.checked_add(count).ok_or(TableError::TotalActionLimitExceeded)?;
        if total_actions > limits.max_total_actions {
            return Err(TableError::TotalActionLimitExceeded);
        }
        timelines.push(IndexedTimeline { index, timeline });
    }
    Ok(TimelineTable { directory, timelines, deleted_count })
}

fn read_string(cursor: &mut SliceReader<'_>, limit: usize) -> Result<Vec<u8>, ParseError> {
    let length = cursor.read_limited_u32_le(limit).map_err(|error| match error {
        ReadError::LimitExceeded => ParseError::NameTooLong,
        ReadError::OutOfBounds => ParseError::Truncated,
    })?;
    Ok(cursor.take(length).map_err(map_read_error)?.to_vec())
}

fn map_read_error(_: ReadError) -> ParseError {
    ParseError::Truncated
}

fn map_moment_error(error: ReadError) -> ParseError {
    match error {
        ReadError::LimitExceeded => ParseError::TooManyMoments,
        ReadError::OutOfBounds => ParseError::Truncated,
    }
}

fn map_action_count_error(error: ReadError) -> ParseError {
    match error {
        ReadError::LimitExceeded => ParseError::TooManyActions,
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

    fn action(out: &mut Vec<u8>) {
        for value in [440u32, 1, 2, 0, 1, 0, 0, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // function name
        out.extend_from_slice(&0u32.to_le_bytes()); // function code
        out.extend_from_slice(&0u32.to_le_bytes()); // parameter count
        out.extend_from_slice(&8u32.to_le_bytes()); // parameter type slots
        for _ in 0..8 {
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        out.extend_from_slice(&u32::MAX.to_le_bytes()); // applies_to = -1
        out.extend_from_slice(&0u32.to_le_bytes()); // relative
        out.extend_from_slice(&8u32.to_le_bytes()); // argument slots
        for _ in 0..8 {
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // invert condition
    }

    fn record(moment_version: u32) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&3u32.to_le_bytes());
        raw.extend_from_slice(b"tl1");
        raw.extend_from_slice(&TIMELINE_VERSION.to_le_bytes());
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&60u32.to_le_bytes());
        raw.extend_from_slice(&moment_version.to_le_bytes());
        raw.extend_from_slice(&1u32.to_le_bytes());
        action(&mut raw);
        compress_to_vec_zlib(&raw, 6)
    }

    fn limits() -> Limits {
        Limits {
            max_compressed_bytes: 4096,
            max_decompressed_bytes: 4096,
            max_name_bytes: 32,
            max_moments: 8,
            max_actions: 8,
        }
    }

    #[test]
    fn parses_timeline_moment_and_reuses_opengmk_action_schema() {
        let timeline = parse_compressed_timeline_record(&record(MOMENT_VERSION), limits()).unwrap();
        assert_eq!(timeline.name, b"tl1");
        assert_eq!(timeline.moments.len(), 1);
        assert_eq!(timeline.moments[0].moment, 60);
        assert_eq!(timeline.moments[0].actions[0].lib_id, 1);
        assert_eq!(timeline.moments[0].actions[0].id, 2);
    }

    #[test]
    fn enforces_moment_version_and_action_budget() {
        assert_eq!(
            parse_compressed_timeline_record(&record(401), limits()),
            Err(ParseError::UnsupportedMomentVersion(401))
        );
        let mut low = limits();
        low.max_actions = 0;
        assert_eq!(parse_compressed_timeline_record(&record(MOMENT_VERSION), low), Err(ParseError::TooManyActions));
    }

    #[test]
    fn rejects_truncated_records_and_excessive_moment_count() {
        let mut raw = vec![1, 0, 0, 0, 1, 0, 0, 0, b'x'];
        raw.extend_from_slice(&TIMELINE_VERSION.to_le_bytes());
        raw.extend_from_slice(&u32::MAX.to_le_bytes());
        let compressed = compress_to_vec_zlib(&raw, 6);
        assert_eq!(parse_compressed_timeline_record(&compressed, limits()), Err(ParseError::TooManyMoments));
        assert_eq!(
            parse_compressed_timeline_record(&record(MOMENT_VERSION)[..4], limits()),
            Err(ParseError::Inflate(InflateError::InvalidOrTruncatedStream))
        );
    }
}
