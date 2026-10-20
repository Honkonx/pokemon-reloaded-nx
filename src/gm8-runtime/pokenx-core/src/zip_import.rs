//! Bounded ZIP central-directory validation for the Switch importer.
//!
//! Validates ZIP metadata and streams extraction into a dedicated staging tree.
//! ZIP64, encryption, symlinks, and unsupported compression methods are rejected.

extern crate alloc;

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(feature = "nx-sd")]
use gm8_format_probe::ReadAt;

// The Switch app requests a 256 MiB heap. Parsing also retains each entry and
// several path indexes, so keep the number of records low enough to leave room
// for the UI, inflater, and runtime alongside the bounded central directory.
pub const MAX_ARCHIVE_ENTRIES: usize = 1_024;
pub const MAX_CENTRAL_DIRECTORY: usize = 16 * 1024 * 1024;
pub const MAX_PATH_BYTES: usize = 1024;
pub const MAX_ENTRY_UNCOMPRESSED: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipError {
    MissingEndRecord,
    InvalidEndRecord,
    UnsupportedZip64,
    CentralDirectoryTooLarge,
    InvalidCentralDirectory,
    TooManyEntries,
    InvalidName,
    UnsafePath,
    DuplicatePath,
    UnsupportedEntry,
    EntryTooLarge,
    TotalTooLarge,
    Io,
    Integrity,
    InvalidGameLayout,
    InsufficientSpace,
    InstallFailed,
    Cancelled,
}

const MAX_VALIDATED_GM80_DATA_BYTES: u64 = 64 * 1024 * 1024;
// The decrypted payload limit alone is not enough: the GM8.0 decrypt record
// also contains two attacker-controlled padding lengths before the payload.
// Bound the complete temporary allocation to keep malformed EXEs from asking
// the 256 MiB Switch applet heap for gigabytes.
const MAX_VALIDATED_GM80_RECORD_BYTES: u64 = MAX_VALIDATED_GM80_DATA_BYTES + 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Normalized, relative path with `/` separators.
    pub path: String,
    pub flags: u16,
    pub method: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub local_header_offset: u32,
    pub is_directory: bool,
}

/// Minimal random-access ZIP input, implemented by `nx::fs::FileAccessor` on Switch.
pub trait ZipReader {
    fn len(&mut self) -> Result<u64, ZipError>;
    fn seek_abs(&mut self, offset: u64) -> Result<(), ZipError>;
    fn read_some(&mut self, out: &mut [u8]) -> Result<usize, ZipError>;
}

/// A sink rooted at one private staging directory. Implementations must never
/// interpret archive paths as absolute and must only clean the staging root.
pub trait ZipSink {
    fn prepare_staging(&mut self, required_bytes: u64) -> Result<(), ZipError>;
    fn make_directory(&mut self, relative_path: &str) -> Result<(), ZipError>;
    fn begin_file(&mut self, relative_path: &str) -> Result<(), ZipError>;
    fn write_file(&mut self, bytes: &[u8]) -> Result<(), ZipError>;
    fn finish_file(&mut self) -> Result<(), ZipError>;
    /// Validate the extracted application layout before replacing `game/`.
    fn validate_staging_game(&mut self) -> Result<(), ZipError>;
    fn clear_staging(&mut self);
    /// Atomically swaps staging into `game/`; preserve an existing backup and
    /// roll back the old game if promotion fails.
    fn install_staging(&mut self) -> Result<(), ZipError>;
}

/// UI-owned control surface for a cancellable import. Progress counts bytes
/// successfully written to staging, not estimates or elapsed time.
pub trait ImportControl {
    fn cancelled(&self) -> bool;
    fn set_total(&self, _bytes: u64) {}
    fn advanced(&self, _bytes: u64) {}
}

#[cfg(test)]
struct NoControl;
#[cfg(test)]
impl ImportControl for NoControl {
    fn cancelled(&self) -> bool {
        false
    }
}

/// Validate a ZIP archive's EOCD and central directory.
///
/// `tail` must contain the final `min(file_size, 65557)` bytes of the archive,
/// and `central` must contain exactly the range described by the EOCD. The
/// caller obtains both using random-access reads from the SD file. This keeps
/// the full archive out of RAM and gives this parser a small, testable input.
pub fn parse_directory(tail: &[u8], central: &[u8]) -> Result<Vec<Entry>, ZipError> {
    let eocd = find_eocd(tail)?;
    let disk = u16le(tail, eocd + 4)?;
    let central_disk = u16le(tail, eocd + 6)?;
    let entries_on_disk = u16le(tail, eocd + 8)?;
    let entry_count = u16le(tail, eocd + 10)? as usize;
    let central_size = u32le(tail, eocd + 12)? as usize;
    let central_offset = u32le(tail, eocd + 16)?;
    let comment_len = u16le(tail, eocd + 20)? as usize;

    if disk != 0 || central_disk != 0 || entries_on_disk as usize != entry_count {
        return Err(ZipError::UnsupportedEntry);
    }
    if entry_count == u16::MAX as usize
        || central_size == u32::MAX as usize
        || central_offset == u32::MAX
    {
        return Err(ZipError::UnsupportedZip64);
    }
    if eocd + 22 + comment_len != tail.len() {
        return Err(ZipError::InvalidEndRecord);
    }
    if central_size > MAX_CENTRAL_DIRECTORY {
        return Err(ZipError::CentralDirectoryTooLarge);
    }
    if central.len() != central_size {
        return Err(ZipError::InvalidCentralDirectory);
    }
    if entry_count > MAX_ARCHIVE_ENTRIES {
        return Err(ZipError::TooManyEntries);
    }

    let mut entries: Vec<Entry> = Vec::with_capacity(entry_count);
    let mut seen_paths = BTreeSet::new();
    let mut file_paths = BTreeSet::new();
    let mut directory_paths = BTreeSet::new();
    let mut cursor = 0usize;
    let mut total_uncompressed = 0u64;
    while cursor < central.len() {
        if u32le(central, cursor)? != 0x0201_4b50 {
            return Err(ZipError::InvalidCentralDirectory);
        }
        let flags = u16le(central, cursor + 8)?;
        let method = u16le(central, cursor + 10)?;
        let crc32 = u32le(central, cursor + 16)?;
        let compressed_size = u32le(central, cursor + 20)?;
        let uncompressed_size = u32le(central, cursor + 24)?;
        let name_len = u16le(central, cursor + 28)? as usize;
        let extra_len = u16le(central, cursor + 30)? as usize;
        let comment_len = u16le(central, cursor + 32)? as usize;
        let start_disk = u16le(central, cursor + 34)?;
        let external_attributes = u32le(central, cursor + 38)?;
        let local_header_offset = u32le(central, cursor + 42)?;
        let record_len = 46usize
            .checked_add(name_len)
            .and_then(|n| n.checked_add(extra_len))
            .and_then(|n| n.checked_add(comment_len))
            .ok_or(ZipError::InvalidCentralDirectory)?;
        let record_end = cursor
            .checked_add(record_len)
            .ok_or(ZipError::InvalidCentralDirectory)?;
        if record_end > central.len() || start_disk != 0 {
            return Err(ZipError::InvalidCentralDirectory);
        }
        if compressed_size == u32::MAX
            || uncompressed_size == u32::MAX
            || local_header_offset == u32::MAX
        {
            return Err(ZipError::UnsupportedZip64);
        }
        // General purpose bit 0 means encrypted. Data descriptors are allowed;
        // the extractor must trust central-directory sizes and verify CRC.
        if flags & 1 != 0 || flags & !0x080e != 0 || (method != 0 && method != 8) {
            return Err(ZipError::UnsupportedEntry);
        }
        // Unix creator systems encode file type in the high external attrs.
        let unix_mode = (external_attributes >> 16) as u16;
        if unix_mode & 0o170000 == 0o120000 {
            return Err(ZipError::UnsupportedEntry);
        }
        let name_start = cursor + 46;
        let raw_name = &central[name_start..name_start + name_len];
        validate_extra_fields(&central[name_start + name_len..record_end])?;
        let path = validate_name(raw_name)?;
        let is_directory = path.ends_with('/');
        if (!is_directory && unix_mode & 0o170000 == 0o040000)
            || (is_directory && uncompressed_size != 0)
        {
            return Err(ZipError::UnsupportedEntry);
        }
        if uncompressed_size as u64 > MAX_ENTRY_UNCOMPRESSED {
            return Err(ZipError::EntryTooLarge);
        }
        total_uncompressed = total_uncompressed
            .checked_add(uncompressed_size as u64)
            .ok_or(ZipError::TotalTooLarge)?;
        if total_uncompressed > MAX_TOTAL_UNCOMPRESSED {
            return Err(ZipError::TotalTooLarge);
        }
        let key = path.trim_end_matches('/').to_ascii_lowercase();
        if !seen_paths.insert(key.clone()) || (!is_directory && directory_paths.contains(&key)) {
            return Err(ZipError::DuplicatePath);
        }
        let mut ancestor_end = 0;
        while let Some(next) = key[ancestor_end..].find('/') {
            ancestor_end += next;
            if file_paths.contains(&key[..ancestor_end]) {
                return Err(ZipError::DuplicatePath);
            }
            directory_paths.insert(key[..ancestor_end].into());
            ancestor_end += 1;
        }
        if is_directory {
            directory_paths.insert(key);
        } else {
            file_paths.insert(key);
        }
        entries.push(Entry {
            path,
            flags,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
            is_directory,
        });
        cursor = record_end;
    }
    if cursor != central.len() || entries.len() != entry_count {
        return Err(ZipError::InvalidCentralDirectory);
    }
    Ok(entries)
}

