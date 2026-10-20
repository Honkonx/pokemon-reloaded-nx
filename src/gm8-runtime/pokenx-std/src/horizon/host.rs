//! C symbols the Horizon build links against. They are provided by newlib/libnx
//! and by `app/engine/host_services.cpp` (structured file-system helpers whose
//! struct layouts are better owned by C than guessed from Rust).

use core::ffi::{c_char, c_int, c_long, c_void};

/// Result of `pokenx_fs_stat`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HostStat {
    pub size: u64,
    /// 0 = missing, 1 = file, 2 = directory.
    pub kind: u32,
}

pub const KIND_FILE: u32 = 1;
pub const KIND_DIR: u32 = 2;

#[cfg(not(test))]
extern "C" {
    // newlib stdio. `FILE*` stays opaque.
    pub fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    pub fn fclose(file: *mut c_void) -> c_int;
    pub fn fread(buf: *mut c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    pub fn fwrite(buf: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    pub fn fseek(file: *mut c_void, offset: c_long, whence: c_int) -> c_int;
    pub fn ftell(file: *mut c_void) -> c_long;
    pub fn fflush(file: *mut c_void) -> c_int;
    pub fn ferror(file: *mut c_void) -> c_int;
    pub fn chdir(path: *const c_char) -> c_int;
    pub fn getcwd(buf: *mut c_char, size: usize) -> *mut c_char;
    pub fn getenv(name: *const c_char) -> *const c_char;

    // host_services.cpp
    pub fn pokenx_fs_stat(path: *const c_char, out: *mut HostStat) -> c_int;
    /// setvbuf(file, NULL, _IOFBF, size): newlib reads a file in chunks of its buffer size (1 KiB by default).
    pub fn pokenx_fs_read_buffer(file: *mut c_void, size: usize) -> c_int;
    pub fn pokenx_fs_set_len(file: *mut c_void, size: u64) -> c_int;
    pub fn pokenx_fs_sync_all(file: *mut c_void) -> c_int;
    pub fn pokenx_fs_create_new(path: *const c_char, readable: c_int, append: c_int, exists: *mut c_int) -> *mut c_void;
    pub fn pokenx_fs_mkdir(path: *const c_char) -> c_int;
    pub fn pokenx_fs_remove_file(path: *const c_char) -> c_int;
    pub fn pokenx_fs_remove_dir(path: *const c_char) -> c_int;
    pub fn pokenx_fs_rename(from: *const c_char, to: *const c_char) -> c_int;
    pub fn pokenx_fs_opendir(path: *const c_char) -> *mut c_void;
    /// Returns 1 and fills `name`/`kind` for the next entry, 0 at the end, <0 on error.
    pub fn pokenx_fs_readdir(dir: *mut c_void, name: *mut c_char, capacity: usize, kind: *mut u32) -> c_int;
    pub fn pokenx_fs_closedir(dir: *mut c_void);
    pub fn pokenx_host_log(line: *const c_char);
    pub fn pokenx_host_sleep_ns(nanoseconds: u64);
}

/// Host-test doubles: the same functions implemented over the real `std`, so the Horizon shim's
/// path, file and directory logic can be exercised on a development machine.
#[cfg(test)]
pub use fake::*;

#[cfg(test)]
mod fake {
    use super::{HostStat, KIND_DIR, KIND_FILE};
    use alloc_crate::{boxed::Box, string::String, vec, vec::Vec};
    use core::ffi::{c_char, c_int, c_long, c_void};
    use real_std::{
        ffi::CStr,
        fs::{File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        path::PathBuf,
    };

    struct Stream { file: File, error: bool }
    real_std::thread_local! {
        static FAIL_SYNC: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        static FAIL_FLUSH: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        static FAIL_CLOSE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    }
    pub fn fail_next_sync() { FAIL_SYNC.set(true); }
    pub fn fail_next_flush() { FAIL_FLUSH.set(true); }
    pub fn fail_next_close() { FAIL_CLOSE.set(true); }

    fn text(ptr: *const c_char) -> String {
        unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
    }

    pub unsafe fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void {
        let mode = text(mode);
        let mut options = OpenOptions::new();
        match mode.as_str() {
            "rb" | "r" => options.read(true),
            "wb" | "w" => options.write(true).create(true).truncate(true),
            "ab" | "a" => options.append(true).create(true),
            "r+b" | "r+" => options.read(true).write(true),
            "w+b" | "w+" => options.read(true).write(true).create(true).truncate(true),
            "a+b" | "a+" => options.read(true).append(true).create(true),
            _ => return core::ptr::null_mut(),
        };
        match options.open(text(path)) {
            Ok(file) => Box::into_raw(Box::new(Stream { file, error: false })).cast(),
            Err(_) => core::ptr::null_mut(),
        }
    }

    pub unsafe fn fclose(file: *mut c_void) -> c_int {
        let mut stream = Box::from_raw(file.cast::<Stream>());
        if FAIL_CLOSE.replace(false) || stream.file.flush().is_err() { -1 } else { 0 }
    }

    pub unsafe fn pokenx_fs_create_new(path: *const c_char, readable: c_int, append: c_int, exists: *mut c_int) -> *mut c_void {
        *exists = 0;
        match OpenOptions::new().create_new(true).read(readable != 0).write(true).append(append != 0).open(text(path)) {
            Ok(file) => Box::into_raw(Box::new(Stream { file, error: false })).cast(),
            Err(error) => { *exists = i32::from(error.kind() == real_std::io::ErrorKind::AlreadyExists); core::ptr::null_mut() },
        }
    }

    pub unsafe fn fread(buf: *mut c_void, size: usize, count: usize, file: *mut c_void) -> usize {
        let stream = &mut *file.cast::<Stream>();
        let file = &mut stream.file;
        let slice = core::slice::from_raw_parts_mut(buf.cast::<u8>(), size * count);
        let mut total = 0;
        while total < slice.len() {
            match file.read(&mut slice[total..]) {
                Ok(0) => break,
                Err(_) => { stream.error = true; break },
                Ok(n) => total += n,
            }
        }
        total / size.max(1)
    }

    pub unsafe fn fwrite(buf: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize {
        let stream = &mut *file.cast::<Stream>();
        let file = &mut stream.file;
        let slice = core::slice::from_raw_parts(buf.cast::<u8>(), size * count);
        match file.write_all(slice) {
            Ok(()) => count,
            Err(_) => { stream.error = true; 0 },
        }
    }

    pub unsafe fn fseek(file: *mut c_void, offset: c_long, whence: c_int) -> c_int {
        let stream = &mut *file.cast::<Stream>();
        let file = &mut stream.file;
        let target = match whence {
            0 => SeekFrom::Start(offset as u64),
            1 => SeekFrom::Current(offset as i64),
            _ => SeekFrom::End(offset as i64),
        };
        if file.seek(target).is_ok() { 0 } else { -1 }
    }

    pub unsafe fn ftell(file: *mut c_void) -> c_long {
        let stream = &mut *file.cast::<Stream>();
        let file = &mut stream.file;
        file.stream_position().map(|p| p as c_long).unwrap_or(-1)
    }

    pub unsafe fn fflush(file: *mut c_void) -> c_int {
        let stream = &mut *file.cast::<Stream>();
        let file = &mut stream.file;
        if !FAIL_FLUSH.replace(false) && file.flush().is_ok() { 0 } else { stream.error = true; -1 }
    }

    pub unsafe fn ferror(file: *mut c_void) -> c_int {
        i32::from((*file.cast::<Stream>()).error)
    }

    pub unsafe fn pokenx_fs_set_len(file: *mut c_void, size: u64) -> c_int {
        let stream = &mut *file.cast::<Stream>();
        if size > i64::MAX as u64 || stream.file.flush().is_err() { return -1 }
        if stream.file.set_len(size).is_ok() { 0 } else { -1 }
    }

    pub unsafe fn pokenx_fs_sync_all(file: *mut c_void) -> c_int {
        if fflush(file) != 0 || FAIL_SYNC.replace(false) { return -1 }
        if (*file.cast::<Stream>()).file.sync_all().is_ok() { 0 } else { -1 }
    }

    pub unsafe fn chdir(path: *const c_char) -> c_int {
        if real_std::env::set_current_dir(text(path)).is_ok() { 0 } else { -1 }
    }

    pub unsafe fn getcwd(buf: *mut c_char, size: usize) -> *mut c_char {
        let cwd = real_std::env::current_dir().unwrap().to_string_lossy().into_owned();
        if cwd.len() + 1 > size {
            return core::ptr::null_mut();
        }
        core::ptr::copy_nonoverlapping(cwd.as_ptr(), buf.cast::<u8>(), cwd.len());
        *buf.add(cwd.len()) = 0;
        buf
    }

    pub unsafe fn getenv(_name: *const c_char) -> *const c_char {
        core::ptr::null()
    }

    pub unsafe fn pokenx_fs_read_buffer(_file: *mut c_void, _size: usize) -> c_int { 0 }

    pub unsafe fn pokenx_fs_stat(path: *const c_char, out: *mut HostStat) -> c_int {
        match real_std::fs::metadata(text(path)) {
            Ok(meta) => {
                *out = HostStat { size: meta.len(), kind: if meta.is_dir() { KIND_DIR } else { KIND_FILE } };
                0
            },
            Err(_) => {
                *out = HostStat::default();
                -1
            },
        }
    }

    pub unsafe fn pokenx_fs_mkdir(path: *const c_char) -> c_int {
        if real_std::fs::create_dir(text(path)).is_ok() { 0 } else { -1 }
    }

    pub unsafe fn pokenx_fs_remove_file(path: *const c_char) -> c_int {
        if real_std::fs::remove_file(text(path)).is_ok() { 0 } else { -1 }
    }

    pub unsafe fn pokenx_fs_remove_dir(path: *const c_char) -> c_int {
        if real_std::fs::remove_dir(text(path)).is_ok() { 0 } else { -1 }
    }

    pub unsafe fn pokenx_fs_rename(from: *const c_char, to: *const c_char) -> c_int {
        if real_std::fs::rename(text(from), text(to)).is_ok() { 0 } else { -1 }
    }

    struct DirState {
        entries: Vec<(String, u32)>,
        next: usize,
    }

    pub unsafe fn pokenx_fs_opendir(path: *const c_char) -> *mut c_void {
        let Ok(read) = real_std::fs::read_dir(PathBuf::from(text(path))) else { return core::ptr::null_mut() };
        let mut entries: Vec<(String, u32)> = vec![(".".into(), KIND_DIR), ("..".into(), KIND_DIR)];
        for entry in read.flatten() {
            let kind = if entry.path().is_dir() { KIND_DIR } else { KIND_FILE };
            entries.push((entry.file_name().to_string_lossy().into_owned(), kind));
        }
        Box::into_raw(Box::new(DirState { entries, next: 0 })).cast()
    }

    pub unsafe fn pokenx_fs_readdir(dir: *mut c_void, name: *mut c_char, capacity: usize, kind: *mut u32) -> c_int {
        let state = &mut *dir.cast::<DirState>();
        let Some((entry, entry_kind)) = state.entries.get(state.next) else { return 0 };
        if entry.len() + 1 > capacity {
            return -1;
        }
        core::ptr::copy_nonoverlapping(entry.as_ptr(), name.cast::<u8>(), entry.len());
        *name.add(entry.len()) = 0;
        *kind = *entry_kind;
        state.next += 1;
        1
    }

    pub unsafe fn pokenx_fs_closedir(dir: *mut c_void) {
        drop(Box::from_raw(dir.cast::<DirState>()));
    }

    pub unsafe fn pokenx_host_log(_line: *const c_char) {}

    pub unsafe fn pokenx_host_sleep_ns(nanoseconds: u64) {
        real_std::thread::sleep(real_std::time::Duration::from_nanos(nanoseconds));
    }
}
