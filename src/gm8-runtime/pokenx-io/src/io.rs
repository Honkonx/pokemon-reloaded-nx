//! `alloc`-only subset of `std::io`: `Read`, `Write`, `Seek`, `Cursor`, `Error`.
//! Only compiled without the `std` feature. Semantics follow `std::io` for the
//! parts the OpenGMK parser uses.

use alloc::{boxed::Box, string::String, vec::Vec};
use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    InvalidInput,
    InvalidData,
    UnexpectedEof,
    WriteZero,
    Interrupted,
    Unsupported,
    OutOfMemory,
    Other,
}

impl ErrorKind {
    fn describe(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "entity not found",
            ErrorKind::PermissionDenied => "permission denied",
            ErrorKind::AlreadyExists => "entity already exists",
            ErrorKind::InvalidInput => "invalid input parameter",
            ErrorKind::InvalidData => "invalid data",
            ErrorKind::UnexpectedEof => "unexpected end of file",
            ErrorKind::WriteZero => "write zero",
            ErrorKind::Interrupted => "operation interrupted",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::OutOfMemory => "out of memory",
            ErrorKind::Other => "other error",
        }
    }
}

pub struct Error {
    kind: ErrorKind,
    message: Option<Box<str>>,
}

pub type Result<T> = core::result::Result<T, Error>;

impl Error {
    pub fn new<M: Into<String>>(kind: ErrorKind, message: M) -> Self {
        Self { kind, message: Some(message.into().into_boxed_str()) }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self { kind, message: None }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(message) => write!(f, "Custom {{ kind: {:?}, error: {:?} }}", self.kind, message),
            None => write!(f, "Kind({:?})", self.kind),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(message) => f.write_str(message),
            None => f.write_str(self.kind.describe()),
        }
    }
}

impl core::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.read(buf) {
                Ok(0) => break,
                Ok(count) => buf = &mut buf[count..],
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        if buf.is_empty() {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::UnexpectedEof, "failed to fill whole buffer"))
        }
    }

    fn read_to_end(&mut self, out: &mut Vec<u8>) -> Result<usize> {
        let start = out.len();
        let mut chunk = [0u8; 4096];
        loop {
            match self.read(&mut chunk) {
                Ok(0) => return Ok(out.len() - start),
                Ok(count) => {
                    out.try_reserve(count)
                        .map_err(|_| Error::new(ErrorKind::InvalidData, "stream allocation failed"))?;
                    out.extend_from_slice(&chunk[..count]);
                },
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }

    fn read_to_string(&mut self, out: &mut String) -> Result<usize> {
        let mut bytes = Vec::new();
        let count = self.read_to_end(&mut bytes)?;
        let text = core::str::from_utf8(&bytes)
            .map_err(|_| Error::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8"))?;
        out.push_str(text);
        Ok(count)
    }

    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }

    fn bytes(self) -> Bytes<Self>
    where
        Self: Sized,
    {
        Bytes { inner: self }
    }
}

/// Iterator over the bytes of a reader (`Read::bytes`).
pub struct Bytes<R> {
    inner: R,
}

impl<R: Read> Iterator for Bytes<R> {
    type Item = Result<u8>;

    fn next(&mut self) -> Option<Result<u8>> {
        let mut byte = [0u8; 1];
        loop {
            return match self.inner.read(&mut byte) {
                Ok(0) => None,
                Ok(_) => Some(Ok(byte[0])),
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => Some(Err(error)),
            }
        }
    }
}

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize>;
    fn flush(&mut self) -> Result<()>;

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        struct Adapter<'a, T: ?Sized> {
            inner: &'a mut T,
            error: Option<Error>,
        }
        impl<T: Write + ?Sized> fmt::Write for Adapter<'_, T> {
            fn write_str(&mut self, text: &str) -> fmt::Result {
                match self.inner.write_all(text.as_bytes()) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        self.error = Some(error);
                        Err(fmt::Error)
                    },
                }
            }
        }
        let mut adapter = Adapter { inner: self, error: None };
        match fmt::write(&mut adapter, args) {
            Ok(()) => Ok(()),
            Err(_) => Err(adapter.error.unwrap_or_else(|| Error::new(ErrorKind::Other, "formatter error"))),
        }
    }

    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => return Err(Error::new(ErrorKind::WriteZero, "failed to write whole buffer")),
                Ok(count) => buf = &buf[count..],
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

pub trait Seek {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64>;

    fn rewind(&mut self) -> Result<()> {
        self.seek(SeekFrom::Start(0)).map(|_| ())
    }

    fn stream_position(&mut self) -> Result<u64> {
        self.seek(SeekFrom::Current(0))
    }
}

impl<R: Read + ?Sized> Read for &mut R {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (**self).read(buf)
    }
}

impl<W: Write + ?Sized> Write for &mut W {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (**self).write(buf)
    }
    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }
}

impl<S: Seek + ?Sized> Seek for &mut S {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        (**self).seek(pos)
    }
}

impl Read for &[u8] {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let count = buf.len().min(self.len());
        buf[..count].copy_from_slice(&self[..count]);
        *self = &self[count..];
        Ok(count)
    }
}

