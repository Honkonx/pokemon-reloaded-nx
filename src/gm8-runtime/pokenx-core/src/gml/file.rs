use crate::image::{ImageError, RgbaImage};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Cursor, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, Mutex},
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum TextHandle {
    Read(Cursor<Arc<[u8]>>),
    Write(BufWriter<File>),
}
#[derive(Debug)]
pub enum BinaryHandle {
    Read(Cursor<Arc<[u8]>>),
    Write(BufWriter<File>),
    ReadWrite(File),
}

#[derive(Clone, Copy, Debug)]
pub enum AccessMode {
    Read,
    Write,
    Special, // 'append' for text files, 'read-write' for binary
}

#[derive(Debug)]
pub enum Error {
    LegacyFileUnopened,
    InvalidFile(i32),
    CantRead,
    CantWrite,
    IOError(io::Error),
    ImageError(ImageError),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::IOError(e)
    }
}

impl From<ImageError> for Error {
    fn from(e: ImageError) -> Self {
        Self::ImageError(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LegacyFileUnopened => write!(f, "file is not opened"),
            Self::InvalidFile(handle) => write!(f, "invalid file handle {}", handle),
            Self::CantRead => write!(f, "file is not open for reading"),
            Self::CantWrite => write!(f, "file is not open for writing"),
            Self::IOError(err) => write!(f, "io error: {}", err),
            Self::ImageError(err) => write!(f, "image error: {}", err),
        }
    }
}

// Helper functions
// Files opened for reading are loaded whole and then read from memory. The GML text and binary functions seek
// constantly (file_text_eof, file_text_readln, file_bin_seek...), and every seek on a BufReader<File> drops its buffer
// and re-reads 8 KiB from the file; on the Switch microSD that made loading a room with a long text file take minutes.
/// Files and bytes loaded by `read_whole` (host tests report them next to slow frames).
pub static READ_FILES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static READ_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Process-wide cache of files the game reads whole. The game re-reads the same data files every time it enters a
/// scene (one transition read 93 files / 12.5 MB), and each read is a trip to the microSD. A file is served from memory
/// when its path, length and modification time are unchanged and nothing has been written, deleted, renamed or copied
/// since (every such call `invalidate_cache`s everything: writes are rare, and a stale read would corrupt a save).
/// `POKENX_FILE_CACHE_MB` sets the budget (0 disables); files over 8 MiB are never cached.
struct CacheEntry {
    data: Arc<[u8]>,
    len: u64,
    modified: Option<std::time::SystemTime>,
    used: u64,
}

#[derive(Default)]
struct FileCache {
    entries: BTreeMap<String, CacheEntry>,
    bytes: usize,
    tick: u64,
}

static FILE_CACHE: Mutex<Option<FileCache>> = Mutex::new(None);
pub static CACHE_HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const CACHE_MAX_FILE: usize = 8 << 20;

fn cache_budget() -> usize {
    let mb = std::env::var("POKENX_FILE_CACHE_MB").ok().and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(32);
    mb << 20
}

/// Drop every cached file (and every decoded image). Called before anything that can change a file the game might read
/// back.
pub fn invalidate_cache() {
    if let Ok(mut cache) = FILE_CACHE.lock() {
        *cache = None;
    }
    if let Ok(mut cache) = IMAGE_CACHE.lock() {
        *cache = None;
    }
}

fn normalized(path: &str) -> String {
    let mut key = path.replace('\\', "/").to_ascii_lowercase();
    while let Some(rest) = key.strip_prefix("./") {
        key = rest.to_owned();
    }
    key
}

