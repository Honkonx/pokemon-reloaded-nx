pub mod background;
pub mod code_action;
pub mod constant;
pub mod extension;
pub mod font;
pub mod included_file;
pub mod object;
pub mod path;
pub mod room;
pub mod script;
pub mod sound;
pub mod sprite;
pub mod timeline;
pub mod trigger;

pub use self::{
    background::Background,
    code_action::CodeAction,
    constant::Constant,
    extension::Extension,
    font::Font,
    included_file::IncludedFile,
    object::Object,
    path::Path,
    room::Room,
    script::Script,
    sound::{Sound, SoundKind},
    sprite::Sprite,
    timeline::Timeline,
    trigger::{Trigger, TriggerKind},
};

use crate::GameVersion;
use pokenx_io::byteorder::LE;
use core::fmt::{self, Display};
use pokenx_io::io;

pub trait Asset: Sized {
    /// Deserializes the asset from the format used in game executables.
    fn deserialize_exe(reader: impl io::Read, version: GameVersion, strict: bool) -> Result<Self, Error>;
    /// Serializes the asset to the format used in game executables.
    fn serialize_exe(&self, writer: impl io::Write, version: GameVersion) -> io::Result<()>;
}

#[derive(Debug)]
pub enum Error {
    IO(io::Error),
    MalformedData,
    VersionError { expected: u32, got: u32 },
}

impl core::error::Error for Error {}
impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            Error::IO(err) => format!("io error: {}", err),
            Error::VersionError { expected, got } => format!(
                "version error: expected {} ({}), found {} ({})",
                *expected,
                *expected as f32 / 100.0,
                *got,
                *got as f32 / 100.0
            ),
            Error::MalformedData => "malformed data while reading".into(),
        })
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::IO(err)
    }
}

impl From<(u32, u32)> for Error {
    fn from(version_error: (u32, u32)) -> Self {
        Error::VersionError { expected: version_error.0, got: version_error.1 }
    }
}

#[inline(always)]
fn assert_ver(got: u32, expected: u32) -> Result<(), Error> {
    if got != expected { Err(Error::VersionError { expected, got }) } else { Ok(()) }
}

#[inline(always)]
fn assert_ver_multiple(got: u32, expected: &[u32]) -> Result<(), Error> {
    if expected.contains(&got) { Ok(()) } else { Err(Error::VersionError { expected: expected[0], got }) }
}

/// A string of characters encoded as UTF-8.
#[derive(Default, PartialEq, Eq)]
pub struct PascalString(pub Box<[u8]>);

impl fmt::Debug for PascalString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Ok(string) = core::str::from_utf8(&self.0) {
            write!(f, "{string:?}")
        } else {
            write!(f, "PascalString({} bytes)", self.0.len())
        }
    }
}

impl Display for PascalString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

impl From<&str> for PascalString {
    fn from(s: &str) -> Self {
        PascalString(s.to_owned().into_boxed_str().into_boxed_bytes())
    }
}

/// Helper trait to read big blocks of raw data.
pub trait ReadChunk: io::Read {
    fn read_chunk(&mut self, len: usize) -> io::Result<Vec<u8>> {
        // File-supplied lengths are untrusted. Do not allocate the advertised
        // size before observing those bytes (a truncated Pascal string can
        // otherwise request gigabytes), or expose uninitialized Vec elements.
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        while buf.len() < len {
            let count = (len - buf.len()).min(chunk.len());
            self.read_exact(&mut chunk[..count])?;
            buf.try_reserve(count)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "resource allocation failed"))?;
            buf.extend_from_slice(&chunk[..count]);
        }
        Ok(buf)
    }
}
impl<R> ReadChunk for R where R: io::Read {}

// pascal-string extension for easy use
pub trait ReadPascalString: pokenx_io::byteorder::ReadBytesExt + io::Read + ReadChunk {
    fn read_pas_string(&mut self) -> io::Result<PascalString> {
        let len = self.read_u32::<LE>()? as usize;
        let buf = self.read_chunk(len)?;
        Ok(PascalString(buf.into_boxed_slice()))
    }
}
impl<R> ReadPascalString for R where R: pokenx_io::byteorder::ReadBytesExt + io::Read + ReadChunk {}

pub trait WritePascalString: pokenx_io::byteorder::WriteBytesExt + io::Write {
    fn write_pas_string(&mut self, s: &PascalString) -> io::Result<()> {
        self.write_u32::<LE>(s.0.len() as u32)?;
        self.write_all(&s.0)?;
        Ok(())
    }
}
impl<W> WritePascalString for W where W: pokenx_io::byteorder::WriteBytesExt + io::Write {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_truncated_chunks_and_pascal_strings_fail_without_large_allocations() {
        let mut chunk = io::Cursor::new(&[1u8][..]);
        assert!(chunk.read_chunk(usize::MAX).is_err());
        let mut string = io::Cursor::new(&[255u8; 4][..]);
        assert!(string.read_pas_string().is_err());
    }

    #[test]
    fn chunk_reader_handles_multiple_blocks_and_preserves_following_bytes() {
        let bytes = vec![42u8; 10_001];
        let mut source = io::Cursor::new(bytes.as_slice());
        assert_eq!(source.read_chunk(10_000).unwrap(), bytes[..10_000]);
        assert_eq!(source.read_chunk(1).unwrap(), [42]);
        assert!(source.read_chunk(0).unwrap().is_empty());
    }
}
