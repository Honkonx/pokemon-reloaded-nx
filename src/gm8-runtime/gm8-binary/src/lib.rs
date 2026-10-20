#![no_std]

//! Bounded slice reader shared by the Switch-compatible GameMaker parsers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    OutOfBounds,
    LimitExceeded,
}

pub struct SliceReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> SliceReader<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub fn at(bytes: &'a [u8], position: usize) -> Result<Self, ReadError> {
        if position > bytes.len() {
            return Err(ReadError::OutOfBounds);
        }
        Ok(Self { bytes, position })
    }

    pub const fn position(&self) -> usize {
        self.position
    }

    pub const fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub fn take(&mut self, length: usize) -> Result<&'a [u8], ReadError> {
        let end = self.position.checked_add(length).ok_or(ReadError::OutOfBounds)?;
        let value = self.bytes.get(self.position..end).ok_or(ReadError::OutOfBounds)?;
        self.position = end;
        Ok(value)
    }

    pub fn skip(&mut self, length: usize) -> Result<(), ReadError> {
        self.take(length).map(|_| ())
    }

    pub fn read_u32_le(&mut self) -> Result<u32, ReadError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn read_u64_le(&mut self) -> Result<u64, ReadError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]))
    }

    pub fn read_limited_u32_le(&mut self, max_length: usize) -> Result<usize, ReadError> {
        let length = usize::try_from(self.read_u32_le()?).map_err(|_| ReadError::OutOfBounds)?;
        if length > max_length {
            return Err(ReadError::LimitExceeded);
        }
        Ok(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_values_and_tracks_position() {
        let mut reader = SliceReader::new(&[0x78, 0x56, 0x34, 0x12, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]);
        assert_eq!(reader.read_u32_le(), Ok(0x1234_5678));
        assert_eq!(reader.position(), 4);
        assert_eq!(reader.remaining(), 8);
        assert_eq!(reader.read_u64_le(), Ok(0x0102_0304_0506_0708));
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn rejects_truncated_reads_without_advancing() {
        let mut reader = SliceReader::new(&[1, 2, 3]);
        assert_eq!(reader.read_u32_le(), Err(ReadError::OutOfBounds));
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn enforces_length_budget_before_payload_read() {
        let mut reader = SliceReader::new(&[9, 0, 0, 0]);
        assert_eq!(reader.read_limited_u32_le(8), Err(ReadError::LimitExceeded));
        assert_eq!(reader.position(), 4);
    }
}
