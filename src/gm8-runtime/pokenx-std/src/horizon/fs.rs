//! `std::fs` over newlib stdio plus the structured helpers in `host_services.cpp`.

use super::host::{self, HostStat, KIND_DIR, KIND_FILE};
use crate::horizon::{
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use alloc_crate::{ffi::CString, string::String, vec::Vec};
use core::ffi::{c_char, c_void};

fn c_path(path: &Path) -> io::Result<CString> {
    let text = path.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))?;
    CString::new(text).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}

fn last_error(kind: io::ErrorKind, what: &'static str) -> io::Error {
    io::Error::new(kind, what)
}

#[derive(Clone, Copy, Debug)]
pub struct Metadata {
    stat: HostStat,
}

impl Metadata {
    pub fn len(&self) -> u64 {
        self.stat.size
    }
    pub fn is_file(&self) -> bool {
        self.stat.kind == KIND_FILE
    }
    pub fn is_dir(&self) -> bool {
        self.stat.kind == KIND_DIR
    }
}

pub fn metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    let c = c_path(path.as_ref())?;
    let mut stat = HostStat::default();
    // SAFETY: valid NUL-terminated path and a valid out pointer.
    let result = unsafe { host::pokenx_fs_stat(c.as_ptr(), &mut stat) };
    if result == 0 && stat.kind != 0 {
        Ok(Metadata { stat })
    } else {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

pub struct File {
    handle: *mut c_void,
}

// SAFETY: the newlib FILE is only used through `&mut self`/`&self` calls that newlib serializes.
unsafe impl Send for File {}

impl File {
    fn open_with(path: &Path, mode: &str) -> io::Result<File> {
        File::open_buffered(path, mode, 0)
    }

    /// `read_buffer` > 0 sizes the stdio buffer before any I/O: newlib's default is 1 KiB, which turns a 22 MiB file
    /// into ~22 000 trips to the microSD.
    fn open_buffered(path: &Path, mode: &str, read_buffer: usize) -> io::Result<File> {
        let c = c_path(path)?;
        let mode = CString::new(mode).expect("static mode");
        // SAFETY: both strings are NUL-terminated.
        let handle = unsafe { host::fopen(c.as_ptr(), mode.as_ptr()) };
        if handle.is_null() {
            Err(last_error(io::ErrorKind::NotFound, "could not open file"))
        } else {
            if read_buffer > 0 {
                // SAFETY: live FILE* with no I/O done yet.
                unsafe { host::pokenx_fs_read_buffer(handle, read_buffer) };
            }
            Ok(File { handle })
        }
    }

    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
        File::open_buffered(path.as_ref(), "rb", 64 << 10)
    }

    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<File> {
        File::open_with(path.as_ref(), "wb")
    }

    pub fn metadata(&self) -> io::Result<Metadata> {
        // SAFETY: `handle` is a live FILE*.
        let current = unsafe { host::ftell(self.handle) };
        // SAFETY: as above.
        unsafe { host::fseek(self.handle, 0, 2) };
        // SAFETY: as above.
        let end = unsafe { host::ftell(self.handle) };
        // SAFETY: as above.
        unsafe { host::fseek(self.handle, current, 0) };
        if end < 0 {
            return Err(io::Error::from(io::ErrorKind::Other));
        }
        Ok(Metadata { stat: HostStat { size: end as u64, kind: KIND_FILE } })
    }

    /// Change length through the native descriptor, preserving the stdio cursor.
    pub fn set_len(&self, size: u64) -> io::Result<()> {
        if size > i64::MAX as u64 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "file size exceeds native offset range"));
        }
        // SAFETY: live FILE*; the host flushes and invalidates its stdio buffers.
        if unsafe { host::pokenx_fs_set_len(self.handle, size) } != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "could not resize file"));
        }
        Ok(())
    }

    pub fn sync_all(&self) -> io::Result<()> {
        // SAFETY: live FILE*. Host flushes stdio then syncs the native descriptor.
        if unsafe { host::pokenx_fs_sync_all(self.handle) } != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "file synchronization failed"));
        }
        Ok(())
    }

    /// Helpers that own a file can report deferred stdio failures explicitly.
    fn finish(mut self) -> io::Result<()> {
        let flushed = self.flush();
        let handle = core::mem::replace(&mut self.handle, core::ptr::null_mut());
        // SAFETY: ownership moved here; Drop no longer closes this handle.
        let closed = unsafe { host::fclose(handle) };
        flushed?;
        if closed != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "file close failed"));
        }
        Ok(())
    }
}

