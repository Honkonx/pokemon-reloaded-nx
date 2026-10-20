#![no_std]

//! Bounded parser for compressed GameMaker 8.0 Path records (version 530).

extern crate alloc;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const PATH_VERSION: u32 = 530;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionKind {
    StraightLine,
    SmoothCurve,
    Unknown(u32),
}

impl ConnectionKind {
    fn from_raw(value: u32) -> Self {
        match value {
            0 => Self::StraightLine,
            1 => Self::SmoothCurve,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub speed: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub name: Vec<u8>,
    pub connection: ConnectionKind,
    pub closed: bool,
    pub precision: u32,
    pub points: Vec<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_name_bytes: usize,
    pub max_points: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 8 * 1024 * 1024,
            max_decompressed_bytes: 16 * 1024 * 1024,
            max_name_bytes: 1024 * 1024,
            max_points: 262_144,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    CompressedInputTooLarge,
    ZeroOutputLimit,
    Inflate(InflateError),
    Truncated,
    DeletedAsset,
    NameTooLong,
    TooManyPoints,
    UnsupportedVersion(u32),
    TrailingBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_paths: u32,
    pub max_total_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexedPath {
    pub index: u32,
    pub path: Path,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathTable {
    pub directory: Gm80DirectorySummary,
    pub paths: Vec<IndexedPath>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    Directory(Gm80DirectoryError),
    TooManyPaths,
    Parse { index: u32, error: ParseError },
    TotalOutputLimitExceeded,
}

pub fn parse_compressed_path_record(input: &[u8], limits: Limits) -> Result<Path, ParseError> {
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

    let name_len = cursor.read_limited_u32_le(limits.max_name_bytes).map_err(|error| match error {
        ReadError::LimitExceeded => ParseError::NameTooLong,
        ReadError::OutOfBounds => ParseError::Truncated,
    })?;
    let name = cursor.take(name_len).map_err(map_read_error)?.to_vec();
    let version = cursor.read_u32_le().map_err(map_read_error)?;
    if version != PATH_VERSION {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let connection = ConnectionKind::from_raw(cursor.read_u32_le().map_err(map_read_error)?);
    let closed = cursor.read_u32_le().map_err(map_read_error)? != 0;
    let precision = cursor.read_u32_le().map_err(map_read_error)?;
    let point_count = cursor.read_limited_u32_le(limits.max_points as usize).map_err(|error| match error {
        ReadError::LimitExceeded => ParseError::TooManyPoints,
        ReadError::OutOfBounds => ParseError::Truncated,
    })?;
    let point_bytes = point_count.checked_mul(24).ok_or(ParseError::TooManyPoints)?;
    if point_bytes > cursor.remaining() {
        return Err(ParseError::Truncated);
    }

    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        let x = f64::from_bits(cursor.read_u64_le().map_err(map_read_error)?);
        let y = f64::from_bits(cursor.read_u64_le().map_err(map_read_error)?);
        let speed = f64::from_bits(cursor.read_u64_le().map_err(map_read_error)?);
        points.push(Point { x, y, speed });
    }
    if cursor.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }
    Ok(Path { name, connection, closed, precision, points })
}

pub fn parse_indexed_path(payload: &[u8], span: AssetSpan, limits: Limits) -> Result<Option<Path>, ParseError> {
    let start = usize::try_from(span.offset).map_err(|_| ParseError::Truncated)?;
    let length = usize::try_from(span.length).map_err(|_| ParseError::Truncated)?;
    let end = start.checked_add(length).ok_or(ParseError::Truncated)?;
    let compressed = payload.get(start..end).ok_or(ParseError::Truncated)?;
    match parse_compressed_path_record(compressed, limits) {
        Ok(path) => Ok(Some(path)),
        Err(ParseError::DeletedAsset) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn load_gm80_path_table(payload: &[u8], limits: TableLimits) -> Result<PathTable, TableError> {
    let mut spans = Vec::new();
    let mut exceeded = false;
    let directory = scan_gm80_resource_directory(payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Path {
            if spans.len() < limits.max_paths as usize {
                spans.push((index, span));
            } else {
                exceeded = true;
            }
        }
    })
    .map_err(TableError::Directory)?;
    if exceeded {
        return Err(TableError::TooManyPaths);
    }

    let mut paths = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_output = 0usize;
    for (index, span) in spans {
        let path =
            parse_indexed_path(payload, span, limits.record).map_err(|error| TableError::Parse { index, error })?;
        let Some(path) = path else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        let bytes = path
            .name
            .len()
            .checked_add(
                path.points
                    .len()
                    .checked_mul(core::mem::size_of::<Point>())
                    .ok_or(TableError::TotalOutputLimitExceeded)?,
            )
            .ok_or(TableError::TotalOutputLimitExceeded)?;
        total_output = total_output.checked_add(bytes).ok_or(TableError::TotalOutputLimitExceeded)?;
        if total_output > limits.max_total_output_bytes {
            return Err(TableError::TotalOutputLimitExceeded);
        }
        paths.push(IndexedPath { index, path });
    }
    Ok(PathTable { directory, paths, deleted_count })
}

fn map_read_error(_: ReadError) -> ParseError {
    ParseError::Truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_pascal(out: &mut Vec<u8>, bytes: &[u8]) {
        push_u32(out, bytes.len() as u32);
        out.extend_from_slice(bytes);
    }

    fn path_record(marker: u32, version: u32, points: &[(f64, f64, f64)]) -> Vec<u8> {
        let mut out = Vec::new();
        push_u32(&mut out, marker);
        push_pascal(&mut out, b"route");
        push_u32(&mut out, version);
        push_u32(&mut out, 1); // smooth curve
        push_u32(&mut out, 1); // closed
        push_u32(&mut out, 4); // precision
        push_u32(&mut out, points.len() as u32);
        for (x, y, speed) in points {
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
            out.extend_from_slice(&speed.to_le_bytes());
        }
        out
    }

    #[test]
    fn parses_path_points_and_preserves_unknown_connection_kind() {
        let raw = path_record(1, PATH_VERSION, &[(1.5, -2.0, 3.25)]);
        let compressed = compress_to_vec_zlib(&raw, 6);
        let path = parse_compressed_path_record(&compressed, Limits::default()).unwrap();
        assert_eq!(path.name, b"route");
        assert_eq!(path.connection, ConnectionKind::SmoothCurve);
        assert!(path.closed);
        assert_eq!(path.precision, 4);
        assert_eq!(path.points, vec![Point { x: 1.5, y: -2.0, speed: 3.25 }]);

        let raw = path_record(1, PATH_VERSION, &[]);
        let mut raw = raw;
        raw[17..21].copy_from_slice(&99u32.to_le_bytes());
        let compressed = compress_to_vec_zlib(&raw, 6);
        let path = parse_compressed_path_record(&compressed, Limits::default()).unwrap();
        assert_eq!(path.connection, ConnectionKind::Unknown(99));
    }

    #[test]
    fn deleted_records_versions_and_point_limits_are_checked() {
        let deleted = compress_to_vec_zlib(&path_record(0, PATH_VERSION, &[]), 6);
        assert_eq!(parse_compressed_path_record(&deleted, Limits::default()), Err(ParseError::DeletedAsset));

        let wrong_version = compress_to_vec_zlib(&path_record(1, 531, &[]), 6);
        assert_eq!(
            parse_compressed_path_record(&wrong_version, Limits::default()),
            Err(ParseError::UnsupportedVersion(531))
        );

        let record = compress_to_vec_zlib(&path_record(1, PATH_VERSION, &[(1.0, 2.0, 3.0)]), 6);
        let limits = Limits { max_points: 0, ..Limits::default() };
        assert_eq!(parse_compressed_path_record(&record, limits), Err(ParseError::TooManyPoints));
    }
}
