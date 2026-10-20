#![no_std]

//! Bounded parser for compressed GameMaker Script records.

extern crate alloc;

pub mod subset_vm;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const SCRIPT_VERSION: u32 = 800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_name_bytes: usize,
    pub max_source_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 8 * 1024 * 1024,
            max_decompressed_bytes: 8 * 1024 * 1024,
            max_name_bytes: 1024 * 1024,
            max_source_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    pub name: Vec<u8>,
    pub source: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    CompressedInputTooLarge,
    ZeroOutputLimit,
    Inflate(InflateError),
    Truncated,
    DeletedAsset,
    NameTooLong,
    SourceTooLong,
    UnsupportedVersion(u32),
    TrailingBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_records: u32,
    pub max_total_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedScript {
    pub index: u32,
    pub script: Script,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptTable {
    pub directory: Gm80DirectorySummary,
    pub scripts: Vec<IndexedScript>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GmlSyntaxSummary {
    pub accepted: u32,
    pub rejected: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GmlSubsetCompileSummary {
    pub compiled: u32,
    pub unsupported: u32,
}

/// Compile each script independently using the portable scalar bytecode slice.
/// Programs are discarded here; callers that need execution should retain the
/// source buffer and call `subset_vm::compile` directly.
pub fn compile_gml_subset(scripts: &[IndexedScript]) -> GmlSubsetCompileSummary {
    let mut summary = GmlSubsetCompileSummary::default();
    for entry in scripts {
        match subset_vm::compile(&entry.script.source, subset_vm::Limits::default()) {
            Ok(_) => summary.compiled = summary.compiled.saturating_add(1),
            Err(_) => summary.unsupported = summary.unsupported.saturating_add(1),
        }
    }
    summary
}

/// Parse every loaded source with OpenGMK's GML AST parser. This checks syntax
/// only: it does not resolve assets, compile instructions, or execute scripts.
pub fn validate_gml_syntax(scripts: &[IndexedScript]) -> GmlSyntaxSummary {
    let mut summary = GmlSyntaxSummary::default();
    for entry in scripts {
        if gml_parser::ast::AST::new(&entry.script.source).is_ok() {
            summary.accepted = summary.accepted.saturating_add(1);
        } else {
            summary.rejected = summary.rejected.saturating_add(1);
        }
    }
    summary
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    Directory(Gm80DirectoryError),
    TooManyRecords,
    Parse { index: u32, error: ParseError },
    TotalOutputLimitExceeded,
}

/// Validate the GM8.0 directory and decode its indexed Script table.
pub fn load_gm80_script_table(payload: &[u8], limits: TableLimits) -> Result<ScriptTable, TableError> {
    let mut spans = Vec::new();
    let mut too_many = false;
    let directory = scan_gm80_resource_directory(payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Script {
            if spans.len() < limits.max_records as usize {
                spans.push((index, span));
            } else {
                too_many = true;
            }
        }
    })
    .map_err(TableError::Directory)?;
    if too_many {
        return Err(TableError::TooManyRecords);
    }

    let mut scripts = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_bytes = 0usize;
    for (index, span) in spans {
        let script =
            parse_indexed_script(payload, span, limits.record).map_err(|error| TableError::Parse { index, error })?;
        let Some(script) = script else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        total_bytes = total_bytes
            .checked_add(script.name.len())
            .and_then(|n| n.checked_add(script.source.len()))
            .ok_or(TableError::TotalOutputLimitExceeded)?;
        if total_bytes > limits.max_total_output_bytes {
            return Err(TableError::TotalOutputLimitExceeded);
        }
        scripts.push(IndexedScript { index, script });
    }
    Ok(ScriptTable { directory, scripts, deleted_count })
}

pub fn parse_compressed_script_record(input: &[u8], limits: Limits) -> Result<Script, ParseError> {
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
    if version != SCRIPT_VERSION {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let source_len = cursor.read_limited_u32_le(limits.max_source_bytes).map_err(map_source_error)?;
    let source = cursor.take(source_len).map_err(map_read_error)?.to_vec();
    if cursor.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }
    Ok(Script { name, source })
}

pub fn parse_indexed_script(payload: &[u8], span: AssetSpan, limits: Limits) -> Result<Option<Script>, ParseError> {
    let start = span.offset as usize;
    let end = start.checked_add(span.length as usize).ok_or(ParseError::Truncated)?;
    let compressed = payload.get(start..end).ok_or(ParseError::Truncated)?;
    match parse_compressed_script_record(compressed, limits) {
        Ok(script) => Ok(Some(script)),
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

fn map_source_error(error: ReadError) -> ParseError {
    match error {
        ReadError::LimitExceeded => ParseError::SourceTooLong,
        ReadError::OutOfBounds => ParseError::Truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    fn record(name: &[u8], source: &[u8], version: u32) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&(name.len() as u32).to_le_bytes());
        raw.extend_from_slice(name);
        raw.extend_from_slice(&version.to_le_bytes());
        raw.extend_from_slice(&(source.len() as u32).to_le_bytes());
        raw.extend_from_slice(source);
        compress_to_vec_zlib(&raw, 6)
    }

    fn limits() -> Limits {
        Limits { max_compressed_bytes: 1024, max_decompressed_bytes: 1024, max_name_bytes: 32, max_source_bytes: 512 }
    }

    #[test]
    fn decodes_script_bytes_without_assuming_encoding() {
        let compressed = record(b"scr_test", b"return 42;\0", SCRIPT_VERSION);
        let script = parse_compressed_script_record(&compressed, limits()).unwrap();
        assert_eq!(script.name, b"scr_test");
        assert_eq!(script.source, b"return 42;\0");
    }

    #[test]
    fn rejects_unsupported_version_and_source_over_limit() {
        let compressed = record(b"s", b"x", 801);
        assert_eq!(parse_compressed_script_record(&compressed, limits()), Err(ParseError::UnsupportedVersion(801)));
        let compressed = record(b"s", &[0; 20], SCRIPT_VERSION);
        let mut small = limits();
        small.max_source_bytes = 8;
        assert_eq!(parse_compressed_script_record(&compressed, small), Err(ParseError::SourceTooLong));
    }

    #[test]
    fn rejects_truncated_and_trailing_data() {
        let mut raw = vec![1, 0, 0, 0, 1, 0, 0, 0, b's'];
        raw.extend_from_slice(&SCRIPT_VERSION.to_le_bytes());
        raw.extend_from_slice(&4u32.to_le_bytes());
        raw.extend_from_slice(b"xy");
        let compressed = compress_to_vec_zlib(&raw, 6);
        assert_eq!(parse_compressed_script_record(&compressed, limits()), Err(ParseError::Truncated));
        let compressed = record(b"s", b"x", SCRIPT_VERSION);
        let mut raw = gm8_zlib::decompress_zlib_bounded(&compressed, 1024).unwrap();
        raw.push(0);
        let compressed = compress_to_vec_zlib(&raw, 6);
        assert_eq!(parse_compressed_script_record(&compressed, limits()), Err(ParseError::TrailingBytes));
    }

    #[test]
    fn validates_each_loaded_script_with_the_opengmk_ast_parser() {
        let scripts = vec![
            IndexedScript { index: 0, script: Script { name: b"ok".to_vec(), source: b"x = 1;".to_vec() } },
            IndexedScript { index: 1, script: Script { name: b"bad".to_vec(), source: b"if (".to_vec() } },
        ];
        assert_eq!(validate_gml_syntax(&scripts), GmlSyntaxSummary { accepted: 1, rejected: 1 });
    }
}