fn last_component(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

/// Drop what a change to `path` can make stale, and keep the rest. A save written every few minutes (`data/partidas`, an
/// ini) used to flush the whole file cache and every decoded picture, so the next scene re-read tens of files and decoded
/// the images again. An entry goes when its normalized path equals `path`, lies under it (a folder), or has the same file
/// name (the game may spell a path relative in one place and absolute in another, and on Horizon the length is the only
/// other check, so a same-size rewrite must never be served stale). A folder, or a path that cannot be told apart, drops
/// everything as before.
pub fn invalidate_path(path: &str) {
    let key = normalized(path);
    if key.is_empty() || std::path::Path::new(path).is_dir() {
        invalidate_cache();
        return;
    }
    let name = last_component(&key).to_owned();
    let under = format!("{key}/");
    let stale = |entry_key: &str| {
        let entry = normalized(entry_key);
        entry == key || entry.starts_with(&under) || last_component(&entry) == name
    };
    if let Ok(mut guard) = FILE_CACHE.lock() {
        if let Some(cache) = guard.as_mut() {
            let doomed: Vec<String> = cache.entries.keys().filter(|k| stale(k)).cloned().collect();
            for k in doomed {
                if let Some(old) = cache.entries.remove(&k) {
                    cache.bytes -= old.data.len();
                }
            }
        }
    }
    if let Ok(mut guard) = IMAGE_CACHE.lock() {
        if let Some(cache) = guard.as_mut() {
            let doomed: Vec<String> = cache.entries.keys().filter(|k| stale(k)).cloned().collect();
            for k in doomed {
                if let Some(old) = cache.entries.remove(&k) {
                    cache.bytes -= old.bytes;
                }
            }
        }
    }
}

/// Decoded images, for `sprite_add`/`sprite_replace`/`background_add`/`show_image`: the game loads the same external
/// pictures every time a scene needs them, and each load was a trip to the microSD plus a PNG decode on the console's
/// CPU. An image is reused while its path and file length are unchanged and nothing has been written (the same
/// invalidation as the file cache). `POKENX_IMAGE_CACHE_MB` sets the budget (default 48; 0 or a disabled file cache turns
/// it off); a decoded image over 8 MiB is never kept.
struct ImageEntry {
    image: Arc<RgbaImage>,
    len: u64,
    bytes: usize,
    used: u64,
}

#[derive(Default)]
struct ImageCache {
    entries: BTreeMap<String, ImageEntry>,
    bytes: usize,
    tick: u64,
}

static IMAGE_CACHE: Mutex<Option<ImageCache>> = Mutex::new(None);
pub static IMAGE_CACHE_HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const IMAGE_MAX_BYTES: usize = 8 << 20;

fn image_budget() -> usize {
    if cache_budget() == 0 {
        return 0;
    }
    let mb = std::env::var("POKENX_IMAGE_CACHE_MB").ok().and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(48);
    mb << 20
}

/// The decoded (non-GIF) image at `path`, shared with the cache.
fn decoded_image(path: &str) -> Result<Arc<RgbaImage>> {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let budget = image_budget();
    if budget > 0 && len > 0 {
        if let Ok(mut guard) = IMAGE_CACHE.lock() {
            let cache = guard.get_or_insert_with(ImageCache::default);
            cache.tick += 1;
            let tick = cache.tick;
            if let Some(entry) = cache.entries.get_mut(path) {
                if entry.len == len {
                    entry.used = tick;
                    IMAGE_CACHE_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Ok(entry.image.clone());
                }
            }
        }
    }
    let bytes = read_whole(File::open(path)?, path)?;
    let image = Arc::new(crate::image::codec::decode(&bytes)?);
    let size = image.width() as usize * image.height() as usize * 4;
    if budget > 0 && len > 0 && size <= IMAGE_MAX_BYTES {
        if let Ok(mut guard) = IMAGE_CACHE.lock() {
            let cache = guard.get_or_insert_with(ImageCache::default);
            cache.tick += 1;
            let used = cache.tick;
            if let Some(old) = cache.entries.insert(path.to_string(), ImageEntry { image: image.clone(), len, bytes: size, used }) {
                cache.bytes -= old.bytes;
            }
            cache.bytes += size;
            while cache.bytes > budget {
                let Some(oldest) = cache.entries.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone()) else { break };
                if let Some(old) = cache.entries.remove(&oldest) {
                    cache.bytes -= old.bytes;
                }
            }
        }
    }
    Ok(image)
}

// Horizon's stat has no usable modification time in the pokenx-std shim: there the length plus the invalidation on
// every write is the whole check.
#[cfg(target_arch = "aarch64")]
fn modified_of(_: &std::fs::Metadata) -> Option<std::time::SystemTime> {
    None
}

#[cfg(not(target_arch = "aarch64"))]
fn modified_of(metadata: &std::fs::Metadata) -> Option<std::time::SystemTime> {
    metadata.modified().ok()
}