/// Parse, validate and stream a ZIP into a staging sink, then ask the sink to
/// promote it. The source archive is never loaded in full into memory.
#[cfg(test)]
pub fn import<R: ZipReader, S: ZipSink>(reader: &mut R, sink: &mut S) -> Result<(), ZipError> {
    import_controlled(reader, sink, &NoControl)
}

pub fn import_controlled<R: ZipReader, S: ZipSink>(
    reader: &mut R,
    sink: &mut S,
    control: &dyn ImportControl,
) -> Result<(), ZipError> {
    let (entries, central_offset, required_bytes) = read_archive_directory(reader, control)?;
    let prefix = game_root_prefix(&entries).ok_or(ZipError::InvalidGameLayout)?;
    if control.cancelled() {
        return Err(ZipError::Cancelled);
    }

    // No SD mutation happens until every central-directory entry has passed
    // validation and the archive has a plausible game layout.
    sink.prepare_staging(required_bytes)?;
    let extracted = extract_entries(reader, sink, &entries, &prefix, central_offset, control);
    if let Err(error) = extracted {
        sink.clear_staging();
        return Err(error);
    }
    // A cancel request may arrive while the final chunk is being written.
    // Honor it before validation or the install transaction starts.
    if control.cancelled() {
        sink.clear_staging();
        return Err(ZipError::Cancelled);
    }
    if let Err(error) = sink.validate_staging_game() {
        sink.clear_staging();
        return Err(error);
    }
    // Installation is a short transactional rename sequence; do not interrupt
    // between moving the old game and promoting the verified staging tree.
    if let Err(error) = sink.install_staging() {
        sink.clear_staging();
        return Err(error);
    }
    Ok(())
}

/// Extract a general archive without requiring a GameMaker layout. The sink
/// still controls the staging root and must promote it without overwriting an
/// existing destination.
pub fn extract_controlled<R: ZipReader, S: ZipSink>(
    reader: &mut R,
    sink: &mut S,
    control: &dyn ImportControl,
) -> Result<(), ZipError> {
    let (entries, central_offset, required_bytes) = read_archive_directory(reader, control)?;
    if entries.is_empty() {
        return Err(ZipError::InvalidCentralDirectory);
    }
    if control.cancelled() {
        return Err(ZipError::Cancelled);
    }
    sink.prepare_staging(required_bytes)?;
    if let Err(error) = extract_entries(reader, sink, &entries, "", central_offset, control) {
        sink.clear_staging();
        return Err(error);
    }
    // Do not promote a completed staging tree after the user requested
    // cancellation during the last write.
    if control.cancelled() {
        sink.clear_staging();
        return Err(ZipError::Cancelled);
    }
    if let Err(error) = sink.install_staging() {
        sink.clear_staging();
        return Err(error);
    }
    Ok(())
}

fn read_archive_directory<R: ZipReader>(
    reader: &mut R,
    control: &dyn ImportControl,
) -> Result<(Vec<Entry>, u64, u64), ZipError> {
    let archive_len = reader.len()?;
    let tail_len = core::cmp::min(archive_len, 65_557) as usize;
    if tail_len < 22 {
        return Err(ZipError::MissingEndRecord);
    }
    let mut tail = alloc::vec![0u8; tail_len];
    reader.seek_abs(archive_len - tail_len as u64)?;
    read_exact(reader, &mut tail)?;
    let eocd = find_eocd(&tail)?;
    let central_size = u32le(&tail, eocd + 12)? as usize;
    let central_offset = u32le(&tail, eocd + 16)? as u64;
    let central_end = central_offset
        .checked_add(central_size as u64)
        .ok_or(ZipError::InvalidCentralDirectory)?;
    let eocd_absolute = archive_len - tail_len as u64 + eocd as u64;
    if central_end > eocd_absolute {
        return Err(ZipError::InvalidCentralDirectory);
    }
    if central_size > MAX_CENTRAL_DIRECTORY {
        return Err(ZipError::InvalidCentralDirectory);
    }
    let mut central = alloc::vec![0u8; central_size];
    reader.seek_abs(central_offset)?;
    read_exact(reader, &mut central)?;
    let entries = parse_directory(&tail, &central)?;
    let required_bytes = entries
        .iter()
        .filter(|entry| !entry.is_directory)
        .try_fold(0u64, |total, entry| {
            total.checked_add(entry.uncompressed_size as u64)
        })
        .ok_or(ZipError::TotalTooLarge)?;
    control.set_total(required_bytes);
    Ok((entries, central_offset, required_bytes))
}

fn extract_entries<R: ZipReader, S: ZipSink>(
    reader: &mut R,
    sink: &mut S,
    entries: &[Entry],
    prefix: &str,
    central_offset: u64,
    control: &dyn ImportControl,
) -> Result<(), ZipError> {
    let mut input = [0u8; 8192];
    let mut output = [0u8; 8192];
    for entry in entries {
        if control.cancelled() {
            return Err(ZipError::Cancelled);
        }
        if is_archive_junk(&entry.path) {
            continue;
        }
        let relative = if prefix.is_empty() {
            entry.path.as_str()
        } else if entry.path == prefix {
            continue;
        } else {
            entry
                .path
                .strip_prefix(prefix)
                .ok_or(ZipError::InvalidGameLayout)?
        };
        let relative = relative.trim_end_matches('/');
        if relative.is_empty() {
            continue;
        }
        if entry.is_directory {
            make_parent_directories(sink, relative)?;
            sink.make_directory(relative)?;
            continue;
        }

        // Verify the local header against the central record before trusting
        // its data offset. ZIP data descriptors are ignored; central sizes are
        // authoritative, then decompressed size and CRC are verified.
        reader.seek_abs(entry.local_header_offset as u64)?;
        let mut local = [0u8; 30];
        read_exact(reader, &mut local)?;
        if u32le(&local, 0)? != 0x0403_4b50
            || u16le(&local, 6)? != entry.flags
            || u16le(&local, 8)? != entry.method
        {
            return Err(ZipError::InvalidCentralDirectory);
        }
        let local_name_len = u16le(&local, 26)? as usize;
        let local_extra_len = u16le(&local, 28)? as usize;
        if local_name_len > MAX_PATH_BYTES || local_name_len != entry.path.len() {
            return Err(ZipError::InvalidName);
        }
        let mut local_name = alloc::vec![0u8; local_name_len];
        read_exact(reader, &mut local_name)?;
        if fold_dos_name(&local_name).as_ref() != entry.path.as_bytes() {
            return Err(ZipError::InvalidCentralDirectory);
        }
        let local_crc = u32le(&local, 14)?;
        let local_compressed = u32le(&local, 18)?;
        let local_uncompressed = u32le(&local, 22)?;
        if entry.flags & 8 == 0 {
            if local_crc != entry.crc32
                || local_compressed != entry.compressed_size
                || local_uncompressed != entry.uncompressed_size
            {
                return Err(ZipError::InvalidCentralDirectory);
            }
        } else if (local_crc != 0 && local_crc != entry.crc32)
            || (local_compressed != 0 && local_compressed != entry.compressed_size)
            || (local_uncompressed != 0 && local_uncompressed != entry.uncompressed_size)
        {
            return Err(ZipError::InvalidCentralDirectory);
        }
        let data_offset = (entry.local_header_offset as u64)
            .checked_add(30 + local_name_len as u64 + local_extra_len as u64)
            .ok_or(ZipError::InvalidCentralDirectory)?;
        let data_end = data_offset
            .checked_add(entry.compressed_size as u64)
            .ok_or(ZipError::InvalidCentralDirectory)?;
        if data_end > central_offset {
            return Err(ZipError::InvalidCentralDirectory);
        }
        reader.seek_abs(data_offset)?;
        make_parent_directories(sink, relative)?;
        sink.begin_file(relative)?;
        let result = match entry.method {
            0 => stream_stored(reader, sink, entry, &mut input, control),
            8 => stream_deflate(reader, sink, entry, &mut input, &mut output, control),
            _ => Err(ZipError::UnsupportedEntry),
        };
        if result.is_err() {
            let _ = sink.finish_file();
            return result;
        }
        sink.finish_file()?;
    }
    Ok(())
}

fn make_parent_directories<S: ZipSink>(sink: &mut S, path: &str) -> Result<(), ZipError> {
    let mut pos = 0;
    while let Some(next) = path[pos..].find('/') {
        pos += next;
        sink.make_directory(&path[..pos])?;
        pos += 1;
    }
    Ok(())
}

