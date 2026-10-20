//! `std::path` for Horizon: forward-slash paths over UTF-8 text, including `sdmc:/...` device prefixes.

use crate::horizon::ffi::{OsStr, OsString};
use crate::horizon::fs;
use alloc_crate::{borrow::Cow, string::String};
use core::{borrow::Borrow, fmt, ops::Deref};

pub const MAIN_SEPARATOR: char = '/';
pub const MAIN_SEPARATOR_STR: &str = "/";

#[repr(transparent)]
pub struct Path(OsStr);

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathBuf(String);

fn text(path: &Path) -> &str {
    path.0.to_str().unwrap_or("")
}

impl Path {
    pub fn new<S: AsRef<OsStr> + ?Sized>(s: &S) -> &Path {
        // SAFETY: `Path` is `repr(transparent)` over `OsStr`.
        unsafe { &*(s.as_ref() as *const OsStr as *const Path) }
    }
    pub fn as_os_str(&self) -> &OsStr {
        &self.0
    }
    pub fn to_str(&self) -> Option<&str> {
        self.0.to_str()
    }
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        self.0.to_string_lossy()
    }
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf(String::from(text(self)))
    }
    pub fn display(&self) -> Display<'_> {
        Display(self)
    }
    pub fn is_absolute(&self) -> bool {
        let t = text(self);
        t.starts_with('/') || t.split('/').next().map_or(false, |first| first.ends_with(':'))
    }
    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }
    pub fn has_root(&self) -> bool {
        self.is_absolute()
    }

    /// Path without its final component, or `None` for the root / an empty path.
    pub fn parent(&self) -> Option<&Path> {
        let t = text(self).trim_end_matches('/');
        if t.is_empty() {
            return None;
        }
        match t.rfind('/') {
            Some(0) => Some(Path::new("/")),
            Some(index) => Some(Path::new(&t[..index])),
            None => {
                if t.ends_with(':') {
                    None
                } else {
                    Some(Path::new(""))
                }
            }
        }
    }

    pub fn file_name(&self) -> Option<&OsStr> {
        let t = text(self).trim_end_matches('/');
        let name = t.rsplit('/').next()?;
        if name.is_empty() || name == ".." || name.ends_with(':') {
            None
        } else {
            Some(OsStr::new(name))
        }
    }

    pub fn file_stem(&self) -> Option<&OsStr> {
        let name = self.file_name()?.to_str()?;
        if name == "." || name == ".." {
            return Some(OsStr::new(name));
        }
        match name.rfind('.') {
            Some(0) | None => Some(OsStr::new(name)),
            Some(index) => Some(OsStr::new(&name[..index])),
        }
    }

    pub fn extension(&self) -> Option<&OsStr> {
        let name = self.file_name()?.to_str()?;
        match name.rfind('.') {
            Some(0) | None => None,
            Some(index) => Some(OsStr::new(&name[index + 1..])),
        }
    }

    pub fn join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let mut buf = self.to_path_buf();
        buf.push(path);
        buf
    }

    pub fn with_extension<S: AsRef<OsStr>>(&self, extension: S) -> PathBuf {
        let mut buf = self.to_path_buf();
        buf.set_extension(extension);
        buf
    }

    pub fn with_file_name<S: AsRef<OsStr>>(&self, name: S) -> PathBuf {
        let mut buf = self.to_path_buf();
        buf.set_file_name(name);
        buf
    }

    pub fn starts_with<P: AsRef<Path>>(&self, base: P) -> bool {
        let mut mine = text(self).split('/').filter(|part| !part.is_empty());
        let rooted = text(self).starts_with('/') == text(base.as_ref()).starts_with('/');
        rooted && text(base.as_ref()).split('/').filter(|part| !part.is_empty()).all(|part| mine.next() == Some(part))
    }

    pub fn ends_with<P: AsRef<Path>>(&self, child: P) -> bool {
        let mine: alloc_crate::vec::Vec<&str> = text(self).split('/').filter(|part| !part.is_empty()).collect();
        let theirs: alloc_crate::vec::Vec<&str> = text(child.as_ref()).split('/').filter(|part| !part.is_empty()).collect();
        theirs.len() <= mine.len() && mine[mine.len() - theirs.len()..] == theirs[..]
    }

    pub fn strip_prefix<P: AsRef<Path>>(&self, base: P) -> Result<&Path, StripPrefixError> {
        let base_text = text(base.as_ref()).trim_end_matches('/');
        let mine = text(self);
        if let Some(rest) = mine.strip_prefix(base_text) {
            if rest.is_empty() || rest.starts_with('/') || base_text.ends_with(':') {
                return Ok(Path::new(rest.trim_start_matches('/')));
            }
        }
        Err(StripPrefixError(()))
    }

    pub fn components(&self) -> Components<'_> {
        Components { rest: text(self), started: false }
    }

    pub fn exists(&self) -> bool {
        fs::metadata(self).is_ok()
    }
    pub fn is_file(&self) -> bool {
        fs::metadata(self).map(|m| m.is_file()).unwrap_or(false)
    }
    pub fn is_dir(&self) -> bool {
        fs::metadata(self).map(|m| m.is_dir()).unwrap_or(false)
    }
    pub fn metadata(&self) -> crate::horizon::io::Result<fs::Metadata> {
        fs::metadata(self)
    }
    /// Horizon has no symlinks: the canonical form is the absolute lexical path.
    pub fn canonicalize(&self) -> crate::horizon::io::Result<PathBuf> {
        if !self.exists() {
            return Err(crate::horizon::io::Error::from(crate::horizon::io::ErrorKind::NotFound));
        }
        if self.is_absolute() {
            Ok(self.to_path_buf())
        } else {
            Ok(crate::horizon::env::current_dir()?.join(self))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripPrefixError(());

impl fmt::Display for StripPrefixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("prefix not found")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component<'a> {
    RootDir,
    CurDir,
    ParentDir,
    Normal(&'a OsStr),
}

impl<'a> Component<'a> {
    pub fn as_os_str(self) -> &'a OsStr {
        match self {
            Component::RootDir => OsStr::new("/"),
            Component::CurDir => OsStr::new("."),
            Component::ParentDir => OsStr::new(".."),
            Component::Normal(name) => name,
        }
    }
}

pub struct Components<'a> {
    rest: &'a str,
    started: bool,
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;
    fn next(&mut self) -> Option<Component<'a>> {
        if !self.started {
            self.started = true;
            if self.rest.starts_with('/') {
                self.rest = self.rest.trim_start_matches('/');
                return Some(Component::RootDir);
            }
        }
        loop {
            let trimmed = self.rest.trim_start_matches('/');
            if trimmed.is_empty() {
                return None;
            }
            let (part, rest) = trimmed.split_once('/').unwrap_or((trimmed, ""));
            self.rest = rest;
            match part {
                "." => continue,
                ".." => return Some(Component::ParentDir),
                name => return Some(Component::Normal(OsStr::new(name))),
            }
        }
    }
}