fn read_whole(mut file: File, path: &str) -> io::Result<Arc<[u8]>> {
    let (len, modified) = match file.metadata() {
        Ok(metadata) => (metadata.len(), modified_of(&metadata)),
        Err(_) => (0, None),
    };
    let budget = cache_budget();
    if budget > 0 && len > 0 {
        if let Ok(mut guard) = FILE_CACHE.lock() {
            let cache = guard.get_or_insert_with(FileCache::default);
            cache.tick += 1;
            let tick = cache.tick;
            if let Some(entry) = cache.entries.get_mut(path) {
                if entry.len == len && entry.modified == modified {
                    entry.used = tick;
                    CACHE_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Ok(entry.data.clone());
                }
            }
        }
    }
    let mut data = Vec::with_capacity(len as usize);
    file.read_to_end(&mut data)?;
    READ_FILES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    READ_BYTES.fetch_add(data.len() as u64, std::sync::atomic::Ordering::Relaxed);
    let data: Arc<[u8]> = data.into();
    if budget > 0 && !data.is_empty() && data.len() <= CACHE_MAX_FILE {
        if let Ok(mut guard) = FILE_CACHE.lock() {
            let cache = guard.get_or_insert_with(FileCache::default);
            cache.tick += 1;
            let used = cache.tick;
            if let Some(old) = cache.entries.insert(path.to_string(), CacheEntry { data: data.clone(), len: data.len() as u64, modified, used }) {
                cache.bytes -= old.data.len();
            }
            cache.bytes += data.len();
            while cache.bytes > budget {
                let Some(oldest) = cache.entries.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone()) else { break };
                if let Some(old) = cache.entries.remove(&oldest) {
                    cache.bytes -= old.data.len();
                }
            }
        }
    }
    Ok(data)
}


fn read_until<P>(file: impl Read, mut end_pred: P) -> io::Result<Vec<u8>>
where
    P: FnMut(u8) -> bool,
{
    let mut out = Vec::new();
    for byte_maybe in file.bytes() {
        let byte = byte_maybe?;
        out.push(byte);
        if end_pred(byte) {
            break
        }
    }
    Ok(out)
}

// Returns Ok(false) on EOF
fn skip_until<P>(file: impl Read, end_pred: P) -> io::Result<bool>
where
    P: Fn(u8) -> bool,
{
    for byte in file.bytes() {
        if end_pred(byte?) {
            return Ok(true)
        }
    }
    Ok(false)
}

/// Converts a GML string into a valid filepath by replacing backslashes with file separators
pub fn to_path(path: &str) -> std::borrow::Cow<str> {
    path.replace("\\", std::path::MAIN_SEPARATOR_STR).into()
}

impl TextHandle {
    pub fn open(path: &str, mode: AccessMode) -> io::Result<Self> {
        #[rustfmt::skip]
        let (read, write, append) = match mode {
            AccessMode::Read    => (true,  false, false),
            AccessMode::Write   => (false, true,  false),
            AccessMode::Special => (false, true,  true ),
        };

        if write {
            invalidate_path(path);
        }
        let file = OpenOptions::new()
            .create(!read)
            .read(read)
            .write(write)
            .append(append)
            .truncate(write && !append)
            .open(path)?;

        Ok(match mode {
            AccessMode::Read => TextHandle::Read(Cursor::new(read_whole(file, path)?)),
            AccessMode::Write | AccessMode::Special => TextHandle::Write(BufWriter::new(file)),
        })
    }

    fn get_reader(&mut self) -> Result<&mut Cursor<Arc<[u8]>>> {
        match self {
            Self::Read(f) => Ok(f),
            _ => Err(Error::CantRead),
        }
    }

    fn get_writer(&mut self) -> Result<&mut BufWriter<File>> {
        match self {
            Self::Write(f) => Ok(f),
            _ => Err(Error::CantWrite),
        }
    }

    pub fn read_real(&mut self) -> Result<f64> {
        Ok(read_real(self.get_reader()?)?)
    }

    pub fn read_string(&mut self) -> Result<Vec<u8>> {
        let mut f = self.get_reader()?;
        let mut bytes = read_until(&mut f, |c| c == 0x0a)?;
        if bytes.last() == Some(&0x0a) {
            // LF
            bytes.pop();
            f.seek(SeekFrom::Current(-1))?;
            if bytes.last() == Some(&0x0d) {
                // CR
                bytes.pop();
                f.seek(SeekFrom::Current(-1))?;
            }
        }
        Ok(bytes)
    }

    pub fn write_real(&mut self, real: f64) -> Result<()> {
        let text = if real.fract() == 0.0 { format!(" {:.0}", real) } else { format!(" {:.6}", real) };
        self.get_writer()?.write_all(text.as_bytes())?;
        Ok(())
    }

    pub fn write_string(&mut self, text: &[u8]) -> Result<()> {
        self.get_writer()?.write_all(text)?;
        Ok(())
    }

    pub fn write_newline(&mut self) -> Result<()> {
        self.get_writer()?.write_all(b"\r\n")?;
        Ok(())
    }

