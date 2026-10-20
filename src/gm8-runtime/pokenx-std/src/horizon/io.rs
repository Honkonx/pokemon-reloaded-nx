//! `std::io`: the `pokenx-io` core plus buffered adapters and the log-backed standard streams.

use alloc_crate::{string::String, vec::Vec};
use core::fmt;

pub use pokenx_io::io::*;

const BUFFER_SIZE: usize = 8 * 1024;

/// `std::io::BufRead` subset.
pub trait BufRead: Read {
    fn fill_buf(&mut self) -> Result<&[u8]>;
    fn consume(&mut self, amount: usize);

    fn read_until(&mut self, delimiter: u8, out: &mut Vec<u8>) -> Result<usize> {
        let mut total = 0;
        loop {
            let (done, used) = {
                let available = self.fill_buf()?;
                match available.iter().position(|&byte| byte == delimiter) {
                    Some(index) => {
                        out.extend_from_slice(&available[..=index]);
                        (true, index + 1)
                    }
                    None => {
                        out.extend_from_slice(available);
                        (false, available.len())
                    }
                }
            };
            self.consume(used);
            total += used;
            if done || used == 0 {
                return Ok(total);
            }
        }
    }

    fn read_line(&mut self, out: &mut String) -> Result<usize> {
        let mut bytes = Vec::new();
        let count = self.read_until(b'\n', &mut bytes)?;
        let text = core::str::from_utf8(&bytes).map_err(|_| Error::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8"))?;
        out.push_str(text);
        Ok(count)
    }
}

#[derive(Debug)]
pub struct BufReader<R> {
    inner: R,
    buffer: Vec<u8>,
    position: usize,
    filled: usize,
}

impl<R: Read> BufReader<R> {
    pub fn new(inner: R) -> Self {
        Self::with_capacity(BUFFER_SIZE, inner)
    }
    pub fn with_capacity(capacity: usize, inner: R) -> Self {
        Self { inner, buffer: alloc_crate::vec![0; capacity.max(1)], position: 0, filled: 0 }
    }
    pub fn get_ref(&self) -> &R {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        if self.position == self.filled && out.len() >= self.buffer.len() {
            return self.inner.read(out);
        }
        let count = {
            let available = self.fill_buf()?;
            let count = available.len().min(out.len());
            out[..count].copy_from_slice(&available[..count]);
            count
        };
        self.consume(count);
        Ok(count)
    }
}

impl<R: Read> BufRead for BufReader<R> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.position >= self.filled {
            self.filled = self.inner.read(&mut self.buffer)?;
            self.position = 0;
        }
        Ok(&self.buffer[self.position..self.filled])
    }
    fn consume(&mut self, amount: usize) {
        self.position = (self.position + amount).min(self.filled);
    }
}

impl<R: Read + Seek> Seek for BufReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let result = match pos {
            SeekFrom::Current(offset) => {
                let remainder = (self.filled - self.position) as i64;
                self.inner.seek(SeekFrom::Current(offset - remainder))?
            }
            other => self.inner.seek(other)?,
        };
        self.position = 0;
        self.filled = 0;
        Ok(result)
    }
}

#[derive(Debug)]
pub struct BufWriter<W: Write> {
    inner: Option<W>,
    buffer: Vec<u8>,
}

impl<W: Write> BufWriter<W> {
    pub fn new(inner: W) -> Self {
        Self::with_capacity(BUFFER_SIZE, inner)
    }
    pub fn with_capacity(capacity: usize, inner: W) -> Self {
        Self { inner: Some(inner), buffer: Vec::with_capacity(capacity) }
    }
    fn flush_buffer(&mut self) -> Result<()> {
        if let Some(inner) = self.inner.as_mut() {
            if !self.buffer.is_empty() {
                inner.write_all(&self.buffer)?;
                self.buffer.clear();
            }
        }
        Ok(())
    }
    pub fn get_ref(&self) -> &W {
        self.inner.as_ref().expect("BufWriter used after into_inner")
    }
    pub fn get_mut(&mut self) -> &mut W {
        self.inner.as_mut().expect("BufWriter used after into_inner")
    }
    pub fn into_inner(mut self) -> core::result::Result<W, Error> {
        self.flush_buffer()?;
        Ok(self.inner.take().expect("BufWriter used after into_inner"))
    }
}

impl<W: Write> Write for BufWriter<W> {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        if self.buffer.len() + data.len() > self.buffer.capacity() {
            self.flush_buffer()?;
        }
        if data.len() >= self.buffer.capacity() {
            self.inner.as_mut().expect("BufWriter used after into_inner").write(data)
        } else {
            self.buffer.extend_from_slice(data);
            Ok(data.len())
        }
    }
    fn flush(&mut self) -> Result<()> {
        self.flush_buffer()?;
        match self.inner.as_mut() {
            Some(inner) => inner.flush(),
            None => Ok(()),
        }
    }
}

impl<W: Write + Seek> Seek for BufWriter<W> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.flush_buffer()?;
        self.get_mut().seek(pos)
    }
}

impl<W: Write> Drop for BufWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush_buffer();
    }
}

/// Standard output and error go to the SD log through the host.
pub struct Stdout;
pub struct Stderr;

pub fn stdout() -> Stdout {
    Stdout
}
pub fn stderr() -> Stderr {
    Stderr
}

fn log_bytes(bytes: &[u8]) {
    // The host expects a NUL-terminated line; split on newlines and bound the length.
    let mut line = [0u8; 256];
    for chunk in bytes.split(|&byte| byte == b'\n') {
        if chunk.is_empty() {
            continue;
        }
        for part in chunk.chunks(line.len() - 1) {
            line[..part.len()].copy_from_slice(part);
            line[part.len()] = 0;
            // SAFETY: `line` is NUL-terminated and valid for the call.
            unsafe { super::host::pokenx_host_log(line.as_ptr().cast()) };
        }
    }
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        log_bytes(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        log_bytes(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Collects the pieces `format_args!` produces (a literal, then each argument, then the line break) so a `println!`
/// reaches the host as one log line instead of one line per piece ("launch: rooms ", "442", ".", "126822", "ms" ...).
/// A line longer than the buffer is written in several parts.
struct LineCollector {
    bytes: [u8; 512],
    len: usize,
}

impl LineCollector {
    const fn new() -> Self {
        Self { bytes: [0; 512], len: 0 }
    }

    fn flush(&mut self) {
        if self.len > 0 {
            log_bytes(&self.bytes[..self.len]);
            self.len = 0;
        }
    }
}

impl fmt::Write for LineCollector {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for &byte in text.as_bytes() {
            if self.len == self.bytes.len() {
                self.flush();
            }
            self.bytes[self.len] = byte;
            self.len += 1;
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
    let mut line = LineCollector::new();
    let _ = fmt::write(&mut line, args);
    line.flush();
}

#[doc(hidden)]
pub fn _eprint(args: fmt::Arguments<'_>) {
    let mut line = LineCollector::new();
    let _ = fmt::write(&mut line, args);
    line.flush();
}
