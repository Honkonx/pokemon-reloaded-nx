#![no_std]

//! Bounded recognizer for the unprotected, standard GameMaker 8.0 loader.
//!
//! The byte signatures and search behavior follow OpenGMK's `gm8exe` GM8.0
//! detector. This deliberately does not claim recognition of GM8.1, UPX,
//! antidec, or patched executables.

use gm8_binary::{ReadError, SliceReader};

const LOADER_CHECK: u64 = 0x000A_49BE;
const HEADER_VERSION_CHECK: u64 = 0x000A_49E2;
const HEADER_OFFSET_FIELD: u64 = 0x0014_4AC0;
const MINIMUM_FILE_SIZE: u64 = HEADER_OFFSET_FIELD + 4;
const SEARCH_STEP: u64 = 10_000;

const LOADER_PREFIX: [u8; 8] = [0x8B, 0x45, 0xF4, 0xE8, 0x2A, 0xBD, 0xFD, 0xFF];
const LOADER_JNZ: [u8; 6] = [0x0F, 0x85, 0x18, 0x01, 0x00, 0x00];
const VERSION_PREFIX: [u8; 7] = [0x8B, 0xC6, 0xE8, 0x07, 0xBD, 0xFD, 0xFF];
const VERSION_JNZ: [u8; 6] = [0x0F, 0x85, 0xF5, 0x00, 0x00, 0x00];