    pub fn skip_line(&mut self) -> Result<()> {
        Ok(skip_line(&mut self.get_reader()?)?)
    }

    pub fn is_eof(&mut self) -> Result<bool> {
        let f = self.get_reader()?;
        let mut buf: [u8; 1] = [0];
        let last_pos = f.stream_position()?;
        let bytes_read = f.read(&mut buf)?;
        f.seek(SeekFrom::Start(last_pos))?;
        Ok(bytes_read == 0)
    }

    pub fn is_eoln(&mut self) -> Result<bool> {
        let f = self.get_reader()?;
        let mut buf: [u8; 2] = [0, 0];
        let last_pos = f.stream_position()?;
        let bytes_read = f.read(&mut buf)?;
        f.seek(SeekFrom::Start(last_pos))?;
        Ok(bytes_read == 0 || (buf[0] == 0x0d && buf[1] == 0x0a))
    }

    pub fn flush(&mut self) -> Result<()> {
        match self {
            Self::Read(_) => Ok(()),
            Self::Write(f) => f.flush().map_err(|e| e.into()),
        }
    }
}

impl BinaryHandle {
    pub fn open(path: &str, mode: AccessMode) -> io::Result<Self> {
        let file = Self::_open(path, mode)?;
        match mode {
            AccessMode::Read => Ok(Self::Read(Cursor::new(read_whole(file, path)?))),
            AccessMode::Write => Ok(Self::Write(BufWriter::new(file))),
            AccessMode::Special => Ok(Self::ReadWrite(file)),
        }
    }

    // Binary files are always created by GM if they doesn't exist, but in such
    // cases it opens them in read-write mode rather than specified, so both the
    // file_bin_read_byte() and file_bin_write_byte() works. However, we can't
    // simply specify the .create(true) because it's explicitly disallowed in the
    // code of std::fs for read-only mode. We also don't use Path::is_file()
    // because it may theoretically fail if someone will create a file with the
    // same name between testing and opening, and also would require an additional
    // function call every time. Instead, when a read-only or write-only mode was
    // requested for a file, we try first to create it and fail if it's exists.
    fn _open(path: &str, mode: AccessMode) -> io::Result<File> {
        // Binary files are created on demand even for reading, and may be written afterwards.
        invalidate_path(path);
        let mut opts = OpenOptions::new();

        #[rustfmt::skip]
        let (read, write) = match mode {
            AccessMode::Read    => (true,  false),
            AccessMode::Write   => (false, true ),
            AccessMode::Special => (true,  true ),
        };

        if !(read && write) {
            // We don't return on other errors (that is, not AlreadyExists) here
            // because the second call to .open() may give us a more exact one.
            if let r @ Ok(_) = opts.create_new(true).read(true).write(true).open(path) {
                return r
            };

            opts.create_new(false);
        };

        // Note that .create() is necessary here not only for read-write case but
        // also to behave correctly if file was erased between .open() calls. Same
        // is also the reason why this opening attempt is secondary, not primary.
        opts.create(write) // not .create(true), read the initial comment why!
            .read(read)
            .write(write)
            .open(path)
    }

    fn get_reader(&mut self) -> Result<&mut dyn Read> {
        match self {
            Self::Read(f) => Ok(f),
            Self::Write(_) => Err(Error::CantRead),
            Self::ReadWrite(f) => Ok(f),
        }
    }

    fn get_writer(&mut self) -> Result<&mut dyn Write> {
        match self {
            Self::Read(_) => Err(Error::CantWrite),
            Self::Write(f) => Ok(f),
            Self::ReadWrite(f) => Ok(f),
        }
    }

    fn get_seeker(&mut self) -> &mut dyn Seek {
        match self {
            Self::Read(f) => f,
            Self::Write(f) => f,
            Self::ReadWrite(f) => f,
        }
    }

    pub fn clear(&mut self) -> Result<()> {
        let f = match self {
            Self::Read(_) => return Err(Error::CantWrite),
            Self::Write(f) => {
                // Otherwise pending bytes are written back after truncation,
                // so file_bin_rewrite would keep the data it should discard.
                f.flush()?;
                f.get_mut()
            },
            Self::ReadWrite(f) => f,
        };
        f.seek(SeekFrom::Start(0))?;
        f.set_len(0)?;
        Ok(())
    }