fn stream_stored<R: ZipReader, S: ZipSink>(
    reader: &mut R,
    sink: &mut S,
    entry: &Entry,
    input: &mut [u8],
    control: &dyn ImportControl,
) -> Result<(), ZipError> {
    if entry.compressed_size != entry.uncompressed_size {
        return Err(ZipError::Integrity);
    }
    let mut remaining = entry.compressed_size as u64;
    let mut crc = !0u32;
    while remaining > 0 {
        if control.cancelled() {
            return Err(ZipError::Cancelled);
        }
        let size = core::cmp::min(remaining, input.len() as u64) as usize;
        read_exact(reader, &mut input[..size])?;
        crc = crc32_update(crc, &input[..size]);
        sink.write_file(&input[..size])?;
        control.advanced(size as u64);
        remaining -= size as u64;
    }
    let actual_crc = !crc;
    if !entry.is_directory
        && (entry.uncompressed_size != entry.compressed_size || actual_crc != entry.crc32)
    {
        return Err(ZipError::Integrity);
    }
    Ok(())
}

fn stream_deflate<R: ZipReader, S: ZipSink>(
    reader: &mut R,
    sink: &mut S,
    entry: &Entry,
    input: &mut [u8],
    output: &mut [u8],
    control: &dyn ImportControl,
) -> Result<(), ZipError> {
    use miniz_oxide::inflate::stream::{inflate, InflateState};
    use miniz_oxide::{DataFormat, MZFlush, MZStatus};

    let mut state = InflateState::new_boxed(DataFormat::Raw);
    let mut compressed_remaining = entry.compressed_size as u64;
    let mut total_out = 0u64;
    let mut crc = !0u32;
    let mut ended = false;
    while compressed_remaining > 0 {
        if control.cancelled() {
            return Err(ZipError::Cancelled);
        }
        let count = core::cmp::min(compressed_remaining, input.len() as u64) as usize;
        read_exact(reader, &mut input[..count])?;
        compressed_remaining -= count as u64;
        let last_chunk = compressed_remaining == 0;
        let mut input_pos = 0usize;
        loop {
            let flush = if last_chunk {
                MZFlush::Finish
            } else {
                MZFlush::None
            };
            let result = inflate(&mut state, &input[input_pos..count], output, flush);
            let status = result.status.map_err(|_| ZipError::Integrity)?;
            input_pos += result.bytes_consumed;
            total_out = total_out
                .checked_add(result.bytes_written as u64)
                .ok_or(ZipError::EntryTooLarge)?;
            if total_out > entry.uncompressed_size as u64 {
                return Err(ZipError::Integrity);
            }
            crc = crc32_update(crc, &output[..result.bytes_written]);
            if result.bytes_written > 0 {
                sink.write_file(&output[..result.bytes_written])?;
                control.advanced(result.bytes_written as u64);
            }
            if status == MZStatus::StreamEnd {
                if !last_chunk || input_pos != count {
                    return Err(ZipError::Integrity);
                }
                ended = true;
                break;
            }
            if input_pos == count && !last_chunk {
                break;
            }
            if result.bytes_consumed == 0 && result.bytes_written == 0 {
                return Err(ZipError::Integrity);
            }
        }
        if ended {
            break;
        }
    }
    if !ended || total_out != entry.uncompressed_size as u64 || !crc != entry.crc32 {
        return Err(ZipError::Integrity);
    }
    Ok(())
}

/// Archive metadata that tools add around the game and that is not part of it.
fn is_archive_junk(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("__MACOSX/") || file == ".DS_Store" || file.eq_ignore_ascii_case("Thumbs.db")
}

fn game_root_prefix(all_entries: &[Entry]) -> Option<String> {
    let entries: Vec<&Entry> = all_entries.iter().filter(|e| !is_archive_junk(&e.path)).collect();
    let has_root_exe = entries.iter().any(|e| {
        !e.is_directory && !e.path.contains('/') && e.path.to_ascii_lowercase().ends_with(".exe")
    });
    let has_root_data = entries
        .iter()
        .any(|e| e.path == "data/" || e.path.starts_with("data/"));
    if has_root_exe && has_root_data {
        return Some(String::new());
    }

    let first = entries
        .iter()
        .find_map(|e| e.path.split_once('/').map(|(a, _)| a))?;
    let prefix = alloc::format!("{first}/");
    if entries.iter().any(|e| !e.path.starts_with(&prefix)) {
        return None;
    }
    let mut exe = false;
    let mut data = false;
    for entry in entries {
        let rel = entry.path.strip_prefix(&prefix)?;
        if !entry.is_directory && !rel.contains('/') && rel.to_ascii_lowercase().ends_with(".exe") {
            exe = true;
        }
        if rel == "data/" || rel.starts_with("data/") {
            data = true;
        }
    }
    (exe && data).then_some(prefix)
}

/// Import a ZIP file from SD, extracting only into a private staging folder.
/// Backups are retained as `game.backup.N`; this function never deletes a
/// previous installation or backup.
#[cfg(feature = "nx-sd")]
#[cfg(test)]
pub fn import_file(zip_path: &str) -> Result<(), ZipError> {
    import_file_controlled(zip_path, &NoControl)
}

#[cfg(feature = "nx-sd")]
pub fn import_file_controlled(zip_path: &str, control: &dyn ImportControl) -> Result<(), ZipError> {
    let file =
        nx::fs::open_file(zip_path, nx::fs::FileOpenOption::Read()).map_err(|_| ZipError::Io)?;
    let mut reader = SwitchReader(file);
    let mut sink = SwitchSink { active: None };
    import_controlled(&mut reader, &mut sink, control)
}

/// Extract a ZIP into a new sibling directory named after the archive. Existing
/// destinations are preserved; files are written to a temporary tree first.
#[cfg(feature = "nx-sd")]
pub fn extract_file_controlled(
    zip_path: &str,
    parent_directory: &str,
    control: &dyn ImportControl,
) -> Result<(), ZipError> {
    if !valid_sd_path(zip_path, false) || !valid_sd_path(parent_directory, true) {
        return Err(ZipError::InvalidName);
    }
    let archive_name = zip_path.rsplit('/').next().ok_or(ZipError::InvalidName)?;
    let archive_stem = archive_name
        .get(..archive_name.len().saturating_sub(4))
        .filter(|_| archive_name.to_ascii_lowercase().ends_with(".zip"))
        .ok_or(ZipError::InvalidName)?;
    validate_name(archive_stem.as_bytes()).map_err(|_| ZipError::InvalidName)?;
    if archive_stem.is_empty() || parent_directory.is_empty() || parent_directory.contains("..") {
        return Err(ZipError::InvalidName);
    }
    let destination = if parent_directory.ends_with('/') {
        alloc::format!("{}{}", parent_directory, archive_stem)
    } else {
        alloc::format!("{}/{}", parent_directory, archive_stem)
    };
    let mut hash = !0u32;
    hash = crc32_update(hash, destination.as_bytes());
    let staging = if parent_directory.ends_with('/') {
        alloc::format!("{}.pokenx-extract-{:08x}.tmp", parent_directory, !hash)
    } else {
        alloc::format!("{}/.pokenx-extract-{:08x}.tmp", parent_directory, !hash)
    };
    if destination.len() > 768 || staging.len() > 768 || !destination.starts_with("sdmc:/") {
        return Err(ZipError::InvalidName);
    }
    let file =
        nx::fs::open_file(zip_path, nx::fs::FileOpenOption::Read()).map_err(|_| ZipError::Io)?;
    let mut reader = SwitchReader(file);
    let mut sink = GenericSwitchSink {
        staging,
        destination,
        active: None,
    };
    extract_controlled(&mut reader, &mut sink, control)
}

#[cfg(feature = "nx-sd")]
fn valid_sd_path(path: &str, allow_directory: bool) -> bool {
    if !path.starts_with("sdmc:/")
        || path.len() > 768
        || path.contains('\\')
        || path.contains('\0')
        || (!allow_directory && path.ends_with('/'))
    {
        return false;
    }
    path.strip_prefix("sdmc:/")
        .unwrap_or("")
        .split('/')
        .filter(|part| !part.is_empty())
        .all(|part| part != "." && part != "..")
}

#[cfg(feature = "nx-sd")]
struct SwitchReader(nx::fs::FileAccessor);

#[cfg(feature = "nx-sd")]
impl ZipReader for SwitchReader {
    fn len(&mut self) -> Result<u64, ZipError> {
        self.0
            .get_size()
            .map(|v| v as u64)
            .map_err(|_| ZipError::Io)
    }
    fn seek_abs(&mut self, offset: u64) -> Result<(), ZipError> {
        self.0
            .seek(nx::fs::SeekFrom::Start(offset))
            .map_err(|_| ZipError::Io)
    }
    fn read_some(&mut self, out: &mut [u8]) -> Result<usize, ZipError> {
        self.0.read_array(out).map_err(|_| ZipError::Io)
    }
}

#[cfg(feature = "nx-sd")]
struct SwitchSink {
    active: Option<nx::fs::FileAccessor>,
}

