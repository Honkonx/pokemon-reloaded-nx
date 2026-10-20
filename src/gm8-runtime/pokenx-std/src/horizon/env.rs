//! `std::env` on Horizon: working directory through newlib, a fixed temp directory, no real environment.

use super::host;
use crate::horizon::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};
use alloc_crate::{ffi::CString, string::String, vec::Vec};
use core::ffi::{c_char, CStr};

/// Where games that use `temp_dir` write on Horizon. Created on demand by the host layer.
pub const TEMP_DIR: &str = "sdmc:/switch/pokenx/cache/tmp";

pub fn temp_dir() -> PathBuf {
    PathBuf::from(TEMP_DIR)
}

pub fn current_dir() -> io::Result<PathBuf> {
    let mut buffer = [0 as c_char; 1024];
    // SAFETY: `buffer` is valid for `buffer.len()` bytes.
    let result = unsafe { host::getcwd(buffer.as_mut_ptr(), buffer.len()) };
    if result.is_null() {
        return Err(io::Error::new(io::ErrorKind::Other, "getcwd failed"));
    }
    // SAFETY: getcwd NUL-terminates on success.
    let text = unsafe { CStr::from_ptr(buffer.as_ptr()) };
    Ok(PathBuf::from(String::from_utf8_lossy(text.to_bytes()).into_owned()))
}

pub fn set_current_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let text = path.as_ref().to_str().ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let c = CString::new(text).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: `c` is NUL-terminated.
    if unsafe { host::chdir(c.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

pub fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
    let name = CString::new(key.as_ref().to_str()?).ok()?;
    // SAFETY: `name` is NUL-terminated.
    let value = unsafe { host::getenv(name.as_ptr()) };
    if value.is_null() {
        None
    } else {
        // SAFETY: getenv returns a NUL-terminated string.
        let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
        Some(OsString::from(String::from_utf8_lossy(bytes).into_owned()))
    }
}

pub fn var<K: AsRef<OsStr>>(key: K) -> Result<String, VarError> {
    var_os(key).and_then(|value| value.into_string().ok()).ok_or(VarError::NotPresent)
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum VarError {
    NotPresent,
    NotUnicode(OsString),
}

pub fn args() -> alloc_crate::vec::IntoIter<String> {
    Vec::<String>::new().into_iter()
}