    pub fn read_byte(&mut self) -> Result<u8> {
        let mut buf: [u8; 1] = [0];
        self.get_reader()?.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    pub fn write_byte(&mut self, byte: u8) -> Result<()> {
        self.get_writer()?.write_all(&[byte])?;
        Ok(())
    }

    pub fn tell(&mut self) -> Result<u64> {
        Ok(self.get_seeker().stream_position()?)
    }

    pub fn seek(&mut self, pos: i32) -> Result<()> {
        self.get_seeker().seek(SeekFrom::Start(pos as u64))?;
        Ok(())
    }

    pub fn size(&mut self) -> Result<u64> {
        // TODO seek_stream_len feature
        let seeker = self.get_seeker();
        let pos = seeker.stream_position()?;
        seeker.seek(SeekFrom::End(0))?;
        let size = seeker.stream_position()?;
        seeker.seek(SeekFrom::Start(pos))?;
        Ok(size)
    }

    pub fn flush(&mut self) -> Result<()> {
        match self {
            Self::Read(_) => Ok(()),
            Self::Write(f) => f.flush(),
            Self::ReadWrite(f) => f.flush(),
        }
        .map_err(|e| e.into())
    }
}

pub fn read_real(mut f: impl Read + Seek) -> io::Result<f64> {
    // Read digits and at most one period or comma, plus one extra character
    let mut period_seen = false;
    let mut nonspace_seen = false;
    let mut bytes = read_until(&mut f, |b| {
        // If you read spaces or dashes at the start, skip them
        if b == 0x20 || b == 0x2d {
            return nonspace_seen
        }
        nonspace_seen = true;
        // Comma or period
        if b == 0x2e || b == 0x2c {
            if period_seen {
                true
            } else {
                period_seen = true;
                false
            }
        } else {
            b < 0x30 || b > 0x39
        }
    })?;
    // read_until leaves a trailing character, so remove that
    if let Some(&b) = bytes.last() {
        if b < 0x30 || b > 0x39 {
            // Remove the trailing character and step back if it's a CR
            if bytes.pop().unwrap() == 0x0d {
                f.seek(SeekFrom::Current(-1))?;
            }
        }
    }
    // Having done that, there may still be a trailing dot, so remove that
    if let Some(&b) = bytes.last() {
        if b == 0x2e || b == 0x2c {
            bytes.pop();
        }
    }
    // These bytes are guaranteed to be UTF-8 so no worries here
    let mut text = String::from_utf8_lossy(bytes.as_slice()).replace(",", ".");
    // Remove spaces and all dashes but one
    let mut minus_seen = false;
    text.retain(|c| {
        if c == '-' {
            if minus_seen {
                return false
            }
            minus_seen = true;
        }
        c != ' '
    });
    text.parse().or(Ok(0.0))
}

pub fn skip_line(f: impl Read) -> io::Result<()> {
    skip_until(f, |c| c == 0x0a)?;
    Ok(())
}

/// Sync a new file before promotion. Use atomic replacement where supported;
/// otherwise retain the previous file as a recovery backup until promotion.
/// This is not a power-loss transaction for FAT/exFAT directory metadata.
pub fn write_atomic(path: &str, data: &[u8]) -> std::io::Result<()> {
    invalidate_path(path);
    write_atomic_with_rename(path, data, |from, to| std::fs::rename(from, to))
}

fn write_atomic_with_rename(path: &str, data: &[u8], mut rename: impl FnMut(&str, &str) -> std::io::Result<()>) -> std::io::Result<()> {
    static WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = WRITER.lock().map_err(|_| io::Error::new(io::ErrorKind::Other, "save writer lock poisoned"))?;
    let (tmp, mut file) = {
        let mut created = None;
        for index in 0..128 {
            let candidate = format!("{path}.pokenx-save.tmp.{index}");
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
                Ok(file) => { created = Some((candidate, file)); break },
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        created.ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "save temporary slots are occupied"))?
    };
    let written = file.write_all(data).and_then(|_| file.flush()).and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&tmp); // only this call's exclusively created file
        return Err(error)
    }
    let promotion_error = match rename(&tmp, path) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    // Never move a directory or an unknown destination to make room for a save.
    if !std::fs::metadata(path).map(|entry| entry.is_file()).unwrap_or(false) {
        return Err(io::Error::new(io::ErrorKind::Other,
            format!("Save promotion failed; new data retained at {}: {}", tmp, promotion_error)))
    }
    let backup = format!("{path}.pokenx-save.bak");
    match std::fs::metadata(&backup) {
        Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists,
            format!("Save recovery backup already exists at {}; new data retained at {}", backup, tmp))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Err(error) => return Err(error),
    }
    rename(path, &backup).map_err(|error| io::Error::new(io::ErrorKind::Other,
        format!("Could not preserve previous save; new data retained at {}: {}", tmp, error)))?;
    match rename(&tmp, path) {
        Ok(()) => {
            // Committed. A cleanup failure keeps a recovery copy and is visible.
            std::fs::remove_file(&backup).map_err(|error| io::Error::new(io::ErrorKind::Other,
                format!("New save committed; recovery backup remains at {}: {}", backup, error)))
        },
        Err(error) => {
            let restored = if std::fs::metadata(path).is_ok() {
                Err(io::Error::new(io::ErrorKind::AlreadyExists, "destination reappeared; not overwritten"))
            } else { rename(&backup, path) };
            match restored {
                Ok(()) => Err(io::Error::new(io::ErrorKind::Other,
                    format!("Save promotion failed; previous save restored; new data retained at {}: {}", tmp, error))),
                Err(restore_error) => Err(io::Error::new(io::ErrorKind::Other,
                    format!("Save promotion failed ({}); restore failed ({}); previous data at {}, new data at {}",
                            error, restore_error, backup, tmp))),
            }
        },
    }
}