#[cfg(feature = "nx-sd")]
struct GenericSwitchSink {
    staging: String,
    destination: String,
    active: Option<nx::fs::FileAccessor>,
}

#[cfg(feature = "nx-sd")]
impl GenericSwitchSink {
    fn full_path(&self, relative: &str) -> Result<String, ZipError> {
        let path = alloc::format!("{}/{}", self.staging, relative);
        if path.len() > 768 {
            Err(ZipError::InvalidName)
        } else {
            Ok(path)
        }
    }

    fn ensure_directory(path: &str) -> Result<(), ZipError> {
        match nx::fs::get_entry_type(path) {
            Ok(nx::fs::DirectoryEntryType::Directory) => Ok(()),
            Ok(_) => Err(ZipError::Io),
            Err(_) => nx::fs::create_directory(path).map_err(|_| ZipError::Io),
        }
    }
}

#[cfg(feature = "nx-sd")]
impl ZipSink for GenericSwitchSink {
    fn prepare_staging(&mut self, required_bytes: u64) -> Result<(), ZipError> {
        if self.active.is_some()
            || nx::fs::get_entry_type(&self.destination).is_ok()
            || nx::fs::get_entry_type(&self.staging).is_ok()
        {
            return Err(ZipError::InstallFailed);
        }
        let free_bytes = nx::fs::get_free_space_size("sdmc:/").map_err(|_| ZipError::Io)? as u64;
        let required_with_margin = required_bytes
            .checked_add(1024 * 1024)
            .ok_or(ZipError::TotalTooLarge)?;
        if free_bytes < required_with_margin {
            return Err(ZipError::InsufficientSpace);
        }
        nx::fs::create_directory(&self.staging).map_err(|_| ZipError::Io)
    }

    fn make_directory(&mut self, relative_path: &str) -> Result<(), ZipError> {
        let path = self.full_path(relative_path)?;
        Self::ensure_directory(&path)
    }

    fn begin_file(&mut self, relative_path: &str) -> Result<(), ZipError> {
        if self.active.is_some() {
            return Err(ZipError::Io);
        }
        let path = self.full_path(relative_path)?;
        if nx::fs::get_entry_type(&path).is_ok() {
            return Err(ZipError::DuplicatePath);
        }
        self.active = Some(
            nx::fs::open_file(
                &path,
                nx::fs::FileOpenOption::Create() | nx::fs::FileOpenOption::Write() | nx::fs::FileOpenOption::Append(),
            )
            .map_err(|_| ZipError::Io)?,
        );
        Ok(())
    }

    fn write_file(&mut self, bytes: &[u8]) -> Result<(), ZipError> {
        self.active
            .as_mut()
            .ok_or(ZipError::Io)?
            .write_array::<u8, false>(bytes)
            .map_err(|_| ZipError::Io)
    }

    fn finish_file(&mut self) -> Result<(), ZipError> {
        let file = self.active.take().ok_or(ZipError::Io)?;
        file.flush().map_err(|_| ZipError::Io)
    }

    fn validate_staging_game(&mut self) -> Result<(), ZipError> {
        Ok(())
    }

    fn clear_staging(&mut self) {
        self.active.take();
        let _ = nx::fs::remove_dir_all(&self.staging);
    }

    fn install_staging(&mut self) -> Result<(), ZipError> {
        if nx::fs::get_entry_type(&self.destination).is_ok() {
            return Err(ZipError::InstallFailed);
        }
        nx::fs::rename_directory(&self.staging, &self.destination)
            .map_err(|_| ZipError::InstallFailed)
    }
}

#[cfg(feature = "nx-sd")]
impl SwitchSink {
    const STAGING: &'static str = "sdmc:/switch/pokenx/.game-import-staging";
    const GAME: &'static str = "sdmc:/switch/pokenx/game";

    fn full_path(relative: &str) -> String {
        alloc::format!("{}/{}", Self::STAGING, relative)
    }

    fn ensure_directory(path: &str) -> Result<(), ZipError> {
        match nx::fs::get_entry_type(path) {
            Ok(nx::fs::DirectoryEntryType::Directory) => Ok(()),
            Ok(_) => Err(ZipError::Io),
            Err(_) => nx::fs::create_directory(path).map_err(|_| ZipError::Io),
        }
    }

    fn select_backup() -> Result<String, ZipError> {
        for suffix in 1..=1000 {
            let path = alloc::format!("sdmc:/switch/pokenx/game.backup.{suffix}");
            if nx::fs::get_entry_type(&path).is_err() {
                return Ok(path);
            }
        }
        Err(ZipError::InstallFailed)
    }

