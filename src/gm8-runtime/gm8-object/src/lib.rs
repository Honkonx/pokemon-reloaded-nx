#![no_std]

//! Bounded decoder for GameMaker 8.0 Object and Drag-and-Drop action records.
//! The field order and record versions follow OpenGMK's `gm8exe` schemas.

extern crate alloc;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const OBJECT_VERSION: u32 = 430;
pub const EVENT_VERSION: u32 = 400;
pub const ACTION_VERSION: u32 = 440;
pub const ACTION_PARAMETER_COUNT: usize = 8;
const EVENT_LIST_COUNT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_string_bytes: usize,
    pub max_sub_events_per_object: usize,
    pub max_actions_per_object: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 16 * 1024 * 1024,
            max_decompressed_bytes: 16 * 1024 * 1024,
            max_string_bytes: 1024 * 1024,
            max_sub_events_per_object: 4096,
            max_actions_per_object: 16 * 1024,
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
    TooManySubEvents,
    TooManyActions,
    InvalidObjectVersion(u32),
    InvalidEventVersion(u32),
    InvalidActionVersion(u32),
    InvalidEventListCount(u32),
    InvalidParameterCount(u32),
    TrailingBytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeAction {
    pub lib_id: u32,
    pub id: u32,
    pub action_kind: u32,
    pub can_be_relative: u32,
    pub is_condition: bool,
    pub applies_to_something: bool,
    pub execution_type: u32,
    pub function_name: Vec<u8>,
    pub function_code: Vec<u8>,
    pub param_count: u32,
    pub param_types: [u32; ACTION_PARAMETER_COUNT],
    pub applies_to: i32,
    pub is_relative: bool,
    pub param_strings: [Vec<u8>; ACTION_PARAMETER_COUNT],
    pub invert_condition: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubEvent {
    pub index: u32,
    pub actions: Vec<CodeAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub name: Vec<u8>,
    pub sprite_index: i32,
    pub solid: bool,
    pub visible: bool,
    pub depth: i32,
    pub persistent: bool,
    pub parent_index: i32,
    pub mask_index: i32,
    /// GM8.0 serializes twelve event lists after storing the value eleven.
    pub events: [Vec<SubEvent>; EVENT_LIST_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_objects: u32,
    pub max_total_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedObject {
    pub index: u32,
    pub object: Object,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectTable {
    pub directory: Gm80DirectorySummary,
    pub objects: Vec<IndexedObject>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    Directory(Gm80DirectoryError),
    TooManyObjects,
    Parse { index: u32, error: ParseError },
    TotalOutputLimitExceeded,
}

/// Parse one compressed GM8.0 Object record, including its common nonzero marker.
pub fn parse_compressed_object_record(input: &[u8], limits: Limits) -> Result<Object, ParseError> {
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

    let name = read_string(&mut cursor, limits.max_string_bytes)?;
    let version = cursor.read_u32_le().map_err(map_read_error)?;
    if version != OBJECT_VERSION {
        return Err(ParseError::InvalidObjectVersion(version));
    }
    let sprite_index = cursor.read_u32_le().map_err(map_read_error)? as i32;
    let solid = read_bool(&mut cursor)?;
    let visible = read_bool(&mut cursor)?;
    let depth = cursor.read_u32_le().map_err(map_read_error)? as i32;
    let persistent = read_bool(&mut cursor)?;
    let parent_index = cursor.read_u32_le().map_err(map_read_error)? as i32;
    let mask_index = cursor.read_u32_le().map_err(map_read_error)? as i32;
    let encoded_list_count = cursor.read_u32_le().map_err(map_read_error)?;
    if encoded_list_count != 11 {
        return Err(ParseError::InvalidEventListCount(encoded_list_count));
    }

    let mut remaining_sub_events = limits.max_sub_events_per_object;
    let mut remaining_actions = limits.max_actions_per_object;
    let mut events: [Vec<SubEvent>; EVENT_LIST_COUNT] = core::array::from_fn(|_| Vec::new());
    for event_list in &mut events {
        loop {
            let sub_event_index = cursor.read_u32_le().map_err(map_read_error)? as i32;
            if sub_event_index < 0 {
                break;
            }
            if remaining_sub_events == 0 {
                return Err(ParseError::TooManySubEvents);
            }
            remaining_sub_events -= 1;
            let event_version = cursor.read_u32_le().map_err(map_read_error)?;
            if event_version != EVENT_VERSION {
                return Err(ParseError::InvalidEventVersion(event_version));
            }
            let action_count = cursor.read_limited_u32_le(remaining_actions).map_err(|e| match e {
                ReadError::LimitExceeded => ParseError::TooManyActions,
                ReadError::OutOfBounds => ParseError::Truncated,
            })?;
            remaining_actions -= action_count;
            let mut actions = Vec::with_capacity(action_count);
            for _ in 0..action_count {
                actions.push(parse_code_action(&mut cursor, limits.max_string_bytes)?);
            }
            event_list.push(SubEvent { index: sub_event_index as u32, actions });
        }
    }
    if cursor.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }

    Ok(Object { name, sprite_index, solid, visible, depth, persistent, parent_index, mask_index, events })
}

pub fn parse_indexed_object(payload: &[u8], span: AssetSpan, limits: Limits) -> Result<Option<Object>, ParseError> {
    let start = span.offset as usize;
    let end = start.checked_add(span.length as usize).ok_or(ParseError::Truncated)?;
    let compressed = payload.get(start..end).ok_or(ParseError::Truncated)?;
    match parse_compressed_object_record(compressed, limits) {
        Ok(object) => Ok(Some(object)),
        Err(ParseError::DeletedAsset) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn load_gm80_object_table(payload: &[u8], limits: TableLimits) -> Result<ObjectTable, TableError> {
    let mut spans = Vec::new();
    let mut too_many = false;
    let directory = scan_gm80_resource_directory(payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Object {
            if spans.len() < limits.max_objects as usize {
                spans.push((index, span));
            } else {
                too_many = true;
            }
        }
    })
    .map_err(TableError::Directory)?;
    if too_many {
        return Err(TableError::TooManyObjects);
    }

    let mut objects = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_output = 0usize;
    for (index, span) in spans {
        let object =
            parse_indexed_object(payload, span, limits.record).map_err(|error| TableError::Parse { index, error })?;
        let Some(object) = object else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        let output_size = object_output_size(&object).ok_or(TableError::TotalOutputLimitExceeded)?;
        total_output = total_output.checked_add(output_size).ok_or(TableError::TotalOutputLimitExceeded)?;
        if total_output > limits.max_total_output_bytes {
            return Err(TableError::TotalOutputLimitExceeded);
        }
        objects.push(IndexedObject { index, object });
    }
    Ok(ObjectTable { directory, objects, deleted_count })
}

/// Parse one OpenGMK code action from a bounded record cursor. Timeline assets
/// use the same serialized action schema as Object event lists.
pub fn parse_code_action(cursor: &mut SliceReader<'_>, max_string_bytes: usize) -> Result<CodeAction, ParseError> {
    let version = cursor.read_u32_le().map_err(map_read_error)?;
    if version != ACTION_VERSION {
        return Err(ParseError::InvalidActionVersion(version));
    }
    let lib_id = cursor.read_u32_le().map_err(map_read_error)?;
    let id = cursor.read_u32_le().map_err(map_read_error)?;
    let action_kind = cursor.read_u32_le().map_err(map_read_error)?;
    let can_be_relative = cursor.read_u32_le().map_err(map_read_error)?;
    let is_condition = read_bool(cursor)?;
    let applies_to_something = read_bool(cursor)?;
    let execution_type = cursor.read_u32_le().map_err(map_read_error)?;
    let function_name = read_string(cursor, max_string_bytes)?;
    let function_code = read_string(cursor, max_string_bytes)?;
    let param_count = cursor.read_u32_le().map_err(map_read_error)?;
    if param_count as usize > ACTION_PARAMETER_COUNT {
        return Err(ParseError::InvalidParameterCount(param_count));
    }
    let type_count = cursor.read_u32_le().map_err(map_read_error)?;
    if type_count as usize != ACTION_PARAMETER_COUNT {
        return Err(ParseError::InvalidParameterCount(type_count));
    }
    let mut param_types = [0; ACTION_PARAMETER_COUNT];
    for item in &mut param_types {
        *item = cursor.read_u32_le().map_err(map_read_error)?;
    }
    let applies_to = cursor.read_u32_le().map_err(map_read_error)? as i32;
    let is_relative = read_bool(cursor)?;
    let argument_count = cursor.read_u32_le().map_err(map_read_error)?;
    if argument_count as usize != ACTION_PARAMETER_COUNT {
        return Err(ParseError::InvalidParameterCount(argument_count));
    }
    let mut param_strings: [Vec<u8>; ACTION_PARAMETER_COUNT] = core::array::from_fn(|_| Vec::new());
    for item in &mut param_strings {
        *item = read_string(cursor, max_string_bytes)?;
    }
    let invert_condition = read_bool(cursor)?;
    Ok(CodeAction {
        lib_id,
        id,
        action_kind,
        can_be_relative,
        is_condition,
        applies_to_something,
        execution_type,
        function_name,
        function_code,
        param_count,
        param_types,
        applies_to,
        is_relative,
        param_strings,
        invert_condition,
    })
}

fn read_string(cursor: &mut SliceReader<'_>, max_bytes: usize) -> Result<Vec<u8>, ParseError> {
    let length = cursor.read_limited_u32_le(max_bytes).map_err(|error| match error {
        ReadError::LimitExceeded => ParseError::NameTooLong,
        ReadError::OutOfBounds => ParseError::Truncated,
    })?;
    Ok(cursor.take(length).map_err(map_read_error)?.to_vec())
}

fn read_bool(cursor: &mut SliceReader<'_>) -> Result<bool, ParseError> {
    Ok(cursor.read_u32_le().map_err(map_read_error)? != 0)
}

fn map_read_error(_: ReadError) -> ParseError {
    ParseError::Truncated
}

fn object_output_size(object: &Object) -> Option<usize> {
    let mut total = object.name.len();
    for event_list in &object.events {
        for event in event_list {
            for action in &event.actions {
                total = total.checked_add(action.function_name.len())?.checked_add(action.function_code.len())?;
                for value in &action.param_strings {
                    total = total.checked_add(value.len())?;
                }
            }
        }
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn push_string(bytes: &mut Vec<u8>, value: &[u8]) {
        push_u32(bytes, value.len() as u32);
        bytes.extend_from_slice(value);
    }

    fn empty_object(name: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        push_u32(&mut raw, 1); // generic asset envelope marker
        push_string(&mut raw, name);
        push_u32(&mut raw, OBJECT_VERSION);
        push_u32(&mut raw, u32::MAX); // no sprite
        push_u32(&mut raw, 0);
        push_u32(&mut raw, 1);
        push_u32(&mut raw, 0); // depth
        push_u32(&mut raw, 0);
        push_u32(&mut raw, u32::MAX); // no parent
        push_u32(&mut raw, u32::MAX); // no mask
        push_u32(&mut raw, 11); // serializes 12 lists
        for _ in 0..EVENT_LIST_COUNT {
            push_u32(&mut raw, u32::MAX);
        }
        raw
    }

    fn valid_action() -> Vec<u8> {
        let mut action = Vec::new();
        for value in [ACTION_VERSION, 1, 2, 0, 1, 0, 0, 2] {
            push_u32(&mut action, value);
        }
        push_string(&mut action, b"execute_code");
        push_string(&mut action, b"return 42;");
        push_u32(&mut action, 0); // used parameter count
        push_u32(&mut action, ACTION_PARAMETER_COUNT as u32);
        for _ in 0..ACTION_PARAMETER_COUNT {
            push_u32(&mut action, 0);
        }
        push_u32(&mut action, u32::MAX); // applies_to = -1
        push_u32(&mut action, 0); // relative
        push_u32(&mut action, ACTION_PARAMETER_COUNT as u32);
        for _ in 0..ACTION_PARAMETER_COUNT {
            push_string(&mut action, b"");
        }
        push_u32(&mut action, 0); // invert condition
        action
    }

    fn limits() -> Limits {
        Limits {
            max_compressed_bytes: 4096,
            max_decompressed_bytes: 4096,
            max_string_bytes: 128,
            max_sub_events_per_object: 32,
            max_actions_per_object: 32,
        }
    }

    #[test]
    fn parses_object_with_all_twelve_empty_event_lists() {
        let compressed = compress_to_vec_zlib(&empty_object(b"obj_player"), 6);
        let object = parse_compressed_object_record(&compressed, limits()).unwrap();
        assert_eq!(object.name, b"obj_player");
        assert_eq!(object.events.len(), 12);
        assert!(object.events.iter().all(Vec::is_empty));
        assert_eq!(object.sprite_index, -1);
        assert!(object.visible);
    }

    #[test]
    fn rejects_event_count_and_limits_and_preserves_deleted_marker() {
        let mut raw = empty_object(b"obj");
        let count_at = 4 + 4 + 3 + 8 * 4;
        raw[count_at..count_at + 4].copy_from_slice(&10u32.to_le_bytes());
        let compressed = compress_to_vec_zlib(&raw, 6);
        assert_eq!(parse_compressed_object_record(&compressed, limits()), Err(ParseError::InvalidEventListCount(10)));

        let compressed = compress_to_vec_zlib(&empty_object(b"obj"), 6);
        let mut small = limits();
        small.max_decompressed_bytes = 16;
        assert!(matches!(parse_compressed_object_record(&compressed, small), Err(ParseError::Inflate(_))));

        let deleted = compress_to_vec_zlib(&0u32.to_le_bytes(), 6);
        assert_eq!(parse_compressed_object_record(&deleted, limits()), Err(ParseError::DeletedAsset));
        let gm8_deleted_marker = [0x78, 0x9C, 0x63, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01];
        assert_eq!(parse_compressed_object_record(&gm8_deleted_marker, limits()), Err(ParseError::DeletedAsset));
    }

    #[test]
    fn parses_code_action_and_keeps_gml_source_as_bytes() {
        let action_bytes = valid_action();
        assert!(matches!(parse_code_action(&mut SliceReader::new(&action_bytes), 128), Ok(_)));
        let mut raw = empty_object(b"obj_script");
        let event_start = raw.len() - EVENT_LIST_COUNT * 4;
        raw[event_start..event_start + 4].copy_from_slice(&0u32.to_le_bytes());
        let mut event = Vec::new();
        push_u32(&mut event, EVENT_VERSION);
        push_u32(&mut event, 1);
        event.extend_from_slice(&action_bytes);
        push_u32(&mut event, u32::MAX); // end event list 0; preserve the other eleven markers
        raw.splice(event_start + 4..event_start + 4, event);
        let mut event_cursor = SliceReader::new(&raw[event_start..]);
        assert_eq!(event_cursor.read_u32_le(), Ok(0));
        assert_eq!(event_cursor.read_u32_le(), Ok(EVENT_VERSION));
        assert_eq!(event_cursor.read_u32_le(), Ok(1));
        assert!(parse_code_action(&mut event_cursor, 128).is_ok());
        let mut header_cursor = SliceReader::new(&raw);
        header_cursor.skip(4).unwrap();
        let name_len = header_cursor.read_u32_le().unwrap() as usize;
        header_cursor.skip(name_len).unwrap();
        header_cursor.skip(8 * 4).unwrap();
        assert_eq!(header_cursor.read_u32_le(), Ok(11));
        assert_eq!(header_cursor.position(), event_start);
        let compressed = compress_to_vec_zlib(&raw, 6);
        let object = parse_compressed_object_record(&compressed, limits()).unwrap();
        assert_eq!(object.events[0][0].index, 0);
        assert_eq!(object.events[0][0].actions[0].function_name, b"execute_code");
        assert_eq!(object.events[0][0].actions[0].function_code, b"return 42;");
        assert_eq!(object.events[0][0].actions[0].param_count, 0);
    }

    #[test]
    fn rejects_truncated_action_payload_and_excessive_string() {
        let mut raw = empty_object(b"obj");
        // Replace list 0 terminator with one event, version 400, one action, then truncate.
        let event_start = raw.len() - EVENT_LIST_COUNT * 4;
        raw[event_start..event_start + 4].copy_from_slice(&0u32.to_le_bytes());
        let mut event_header = Vec::new();
        push_u32(&mut event_header, EVENT_VERSION);
        push_u32(&mut event_header, 1);
        raw.splice(event_start + 4..event_start + 4, event_header);
        let compressed = compress_to_vec_zlib(&raw, 6);
        assert_eq!(
            parse_compressed_object_record(&compressed, limits()),
            Err(ParseError::InvalidActionVersion(u32::MAX))
        );

        let compressed = compress_to_vec_zlib(&empty_object(&[b'n'; 129]), 6);
        assert_eq!(parse_compressed_object_record(&compressed, limits()), Err(ParseError::NameTooLong));
    }
}