pub fn file_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

#[cfg(all(test, feature = "std"))]
mod save_tests {
    use super::*;
    struct Scratch { root: std::path::PathBuf, directory: std::path::PathBuf }
    impl Scratch {
        fn new() -> Self {
            let root = std::env::temp_dir().canonicalize().unwrap();
            // Tests run in parallel: a counter keeps two scratch folders apart even within the same clock tick.
            static NEXT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
            let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let serial = NEXT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let directory = root.join(format!("pokenx-save-{}-{nonce}-{serial}", std::process::id()));
            std::fs::create_dir(&directory).unwrap();
            Self { root, directory }
        }
        fn save(&self) -> String { self.directory.join("save.bin").to_str().unwrap().into() }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            // Only remove the fresh test directory verified directly under temp.
            if let Ok(path) = self.directory.canonicalize() {
                if path.parent() == Some(self.root.as_path()) && path.file_name().unwrap().to_string_lossy().starts_with("pokenx-save-") {
                    let _ = std::fs::remove_dir_all(path);
                }
            }
        }
    }
    #[test]
    fn failed_save_promotion_restores_the_previous_file() {
        let scratch = Scratch::new();
        let path = scratch.save();
        std::fs::write(&path, b"previous save").unwrap();
        let error = write_atomic_with_rename(&path, b"new save", |from, to| {
            if to == path && from != format!("{path}.pokenx-save.bak") {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected promotion failure"))
            } else { std::fs::rename(from, to) }
        });
        assert!(error.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous save");
    }

    #[test]
    fn native_and_non_replacing_rename_commit_new_data() {
        let scratch = Scratch::new();
        let path = scratch.save();
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        write_atomic_with_rename(&path, b"third", |from, to| {
            if Path::new(to).exists() { Err(io::ErrorKind::AlreadyExists.into()) }
            else { std::fs::rename(from, to) }
        }).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"third");
        assert_eq!(std::fs::read_dir(&scratch.directory).unwrap().count(), 1);
    }

    #[test]
    fn failed_promotion_and_restore_keep_both_recovery_files() {
        let scratch = Scratch::new();
        let path = scratch.save();
        std::fs::write(&path, b"old").unwrap();
        let error = write_atomic_with_rename(&path, b"new", |from, to| {
            if to == path { Err(io::ErrorKind::PermissionDenied.into()) }
            else { std::fs::rename(from, to) }
        }).unwrap_err();
        assert!(error.to_string().contains("restore failed"));
        assert_eq!(std::fs::read(format!("{path}.pokenx-save.bak")).unwrap(), b"old");
        assert_eq!(std::fs::read(format!("{path}.pokenx-save.tmp.0")).unwrap(), b"new");
    }

    #[test]
    fn existing_sidecars_are_never_overwritten_or_deleted() {
        let scratch = Scratch::new();
        let path = scratch.save();
        let temporary = format!("{path}.pokenx-save.tmp.0");
        let backup = format!("{path}.pokenx-save.bak");
        std::fs::write(&path, b"old").unwrap();
        std::fs::write(&temporary, b"pending").unwrap();
        std::fs::write(&backup, b"recovery").unwrap();
        let result = write_atomic_with_rename(&path, b"new", |_, _| Err(io::ErrorKind::AlreadyExists.into()));
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(std::fs::read(&temporary).unwrap(), b"pending");
        assert_eq!(std::fs::read(&backup).unwrap(), b"recovery");
        assert_eq!(std::fs::read(format!("{path}.pokenx-save.tmp.1")).unwrap(), b"new");
    }

    #[test]
    fn failed_backup_move_and_directory_destination_preserve_existing_data() {
        let scratch = Scratch::new();
        let path = scratch.save();
        std::fs::write(&path, b"old").unwrap();
        assert!(write_atomic_with_rename(&path, b"new", |_, _| Err(io::ErrorKind::PermissionDenied.into())).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        let directory = scratch.directory.join("folder");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("owned.txt"), b"keep").unwrap();
        assert!(write_atomic(directory.to_str().unwrap(), b"new").is_err());
        assert_eq!(std::fs::read(directory.join("owned.txt")).unwrap(), b"keep");
    }
}