    fn validate_staging_game(&mut self) -> Result<(), ZipError> {
        let mut supported_exe_found = false;
        let mut root =
            nx::fs::open_directory(Self::STAGING, nx::fs::DirectoryOpenMode::ReadFiles())
                .map_err(|_| ZipError::InvalidGameLayout)?;
        loop {
            match root.read_next() {
                Ok(Some(entry)) if entry.entry_type == nx::fs::DirectoryEntryType::File => {
                    let name = entry.name.get_str().unwrap_or("");
                    if name.to_ascii_lowercase().ends_with(".exe") {
                        let exe_path = Self::full_path(name);
                        if has_i386_pe_header(&exe_path) && verify_standard_gm80_file(&exe_path) {
                            supported_exe_found = true;
                            break;
                        }
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => return Err(ZipError::Io),
            }
        }
        if !supported_exe_found {
            return Err(ZipError::InvalidGameLayout);
        }
        let data_path = Self::full_path("data");
        let mut data = nx::fs::open_directory(
            &data_path,
            nx::fs::DirectoryOpenMode::ReadDirectories() | nx::fs::DirectoryOpenMode::ReadFiles(),
        )
        .map_err(|_| ZipError::InvalidGameLayout)?;
        if !matches!(data.read_next(), Ok(Some(_))) {
            return Err(ZipError::InvalidGameLayout);
        }
        Ok(())
    }
}

/// Loose check used only as a fallback: the file starts with the DOS `MZ` signature.
#[cfg(feature = "nx-sd")]
pub fn has_mz_header(path: &str) -> bool {
    let Ok(mut file) = nx::fs::open_file(path, nx::fs::FileOpenOption::Read()) else { return false };
    let mut head = [0u8; 2];
    file.read_array(&mut head).ok() == Some(2) && &head == b"MZ"
}

#[cfg(feature = "nx-sd")]
pub fn has_i386_pe_header(path: &str) -> bool {
    let Ok(mut file) = nx::fs::open_file(path, nx::fs::FileOpenOption::Read()) else {
        return false;
    };
    let mut dos_header = [0u8; 64];
    if file.read_array(&mut dos_header).ok() != Some(dos_header.len()) {
        return false;
    }
    let Ok(pe_offset) = switch_pe_probe::read_pe_offset(&dos_header) else {
        return false;
    };
    if pe_offset < dos_header.len() as u32 {
        return false;
    }
    let Ok(file_size) = file.get_size() else {
        return false;
    };
    let Some(header_end) = (pe_offset as usize).checked_add(24) else {
        return false;
    };
    if header_end > file_size {
        return false;
    }
    if file
        .seek(nx::fs::SeekFrom::Start(pe_offset as u64))
        .is_err()
    {
        return false;
    }
    let mut pe_header = [0u8; 24];
    if file.read_array(&mut pe_header).ok() != Some(pe_header.len()) {
        return false;
    }
    switch_pe_probe::parse_pe_header_parts(pe_offset, &dos_header, &pe_header).is_ok()
}

/// Bounded container identification for both the menu and the runtime loader.
/// Does not inflate resources or treat arbitrary MZ/PE files as GameMaker games.
#[cfg(feature = "nx-sd")]
pub fn is_standard_gm80_container(path: &str) -> bool {
    let Ok(mut file) = nx::fs::open_file(path, nx::fs::FileOpenOption::Read()) else { return false };
    let Ok(len) = file.get_size() else { return false };
    let mut reader = SwitchFileReadAt { file: &mut file, len: len as u64 };
    matches!(gm8_format_probe::detect_standard_gm80_executable(&mut reader), Ok(Some(_)))
}

/// One-line description of what the container probe sees in `path`, for the menu log.
#[cfg(feature = "nx-sd")]
pub fn probe_report(path: &str) -> alloc::string::String {
    let Ok(mut file) = nx::fs::open_file(path, nx::fs::FileOpenOption::Read()) else {
        return alloc::string::String::from("no se pudo abrir");
    };
    let Ok(len) = file.get_size() else { return alloc::string::String::from("no se pudo leer el tamano") };
    let mut reader = SwitchFileReadAt { file: &mut file, len: len as u64 };
    alloc::format!("tamano {} bytes; sonda {:?}", len, gm8_format_probe::detect_standard_gm80_executable(&mut reader))
}

/// Read and decrypt the standard GM8.0 resource block. The full PE and
/// settings/DLL prelude are accessed by offset; the encrypted resource block
/// and its padding are read into temporary memory, capped at 80 MiB total.
#[cfg(feature = "nx-sd")]
fn decrypted_standard_gm80_payload(path: &str) -> Option<(Vec<u8>, usize, usize)> {
    let Ok(mut file) = nx::fs::open_file(path, nx::fs::FileOpenOption::Read()) else {
        return None;
    };
    let Ok(file_size) = file.get_size() else {
        return None;
    };
    if file_size == 0 {
        return None;
    }
    let mut reader = SwitchFileReadAt {
        file: &mut file,
        len: file_size as u64,
    };
    let header = match gm8_format_probe::detect_standard_gm80(&mut reader) {
        Ok(Some(header)) => header,
        _ => return None,
    };
    let Ok(record_offset) = gm8_format_probe::locate_standard_gm80_decrypt_payload_at(
        &mut reader,
        header.payload_offset,
    ) else {
        return None;
    };

    let mut record_header = [0u8; 8];
    if reader
        .read_exact_at(record_offset, &mut record_header)
        .is_err()
    {
        return None;
    }
    let garbage_1 = u32::from_le_bytes(record_header[..4].try_into().unwrap()) as u64;
    let garbage_2 = u32::from_le_bytes(record_header[4..].try_into().unwrap()) as u64;
    let Some(encrypted_len_offset) = gm80_encrypted_len_offset(record_offset, garbage_1, garbage_2)
    else {
        return None;
    };
    let mut len_bytes = [0u8; 4];
    if reader
        .read_exact_at(encrypted_len_offset, &mut len_bytes)
        .is_err()
    {
        return None;
    }
    let encrypted_len = u32::from_le_bytes(len_bytes) as u64;
    if encrypted_len < 2 || encrypted_len > MAX_VALIDATED_GM80_DATA_BYTES {
        return None;
    }
    let Some(record_size) = gm80_record_size(
        record_offset,
        encrypted_len_offset,
        encrypted_len,
        file_size as u64,
    ) else {
        return None;
    };
    let mut record = Vec::new();
    if record.try_reserve_exact(record_size).is_err() {
        return None;
    }
    record.resize(record_size, 0);
    if reader.read_exact_at(record_offset, &mut record).is_err() {
        return None;
    }
    let Ok(payload) = gm8_format_probe::decrypt_standard_gm80_payload(&mut record, 0) else {
        return None;
    };
    let Some(payload_end) = payload.data_offset.checked_add(payload.encrypted_data_len) else {
        return None;
    };
    if record.get(payload.data_offset..payload_end).is_none() {
        return None;
    }
    Some((record, payload.data_offset, payload_end))
}

fn gm80_encrypted_len_offset(record_offset: u64, garbage_1: u64, garbage_2: u64) -> Option<u64> {
    record_offset
        .checked_add(8)?
        .checked_add(garbage_1.checked_mul(4)?)?
        .checked_add(256)?
        .checked_add(garbage_2.checked_mul(4)?)
}

fn gm80_record_size(
    record_offset: u64,
    encrypted_len_offset: u64,
    encrypted_len: u64,
    file_size: u64,
) -> Option<usize> {
    if encrypted_len < 2 || encrypted_len > MAX_VALIDATED_GM80_DATA_BYTES {
        return None;
    }
    let record_len = encrypted_len_offset
        .checked_sub(record_offset)?
        .checked_add(4)?
        .checked_add(encrypted_len)?;
    if record_len > MAX_VALIDATED_GM80_RECORD_BYTES
        || record_offset.checked_add(record_len)? > file_size
    {
        return None;
    }
    usize::try_from(record_len).ok()
}

#[cfg(feature = "nx-sd")]
pub fn verify_standard_gm80_file(path: &str) -> bool {
    let Some((record, start, end)) = decrypted_standard_gm80_payload(path) else {
        return false;
    };
    let decrypted_payload = &record[start..end];
    let limits = gm8_format_probe::Gm80IndexLimits {
        max_records: 262_144,
        max_pascal_string_bytes: 1024 * 1024,
        max_compressed_asset_bytes: MAX_VALIDATED_GM80_DATA_BYTES as u32,
    };
    gm8_assets::Gm80AssetIndex::load(
        decrypted_payload,
        gm8_assets::Limits {
            directory: limits,
            max_indexed_assets: 65_536,
        },
    )
    .is_ok()
}

/// Parse the indexed Sound 800 table for an optional runtime diagnostic.
/// This remains separate from installation validation so unusual sound names
/// or a parser edge case cannot prevent an otherwise valid game import.
#[cfg(feature = "nx-sd")]
pub fn inspect_standard_gm80_sounds(path: &str) -> Option<(u32, u32)> {
    let Some((record, start, end)) = decrypted_standard_gm80_payload(path) else {
        return None;
    };
    let limits = gm8_sound::SoundTableLimits {
        index: gm8_format_probe::Gm80IndexLimits {
            max_records: 262_144,
            max_pascal_string_bytes: 1024 * 1024,
            max_compressed_asset_bytes: MAX_VALIDATED_GM80_DATA_BYTES as u32,
        },
        record: gm8_sound::Limits::default(),
        max_sound_records: 4096,
        max_total_output_bytes: 64 * 1024 * 1024,
    };
    let table = gm8_sound::load_gm80_sound_table(&record[start..end], limits).ok()?;
    Some((table.sounds.len() as u32, table.deleted_count))
}

/// Parse indexed Background, Sprite, Script, Object, Room and Font tables for a
/// bounded resource diagnostic. Script source is syntax-parsed and tested
/// against the portable scalar bytecode slice; it is not OpenGMK bytecode and
/// is never executed by this diagnostic.
#[cfg(feature = "nx-sd")]
pub fn inspect_standard_gm80_graphics(
    path: &str,
) -> Option<(
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
)> {
    let Some((record, start, end)) = decrypted_standard_gm80_payload(path) else {
        return None;
    };
    let limits = gm8_background::BackgroundTableLimits {
        index: gm8_format_probe::Gm80IndexLimits {
            max_records: 262_144,
            max_pascal_string_bytes: 1024 * 1024,
            max_compressed_asset_bytes: MAX_VALIDATED_GM80_DATA_BYTES as u32,
        },
        record: gm8_background::Limits::default(),
        max_background_records: 4096,
        max_total_output_bytes: 64 * 1024 * 1024,
    };
    let payload = &record[start..end];
    let resource_limits = gm8_resource_loader::Limits {
        directory: limits.index,
        max_indexed_assets: 65_536,
        max_sprite_records: 4096,
        max_background_records: 4096,
        max_total_owned_bytes: 64 * 1024 * 1024,
        ..gm8_resource_loader::Limits::default()
    };
    let backgrounds = gm8_resource_loader::load_gm80_background_resources(
        payload,
        resource_limits,
    )
    .ok()?;
    let background_count = backgrounds.records.len() as u32;
    let deleted_background_count = backgrounds.deleted_records;
    drop(backgrounds);
    let sprites = gm8_resource_loader::load_gm80_sprite_resources(payload, resource_limits).ok()?;
    let frame_count = sprites.records.iter().try_fold(0u32, |total, entry| {
        total.checked_add(entry.sprite.frames.len() as u32)
    })?;
    let sprite_count = sprites.records.len() as u32;
    let deleted_sprite_count = sprites.deleted_records;
    drop(sprites);
    let paths = gm8_path::load_gm80_path_table(
        payload,
        gm8_path::TableLimits {
            index: limits.index,
            record: gm8_path::Limits::default(),
            max_paths: 4096,
            max_total_output_bytes: 32 * 1024 * 1024,
        },
    )
    .ok()?;
    let path_count = paths.paths.len() as u32;
    drop(paths);
    let scripts = gm8_script::load_gm80_script_table(
        payload,
        gm8_script::TableLimits {
            index: limits.index,
            record: gm8_script::Limits::default(),
            max_records: 8192,
            max_total_output_bytes: 64 * 1024 * 1024,
        },
    )
    .ok()?;
    let script_count = scripts.scripts.len() as u32;
    let syntax = gm8_script::validate_gml_syntax(&scripts.scripts);
    let subset = gm8_script::compile_gml_subset(&scripts.scripts);
    drop(scripts);
    let objects = gm8_object::load_gm80_object_table(
        payload,
        gm8_object::TableLimits {
            index: limits.index,
            record: gm8_object::Limits::default(),
            max_objects: 4096,
            max_total_output_bytes: 64 * 1024 * 1024,
        },
    )
    .ok()?;
    let action_count = objects.objects.iter().try_fold(0u32, |total, entry| {
        entry
            .object
            .events
            .iter()
            .try_fold(total, |total, event_list| {
                event_list.iter().try_fold(total, |total, event| {
                    total.checked_add(event.actions.len() as u32)
                })
            })
    })?;
    let object_count = objects.objects.len() as u32;
    drop(objects);
    let rooms = gm8_room::load_gm80_room_table(
        payload,
        gm8_room::TableLimits {
            index: limits.index,
            record: gm8_room::Limits::default(),
            max_rooms: 4096,
            max_total_string_bytes: 64 * 1024 * 1024,
        },
    )
    .ok()?;
    let room_count = rooms.rooms.len() as u32;
    drop(rooms);
    let fonts = gm8_font::load_gm80_font_table(
        payload,
        gm8_font::TableLimits {
            index: limits.index,
            record: gm8_font::Limits::default(),
            max_records: 4096,
            max_total_atlas_bytes: 64 * 1024 * 1024,
        },
    )
    .ok()?;
    let font_count = fonts.fonts.len() as u32;
    drop(fonts);
    let timelines = gm8_timeline::load_gm80_timeline_table(
        payload,
        gm8_timeline::TableLimits {
            index: limits.index,
            record: gm8_timeline::Limits::default(),
            max_timelines: 4096,
            max_total_actions: 262_144,
        },
    )
    .ok()?;
    let timeline_count = timelines.timelines.len() as u32;
    Some((
        background_count,
        deleted_background_count,
        sprite_count,
        deleted_sprite_count,
        frame_count,
        script_count,
        object_count,
        action_count,
        room_count,
        path_count,
        font_count,
        syntax.accepted,
        syntax.rejected,
        subset.compiled,
        subset.unsupported,
        timeline_count,
    ))
}

#[cfg(feature = "nx-sd")]
struct SwitchFileReadAt<'a> {
    file: &'a mut nx::fs::FileAccessor,
    len: u64,
}

#[cfg(feature = "nx-sd")]
impl gm8_format_probe::ReadAt for SwitchFileReadAt<'_> {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_exact_at(
        &mut self,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), gm8_format_probe::ProbeError> {
        let end = offset
            .checked_add(out.len() as u64)
            .ok_or(gm8_format_probe::ProbeError::OutOfBounds)?;
        if end > self.len {
            return Err(gm8_format_probe::ProbeError::OutOfBounds);
        }
        self.file
            .seek(nx::fs::SeekFrom::Start(offset))
            .map_err(|_| gm8_format_probe::ProbeError::InvalidRead)?;
        let mut read = 0;
        while read < out.len() {
            let count = self
                .file
                .read_array(&mut out[read..])
                .map_err(|_| gm8_format_probe::ProbeError::InvalidRead)?;
            if count == 0 || count > out.len() - read {
                return Err(gm8_format_probe::ProbeError::InvalidRead);
            }
            read += count;
        }
        Ok(())
    }
}