impl core::fmt::Debug for File {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("File { .. }")
    }
}

impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: `handle` is a live FILE* and is not used afterwards.
        if !self.handle.is_null() {
            unsafe { host::fclose(self.handle) };
        }
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        if buf.is_empty() { return Ok(0) }
        let count = unsafe { host::fread(buf.as_mut_ptr().cast(), 1, buf.len(), self.handle) };
        if count == 0 && unsafe { host::ferror(self.handle) } != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "file read failed"));
        }
        Ok(count)
    }
}
impl Read for &File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: as above.
        if buf.is_empty() { return Ok(0) }
        let count = unsafe { host::fread(buf.as_mut_ptr().cast(), 1, buf.len(), self.handle) };
        if count == 0 && unsafe { host::ferror(self.handle) } != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "file read failed"));
        }
        Ok(count)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        let written = unsafe { host::fwrite(buf.as_ptr().cast(), 1, buf.len(), self.handle) };
        if written == 0 && !buf.is_empty() {
            Err(io::Error::from(io::ErrorKind::WriteZero))
        } else {
            Ok(written)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        // SAFETY: `handle` is a live FILE*.
        if unsafe { host::fflush(self.handle) } != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "fflush failed"));
        }
        Ok(())
    }
}

impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let (offset, whence) = match pos {
            SeekFrom::Start(n) => (n as i64, 0),
            SeekFrom::Current(n) => (n, 1),
            SeekFrom::End(n) => (n, 2),
        };
        // SAFETY: `handle` is a live FILE*.
        if unsafe { host::fseek(self.handle, offset as _, whence) } != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek failed"));
        }
        // SAFETY: as above.
        let position = unsafe { host::ftell(self.handle) };
        if position < 0 {
            Err(io::Error::from(io::ErrorKind::Other))
        } else {
            Ok(position as u64)
        }
    }
}

#[derive(Clone, Debug)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions { read: false, write: false, append: false, truncate: false, create: false, create_new: false }
    }
    pub fn read(&mut self, value: bool) -> &mut Self {
        self.read = value;
        self
    }
    pub fn write(&mut self, value: bool) -> &mut Self {
        self.write = value;
        self
    }
    pub fn append(&mut self, value: bool) -> &mut Self {
        self.append = value;
        self
    }
    pub fn truncate(&mut self, value: bool) -> &mut Self {
        self.truncate = value;
        self
    }
    pub fn create(&mut self, value: bool) -> &mut Self {
        self.create = value;
        self
    }
    pub fn create_new(&mut self, value: bool) -> &mut Self {
        self.create_new = value;
        self
    }

    pub fn open<P: AsRef<Path>>(&self, path: P) -> io::Result<File> {
        let path = path.as_ref();
        if self.create_new {
            if !self.write && !self.append {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "exclusive creation requires write access"));
            }
            let path = c_path(path)?;
            let mut exists = 0;
            // Native open(O_CREAT|O_EXCL), not a check-then-fopen race.
            let handle = unsafe { host::pokenx_fs_create_new(path.as_ptr(), self.read as _, self.append as _, &mut exists) };
            if handle.is_null() {
                return Err(io::Error::new(if exists != 0 { io::ErrorKind::AlreadyExists } else { io::ErrorKind::Other },
                                         "could not exclusively create file"));
            }
            return Ok(File { handle });
        }
        let exists = metadata(path).is_ok();
        if !exists && !(self.create || self.create_new) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let writing = self.write || self.append;
        // stdio modes: r (must exist), w (truncate/create), a (append/create), r+ (read-write, must exist).
        let mode = if self.append {
            if self.read { "a+b" } else { "ab" }
        } else if writing && (self.truncate || (!exists && !self.read)) {
            if self.read { "w+b" } else { "wb" }
        } else if writing {
            if exists { "r+b" } else { "w+b" }
        } else {
            "rb"
        };
        File::open_with(path, mode)
    }
}

pub fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    path.as_ref().canonicalize()
}

pub fn exists<P: AsRef<Path>>(path: P) -> io::Result<bool> {
    Ok(metadata(path).is_ok())
}

pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    let mut file = File::open_buffered(path.as_ref(), "rb", 1 << 20)?;
    let mut data = Vec::new();
    // Reserve the whole file at once: growing by doubling copies a 22 MiB game executable several times and
    // briefly holds ~2x its size, which matters on a console with little memory. A failed reservation is an
    // error, not an abort.
    if let Ok(meta) = file.metadata() {
        let wanted = meta.len() as usize;
        if wanted > 0 && data.try_reserve_exact(wanted).is_err() {
            return Err(io::Error::new(io::ErrorKind::OutOfMemory, "not enough memory to read the file"));
        }
    }
    file.read_to_end(&mut data)?;
    Ok(data)
}

pub fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let bytes = read(path)?;
    String::from_utf8(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "stream did not contain valid UTF-8"))
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(contents.as_ref())?;
    file.finish()
}

pub fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<u64> {
    let mut input = File::open(from)?;
    let mut output = File::create(to)?;
    let mut buffer = alloc_crate::vec![0u8; 32 * 1024];
    let mut total = 0u64;
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            output.finish()?;
            return Ok(total);
        }
        output.write_all(&buffer[..count])?;
        total += count as u64;
    }
}

pub fn create_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let c = c_path(path.as_ref())?;
    // SAFETY: NUL-terminated path.
    if unsafe { host::pokenx_fs_mkdir(c.as_ptr()) } == 0 {
        Ok(())
    } else if path.as_ref().is_dir() {
        Err(io::Error::from(io::ErrorKind::AlreadyExists))
    } else {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

pub fn create_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        if !parent.to_str().unwrap_or("").is_empty() && !parent.exists() {
            create_dir_all(parent)?;
        }
    }
    match create_dir(path) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    }
}

pub fn remove_file<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let c = c_path(path.as_ref())?;
    // SAFETY: NUL-terminated path.
    if unsafe { host::pokenx_fs_remove_file(c.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

pub fn remove_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let c = c_path(path.as_ref())?;
    // SAFETY: NUL-terminated path.
    if unsafe { host::pokenx_fs_remove_dir(c.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    for entry in read_dir(path)? {
        let entry = entry?;
        if entry.metadata()?.is_dir() {
            remove_dir_all(entry.path())?;
        } else {
            remove_file(entry.path())?;
        }
    }
    remove_dir(path)
}

pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
    let from = c_path(from.as_ref())?;
    let to = c_path(to.as_ref())?;
    // SAFETY: both paths are NUL-terminated.
    if unsafe { host::pokenx_fs_rename(from.as_ptr(), to.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

pub struct DirEntry {
    path: PathBuf,
    kind: u32,
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn file_name(&self) -> crate::horizon::ffi::OsString {
        self.path.file_name().map(|name| name.to_os_string()).unwrap_or_default()
    }
    pub fn metadata(&self) -> io::Result<Metadata> {
        metadata(&self.path)
    }
    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(FileType(self.kind))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileType(u32);

impl FileType {
    pub fn is_file(&self) -> bool {
        self.0 == KIND_FILE
    }
    pub fn is_dir(&self) -> bool {
        self.0 == KIND_DIR
    }
}

pub struct ReadDir {
    handle: *mut c_void,
    base: PathBuf,
}

pub fn read_dir<P: AsRef<Path>>(path: P) -> io::Result<ReadDir> {
    let c = c_path(path.as_ref())?;
    // SAFETY: NUL-terminated path.
    let handle = unsafe { host::pokenx_fs_opendir(c.as_ptr()) };
    if handle.is_null() {
        Err(io::Error::from(io::ErrorKind::NotFound))
    } else {
        Ok(ReadDir { handle, base: path.as_ref().to_path_buf() })
    }
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut name = [0 as c_char; 512];
        let mut kind = 0u32;
        loop {
            // SAFETY: `name` is valid for `name.len()` bytes and `kind` for one u32.
            let status = unsafe { host::pokenx_fs_readdir(self.handle, name.as_mut_ptr(), name.len(), &mut kind) };
            if status < 0 {
                return Some(Err(io::Error::from(io::ErrorKind::Other)));
            }
            if status == 0 {
                return None;
            }
            // SAFETY: the host NUL-terminates the name on success.
            let text = unsafe { core::ffi::CStr::from_ptr(name.as_ptr()) }.to_string_lossy();
            if text == "." || text == ".." {
                continue;
            }
            return Some(Ok(DirEntry { path: self.base.join(text.as_ref()), kind }));
        }
    }
}

impl Drop for ReadDir {
    fn drop(&mut self) {
        // SAFETY: `handle` came from `pokenx_fs_opendir` and is closed once.
        unsafe { host::pokenx_fs_closedir(self.handle) };
    }
}