pub struct Display<'a>(&'a Path);

impl fmt::Display for Display<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(text(self.0))
    }
}
impl fmt::Debug for Display<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(text(self.0), f)
    }
}
impl fmt::Debug for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(text(self), f)
    }
}
impl PartialEq for Path {
    fn eq(&self, other: &Path) -> bool {
        text(self) == text(other)
    }
}
impl Eq for Path {}
impl PartialOrd for Path {
    fn partial_cmp(&self, other: &Path) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Path {
    fn cmp(&self, other: &Path) -> core::cmp::Ordering {
        text(self).cmp(text(other))
    }
}
impl core::hash::Hash for Path {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        text(self).hash(state)
    }
}
impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}
impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}
impl AsRef<Path> for String {
    fn as_ref(&self) -> &Path {
        Path::new(self.as_str())
    }
}
impl AsRef<Path> for OsStr {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}
impl AsRef<Path> for OsString {
    fn as_ref(&self) -> &Path {
        Path::new(self.as_os_str())
    }
}
impl AsRef<OsStr> for Path {
    fn as_ref(&self) -> &OsStr {
        &self.0
    }
}
impl alloc_crate::borrow::ToOwned for Path {
    type Owned = PathBuf;
    fn to_owned(&self) -> PathBuf {
        self.to_path_buf()
    }
}

