//! Fixed-buffer formatting so logging never allocates or panics.

use core::fmt::{self, Write};

// Long enough for a panic message with its file and line.
const LINE_CAPACITY: usize = 512;

pub struct LineBuf {
    bytes: [u8; LINE_CAPACITY],
    len: usize,
}

impl LineBuf {
    pub const fn new() -> Self {
        Self { bytes: [0; LINE_CAPACITY], len: 0 }
    }

    /// NUL-terminated view for the C log callback. Truncates silently.
    pub fn as_c_ptr(&mut self) -> *const core::ffi::c_char {
        let end = self.len.min(LINE_CAPACITY - 1);
        self.bytes[end] = 0;
        self.bytes.as_ptr().cast()
    }

    #[cfg(test)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len.min(LINE_CAPACITY - 1)]
    }
}

impl Write for LineBuf {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if self.len >= LINE_CAPACITY - 1 {
                break;
            }
            // Keep the log single-line and ASCII-safe for the SD text file.
            self.bytes[self.len] = if byte == b'\n' || byte == b'\r' { b' ' } else { byte };
            self.len += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_truncates_without_panicking() {
        let mut line = LineBuf::new();
        write!(line, "frame {} ok\n", 7).unwrap();
        assert_eq!(line.as_bytes(), b"frame 7 ok ");
        for _ in 0..(LINE_CAPACITY + 100) {
            line.write_str("x").unwrap();
        }
        assert_eq!(line.as_bytes().len(), LINE_CAPACITY - 1);
    }
}
