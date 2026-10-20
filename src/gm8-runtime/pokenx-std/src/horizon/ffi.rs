//! `std::ffi`: C strings from `core`/`alloc`, plus `OsStr`/`OsString`, which on Horizon are UTF-8 text.

use alloc_crate::{borrow::Cow, string::String};
use core::{fmt, ops::Deref};

pub use alloc_crate::ffi::{CString, NulError};
pub use core::ffi::{
    c_char, c_double, c_float, c_int, c_long, c_longlong, c_schar, c_short, c_uchar, c_uint, c_ulong, c_ulonglong,
    c_ushort, c_void, CStr,
};

#[repr(transparent)]
pub struct OsStr(str);

impl OsStr {
    pub fn new<S: AsRef<OsStr> + ?Sized>(s: &S) -> &OsStr {
        s.as_ref()
    }
    pub(crate) fn from_str(s: &str) -> &OsStr {
        // SAFETY: `OsStr` is `repr(transparent)` over `str`.
        unsafe { &*(s as *const str as *const OsStr) }
    }
    pub fn to_str(&self) -> Option<&str> {
        Some(&self.0)
    }
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.0)
    }
    pub fn to_os_string(&self) -> OsString {
        OsString(String::from(&self.0))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn as_encoded_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for OsStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
impl PartialEq for OsStr {
    fn eq(&self, other: &OsStr) -> bool {
        self.0 == other.0
    }
}
impl PartialEq<str> for OsStr {
    fn eq(&self, other: &str) -> bool {
        &self.0 == other
    }
}
impl Eq for OsStr {}
impl PartialOrd for OsStr {
    fn partial_cmp(&self, other: &OsStr) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OsStr {
    fn cmp(&self, other: &OsStr) -> core::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}
impl core::hash::Hash for OsStr {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}
impl AsRef<OsStr> for OsStr {
    fn as_ref(&self) -> &OsStr {
        self
    }
}
impl AsRef<OsStr> for str {
    fn as_ref(&self) -> &OsStr {
        OsStr::from_str(self)
    }
}
impl AsRef<OsStr> for String {
    fn as_ref(&self) -> &OsStr {
        OsStr::from_str(self)
    }
}
impl alloc_crate::borrow::ToOwned for OsStr {
    type Owned = OsString;
    fn to_owned(&self) -> OsString {
        self.to_os_string()
    }
}

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OsString(String);

impl OsString {
    pub fn new() -> Self {
        Self(String::new())
    }
    pub fn as_os_str(&self) -> &OsStr {
        OsStr::from_str(&self.0)
    }
    pub fn into_string(self) -> Result<String, OsString> {
        Ok(self.0)
    }
    pub fn push<T: AsRef<OsStr>>(&mut self, s: T) {
        self.0.push_str(&s.as_ref().0)
    }
}
impl Deref for OsString {
    type Target = OsStr;
    fn deref(&self) -> &OsStr {
        self.as_os_str()
    }
}
impl fmt::Debug for OsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
impl From<String> for OsString {
    fn from(s: String) -> Self {
        Self(s)
    }
}
impl From<&str> for OsString {
    fn from(s: &str) -> Self {
        Self(String::from(s))
    }
}
impl AsRef<OsStr> for OsString {
    fn as_ref(&self) -> &OsStr {
        self.as_os_str()
    }
}
impl core::borrow::Borrow<OsStr> for OsString {
    fn borrow(&self) -> &OsStr {
        self.as_os_str()
    }
}