#[cfg(feature = "nx-sd")]
impl ZipSink for SwitchSink {
    fn prepare_staging(&mut self, required_bytes: u64) -> Result<(), ZipError> {
        if self.active.is_some() {
            return Err(ZipError::Io);
        }
        match nx::fs::get_entry_type(Self::STAGING) {
            Ok(nx::fs::DirectoryEntryType::Directory) => {
                nx::fs::remove_dir_all(Self::STAGING).map_err(|_| ZipError::Io)?;
            }
            Ok(_) => nx::fs::remove_file(Self::STAGING).map_err(|_| ZipError::Io)?,
            Err(_) => {}
        }
        Self::ensure_directory("sdmc:/switch/pokenx")?;
        let free_bytes =
            nx::fs::get_free_space_size("sdmc:/switch/pokenx").map_err(|_| ZipError::Io)? as u64;
        let required_with_margin = required_bytes
            .checked_add(1024 * 1024)
            .ok_or(ZipError::TotalTooLarge)?;
        if free_bytes < required_with_margin {
            return Err(ZipError::InsufficientSpace);
        }
        nx::fs::create_directory(Self::STAGING).map_err(|_| ZipError::Io)
    }

    fn make_directory(&mut self, relative_path: &str) -> Result<(), ZipError> {
        let path = Self::full_path(relative_path);
        Self::ensure_directory(&path)
    }

    fn begin_file(&mut self, relative_path: &str) -> Result<(), ZipError> {
        if self.active.is_some() {
            return Err(ZipError::Io);
        }
        let path = Self::full_path(relative_path);
        self.active = Some(
            nx::fs::open_file(
                &path,
                nx::fs::FileOpenOption::Create() | nx::fs::FileOpenOption::Write() | nx::fs::FileOpenOption::Append(),
            )
            .map_err(|_| ZipError::Io)?,
        );
        Ok(())
    }

    fn write_file(&mut self, bytes: &[u8]) -> Result<(), ZipError> {
        self.active
            .as_mut()
            .ok_or(ZipError::Io)?
            .write_array::<u8, false>(bytes)
            .map_err(|_| ZipError::Io)
    }

    fn finish_file(&mut self) -> Result<(), ZipError> {
        let file = self.active.take().ok_or(ZipError::Io)?;
        file.flush().map_err(|_| ZipError::Io)
    }

    fn validate_staging_game(&mut self) -> Result<(), ZipError> {
        SwitchSink::validate_staging_game(self)
    }

    fn clear_staging(&mut self) {
        self.active.take();
        let _ = nx::fs::remove_dir_all(Self::STAGING);
    }

    fn install_staging(&mut self) -> Result<(), ZipError> {
        let game_exists = matches!(
            nx::fs::get_entry_type(Self::GAME),
            Ok(nx::fs::DirectoryEntryType::Directory)
        );
        if !game_exists {
            return nx::fs::rename_directory(Self::STAGING, Self::GAME)
                .map_err(|_| ZipError::InstallFailed);
        }
        let backup = Self::select_backup()?;
        nx::fs::rename_directory(Self::GAME, &backup).map_err(|_| ZipError::InstallFailed)?;
        if nx::fs::rename_directory(Self::STAGING, Self::GAME).is_err() {
            let _ = nx::fs::rename_directory(&backup, Self::GAME);
            return Err(ZipError::InstallFailed);
        }
        Ok(())
    }
}

fn read_exact<R: ZipReader>(reader: &mut R, out: &mut [u8]) -> Result<(), ZipError> {
    let mut cursor = 0;
    while cursor < out.len() {
        let count = reader.read_some(&mut out[cursor..])?;
        if count == 0 || count > out.len() - cursor {
            return Err(ZipError::Io);
        }
        cursor += count;
    }
    Ok(())
}

fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    crc
}

fn validate_extra_fields(extra: &[u8]) -> Result<(), ZipError> {
    let mut cursor = 0usize;
    while cursor < extra.len() {
        let id = u16le(extra, cursor)?;
        let len = u16le(extra, cursor + 2)? as usize;
        let end = cursor
            .checked_add(4)
            .and_then(|n| n.checked_add(len))
            .ok_or(ZipError::InvalidCentralDirectory)?;
        if end > extra.len() {
            return Err(ZipError::InvalidCentralDirectory);
        }
        if id == 0x0001 {
            return Err(ZipError::UnsupportedZip64);
        }
        cursor = end;
    }
    Ok(())
}

fn find_eocd(tail: &[u8]) -> Result<usize, ZipError> {
    if tail.len() < 22 {
        return Err(ZipError::MissingEndRecord);
    }
    for i in (0..=tail.len() - 22).rev() {
        if tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            let comment = u16le(tail, i + 20)? as usize;
            if i + 22 + comment == tail.len() {
                return Ok(i);
            }
        }
    }
    Err(ZipError::MissingEndRecord)
}

/// Zips made with a DOS code page store accents as single bytes (`0xA1` for the "i" of
/// "Especificas") without the UTF-8 flag. Names that are not valid UTF-8 are folded byte for byte to
/// the plain ASCII letter, so lengths and offsets stay the same; valid UTF-8 names are untouched.
/// Same mapping as the Android importer (`GameRepository.kt`).
fn fold_dos_name(raw: &[u8]) -> alloc::borrow::Cow<'_, [u8]> {
    if core::str::from_utf8(raw).is_ok() {
        return alloc::borrow::Cow::Borrowed(raw);
    }
    let folded = raw
        .iter()
        .map(|&b| match b {
            0..=0x7F => b,
            0x82 | 0x90 => b'e',
            0xA0 | 0xB5 => b'a',
            0xA1 | 0xD6 => b'i',
            0xA2 | 0xE0 => b'o',
            0xA3 | 0xE9 | 0x81 | 0x9A => b'u',
            0xA4 | 0xA5 => b'n',
            _ => b'_',
        })
        .collect();
    alloc::borrow::Cow::Owned(folded)
}

fn validate_name(raw: &[u8]) -> Result<String, ZipError> {
    if raw.is_empty() || raw.contains(&0) || raw.contains(&b'\\') || raw[0] == b'/' {
        return Err(ZipError::UnsafePath);
    }
    if raw.len() > MAX_PATH_BYTES {
        return Err(ZipError::InvalidName);
    }
    let folded = fold_dos_name(raw);
    let name = core::str::from_utf8(&folded).map_err(|_| ZipError::InvalidName)?;
    if name.as_bytes().get(1) == Some(&b':') {
        return Err(ZipError::UnsafePath);
    }
    let is_dir = name.ends_with('/');
    let mut normalized = String::new();
    for (i, part) in name.split('/').enumerate() {
        if part.is_empty() {
            if is_dir && i == name.split('/').count() - 1 {
                continue;
            }
            return Err(ZipError::UnsafePath);
        }
        if part == "." || part == ".." {
            return Err(ZipError::UnsafePath);
        }
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(part);
    }
    if is_dir {
        normalized.push('/');
    }
    if normalized.is_empty() {
        return Err(ZipError::UnsafePath);
    }
    Ok(normalized)
}