pub trait ReadAt {
    fn len(&self) -> u64;
    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ProbeError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeError {
    OutOfBounds,
    InvalidRead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gm80Header {
    /// Offset where OpenGMK begins GM8.0 payload decryption and header parsing.
    pub payload_offset: u64,
    /// Loader magic values, absent when the corresponding runtime check was patched out.
    pub loader_magic: Option<u32>,
    pub header_version: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gm80PayloadInfo {
    pub data_offset: usize,
    pub encrypted_data_len: usize,
    pub pro_flag: bool,
    pub game_id: u32,
}

/// Byte range of one compressed record in a GameMaker asset table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssetSpan {
    pub offset: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexError {
    Truncated,
    TooManyEntries,
    InvalidOffset,
    InvalidLength,
    InvalidTag,
    StringTooLong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gm80AssetKind {
    Trigger,
    Sound,
    Sprite,
    Background,
    Path,
    Script,
    Font,
    Timeline,
    Object,
    Room,
    IncludedFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gm80IndexLimits {
    /// Shared budget for extensions, extension subrecords, and indexed assets.
    pub max_records: u32,
    pub max_pascal_string_bytes: u32,
    pub max_compressed_asset_bytes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gm80IndexSummary {
    pub pro_flag: bool,
    pub game_id: u32,
    pub guid: [u32; 4],
    pub extension_count: u32,
    pub trigger_count: u32,
    pub constant_count: u32,
    pub sound_count: u32,
    /// Number of bounded records consumed by this scan; pass the remaining
    /// budget to `scan_gm80_resource_tail` rather than resetting the limit.
    pub records_used: u32,
    pub end_offset: u32,
}

/// Counts and final offset for the resource tables after sounds. This completes
/// the directory walk in OpenGMK `gm8exe::reader` without inflating any record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gm80TailSummary {
    pub sprite_count: u32,
    pub background_count: u32,
    pub path_count: u32,
    pub script_count: u32,
    pub font_count: u32,
    pub timeline_count: u32,
    pub object_count: u32,
    pub room_count: u32,
    pub included_file_count: u32,
    pub last_instance_id: i32,
    pub last_tile_id: i32,
    pub room_order_count: u32,
    /// Number of bounded records consumed by this continuation scan.
    pub records_used: u32,
    pub end_offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gm80DirectoryError {
    Prefix(IndexError),
    Tail(IndexError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gm80DirectorySummary {
    pub prefix: Gm80IndexSummary,
    pub tail: Gm80TailSummary,
}

/// Walk every standard GM8.0 resource section while sharing one record budget.
/// Assets are reported as spans into `data`; this does not inflate or decode
/// their contents. Use this entry point when callers need a complete directory
/// validation and want to avoid accidentally resetting the prefix budget.
pub fn scan_gm80_resource_directory(
    data: &[u8],
    limits: Gm80IndexLimits,
    on_asset: impl FnMut(Gm80AssetKind, u32, AssetSpan),
) -> Result<Gm80DirectorySummary, Gm80DirectoryError> {
    let mut on_asset = on_asset;
    let prefix = scan_gm80_resource_prefix(data, limits, &mut on_asset).map_err(Gm80DirectoryError::Prefix)?;
    let tail_limits = Gm80IndexLimits { max_records: limits.max_records.saturating_sub(prefix.records_used), ..limits };
    let tail = scan_gm80_resource_tail(data, prefix.end_offset, tail_limits, &mut on_asset)
        .map_err(Gm80DirectoryError::Tail)?;
    Ok(Gm80DirectorySummary { prefix, tail })
}

/// Continue indexing after `scan_gm80_resource_prefix`'s `end_offset` and
/// walk the remaining GM8.0 resource sections. Set `limits.max_records` to
/// the original budget minus `Gm80IndexSummary::records_used`; this keeps the
/// cap shared across both calls. Record payloads are returned as spans; the
/// function does not inflate, decode, or execute them.
pub fn scan_gm80_resource_tail(
    data: &[u8],
    start_offset: u32,
    limits: Gm80IndexLimits,
    mut on_asset: impl FnMut(Gm80AssetKind, u32, AssetSpan),
) -> Result<Gm80TailSummary, IndexError> {
    let mut cursor = BoundedCursor::new(data, start_offset as usize, limits.max_records, limits)?;
    let mut table = |kind| -> Result<u32, IndexError> {
        cursor.expect_tag(800)?;
        cursor.asset_table(kind, &mut on_asset)
    };
    let sprite_count = table(Gm80AssetKind::Sprite)?;
    let background_count = table(Gm80AssetKind::Background)?;
    let path_count = table(Gm80AssetKind::Path)?;
    let script_count = table(Gm80AssetKind::Script)?;
    let font_count = table(Gm80AssetKind::Font)?;
    let timeline_count = table(Gm80AssetKind::Timeline)?;
    let object_count = table(Gm80AssetKind::Object)?;
    let room_count = table(Gm80AssetKind::Room)?;
    drop(table);
    let last_instance_id = cursor.u32()? as i32;
    let last_tile_id = cursor.u32()? as i32;
    cursor.expect_tag(800)?;
    let included_file_count = cursor.asset_table(Gm80AssetKind::IncludedFile, &mut on_asset)?;

    cursor.expect_tag(800)?;
    let help_len = cursor.u32()?;
    if help_len > limits.max_compressed_asset_bytes {
        return Err(IndexError::InvalidLength);
    }
    cursor.skip(help_len)?;

    cursor.expect_tag(500)?;
    let init_string_count = cursor.count()?;
    for _ in 0..init_string_count {
        cursor.pascal_string()?;
    }

    cursor.expect_tag(700)?;
    let room_order_count = cursor.count()?;
    cursor.skip(checked_mul_u32(room_order_count, 4)?)?;
    let records_used = limits.max_records - cursor.records_left;
    let end_offset = u32::try_from(cursor.position()).map_err(|_| IndexError::InvalidOffset)?;
    Ok(Gm80TailSummary {
        sprite_count,
        background_count,
        path_count,
        script_count,
        font_count,
        timeline_count,
        object_count,
        room_count,
        included_file_count,
        last_instance_id,
        last_tile_id,
        room_order_count,
        records_used,
        end_offset,
    })
}

/// Scan the GM8.0 plaintext resource prefix through sounds, without inflating
/// assets or allocating from file-controlled counts. Asset spans are reported
/// to the callback as offsets into `data`.
pub fn scan_gm80_resource_prefix(
    data: &[u8],
    limits: Gm80IndexLimits,
    mut on_asset: impl FnMut(Gm80AssetKind, u32, AssetSpan),
) -> Result<Gm80IndexSummary, IndexError> {
    let mut cursor = BoundedCursor::new(data, 0, limits.max_records, limits)?;
    let garbage_dwords = cursor.u32()?;
    cursor.skip(checked_mul_u32(garbage_dwords, 4)?)?;
    let pro_flag = cursor.u32()? != 0;
    let game_id = cursor.u32()?;
    let guid = [cursor.u32()?, cursor.u32()?, cursor.u32()?, cursor.u32()?];

    cursor.expect_tag(700)?;
    let extension_count = cursor.count()?;
    for _ in 0..extension_count {
        cursor.expect_tag(700)?;
        cursor.pascal_string()?;
        cursor.pascal_string()?;
        let file_count = cursor.count()?;
        for _ in 0..file_count {
            cursor.expect_tag(700)?;
            cursor.pascal_string()?;
            cursor.skip(4)?; // extension file kind
            cursor.pascal_string()?;
            cursor.pascal_string()?;
            let function_count = cursor.count()?;
            for _ in 0..function_count {
                cursor.expect_tag(700)?;
                cursor.pascal_string()?;
                cursor.pascal_string()?;
                cursor.skip(84)?; // calling convention, id, arguments, and return kind
            }
            let constant_count = cursor.count()?;
            for _ in 0..constant_count {
                cursor.expect_tag(700)?;
                cursor.pascal_string()?;
                cursor.pascal_string()?;
            }
        }
        // OpenGMK stores a length including the four-byte encryption seed.
        let contents_len = cursor.u32()?;
        if contents_len < 4 {
            return Err(IndexError::InvalidLength);
        }
        cursor.skip(4)?; // seed
        cursor.skip(contents_len - 4)?;
    }

    cursor.expect_tag(800)?;
    let trigger_count = cursor.asset_table(Gm80AssetKind::Trigger, &mut on_asset)?;

    cursor.expect_tag(800)?;
    let constant_count = cursor.count()?;
    for _ in 0..constant_count {
        cursor.pascal_string()?;
        cursor.pascal_string()?;
    }

    cursor.expect_tag(800)?;
    let sound_count = cursor.asset_table(Gm80AssetKind::Sound, &mut on_asset)?;
    let records_used = limits.max_records - cursor.records_left;
    let end_offset = u32::try_from(cursor.position()).map_err(|_| IndexError::InvalidOffset)?;
    Ok(Gm80IndexSummary {
        pro_flag,
        game_id,
        guid,
        extension_count,
        trigger_count,
        constant_count,
        sound_count,
        records_used,
        end_offset,
    })
}

struct BoundedCursor<'a> {
    reader: SliceReader<'a>,
    records_left: u32,
    limits: Gm80IndexLimits,
}

impl<'a> BoundedCursor<'a> {
    fn new(data: &'a [u8], offset: usize, records_left: u32, limits: Gm80IndexLimits) -> Result<Self, IndexError> {
        let reader = SliceReader::at(data, offset).map_err(map_reader_error)?;
        Ok(Self { reader, records_left, limits })
    }

    fn position(&self) -> usize {
        self.reader.position()
    }

    fn u32(&mut self) -> Result<u32, IndexError> {
        self.reader.read_u32_le().map_err(map_reader_error)
    }

    fn skip(&mut self, length: u32) -> Result<(), IndexError> {
        self.reader.skip(length as usize).map_err(map_reader_error)
    }

    fn count(&mut self) -> Result<u32, IndexError> {
        let count = self.u32()?;
        if count > self.records_left {
            return Err(IndexError::TooManyEntries);
        }
        self.records_left -= count;
        Ok(count)
    }

    fn expect_tag(&mut self, expected: u32) -> Result<(), IndexError> {
        if self.u32()? == expected { Ok(()) } else { Err(IndexError::InvalidTag) }
    }

    fn pascal_string(&mut self) -> Result<(), IndexError> {
        let length = self.u32()?;
        if length > self.limits.max_pascal_string_bytes {
            return Err(IndexError::StringTooLong);
        }
        self.skip(length)
    }

    fn asset_table(
        &mut self,
        kind: Gm80AssetKind,
        on_asset: &mut impl FnMut(Gm80AssetKind, u32, AssetSpan),
    ) -> Result<u32, IndexError> {
        let count = self.count()?;
        for index in 0..count {
            let length = self.u32()?;
            if length > self.limits.max_compressed_asset_bytes {
                return Err(IndexError::InvalidLength);
            }
            let offset = u32::try_from(self.position()).map_err(|_| IndexError::InvalidOffset)?;
            let span = AssetSpan { offset, length };
            self.skip(length)?;
            on_asset(kind, index, span);
        }
        Ok(count)
    }
}

fn map_reader_error(error: ReadError) -> IndexError {
    match error {
        ReadError::OutOfBounds => IndexError::Truncated,
        ReadError::LimitExceeded => IndexError::InvalidLength,
    }
}

fn checked_mul_u32(value: u32, factor: u32) -> Result<u32, IndexError> {
    value.checked_mul(factor).ok_or(IndexError::InvalidLength)
}

/// Index one OpenGMK-style asset table without allocating or decompressing it.
/// The table layout is a u32 entry count followed by repeated `u32 length`
/// and that many bytes. `spans` is caller-owned so a Switch caller can enforce
/// its own memory cap before parsing untrusted game data.
pub fn index_asset_table(data: &[u8], offset: usize, spans: &mut [AssetSpan]) -> Result<(usize, usize), IndexError> {
    let count_end = offset.checked_add(4).ok_or(IndexError::InvalidOffset)?;
    let count_bytes = data.get(offset..count_end).ok_or(IndexError::Truncated)?;
    let count = u32::from_le_bytes([count_bytes[0], count_bytes[1], count_bytes[2], count_bytes[3]]) as usize;
    if count > spans.len() {
        return Err(IndexError::TooManyEntries);
    }

    let mut cursor = count_end;
    for span in spans.iter_mut().take(count) {
        let length_end = cursor.checked_add(4).ok_or(IndexError::InvalidOffset)?;
        let length_bytes = data.get(cursor..length_end).ok_or(IndexError::Truncated)?;
        let length = u32::from_le_bytes([length_bytes[0], length_bytes[1], length_bytes[2], length_bytes[3]]);
        cursor = length_end;
        let end = cursor.checked_add(length as usize).ok_or(IndexError::InvalidOffset)?;
        if end > data.len() {
            return Err(IndexError::Truncated);
        }
        *span = AssetSpan { offset: u32::try_from(cursor).map_err(|_| IndexError::InvalidOffset)?, length };
        cursor = end;
    }
    Ok((count, cursor))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecryptError {
    InvalidOffset,
    TruncatedHeader,
    InvalidLength,
    InvalidMetadata,
}

/// Locate the in-place GM8.0 decryption record after OpenGMK's settings and
/// embedded-DLL sections. The compressed settings and DLL bytes are skipped,
/// not loaded or interpreted.
pub fn locate_standard_gm80_decrypt_payload(exe: &[u8], payload_offset: usize) -> Result<usize, DecryptError> {
    let mut cursor = payload_offset;
    let settings_len = read_u32(exe, &mut cursor)? as usize;
    cursor = cursor.checked_add(settings_len).ok_or(DecryptError::InvalidLength)?;
    if cursor > exe.len() {
        return Err(DecryptError::TruncatedHeader);
    }

    let embedded_name_len = read_u32(exe, &mut cursor)? as usize;
    cursor = cursor.checked_add(embedded_name_len).ok_or(DecryptError::InvalidLength)?;
    if cursor > exe.len() {
        return Err(DecryptError::TruncatedHeader);
    }

    let embedded_dll_len = read_u32(exe, &mut cursor)? as usize;
    cursor = cursor.checked_add(embedded_dll_len).ok_or(DecryptError::InvalidLength)?;
    if cursor > exe.len() {
        return Err(DecryptError::TruncatedHeader);
    }
    Ok(cursor)
}

/// Locate the GM8.0 decrypt-record start using random-access reads, without
/// loading the executable or its settings/DLL payloads into memory.
pub fn locate_standard_gm80_decrypt_payload_at<R: ReadAt + ?Sized>(
    exe: &mut R,
    payload_offset: u64,
) -> Result<u64, ProbeError> {
    let mut cursor = payload_offset;
    let mut dword = [0u8; 4];
    exe.read_exact_at(cursor, &mut dword)?;
    let settings_len = u32::from_le_bytes(dword) as u64;
    cursor = cursor.checked_add(4).and_then(|v| v.checked_add(settings_len)).ok_or(ProbeError::OutOfBounds)?;

    exe.read_exact_at(cursor, &mut dword)?;
    let dll_name_len = u32::from_le_bytes(dword) as u64;
    cursor = cursor.checked_add(4).and_then(|v| v.checked_add(dll_name_len)).ok_or(ProbeError::OutOfBounds)?;

    exe.read_exact_at(cursor, &mut dword)?;
    let dll_len = u32::from_le_bytes(dword) as u64;
    cursor = cursor.checked_add(4).and_then(|v| v.checked_add(dll_len)).ok_or(ProbeError::OutOfBounds)?;
    if cursor > exe.len() {
        return Err(ProbeError::OutOfBounds);
    }
    Ok(cursor)
}

/// Validate PE/i386 as well as the GM8 loader before choosing a game's `.exe`.
/// This bounded probe identifies the container, not the title or asset validity.
pub fn detect_standard_gm80_executable<R: ReadAt + ?Sized>(exe: &mut R) -> Result<Option<Gm80Header>, ProbeError> {
    if exe.len() < 64 { return Ok(None) }
    let mut dos = [0u8; 64];
    exe.read_exact_at(0, &mut dos)?;
    if &dos[..2] != b"MZ" { return Ok(None) }
    let pe = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]) as u64;
    if pe < 64 || pe.checked_add(24).filter(|end| *end <= exe.len()).is_none() { return Ok(None) }
    let mut header = [0u8; 6];
    exe.read_exact_at(pe, &mut header)?;
    if &header != b"PE\0\0\x4c\x01" { return Ok(None) }
    let found = detect_standard_gm80(exe)?;
    if let Some(header) = found {
        locate_standard_gm80_decrypt_payload_at(exe, header.payload_offset)?;
    }
    Ok(found)
}

/// Recognize the standard GM8.0 loading sequence and return the payload parser offset.
pub fn detect_standard_gm80<R: ReadAt + ?Sized>(exe: &mut R) -> Result<Option<Gm80Header>, ProbeError> {
    if exe.len() < MINIMUM_FILE_SIZE {
        return Ok(None);
    }

    let mut bytes8 = [0u8; 8];
    exe.read_exact_at(LOADER_CHECK, &mut bytes8)?;
    if bytes8 != LOADER_PREFIX {
        return Ok(None);
    }

    let mut one = [0u8; 1];
    exe.read_exact_at(LOADER_CHECK + 8, &mut one)?;
    let loader_magic = match one[0] {
        0x3D => read_checked_magic(exe, LOADER_CHECK + 9, &LOADER_JNZ)?,
        0x90 => None,
        _ => return Ok(None),
    };

    exe.read_exact_at(HEADER_VERSION_CHECK, &mut bytes8[..7])?;
    let header_version = if bytes8[..7] == VERSION_PREFIX {
        exe.read_exact_at(HEADER_VERSION_CHECK + 7, &mut one)?;
        match one[0] {
            0x3D => read_checked_magic(exe, HEADER_VERSION_CHECK + 8, &VERSION_JNZ)?,
            0x90 => None,
            _ => return Ok(None),
        }
    } else {
        None
    };

    let mut dword = [0u8; 4];
    exe.read_exact_at(HEADER_OFFSET_FIELD, &mut dword)?;
    let mut cursor = u32::from_le_bytes(dword) as u64;

    if let Some(magic) = loader_magic {
        loop {
            let end = cursor.checked_add(4).ok_or(ProbeError::OutOfBounds)?;
            if end > exe.len() {
                return Ok(None);
            }
            exe.read_exact_at(cursor, &mut dword)?;
            if u32::from_le_bytes(dword) == magic {
                cursor = end;
                break;
            }
            cursor = cursor.checked_add(SEARCH_STEP).ok_or(ProbeError::OutOfBounds)?;
        }
    } else {
        cursor = cursor.checked_add(4).ok_or(ProbeError::OutOfBounds)?;
    }

    if let Some(expected) = header_version {
        if cursor.checked_add(4).filter(|&end| end <= exe.len()).is_none() {
            return Ok(None);
        }
        exe.read_exact_at(cursor, &mut dword)?;
        if u32::from_le_bytes(dword) != expected {
            return Ok(None);
        }
    }

    let payload_start = cursor.checked_add(12).ok_or(ProbeError::OutOfBounds)?;
    if payload_start > exe.len() {
        return Ok(None);
    }
    Ok(Some(Gm80Header { payload_offset: payload_start, loader_magic, header_version }))
}

fn read_checked_magic<R: ReadAt + ?Sized>(
    exe: &mut R,
    offset: u64,
    expected_jump: &[u8; 6],
) -> Result<Option<u32>, ProbeError> {
    let mut magic = [0u8; 4];
    let mut jump = [0u8; 6];
    exe.read_exact_at(offset, &mut magic)?;
    exe.read_exact_at(offset + 4, &mut jump)?;
    Ok((jump == *expected_jump).then(|| u32::from_le_bytes(magic)))
}

/// Apply OpenGMK's standard GM8.0 in-place payload decryption and read the
/// first metadata fields that `gm8exe::reader::from_exe` consumes next.
pub fn decrypt_standard_gm80_payload(exe: &mut [u8], payload_offset: usize) -> Result<Gm80PayloadInfo, DecryptError> {
    let mut cursor = payload_offset;
    let garbage_1 = read_u32(exe, &mut cursor)? as usize;
    let garbage_2 = read_u32(exe, &mut cursor)? as usize;
    let garbage_1_bytes = garbage_1.checked_mul(4).ok_or(DecryptError::InvalidLength)?;
    let garbage_2_bytes = garbage_2.checked_mul(4).ok_or(DecryptError::InvalidLength)?;
    cursor = cursor.checked_add(garbage_1_bytes).ok_or(DecryptError::InvalidLength)?;
    let table_end = cursor.checked_add(256).ok_or(DecryptError::InvalidLength)?;
    let swap_bytes = exe.get(cursor..table_end).ok_or(DecryptError::TruncatedHeader)?;
    let mut swap_table = [0u8; 256];
    swap_table.copy_from_slice(swap_bytes);
    let mut reverse_table = [0u8; 256];
    for (index, &value) in swap_table.iter().enumerate() {
        reverse_table[value as usize] = index as u8;
    }
    cursor = table_end.checked_add(garbage_2_bytes).ok_or(DecryptError::InvalidLength)?;
    let encrypted_data_len = read_u32(exe, &mut cursor)? as usize;
    let data_offset = cursor;
    let data_end = data_offset.checked_add(encrypted_data_len).ok_or(DecryptError::InvalidLength)?;
    if encrypted_data_len < 2 || data_end > exe.len() {
        return Err(DecryptError::InvalidLength);
    }

    // Keep the same descending first pass used by OpenGMK. The preceding byte
    // is still encrypted at each step, which is why this pass runs backwards.
    for index in ((data_offset + 2)..=data_end).rev() {
        let current = exe[index - 1];
        let previous = exe[index - 2];
        let relative = index - (data_offset + 1);
        exe[index - 1] = reverse_table[current as usize].wrapping_sub(previous.wrapping_add(relative as u8));
    }

    // The second pass reverses byte positions. A saturating subtraction keeps
    // malformed offsets from wrapping before the lower bound is applied.
    for index in (data_offset..data_end).rev() {
        let table_index = (index - data_offset) & 0xFF;
        let other = (index as u32).saturating_sub(swap_table[table_index] as u32).max(data_offset as u32) as usize;
        if other >= data_end {
            return Err(DecryptError::InvalidOffset);
        }
        exe.swap(index, other);
    }

    let mut metadata = data_offset;
    let garbage_dwords = read_u32(exe, &mut metadata)? as usize;
    let metadata_skip = garbage_dwords.checked_mul(4).ok_or(DecryptError::InvalidMetadata)?;
    metadata = metadata.checked_add(metadata_skip).ok_or(DecryptError::InvalidMetadata)?;
    let metadata_end = metadata.checked_add(8).ok_or(DecryptError::InvalidMetadata)?;
    if metadata_end > data_end {
        return Err(DecryptError::InvalidMetadata);
    }
    let pro_flag = read_u32(exe, &mut metadata)? != 0;
    let game_id = read_u32(exe, &mut metadata)?;

    Ok(Gm80PayloadInfo { data_offset, encrypted_data_len, pro_flag, game_id })
}

fn read_u32(data: &[u8], cursor: &mut usize) -> Result<u32, DecryptError> {
    let end = cursor.checked_add(4).ok_or(DecryptError::TruncatedHeader)?;
    let bytes = data.get(*cursor..end).ok_or(DecryptError::TruncatedHeader)?;
    *cursor = end;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

impl ReadAt for [u8] {
    fn len(&self) -> u64 {
        self.len() as u64
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ProbeError> {
        let start = usize::try_from(offset).map_err(|_| ProbeError::OutOfBounds)?;
        let end = start.checked_add(out.len()).ok_or(ProbeError::OutOfBounds)?;
        let source = self.get(start..end).ok_or(ProbeError::OutOfBounds)?;
        out.copy_from_slice(source);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_pascal(out: &mut Vec<u8>, value: &[u8]) {
        push_u32(out, value.len() as u32);
        out.extend_from_slice(value);
    }

    fn empty_resource_prefix(extension_count: u32) -> Vec<u8> {
        let mut out = Vec::new();
        push_u32(&mut out, 0); // random dword count
        push_u32(&mut out, 1); // Pro
        push_u32(&mut out, 0x1234_5678);
        for value in [1, 2, 3, 4] {
            push_u32(&mut out, value);
        }
        push_u32(&mut out, 700);
        push_u32(&mut out, extension_count);
        out
    }

    fn append_empty_tables(out: &mut Vec<u8>) {
        push_u32(out, 800); // triggers
        push_u32(out, 0);
        push_u32(out, 800); // constants
        push_u32(out, 0);
        push_u32(out, 800); // sounds
        push_u32(out, 0);
    }

    fn fixture(patched: bool) -> Vec<u8> {
        let mut exe = vec![0u8; (HEADER_OFFSET_FIELD + 0x3000) as usize];
        let at = LOADER_CHECK as usize;
        exe[at..at + 8].copy_from_slice(&LOADER_PREFIX);
        exe[at + 8] = if patched { 0x90 } else { 0x3D };
        let loader_magic = 0x0123_4567u32;
        exe[at + 9..at + 13].copy_from_slice(&loader_magic.to_le_bytes());
        exe[at + 13..at + 19].copy_from_slice(&LOADER_JNZ);

        let ver_at = HEADER_VERSION_CHECK as usize;
        exe[ver_at..ver_at + 7].copy_from_slice(&VERSION_PREFIX);
        exe[ver_at + 7] = if patched { 0x90 } else { 0x3D };
        let version = 0x320u32;
        exe[ver_at + 8..ver_at + 12].copy_from_slice(&version.to_le_bytes());
        exe[ver_at + 12..ver_at + 18].copy_from_slice(&VERSION_JNZ);

        let header = (HEADER_OFFSET_FIELD + 0x1000) as u32;
        exe[HEADER_OFFSET_FIELD as usize..HEADER_OFFSET_FIELD as usize + 4].copy_from_slice(&header.to_le_bytes());
        let h = header as usize;
        if !patched {
            exe[h..h + 4].copy_from_slice(&loader_magic.to_le_bytes());
            exe[h + 4..h + 8].copy_from_slice(&version.to_le_bytes());
        }
        exe
    }

    #[test]
    fn detects_standard_loading_sequence_and_payload_offset() {
        let mut exe = fixture(false);
        let result = detect_standard_gm80(exe.as_mut_slice()).unwrap().unwrap();
        assert_eq!(result.payload_offset, HEADER_OFFSET_FIELD + 0x1010);
        assert_eq!(result.loader_magic, Some(0x0123_4567));
        assert_eq!(result.header_version, Some(0x320));
    }

    #[test]
    fn executable_detection_requires_pe_i386_and_a_game_loader() {
        let mut exe = fixture(false);
        assert!(detect_standard_gm80_executable(exe.as_mut_slice()).unwrap().is_none());
        exe[..2].copy_from_slice(b"MZ");
        exe[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        exe[0x80..0x86].copy_from_slice(b"PE\0\0\x4c\x01");
        assert!(detect_standard_gm80_executable(exe.as_mut_slice()).unwrap().is_some());
        exe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        assert!(detect_standard_gm80_executable(exe.as_mut_slice()).unwrap().is_none());
        exe[0x84..0x86].copy_from_slice(&0x14cu16.to_le_bytes());
        exe[LOADER_CHECK as usize] = 0;
        assert!(detect_standard_gm80_executable(exe.as_mut_slice()).unwrap().is_none());
    }

    #[test]
    fn detects_loader_with_magic_checks_patched_out() {
        let mut exe = fixture(true);
        let result = detect_standard_gm80(exe.as_mut_slice()).unwrap().unwrap();
        assert_eq!(result.payload_offset, HEADER_OFFSET_FIELD + 0x1010);
        assert_eq!(result.loader_magic, None);
        assert_eq!(result.header_version, None);
    }

    #[test]
    fn rejects_short_or_mismatched_images() {
        let mut short = vec![0u8; 128];
        assert_eq!(detect_standard_gm80(short.as_mut_slice()).unwrap(), None);
        let mut wrong = fixture(false);
        wrong[LOADER_CHECK as usize] ^= 1;
        assert_eq!(detect_standard_gm80(wrong.as_mut_slice()).unwrap(), None);
    }

    #[test]
    fn decrypts_bounded_synthetic_payload_and_reads_metadata() {
        let payload_offset = 8usize;
        let mut image = vec![0u8; 512];
        let mut cursor = payload_offset;
        image[cursor..cursor + 4].copy_from_slice(&0u32.to_le_bytes());
        image[cursor + 4..cursor + 8].copy_from_slice(&0u32.to_le_bytes());
        cursor += 8;
        for (index, byte) in image[cursor..cursor + 256].iter_mut().enumerate() {
            *byte = index as u8;
        }
        cursor += 256;
        let expected = [0u8, 0, 0, 0, 1, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 9, 8, 7, 6];
        image[cursor..cursor + 4].copy_from_slice(&(expected.len() as u32).to_le_bytes());
        cursor += 4;
        let data_offset = cursor;

        // Invert the decoder's second (descending) swap pass.
        let mut encoded = expected;
        for index in 0..encoded.len() {
            let table_index = index & 0xFF;
            let other = ((data_offset + index) as u32).saturating_sub(table_index as u32).max(data_offset as u32)
                as usize
                - data_offset;
            encoded.swap(index, other);
        }
        // Invert the first pass; its previous byte is still encrypted.
        for index in 1..encoded.len() {
            encoded[index] = encoded[index].wrapping_add(encoded[index - 1]).wrapping_add(index as u8);
        }
        image[data_offset..data_offset + encoded.len()].copy_from_slice(&encoded);

        let info = decrypt_standard_gm80_payload(&mut image, payload_offset).unwrap();
        assert_eq!(info.data_offset, data_offset);
        assert_eq!(info.encrypted_data_len, expected.len());
        assert!(info.pro_flag);
        assert_eq!(info.game_id, 0x1234_5678);
        assert_eq!(&image[data_offset..data_offset + expected.len()], &expected);
    }

    #[test]
    fn rejects_truncated_gm80_decryption_records() {
        let mut short = vec![0u8; 16];
        short[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decrypt_standard_gm80_payload(&mut short, 4), Err(DecryptError::TruncatedHeader));
        assert_eq!(locate_standard_gm80_decrypt_payload(&short, 0), Err(DecryptError::TruncatedHeader));
    }

    #[test]
    fn rejects_metadata_that_would_escape_the_decrypted_block() {
        let payload_offset = 8usize;
        let mut image = vec![0u8; 320];
        let mut cursor = payload_offset;
        cursor += 8; // zero-length garbage spans
        for (index, byte) in image[cursor..cursor + 256].iter_mut().enumerate() {
            *byte = index as u8;
        }
        cursor += 256;
        image[cursor..cursor + 4].copy_from_slice(&8u32.to_le_bytes());
        cursor += 4;
        image[cursor..cursor + 8].fill(0);
        assert_eq!(decrypt_standard_gm80_payload(&mut image, payload_offset), Err(DecryptError::InvalidMetadata));
    }

    #[test]
    fn indexes_asset_table_without_copying_payloads() {
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&[1, 2, 3]);
        data.extend_from_slice(&0u32.to_le_bytes());
        let mut spans = [AssetSpan::default(); 2];
        let (count, end) = index_asset_table(&data, 0, &mut spans).unwrap();
        assert_eq!(count, 2);
        assert_eq!(spans[0], AssetSpan { offset: 8, length: 3 });
        assert_eq!(spans[1], AssetSpan { offset: 15, length: 0 });
        assert_eq!(end, data.len());
    }

    #[test]
    fn bounds_asset_table_count_and_lengths() {
        let too_many = 3u32.to_le_bytes();
        let mut spans = [AssetSpan::default(); 2];
        assert_eq!(index_asset_table(&too_many, 0, &mut spans), Err(IndexError::TooManyEntries));

        let mut truncated = Vec::new();
        truncated.extend_from_slice(&1u32.to_le_bytes());
        truncated.extend_from_slice(&4u32.to_le_bytes());
        truncated.extend_from_slice(&[1, 2]);
        assert_eq!(index_asset_table(&truncated, 0, &mut spans), Err(IndexError::Truncated));
    }

    #[test]
    fn scans_trigger_constants_and_sound_spans_without_inflating() {
        let mut data = empty_resource_prefix(0);
        push_u32(&mut data, 800);
        push_u32(&mut data, 1);
        push_u32(&mut data, 3);
        data.extend_from_slice(&[0x78, 0x9C, 0]);
        push_u32(&mut data, 800);
        push_u32(&mut data, 1);
        push_pascal(&mut data, b"synthetic");
        push_pascal(&mut data, b"0");
        push_u32(&mut data, 800);
        push_u32(&mut data, 0);

        let mut spans = Vec::new();
        let summary = scan_gm80_resource_prefix(
            &data,
            Gm80IndexLimits { max_records: 16, max_pascal_string_bytes: 64, max_compressed_asset_bytes: 1024 },
            |kind, index, span| spans.push((kind, index, span)),
        )
        .unwrap();
        assert!(summary.pro_flag);
        assert_eq!(summary.game_id, 0x1234_5678);
        assert_eq!(summary.guid, [1, 2, 3, 4]);
        assert_eq!((summary.trigger_count, summary.constant_count, summary.sound_count), (1, 1, 0));
        assert_eq!(summary.end_offset as usize, data.len());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0, Gm80AssetKind::Trigger);
        assert_eq!(spans[0].1, 0);
        assert_eq!(&data[spans[0].2.offset as usize..(spans[0].2.offset + spans[0].2.length) as usize], &[
            0x78, 0x9C, 0
        ]);
    }

    #[test]
    fn walks_bounded_extension_records_and_rejects_truncation() {
        let mut data = empty_resource_prefix(1);
        push_u32(&mut data, 700); // extension
        push_pascal(&mut data, b"extension");
        push_pascal(&mut data, b"folder");
        push_u32(&mut data, 1); // files
        push_u32(&mut data, 700); // file
        push_pascal(&mut data, b"file");
        push_u32(&mut data, 4); // other file type
        push_pascal(&mut data, b""); // initializer
        push_pascal(&mut data, b""); // finalizer
        push_u32(&mut data, 1); // functions
        push_u32(&mut data, 700); // function
        push_pascal(&mut data, b"function");
        push_pascal(&mut data, b"external");
        data.extend_from_slice(&[0; 84]);
        push_u32(&mut data, 1); // file constants
        push_u32(&mut data, 700);
        push_pascal(&mut data, b"constant");
        push_pascal(&mut data, b"1");
        push_u32(&mut data, 4); // content length includes seed
        push_u32(&mut data, 0); // seed
        append_empty_tables(&mut data);

        let limits = Gm80IndexLimits { max_records: 16, max_pascal_string_bytes: 64, max_compressed_asset_bytes: 1024 };
        let summary = scan_gm80_resource_prefix(&data, limits, |_, _, _| {}).unwrap();
        assert_eq!(summary.extension_count, 1);
        assert_eq!(summary.end_offset as usize, data.len());
        assert_eq!(
            scan_gm80_resource_prefix(&data[..data.len() - 1], limits, |_, _, _| {}),
            Err(IndexError::Truncated)
        );
    }

    #[test]
    fn enforces_resource_prefix_budgets_and_section_tags() {
        let mut data = empty_resource_prefix(0);
        append_empty_tables(&mut data);
        let strict_strings =
            Gm80IndexLimits { max_records: 16, max_pascal_string_bytes: 0, max_compressed_asset_bytes: 0 };
        assert_eq!(scan_gm80_resource_prefix(&data, strict_strings, |_, _, _| {}).unwrap().sound_count, 0);

        let mut wrong_tag = data.clone();
        wrong_tag[28..32].copy_from_slice(&999u32.to_le_bytes());
        let limits = Gm80IndexLimits { max_records: 16, max_pascal_string_bytes: 64, max_compressed_asset_bytes: 1024 };
        assert_eq!(scan_gm80_resource_prefix(&wrong_tag, limits, |_, _, _| {}), Err(IndexError::InvalidTag));

        let mut too_many = empty_resource_prefix(0);
        push_u32(&mut too_many, 800);
        push_u32(&mut too_many, 2);
        let low_record_limit = Gm80IndexLimits { max_records: 1, ..limits };
        assert_eq!(
            scan_gm80_resource_prefix(&too_many, low_record_limit, |_, _, _| {}),
            Err(IndexError::TooManyEntries)
        );
    }

    #[test]
    fn scans_remaining_gm80_sections_without_decoding_asset_blobs() {
        let mut data = Vec::new();
        for bytes in [
            b"spr".as_slice(),
            b"bg".as_slice(),
            b"path".as_slice(),
            b"script".as_slice(),
            b"font".as_slice(),
            b"timeline".as_slice(),
            b"object".as_slice(),
            b"room".as_slice(),
        ] {
            push_u32(&mut data, 800);
            push_u32(&mut data, 1);
            push_u32(&mut data, bytes.len() as u32);
            data.extend_from_slice(bytes);
        }
        push_u32(&mut data, 123); // last instance ID
        push_u32(&mut data, 456); // last tile ID
        push_u32(&mut data, 800);
        push_u32(&mut data, 1);
        push_u32(&mut data, 3);
        data.extend_from_slice(b"inc");
        push_u32(&mut data, 800); // help dialog
        push_u32(&mut data, 0); // empty compressed blob
        push_u32(&mut data, 500); // action library init
        push_u32(&mut data, 1);
        push_pascal(&mut data, b"init");
        push_u32(&mut data, 700); // room order
        push_u32(&mut data, 2);
        push_u32(&mut data, 0);
        push_u32(&mut data, 1);

        let mut reported = Vec::new();
        let limits = Gm80IndexLimits { max_records: 32, max_pascal_string_bytes: 16, max_compressed_asset_bytes: 64 };
        let summary = scan_gm80_resource_tail(&data, 0, limits, |kind, index, span| {
            reported.push((kind, index, span));
        })
        .unwrap();
        assert_eq!(reported.len(), 9);
        assert_eq!(reported.iter().map(|entry| entry.0).collect::<Vec<_>>(), [
            Gm80AssetKind::Sprite,
            Gm80AssetKind::Background,
            Gm80AssetKind::Path,
            Gm80AssetKind::Script,
            Gm80AssetKind::Font,
            Gm80AssetKind::Timeline,
            Gm80AssetKind::Object,
            Gm80AssetKind::Room,
            Gm80AssetKind::IncludedFile,
        ],);
        assert_eq!(summary.sprite_count, 1);
        assert_eq!(summary.room_count, 1);
        assert_eq!(summary.included_file_count, 1);
        assert_eq!(summary.last_instance_id, 123);
        assert_eq!(summary.last_tile_id, 456);
        assert_eq!(summary.room_order_count, 2);
        assert_eq!(summary.records_used, 12);
        assert_eq!(summary.end_offset as usize, data.len());
        let remaining_budget = Gm80IndexLimits { max_records: summary.records_used - 1, ..limits };
        assert_eq!(scan_gm80_resource_tail(&data, 0, remaining_budget, |_, _, _| {}), Err(IndexError::TooManyEntries),);
        assert_eq!(
            scan_gm80_resource_tail(&data[..data.len() - 1], 0, limits, |_, _, _| {}),
            Err(IndexError::Truncated),
        );
    }

    #[test]
    fn full_directory_scan_shares_budget_between_prefix_and_tail() {
        let mut data = empty_resource_prefix(0);
        push_u32(&mut data, 800); // triggers
        push_u32(&mut data, 1);
        push_u32(&mut data, 1);
        data.push(0xaa);
        push_u32(&mut data, 800); // constants
        push_u32(&mut data, 0);
        push_u32(&mut data, 800); // sounds
        push_u32(&mut data, 0);
        let prefix_end = data.len();

        push_u32(&mut data, 800); // one sprite in the tail
        push_u32(&mut data, 1);
        push_u32(&mut data, 1);
        data.push(0xbb);
        for _ in 1..8 {
            push_u32(&mut data, 800);
            push_u32(&mut data, 0);
        }
        push_u32(&mut data, 0); // last instance id
        push_u32(&mut data, 0); // last tile id
        push_u32(&mut data, 800); // included files
        push_u32(&mut data, 0);
        push_u32(&mut data, 800); // help data
        push_u32(&mut data, 0);
        push_u32(&mut data, 500); // initialization strings
        push_u32(&mut data, 0);
        push_u32(&mut data, 700); // room order
        push_u32(&mut data, 0);

        let limits = Gm80IndexLimits { max_records: 2, max_pascal_string_bytes: 16, max_compressed_asset_bytes: 16 };
        let mut reported = Vec::new();
        let summary = scan_gm80_resource_directory(&data, limits, |kind, index, span| {
            reported.push((kind, index, span));
        })
        .unwrap();
        assert_eq!(summary.prefix.end_offset as usize, prefix_end);
        assert_eq!(summary.prefix.records_used, 1);
        assert_eq!(summary.tail.records_used, 1);
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].0, Gm80AssetKind::Trigger);
        assert_eq!(reported[1].0, Gm80AssetKind::Sprite);

        let too_small = Gm80IndexLimits { max_records: 1, ..limits };
        assert_eq!(
            scan_gm80_resource_directory(&data, too_small, |_, _, _| {}),
            Err(Gm80DirectoryError::Tail(IndexError::TooManyEntries))
        );
    }

    #[test]
    fn locates_decrypt_record_after_settings_and_embedded_dll() {
        let mut exe = vec![0u8; 64];
        exe[4..8].copy_from_slice(&3u32.to_le_bytes());
        exe[8..11].copy_from_slice(b"cfg");
        exe[11..15].copy_from_slice(&2u32.to_le_bytes());
        exe[15..17].copy_from_slice(b"nm");
        exe[17..21].copy_from_slice(&5u32.to_le_bytes());
        assert_eq!(locate_standard_gm80_decrypt_payload(&exe, 4), Ok(26));
        assert_eq!(locate_standard_gm80_decrypt_payload_at(exe.as_mut_slice(), 4), Ok(26));

        exe[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(locate_standard_gm80_decrypt_payload(&exe, 4), Err(DecryptError::TruncatedHeader));
        assert_eq!(locate_standard_gm80_decrypt_payload_at(exe.as_mut_slice(), 4), Err(ProbeError::OutOfBounds));
    }
}