impl Write for Vec<u8> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// In-memory reader/writer with a position, like `std::io::Cursor`.
#[derive(Clone, Debug, Default)]
pub struct Cursor<T> {
    inner: T,
    pos: u64,
}

impl<T> Cursor<T> {
    pub const fn new(inner: T) -> Self {
        Self { inner, pos: 0 }
    }
    pub fn into_inner(self) -> T {
        self.inner
    }
    pub const fn get_ref(&self) -> &T {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
    pub const fn position(&self) -> u64 {
        self.pos
    }
    pub fn set_position(&mut self, pos: u64) {
        self.pos = pos;
    }
}

impl<T: AsRef<[u8]>> Read for Cursor<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let data = self.inner.as_ref();
        let start = (self.pos as usize).min(data.len());
        let count = buf.len().min(data.len() - start);
        buf[..count].copy_from_slice(&data[start..start + count]);
        self.pos += count as u64;
        Ok(count)
    }
}

impl<T: AsRef<[u8]>> Seek for Cursor<T> {
    fn seek(&mut self, style: SeekFrom) -> Result<u64> {
        let (base, offset) = match style {
            SeekFrom::Start(n) => {
                self.pos = n;
                return Ok(n);
            }
            SeekFrom::End(n) => (self.inner.as_ref().len() as u64, n),
            SeekFrom::Current(n) => (self.pos, n),
        };
        match base.checked_add_signed(offset) {
            Some(n) => {
                self.pos = n;
                Ok(n)
            }
            None => Err(Error::new(ErrorKind::InvalidInput, "invalid seek to a negative or overflowing position")),
        }
    }
}

fn write_at(data: &mut [u8], pos: u64, buf: &[u8]) -> usize {
    let start = (pos as usize).min(data.len());
    let count = buf.len().min(data.len() - start);
    data[start..start + count].copy_from_slice(&buf[..count]);
    count
}

impl Write for Cursor<&mut [u8]> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let count = write_at(self.inner, self.pos, buf);
        self.pos += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

fn vec_write(vec: &mut Vec<u8>, pos: u64, buf: &[u8]) -> usize {
    let pos = pos as usize;
    if vec.len() < pos {
        vec.resize(pos, 0);
    }
    let overlap = (vec.len() - pos).min(buf.len());
    vec[pos..pos + overlap].copy_from_slice(&buf[..overlap]);
    vec.extend_from_slice(&buf[overlap..]);
    buf.len()
}

impl Write for Cursor<Vec<u8>> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let count = vec_write(&mut self.inner, self.pos, buf);
        self.pos += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Write for Cursor<&mut Vec<u8>> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let count = vec_write(self.inner, self.pos, buf);
        self.pos += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_reads_seeks_and_hits_eof() {
        let mut cursor = Cursor::new(alloc::vec![1u8, 2, 3, 4, 5]);
        let mut two = [0u8; 2];
        cursor.read_exact(&mut two).unwrap();
        assert_eq!(two, [1, 2]);
        assert_eq!(cursor.seek(SeekFrom::Current(1)).unwrap(), 3);
        cursor.read_exact(&mut two).unwrap();
        assert_eq!(two, [4, 5]);
        assert_eq!(cursor.read_exact(&mut two).unwrap_err().kind(), ErrorKind::UnexpectedEof);
        assert_eq!(cursor.seek(SeekFrom::End(-1)).unwrap(), 4);
        assert!(cursor.seek(SeekFrom::Current(-10)).is_err());
    }

    #[test]
    fn writes_into_slice_vec_and_cursor_vec() {
        let mut backing = [0u8; 4];
        let mut slice_cursor = Cursor::new(&mut backing[..]);
        slice_cursor.write_all(&[9, 8]).unwrap();
        assert_eq!(slice_cursor.write(&[7, 6, 5]).unwrap(), 2);
        assert_eq!(backing, [9, 8, 7, 6]);

        let mut cursor = Cursor::new(Vec::new());
        cursor.write_all(b"abcd").unwrap();
        cursor.set_position(1);
        cursor.write_all(b"ZZ").unwrap();
        cursor.write_all(b"yz").unwrap();
        assert_eq!(cursor.into_inner(), b"aZZyz");
    }

    #[test]
    fn read_to_end_and_slice_reader() {
        let mut reader: &[u8] = b"hello";
        let mut out = Vec::new();
        assert_eq!(reader.read_to_end(&mut out).unwrap(), 5);
        assert_eq!(out, b"hello");
    }

    #[test]
    fn error_displays_message_or_kind() {
        assert_eq!(Error::new(ErrorKind::InvalidData, "bad header").to_string_for_test(), "bad header");
        assert_eq!(Error::from(ErrorKind::UnexpectedEof).to_string_for_test(), "unexpected end of file");
    }

    trait ToStringForTest {
        fn to_string_for_test(&self) -> String;
    }
    impl ToStringForTest for Error {
        fn to_string_for_test(&self) -> String {
            use core::fmt::Write as _;
            let mut s = String::new();
            write!(s, "{}", self).unwrap();
            s
        }
    }
}