fn u16le(data: &[u8], at: usize) -> Result<u16, ZipError> {
    let bytes = data
        .get(at..at + 2)
        .ok_or(ZipError::InvalidCentralDirectory)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32le(data: &[u8], at: usize) -> Result<u32, ZipError> {
    let bytes = data
        .get(at..at + 4)
        .ok_or(ZipError::InvalidCentralDirectory)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemReader {
        data: Vec<u8>,
        pos: usize,
    }
    impl ZipReader for MemReader {
        fn len(&mut self) -> Result<u64, ZipError> {
            Ok(self.data.len() as u64)
        }
        fn seek_abs(&mut self, offset: u64) -> Result<(), ZipError> {
            self.pos = offset as usize;
            if self.pos <= self.data.len() {
                Ok(())
            } else {
                Err(ZipError::Io)
            }
        }
        fn read_some(&mut self, out: &mut [u8]) -> Result<usize, ZipError> {
            let count = core::cmp::min(out.len(), self.data.len().saturating_sub(self.pos));
            out[..count].copy_from_slice(&self.data[self.pos..self.pos + count]);
            self.pos += count;
            Ok(count)
        }
    }

    #[derive(Default)]
    struct MemSink {
        current: Vec<u8>,
        files: Vec<(String, Vec<u8>)>,
        prepared: bool,
        installed: bool,
        cleared: bool,
    }
    impl ZipSink for MemSink {
        fn prepare_staging(&mut self, _required_bytes: u64) -> Result<(), ZipError> {
            self.prepared = true;
            Ok(())
        }
        fn make_directory(&mut self, _path: &str) -> Result<(), ZipError> {
            Ok(())
        }
        fn begin_file(&mut self, path: &str) -> Result<(), ZipError> {
            self.files.push((String::from(path), Vec::new()));
            Ok(())
        }
        fn write_file(&mut self, bytes: &[u8]) -> Result<(), ZipError> {
            self.files
                .last_mut()
                .ok_or(ZipError::Io)?
                .1
                .extend_from_slice(bytes);
            Ok(())
        }
        fn finish_file(&mut self) -> Result<(), ZipError> {
            Ok(())
        }
        fn validate_staging_game(&mut self) -> Result<(), ZipError> {
            let exe = self
                .files
                .iter()
                .find(|(path, _)| {
                    !path.contains('/') && path.to_ascii_lowercase().ends_with(".exe")
                })
                .ok_or(ZipError::InvalidGameLayout)?;
            if exe.1.len() < 88
                || switch_pe_probe::parse_pe_header_parts(64, &exe.1[..64], &exe.1[64..88]).is_err()
            {
                return Err(ZipError::InvalidGameLayout);
            }
            if !self.files.iter().any(|(path, _)| path.starts_with("data/")) {
                return Err(ZipError::InvalidGameLayout);
            }
            Ok(())
        }
        fn clear_staging(&mut self) {
            self.files.clear();
            self.cleared = true;
        }
        fn install_staging(&mut self) -> Result<(), ZipError> {
            self.installed = true;
            Ok(())
        }
    }

    fn crc(bytes: &[u8]) -> u32 {
        !crc32_update(!0u32, bytes)
    }

    #[test]
    fn gm80_record_allocation_is_bounded_and_inside_the_executable() {
        let record_offset = 100u64;
        let encrypted_len_offset = gm80_encrypted_len_offset(record_offset, 2, 3).unwrap();
        let file_size = encrypted_len_offset + 4 + 4096;
        let record_size = gm80_record_size(record_offset, encrypted_len_offset, 4096, file_size).unwrap();
        assert_eq!(record_size as u64, encrypted_len_offset - record_offset + 4 + 4096);

        // Before this bound, the encrypted payload limit did not constrain the
        // two attacker controlled padding counts used for the temporary Vec.
        let enormous_offset = gm80_encrypted_len_offset(record_offset, u32::MAX as u64, 0).unwrap();
        assert_eq!(gm80_record_size(record_offset, enormous_offset, 4096, u64::MAX), None);

        // Reject truncated files before allocating the claimed record.
        assert_eq!(gm80_record_size(record_offset, encrypted_len_offset, 4096, file_size - 1), None);
    }

    fn pe_i386_fixture() -> Vec<u8> {
        let mut image = alloc::vec![0u8; 88];
        image[..2].copy_from_slice(b"MZ");
        image[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        image[64..68].copy_from_slice(b"PE\0\0");
        image[68..70].copy_from_slice(&0x014cu16.to_le_bytes());
        image
    }

    fn append_zip_entry(
        zip: &mut Vec<u8>,
        central: &mut Vec<u8>,
        name: &[u8],
        method: u16,
        raw: &[u8],
        data: &[u8],
    ) {
        let offset = zip.len() as u32;
        let checksum = crc(raw);
        let mut local = alloc::vec![0u8; 30];
        local[..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
        local[4..6].copy_from_slice(&20u16.to_le_bytes());
        local[8..10].copy_from_slice(&method.to_le_bytes());
        local[14..18].copy_from_slice(&checksum.to_le_bytes());
        local[18..22].copy_from_slice(&(data.len() as u32).to_le_bytes());
        local[22..26].copy_from_slice(&(raw.len() as u32).to_le_bytes());
        local[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&local);
        zip.extend_from_slice(name);
        zip.extend_from_slice(data);

        let mut c = alloc::vec![0u8; 46];
        c[..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
        c[4..6].copy_from_slice(&20u16.to_le_bytes());
        c[6..8].copy_from_slice(&20u16.to_le_bytes());
        c[10..12].copy_from_slice(&method.to_le_bytes());
        c[16..20].copy_from_slice(&checksum.to_le_bytes());
        c[20..24].copy_from_slice(&(data.len() as u32).to_le_bytes());
        c[24..28].copy_from_slice(&(raw.len() as u32).to_le_bytes());
        c[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        c[42..46].copy_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&c);
        central.extend_from_slice(name);
    }

    fn game_zip(corrupt_crc: bool) -> Vec<u8> {
        game_zip_with_exe(&pe_i386_fixture(), corrupt_crc)
    }

    fn generic_zip(name: &[u8], data: &[u8]) -> Vec<u8> {
        let mut zip = Vec::new();
        let mut central = Vec::new();
        append_zip_entry(&mut zip, &mut central, name, 0, data, data);
        let central_offset = zip.len() as u32;
        zip.extend_from_slice(&central);
        let mut end = eocd(1, central.len() as u32);
        end[16..20].copy_from_slice(&central_offset.to_le_bytes());
        zip.extend_from_slice(&end);
        zip
    }

    fn game_zip_with_exe(exe: &[u8], corrupt_crc: bool) -> Vec<u8> {
        let mut zip = Vec::new();
        let mut central = Vec::new();
        append_zip_entry(&mut zip, &mut central, b"demo.exe", 0, &exe, &exe);
        // Raw DEFLATE stream for the ASCII string `hello`.
        append_zip_entry(
            &mut zip,
            &mut central,
            b"data/hello.txt",
            8,
            if corrupt_crc { b"hellO" } else { b"hello" },
            &[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00],
        );
        let central_offset = zip.len() as u32;
        zip.extend_from_slice(&central);
        let mut end = eocd(2, central.len() as u32);
        end[16..20].copy_from_slice(&central_offset.to_le_bytes());
        zip.extend_from_slice(&end);
        zip
    }

    fn stored_deflate(bytes: &[u8]) -> Vec<u8> {
        assert!(bytes.len() < u16::MAX as usize);
        let len = bytes.len() as u16;
        let mut compressed = alloc::vec![1u8]; // final stored block
        compressed.extend_from_slice(&len.to_le_bytes());
        compressed.extend_from_slice(&(!len).to_le_bytes());
        compressed.extend_from_slice(bytes);
        compressed
    }

    fn wrapped_game_zip(payload: &[u8]) -> Vec<u8> {
        let mut zip = Vec::new();
        let mut central = Vec::new();
        let exe = pe_i386_fixture();
        append_zip_entry(&mut zip, &mut central, b"bundle/Game.exe", 0, &exe, &exe);
        let compressed = stored_deflate(payload);
        append_zip_entry(
            &mut zip,
            &mut central,
            b"bundle/data/big.bin",
            8,
            payload,
            &compressed,
        );
        let central_offset = zip.len() as u32;
        zip.extend_from_slice(&central);
        let mut end = eocd(2, central.len() as u32);
        end[16..20].copy_from_slice(&central_offset.to_le_bytes());
        zip.extend_from_slice(&end);
        zip
    }

    fn entry_record(name: &[u8]) -> Vec<u8> {
        let mut out = alloc::vec![0u8; 46 + name.len()];
        out[0..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
        out[8..10].copy_from_slice(&0u16.to_le_bytes()); // flags
        out[10..12].copy_from_slice(&0u16.to_le_bytes()); // stored
        out[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        out[46..].copy_from_slice(name);
        out
    }

    fn eocd(entry_count: u16, directory_len: u32) -> Vec<u8> {
        let mut out = alloc::vec![0u8; 22];
        out[0..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
        out[8..10].copy_from_slice(&entry_count.to_le_bytes());
        out[10..12].copy_from_slice(&entry_count.to_le_bytes());
        out[12..16].copy_from_slice(&directory_len.to_le_bytes());
        out
    }

    #[test]
    fn rejects_traversal_and_absolute_names() {
        for bad in [
            b"../evil.exe".as_slice(),
            b"/root.exe",
            b"C:/root.exe",
            b"a\\b",
        ] {
            assert!(matches!(validate_name(bad), Err(ZipError::UnsafePath)));
        }
    }

    #[test]
    fn accepts_nested_relative_path() {
        assert_eq!(
            validate_name(b"data/maps/map.bin").unwrap(),
            "data/maps/map.bin"
        );
        assert_eq!(validate_name(b"data/").unwrap(), "data/");
        assert_eq!(validate_name(b"data/Espec\xa1ficas/a.bin").unwrap(), "data/Especificas/a.bin");
        assert_eq!(validate_name("data/Informaci\u{f3}n".as_bytes()).unwrap(), "data/Informaci\u{f3}n");
        assert!(is_archive_junk("__MACOSX/data/._a") && is_archive_junk("data/.DS_Store"));
        assert!(!is_archive_junk("data/a.bin"));
    }

    #[test]
    fn generic_extraction_does_not_require_game_layout_and_keeps_archive_paths() {
        let mut reader = MemReader {
            data: generic_zip(b"docs/readme.txt", b"hello from zip"),
            pos: 0,
        };
        let mut sink = MemSink::default();
        extract_controlled(&mut reader, &mut sink, &NoControl).unwrap();
        assert!(sink.prepared && sink.installed && !sink.cleared);
        assert_eq!(
            sink.files,
            alloc::vec![(String::from("docs/readme.txt"), b"hello from zip".to_vec())]
        );
    }

    #[test]
    fn parses_bounded_central_directory_and_rejects_zip_slip() {
        let central = entry_record(b"data/game.dat");
        let tail = eocd(1, central.len() as u32);
        let parsed = parse_directory(&tail, &central).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, "data/game.dat");

        let malicious = entry_record(b"../outside.exe");
        let tail = eocd(1, malicious.len() as u32);
        assert_eq!(
            parse_directory(&tail, &malicious),
            Err(ZipError::UnsafePath)
        );

        let mut collision = entry_record(b"a");
        collision.extend_from_slice(&entry_record(b"a/b"));
        let tail = eocd(2, collision.len() as u32);
        assert_eq!(
            parse_directory(&tail, &collision),
            Err(ZipError::DuplicatePath)
        );
    }

    #[test]
    fn rejects_archive_entry_count_above_switch_memory_limit() {
        let count = MAX_ARCHIVE_ENTRIES + 1;
        let mut central = Vec::new();
        for index in 0..count {
            let name = alloc::format!("entry-{index}");
            central.extend_from_slice(&entry_record(name.as_bytes()));
        }
        let tail = eocd(count as u16, central.len() as u32);
        assert_eq!(
            parse_directory(&tail, &central),
            Err(ZipError::TooManyEntries)
        );
    }

    #[test]
    fn streams_stored_and_deflate_and_installs_only_after_crc_checks() {
        let mut state =
            miniz_oxide::inflate::stream::InflateState::new_boxed(miniz_oxide::DataFormat::Raw);
        let mut out = [0u8; 64];
        let probe = miniz_oxide::inflate::stream::inflate(
            &mut state,
            &[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00],
            &mut out,
            miniz_oxide::MZFlush::Finish,
        );
        assert_eq!(probe.status, Ok(miniz_oxide::MZStatus::StreamEnd));
        assert_eq!(&out[..probe.bytes_written], b"hello");
        let mut reader = MemReader {
            data: game_zip(false),
            pos: 0,
        };
        let len = reader.len().unwrap() as usize;
        reader.seek_abs((len - 22) as u64).unwrap();
        let mut tail = alloc::vec![0u8; 22];
        read_exact(&mut reader, &mut tail).unwrap();
        let central_offset = u32le(&tail, 16).unwrap() as usize;
        let central_size = u32le(&tail, 12).unwrap() as usize;
        reader.seek_abs(central_offset as u64).unwrap();
        let mut central = alloc::vec![0u8; central_size];
        read_exact(&mut reader, &mut central).unwrap();
        let parsed = parse_directory(&tail, &central).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].crc32, crc(b"hello"));
        let mut probe_sink = MemSink::default();
        probe_sink.prepare_staging(0).unwrap();
        probe_sink.begin_file("data/hello.txt").unwrap();
        let mut input = [0u8; 8192];
        let mut output = [0u8; 8192];
        assert_eq!(parsed[1].compressed_size, 7);
        let data_pos = parsed[1].local_header_offset as u64 + 30 + 14;
        reader.seek_abs(data_pos).unwrap();
        let mut sample = [0u8; 7];
        read_exact(&mut reader, &mut sample).unwrap();
        assert_eq!(sample, [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00]);
        reader.seek_abs(data_pos).unwrap();
        assert_eq!(
            stream_deflate(
                &mut reader,
                &mut probe_sink,
                &parsed[1],
                &mut input,
                &mut output,
                &NoControl,
            ),
            Ok(())
        );
        let mut sink = MemSink::default();
        import(&mut reader, &mut sink).unwrap();
        assert!(sink.prepared && sink.installed && !sink.cleared);
        assert_eq!(sink.files[0].0, "demo.exe");
        assert_eq!(
            sink.files[1],
            (String::from("data/hello.txt"), b"hello".to_vec())
        );

        let mut reader = MemReader {
            data: game_zip(true),
            pos: 0,
        };
        let mut sink = MemSink::default();
        assert_eq!(import(&mut reader, &mut sink), Err(ZipError::Integrity));
        assert!(sink.prepared && sink.cleared && !sink.installed);
    }

    #[test]
    fn invalid_pe_is_rejected_before_install_and_staging_is_cleared() {
        let mut reader = MemReader {
            data: game_zip_with_exe(b"MZ", false),
            pos: 0,
        };
        let mut sink = MemSink::default();
        assert_eq!(
            import(&mut reader, &mut sink),
            Err(ZipError::InvalidGameLayout)
        );
        assert!(!sink.installed);
        assert!(sink.cleared);
    }

    #[test]
    fn cancellation_reports_real_written_bytes_and_cleans_only_staging() {
        struct CancelAfterFirstChunk {
            total: core::cell::Cell<u64>,
            written: core::cell::Cell<u64>,
        }
        impl ImportControl for CancelAfterFirstChunk {
            fn cancelled(&self) -> bool {
                self.written.get() > 0
            }
            fn set_total(&self, bytes: u64) {
                self.total.set(bytes);
            }
            fn advanced(&self, bytes: u64) {
                self.written.set(self.written.get() + bytes);
            }
        }
        let control = CancelAfterFirstChunk {
            total: core::cell::Cell::new(0),
            written: core::cell::Cell::new(0),
        };
        let mut reader = MemReader {
            data: game_zip(false),
            pos: 0,
        };
        let mut sink = MemSink::default();
        assert_eq!(
            import_controlled(&mut reader, &mut sink, &control),
            Err(ZipError::Cancelled)
        );
        assert_eq!(control.total.get(), 93);
        assert_eq!(control.written.get(), 88);
        assert!(sink.prepared);
        assert!(sink.cleared);
        assert!(!sink.installed);
    }

    #[test]
    fn cancellation_after_final_write_does_not_install_staging() {
        struct CancelAfterWrite(core::cell::Cell<bool>);
        impl ImportControl for CancelAfterWrite {
            fn cancelled(&self) -> bool {
                self.0.get()
            }
            fn advanced(&self, bytes: u64) {
                if bytes > 0 {
                    self.0.set(true);
                }
            }
        }

        let control = CancelAfterWrite(core::cell::Cell::new(false));
        let mut reader = MemReader {
            data: generic_zip(b"docs/readme.txt", b"hello"),
            pos: 0,
        };
        let mut sink = MemSink::default();
        assert_eq!(
            extract_controlled(&mut reader, &mut sink, &control),
            Err(ZipError::Cancelled)
        );
        assert!(sink.prepared);
        assert!(sink.cleared);
        assert!(!sink.installed);
    }

    #[test]
    fn streams_multiple_buffers_and_unwraps_one_top_folder() {
        let payload = alloc::vec![b'x'; 20_000];
        let mut reader = MemReader {
            data: wrapped_game_zip(&payload),
            pos: 0,
        };
        let mut sink = MemSink::default();
        import(&mut reader, &mut sink).unwrap();
        assert!(sink.installed);
        assert_eq!(sink.files[0].0, "Game.exe");
        assert_eq!(sink.files[1].0, "data/big.bin");
        assert_eq!(sink.files[1].1, payload);
    }
}