pub fn rename(from: &str, to: &str) -> Result<()> {
    invalidate_path(from);
    invalidate_path(to);
    if !Path::new(to).exists() {
        std::fs::rename(from, to)?;
    }
    Ok(())
}

pub fn copy(from: &str, to: &str) -> Result<()> {
    invalidate_path(to);
    std::fs::copy(from, to)?;
    Ok(())
}

pub fn dir_exists(path: &str) -> bool {
    Path::new(path).is_dir()
}

pub fn dir_create(path: &str) -> Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}

pub fn delete(path: &str) -> Result<()> {
    invalidate_path(path);
    if Path::new(path).exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn load_image(path: &str) -> Result<RgbaImage> {
    Ok((*decoded_image(path)?).clone())
}

/// Loads a sprite strip. A GIF yields its frames; any other format is cut into `imgnumb` equal
/// vertical slices (left to right), like GameMaker's `sprite_add`.
pub fn load_animation(path: &str, imgnumb: usize) -> Result<Vec<RgbaImage>> {
    let mut magic = [0u8; 4];
    let is_gif = File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && &magic == b"GIF8";
    if is_gif {
        let bytes = std::fs::read(path)?;
        return Ok(crate::image::codec::decode_gif(&bytes)?)
    }
    let shared = decoded_image(path)?;
    let image = (*shared).clone();
    if imgnumb <= 1 {
        return Ok(vec![image])
    }
    let sprite_width = image.width() as usize / imgnumb;
    if sprite_width == 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "image is narrower than the number of frames").into());
    }
    let sprite_height = image.height() as usize;
    let mut images = Vec::with_capacity(imgnumb);
    for i in 0..imgnumb {
        let mut pixels = Vec::with_capacity(sprite_width * sprite_height * 4);
        for y in 0..sprite_height {
            for x in 0..sprite_width {
                pixels.extend_from_slice(&image.get_pixel((i * sprite_width + x) as u32, y as u32).0);
            }
        }
        images.push(RgbaImage::from_vec(sprite_width as _, sprite_height as _, pixels).unwrap());
    }
    Ok(images)
}

