#![no_std]

//! Bounded decoder for standard GameMaker 8.0 Room records (version 541).
//! Room 8.1 transform extensions are intentionally unsupported here.

extern crate alloc;

use alloc::vec::Vec;
use gm8_binary::{ReadError, SliceReader};
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};
use gm8_zlib::{InflateError, decompress_zlib_bounded};

pub const ROOM_VERSION_GM80: u32 = 541;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_string_bytes: usize,
    pub max_backgrounds: usize,
    pub max_views: usize,
    pub max_instances: usize,
    pub max_tiles: usize,
    pub max_total_string_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 16 * 1024 * 1024,
            max_decompressed_bytes: 16 * 1024 * 1024,
            max_string_bytes: 1024 * 1024,
            max_backgrounds: 4096,
            max_views: 4096,
            max_instances: 65_536,
            max_tiles: 131_072,
            max_total_string_bytes: 8 * 1024 * 1024,
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
    StringTooLong,
    TooManyBackgrounds,
    TooManyViews,
    TooManyInstances,
    TooManyTiles,
    TotalStringLimitExceeded,
    UnsupportedVersion(u32),
    TrailingBytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomBackground {
    pub visible_on_start: bool,
    pub is_foreground: bool,
    pub source_bg: i32,
    pub xoffset: i32,
    pub yoffset: i32,
    pub tile_horz: bool,
    pub tile_vert: bool,
    pub hspeed: i32,
    pub vspeed: i32,
    pub stretch: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewFollowData {
    pub hborder: i32,
    pub vborder: i32,
    pub hspeed: i32,
    pub vspeed: i32,
    pub target: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomView {
    pub visible: bool,
    pub source_x: i32,
    pub source_y: i32,
    pub source_w: u32,
    pub source_h: u32,
    pub port_x: i32,
    pub port_y: i32,
    pub port_w: u32,
    pub port_h: u32,
    pub following: ViewFollowData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomInstance {
    pub x: i32,
    pub y: i32,
    pub object: i32,
    pub id: i32,
    pub creation_code: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomTile {
    pub x: i32,
    pub y: i32,
    pub source_bg: i32,
    pub tile_x: u32,
    pub tile_y: u32,
    pub width: u32,
    pub height: u32,
    pub depth: i32,
    pub id: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    pub name: Vec<u8>,
    pub caption: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub speed: u32,
    pub persistent: bool,
    pub bg_colour: u32,
    pub clear_screen: bool,
    /// OpenGMK treats the GM8.0 clear-region flag as true.
    pub clear_region: bool,
    pub creation_code: Vec<u8>,
    pub backgrounds: Vec<RoomBackground>,
    pub views_enabled: bool,
    pub views: Vec<RoomView>,
    pub instances: Vec<RoomInstance>,
    pub tiles: Vec<RoomTile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableLimits {
    pub index: Gm80IndexLimits,
    pub record: Limits,
    pub max_rooms: u32,
    pub max_total_string_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedRoom {
    pub index: u32,
    pub room: Room,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomTable {
    pub directory: Gm80DirectorySummary,
    pub rooms: Vec<IndexedRoom>,
    pub deleted_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    Directory(Gm80DirectoryError),
    TooManyRooms,
    Parse { index: u32, error: ParseError },
    TotalStringLimitExceeded,
}

pub fn parse_compressed_room_record(input: &[u8], limits: Limits) -> Result<Room, ParseError> {
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
    let mut total_strings = 0usize;
    let name = read_string(&mut cursor, limits, &mut total_strings)?;
    let version = cursor.read_u32_le().map_err(map_read_error)?;
    if version != ROOM_VERSION_GM80 {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let caption = read_string(&mut cursor, limits, &mut total_strings)?;
    let width = cursor.read_u32_le().map_err(map_read_error)?;
    let height = cursor.read_u32_le().map_err(map_read_error)?;
    let speed = cursor.read_u32_le().map_err(map_read_error)?;
    let persistent = read_bool(&mut cursor)?;
    let bg_colour = cursor.read_u32_le().map_err(map_read_error)?;
    let clear_screen = read_bool(&mut cursor)?;
    let creation_code = read_string(&mut cursor, limits, &mut total_strings)?;

    let background_count = read_count(&mut cursor, limits.max_backgrounds, ParseError::TooManyBackgrounds)?;
    let mut backgrounds = Vec::with_capacity(background_count);
    for _ in 0..background_count {
        backgrounds.push(RoomBackground {
            visible_on_start: read_bool(&mut cursor)?,
            is_foreground: read_bool(&mut cursor)?,
            source_bg: read_i32(&mut cursor)?,
            xoffset: read_i32(&mut cursor)?,
            yoffset: read_i32(&mut cursor)?,
            tile_horz: read_bool(&mut cursor)?,
            tile_vert: read_bool(&mut cursor)?,
            hspeed: read_i32(&mut cursor)?,
            vspeed: read_i32(&mut cursor)?,
            stretch: read_bool(&mut cursor)?,
        });
    }
    let views_enabled = read_bool(&mut cursor)?;
    let view_count = read_count(&mut cursor, limits.max_views, ParseError::TooManyViews)?;
    let mut views = Vec::with_capacity(view_count);
    for _ in 0..view_count {
        views.push(RoomView {
            visible: read_bool(&mut cursor)?,
            source_x: read_i32(&mut cursor)?,
            source_y: read_i32(&mut cursor)?,
            source_w: cursor.read_u32_le().map_err(map_read_error)?,
            source_h: cursor.read_u32_le().map_err(map_read_error)?,
            port_x: read_i32(&mut cursor)?,
            port_y: read_i32(&mut cursor)?,
            port_w: cursor.read_u32_le().map_err(map_read_error)?,
            port_h: cursor.read_u32_le().map_err(map_read_error)?,
            following: ViewFollowData {
                hborder: read_i32(&mut cursor)?,
                vborder: read_i32(&mut cursor)?,
                hspeed: read_i32(&mut cursor)?,
                vspeed: read_i32(&mut cursor)?,
                target: read_i32(&mut cursor)?,
            },
        });
    }
    let instance_count = read_count(&mut cursor, limits.max_instances, ParseError::TooManyInstances)?;
    let mut instances = Vec::with_capacity(instance_count);
    for _ in 0..instance_count {
        instances.push(RoomInstance {
            x: read_i32(&mut cursor)?,
            y: read_i32(&mut cursor)?,
            object: read_i32(&mut cursor)?,
            id: read_i32(&mut cursor)?,
            creation_code: read_string(&mut cursor, limits, &mut total_strings)?,
        });
    }
    let tile_count = read_count(&mut cursor, limits.max_tiles, ParseError::TooManyTiles)?;
    let mut tiles = Vec::with_capacity(tile_count);
    for _ in 0..tile_count {
        tiles.push(RoomTile {
            x: read_i32(&mut cursor)?,
            y: read_i32(&mut cursor)?,
            source_bg: read_i32(&mut cursor)?,
            tile_x: cursor.read_u32_le().map_err(map_read_error)?,
            tile_y: cursor.read_u32_le().map_err(map_read_error)?,
            width: cursor.read_u32_le().map_err(map_read_error)?,
            height: cursor.read_u32_le().map_err(map_read_error)?,
            depth: read_i32(&mut cursor)?,
            id: read_i32(&mut cursor)?,
        });
    }
    if cursor.remaining() != 0 {
        return Err(ParseError::TrailingBytes);
    }
    Ok(Room {
        name,
        caption,
        width,
        height,
        speed,
        persistent,
        bg_colour,
        clear_screen,
        clear_region: true,
        creation_code,
        backgrounds,
        views_enabled,
        views,
        instances,
        tiles,
    })
}

pub fn parse_indexed_room(payload: &[u8], span: AssetSpan, limits: Limits) -> Result<Option<Room>, ParseError> {
    let start = span.offset as usize;
    let end = start.checked_add(span.length as usize).ok_or(ParseError::Truncated)?;
    let compressed = payload.get(start..end).ok_or(ParseError::Truncated)?;
    match parse_compressed_room_record(compressed, limits) {
        Ok(room) => Ok(Some(room)),
        Err(ParseError::DeletedAsset) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn load_gm80_room_table(payload: &[u8], limits: TableLimits) -> Result<RoomTable, TableError> {
    let mut spans = Vec::new();
    let mut too_many = false;
    let directory = scan_gm80_resource_directory(payload, limits.index, |kind, index, span| {
        if kind == Gm80AssetKind::Room {
            if spans.len() < limits.max_rooms as usize {
                spans.push((index, span));
            } else {
                too_many = true;
            }
        }
    })
    .map_err(TableError::Directory)?;
    if too_many {
        return Err(TableError::TooManyRooms);
    }
    let mut rooms = Vec::new();
    let mut deleted_count = 0u32;
    let mut total_strings = 0usize;
    for (index, span) in spans {
        let room =
            parse_indexed_room(payload, span, limits.record).map_err(|error| TableError::Parse { index, error })?;
        let Some(room) = room else {
            deleted_count = deleted_count.saturating_add(1);
            continue;
        };
        let room_bytes = room
            .name
            .len()
            .checked_add(room.caption.len())
            .and_then(|size| size.checked_add(room.creation_code.len()))
            .and_then(|size| {
                room.instances.iter().try_fold(size, |size, instance| size.checked_add(instance.creation_code.len()))
            })
            .ok_or(TableError::TotalStringLimitExceeded)?;
        total_strings = total_strings.checked_add(room_bytes).ok_or(TableError::TotalStringLimitExceeded)?;
        if total_strings > limits.max_total_string_bytes {
            return Err(TableError::TotalStringLimitExceeded);
        }
        rooms.push(IndexedRoom { index, room });
    }
    Ok(RoomTable { directory, rooms, deleted_count })
}

fn read_string(cursor: &mut SliceReader<'_>, limits: Limits, total: &mut usize) -> Result<Vec<u8>, ParseError> {
    let len = cursor.read_limited_u32_le(limits.max_string_bytes).map_err(|error| match error {
        ReadError::LimitExceeded => ParseError::StringTooLong,
        ReadError::OutOfBounds => ParseError::Truncated,
    })?;
    *total = total.checked_add(len).ok_or(ParseError::TotalStringLimitExceeded)?;
    if *total > limits.max_total_string_bytes {
        return Err(ParseError::TotalStringLimitExceeded);
    }
    Ok(cursor.take(len).map_err(map_read_error)?.to_vec())
}

fn read_count(cursor: &mut SliceReader<'_>, limit: usize, over_limit: ParseError) -> Result<usize, ParseError> {
    cursor.read_limited_u32_le(limit).map_err(|error| match error {
        ReadError::LimitExceeded => over_limit,
        ReadError::OutOfBounds => ParseError::Truncated,
    })
}

fn read_i32(cursor: &mut SliceReader<'_>) -> Result<i32, ParseError> {
    Ok(cursor.read_u32_le().map_err(map_read_error)? as i32)
}
fn read_bool(cursor: &mut SliceReader<'_>) -> Result<bool, ParseError> {
    Ok(cursor.read_u32_le().map_err(map_read_error)? != 0)
}
fn map_read_error(_: ReadError) -> ParseError {
    ParseError::Truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    fn u32le(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }
    fn string(out: &mut Vec<u8>, value: &[u8]) {
        u32le(out, value.len() as u32);
        out.extend_from_slice(value);
    }

    fn empty_room() -> Vec<u8> {
        let mut raw = Vec::new();
        u32le(&mut raw, 1); // generic asset envelope marker
        string(&mut raw, b"room_start");
        u32le(&mut raw, ROOM_VERSION_GM80);
        string(&mut raw, b"Welcome");
        u32le(&mut raw, 640);
        u32le(&mut raw, 480);
        u32le(&mut raw, 60);
        u32le(&mut raw, 0); // persistent
        u32le(&mut raw, 0x102030); // bg color
        u32le(&mut raw, 1); // clear screen
        string(&mut raw, b""); // room creation code
        u32le(&mut raw, 0); // backgrounds
        u32le(&mut raw, 0); // views enabled
        u32le(&mut raw, 0); // views
        u32le(&mut raw, 0); // instances
        u32le(&mut raw, 0); // tiles
        raw
    }

    fn room_with_contents() -> Vec<u8> {
        let mut raw = Vec::new();
        u32le(&mut raw, 1);
        string(&mut raw, b"room_test");
        u32le(&mut raw, ROOM_VERSION_GM80);
        string(&mut raw, b"Testing");
        u32le(&mut raw, 800);
        u32le(&mut raw, 600);
        u32le(&mut raw, 30);
        u32le(&mut raw, 1);
        u32le(&mut raw, 0xABCDEF);
        u32le(&mut raw, 0);
        string(&mut raw, b"room_code");
        u32le(&mut raw, 1); // backgrounds
        for value in [1, 0, 3, 4, 5, 1, 0, 6, 7, 1] {
            u32le(&mut raw, value);
        }
        u32le(&mut raw, 1); // views enabled
        u32le(&mut raw, 1); // views
        for value in [1, (-10i32) as u32, (-20i32) as u32, 320, 240, 4, 5, 320, 240, 8, 9, 10, 11, 12] {
            u32le(&mut raw, value);
        }
        u32le(&mut raw, 1); // instances
        for value in [10, 20, 2, 42] {
            u32le(&mut raw, value);
        }
        string(&mut raw, b"x = 1;");
        u32le(&mut raw, 1); // tiles
        for value in [30, 40, 3, 1, 2, 16, 16, (-100i32) as u32, 7] {
            u32le(&mut raw, value);
        }
        raw
    }

    fn view_count_offset(room: &[u8]) -> usize {
        let mut cursor = SliceReader::new(room);
        cursor.skip(4).unwrap();
        let name_len = cursor.read_u32_le().unwrap() as usize;
        cursor.skip(name_len).unwrap();
        cursor.skip(4).unwrap(); // version
        let caption_len = cursor.read_u32_le().unwrap() as usize;
        cursor.skip(caption_len).unwrap();
        cursor.skip(6 * 4).unwrap(); // width through clear_screen
        let code_len = cursor.read_u32_le().unwrap() as usize;
        cursor.skip(code_len).unwrap();
        cursor.skip(4 + 4).unwrap(); // zero backgrounds, views-enabled
        cursor.position()
    }

    fn limits() -> Limits {
        Limits {
            max_compressed_bytes: 4096,
            max_decompressed_bytes: 4096,
            max_string_bytes: 128,
            max_backgrounds: 8,
            max_views: 8,
            max_instances: 8,
            max_tiles: 8,
            max_total_string_bytes: 512,
        }
    }

    #[test]
    fn parses_minimal_gm80_room_and_defaults_clear_region() {
        let input = compress_to_vec_zlib(&empty_room(), 6);
        let room = parse_compressed_room_record(&input, limits()).unwrap();
        assert_eq!(room.name, b"room_start");
        assert_eq!(room.caption, b"Welcome");
        assert_eq!((room.width, room.height, room.speed), (640, 480, 60));
        assert!(room.clear_screen);
        assert!(room.clear_region);
        assert!(room.backgrounds.is_empty() && room.views.is_empty());
        assert!(room.instances.is_empty() && room.tiles.is_empty());
    }

    #[test]
    fn parses_background_view_instance_and_tile_fields() {
        let input = compress_to_vec_zlib(&room_with_contents(), 6);
        let room = parse_compressed_room_record(&input, limits()).unwrap();
        assert_eq!(room.name, b"room_test");
        assert_eq!(room.backgrounds[0].source_bg, 3);
        assert!(room.backgrounds[0].visible_on_start && room.backgrounds[0].stretch);
        assert_eq!((room.views[0].source_x, room.views[0].source_y), (-10, -20));
        assert_eq!(room.views[0].following.target, 12);
        assert_eq!((room.instances[0].object, room.instances[0].id), (2, 42));
        assert_eq!(room.instances[0].creation_code, b"x = 1;");
        assert_eq!((room.tiles[0].x, room.tiles[0].depth), (30, -100));
    }

    #[test]
    fn rejects_version_count_string_and_truncated_records() {
        let mut raw = empty_room();
        let version_offset = 4 + 4 + b"room_start".len();
        raw[version_offset..version_offset + 4].copy_from_slice(&810u32.to_le_bytes());
        let input = compress_to_vec_zlib(&raw, 6);
        assert_eq!(parse_compressed_room_record(&input, limits()), Err(ParseError::UnsupportedVersion(810)));

        let mut small = limits();
        small.max_views = 0;
        let mut with_view_count = empty_room();
        // The view count follows the fixed header, empty background table and views flag.
        let view_count_offset = view_count_offset(&with_view_count);
        with_view_count[view_count_offset..view_count_offset + 4].copy_from_slice(&1u32.to_le_bytes());
        let input = compress_to_vec_zlib(&with_view_count, 6);
        assert_eq!(parse_compressed_room_record(&input, small), Err(ParseError::TooManyViews));

        assert!(matches!(
            parse_compressed_room_record(&input[..input.len() - 1], limits()),
            Err(ParseError::Inflate(_))
        ));
        assert_eq!(parse_compressed_room_record(&[], limits()), Err(ParseError::Inflate(InflateError::EmptyInput)));

        let mut trailing = empty_room();
        trailing.push(0);
        let input = compress_to_vec_zlib(&trailing, 6);
        assert_eq!(parse_compressed_room_record(&input, limits()), Err(ParseError::TrailingBytes));
    }
}