impl PathBuf {
    pub fn new() -> PathBuf {
        PathBuf(String::new())
    }
    pub fn as_path(&self) -> &Path {
        Path::new(self.0.as_str())
    }
    pub fn push<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        if path.is_absolute() {
            self.0.clear();
        } else if !self.0.is_empty() && !self.0.ends_with('/') && !self.0.ends_with(':') {
            self.0.push('/');
        }
        self.0.push_str(text(path));
    }
    pub fn pop(&mut self) -> bool {
        match self.as_path().parent().map(|parent| parent.to_str().unwrap_or("").len()) {
            Some(length) => {
                self.0.truncate(length);
                true
            }
            None => false,
        }
    }
    pub fn set_file_name<S: AsRef<OsStr>>(&mut self, name: S) {
        if self.as_path().file_name().is_some() {
            self.pop();
        }
        self.push(Path::new(name.as_ref()));
    }
    pub fn set_extension<S: AsRef<OsStr>>(&mut self, extension: S) -> bool {
        let Some(stem) = self.as_path().file_stem().and_then(|stem| stem.to_str()).map(String::from) else {
            return false;
        };
        let extension = extension.as_ref().to_str().unwrap_or("");
        let mut name = stem;
        if !extension.is_empty() {
            name.push('.');
            name.push_str(extension);
        }
        self.set_file_name(name.as_str());
        true
    }
    pub fn into_os_string(self) -> OsString {
        OsString::from(self.0)
    }
    pub fn into_boxed_path(self) -> alloc_crate::boxed::Box<Path> {
        let boxed: alloc_crate::boxed::Box<str> = self.0.into_boxed_str();
        // SAFETY: `Path` is `repr(transparent)` over `OsStr`, which is over `str`.
        unsafe { alloc_crate::boxed::Box::from_raw(alloc_crate::boxed::Box::into_raw(boxed) as *mut Path) }
    }
}

impl Deref for PathBuf {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.as_path()
    }
}
impl Borrow<Path> for PathBuf {
    fn borrow(&self) -> &Path {
        self.as_path()
    }
}
impl AsRef<Path> for PathBuf {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}
impl AsRef<OsStr> for PathBuf {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(self.0.as_str())
    }
}
impl fmt::Debug for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0.as_str(), f)
    }
}
impl From<&str> for PathBuf {
    fn from(value: &str) -> PathBuf {
        PathBuf(String::from(value))
    }
}
impl From<String> for PathBuf {
    fn from(value: String) -> PathBuf {
        PathBuf(value)
    }
}
impl From<OsString> for PathBuf {
    fn from(value: OsString) -> PathBuf {
        PathBuf(value.into_string().unwrap_or_default())
    }
}
impl From<&OsStr> for PathBuf {
    fn from(value: &OsStr) -> PathBuf {
        PathBuf(String::from(value.to_str().unwrap_or("")))
    }
}
impl From<&Path> for PathBuf {
    fn from(value: &Path) -> PathBuf {
        value.to_path_buf()
    }
}
impl<P: AsRef<Path>> FromIterator<P> for PathBuf {
    fn from_iter<I: IntoIterator<Item = P>>(iter: I) -> PathBuf {
        let mut buf = PathBuf::new();
        for part in iter {
            buf.push(part);
        }
        buf
    }
}