pub fn save_image<P: AsRef<Path>>(path: P, image: RgbaImage) -> Result<()> {
    // save to png if the filename is .png otherwise bmp regardless of filename
    let png = path.as_ref().extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("png")).unwrap_or(false);
    let bytes = if png { crate::image::codec::encode_png(&image) } else { crate::image::codec::encode_bmp(&image) };
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod read_in_memory_tests {
    use super::*;

    #[test]
    fn text_file_reads_from_memory_with_the_same_line_semantics() {
        let path = std::env::temp_dir().join(format!("pokenx-text-read-{}.txt", std::process::id()));
        std::fs::write(&path, b"one\r\ntwo\nthree 42\r\n").unwrap();
        let mut file = TextHandle::open(path.to_str().unwrap(), AccessMode::Read).unwrap();
        assert!(!file.is_eof().unwrap());
        assert_eq!(file.read_string().unwrap(), b"one");
        file.skip_line().unwrap();
        assert_eq!(file.read_string().unwrap(), b"two");
        file.skip_line().unwrap();
        assert_eq!(file.read_string().unwrap(), b"three 42");
        file.skip_line().unwrap();
        assert!(file.is_eof().unwrap());
        // the file on disk can change or vanish without affecting the open handle
        std::fs::remove_file(&path).unwrap();
        assert!(file.is_eof().unwrap());
    }

    #[test]
    fn binary_file_tell_seek_size_read_from_memory() {
        let path = std::env::temp_dir().join(format!("pokenx-bin-read-{}.bin", std::process::id()));
        std::fs::write(&path, [1u8, 2, 3, 4]).unwrap();
        let mut file = BinaryHandle::open(path.to_str().unwrap(), AccessMode::Read).unwrap();
        assert_eq!(file.size().unwrap(), 4);
        assert_eq!(file.read_byte().unwrap(), 1);
        assert_eq!(file.tell().unwrap(), 1);
        file.seek(3).unwrap();
        assert_eq!(file.read_byte().unwrap(), 4);
        assert!(file.read_byte().is_err());
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod file_cache_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    // Cache state is process-wide: one test body so parallel tests cannot interleave invalidations.
    #[test]
    fn a_decoded_image_is_reused_until_something_is_written() {
        let path = std::env::temp_dir().join(format!("pokenx-image-cache-{}.png", std::process::id()));
        let name = path.to_str().unwrap();
        let picture = RgbaImage::from_vec(3, 2, vec![200u8; 24]).unwrap();
        std::fs::write(&path, crate::image::codec::encode_png(&picture)).unwrap();
        invalidate_cache();
        let hits = IMAGE_CACHE_HITS.load(Ordering::Relaxed);
        let mut got_hit = false;
        for _ in 0..20 {
            let first = load_image(name).unwrap();
            let second = load_image(name).unwrap();
            assert_eq!((first.width(), first.height()), (3, 2));
            assert_eq!(first, second);
            if IMAGE_CACHE_HITS.load(Ordering::Relaxed) > hits { got_hit = true; break }
        }
        assert!(got_hit, "a repeated load must reuse the decoded image");
        invalidate_cache();
        let sliced = load_animation(name, 3).unwrap();
        assert_eq!(sliced.len(), 3, "a strip is cut from the cached picture");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn second_read_is_served_from_memory_until_something_is_written() {
        let path = std::env::temp_dir().join(format!("pokenx-cache-{}.txt", std::process::id()));
        let name = path.to_str().unwrap();
        std::fs::write(&path, b"alpha\r\nbeta\r\n").unwrap();
        invalidate_cache();
        // Other tests may invalidate the shared cache between two opens; retry a few times.
        let hits = CACHE_HITS.load(Ordering::Relaxed);
        for _ in 0..20 {
            let mut first = TextHandle::open(name, AccessMode::Read).unwrap();
            assert_eq!(first.read_string().unwrap(), b"alpha");
            let mut second = TextHandle::open(name, AccessMode::Read).unwrap();
            assert_eq!(second.read_string().unwrap(), b"alpha");
            if CACHE_HITS.load(Ordering::Relaxed) > hits { break }
        }
        assert!(CACHE_HITS.load(Ordering::Relaxed) > hits, "a repeated open must hit the cache");
        // a write through the game's own API invalidates, so the next read sees the new contents
        let mut writer = TextHandle::open(name, AccessMode::Write).unwrap();
        writer.write_string(b"gamma").unwrap();
        writer.flush().unwrap();
        drop(writer);
        let mut third = TextHandle::open(name, AccessMode::Read).unwrap();
        assert_eq!(third.read_string().unwrap(), b"gamma");
        let _ = std::fs::remove_file(&path);
        invalidate_cache();
    }

    #[test]
    fn writing_one_file_keeps_the_cached_others() {
        let dir = std::env::temp_dir().join(format!("pokenx-invalidate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let kept = dir.join("kept.txt");
        let saved = dir.join("saved.txt");
        std::fs::write(&kept, b"kept\r\n").unwrap();
        std::fs::write(&saved, b"one\r\n").unwrap();
        let (kept_name, saved_name) = (kept.to_str().unwrap().to_owned(), saved.to_str().unwrap().to_owned());
        // Other tests may invalidate the shared cache at any time: look at the entries, not at the hit counter.
        let cached = |name: &str| FILE_CACHE.lock().unwrap().as_ref().map_or(false, |c| c.entries.contains_key(name));
        let mut ok = false;
        for _ in 0..50 {
            TextHandle::open(&kept_name, AccessMode::Read).unwrap();
            TextHandle::open(&saved_name, AccessMode::Read).unwrap();
            if cached(&kept_name) && cached(&saved_name) {
                // Writing `saved.txt` drops it (the same-size rewrite must not be served stale) but not `kept.txt`.
                invalidate_path(&saved_name);
                if cached(&kept_name) || !cached(&saved_name) {
                    ok = !cached(&saved_name);
                    break;
                }
            }
        }
        assert!(ok, "the written file must leave the cache");
        // A different spelling of the same file (relative form, other case, backslashes) is still dropped.
        assert_eq!(normalized(".\\Data\\Partidas\\A.sav"), "data/partidas/a.sav");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
