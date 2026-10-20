//! Little-endian subset of the `byteorder` API used by `gm8exe` and the emulator.

use crate::io;

/// Marker for little-endian, matching `byteorder::LE`.
pub enum LE {}
/// Marker for big-endian, matching `byteorder::BE`.
pub enum BE {}

pub trait ByteOrder {
    fn u16_from(bytes: [u8; 2]) -> u16;
    fn u32_from(bytes: [u8; 4]) -> u32;
    fn u64_from(bytes: [u8; 8]) -> u64;
    fn u16_to(value: u16) -> [u8; 2];
    fn u32_to(value: u32) -> [u8; 4];
    fn u64_to(value: u64) -> [u8; 8];
}

impl ByteOrder for LE {
    fn u16_from(b: [u8; 2]) -> u16 { u16::from_le_bytes(b) }
    fn u32_from(b: [u8; 4]) -> u32 { u32::from_le_bytes(b) }
    fn u64_from(b: [u8; 8]) -> u64 { u64::from_le_bytes(b) }
    fn u16_to(v: u16) -> [u8; 2] { v.to_le_bytes() }
    fn u32_to(v: u32) -> [u8; 4] { v.to_le_bytes() }
    fn u64_to(v: u64) -> [u8; 8] { v.to_le_bytes() }
}

impl ByteOrder for BE {
    fn u16_from(b: [u8; 2]) -> u16 { u16::from_be_bytes(b) }
    fn u32_from(b: [u8; 4]) -> u32 { u32::from_be_bytes(b) }
    fn u64_from(b: [u8; 8]) -> u64 { u64::from_be_bytes(b) }
    fn u16_to(v: u16) -> [u8; 2] { v.to_be_bytes() }
    fn u32_to(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn u64_to(v: u64) -> [u8; 8] { v.to_be_bytes() }
}

pub trait ReadBytesExt: io::Read {
    fn read_u8(&mut self) -> io::Result<u8> {
        let mut buf = [0u8; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }
    fn read_i8(&mut self) -> io::Result<i8> {
        Ok(self.read_u8()? as i8)
    }
    fn read_u16<T: ByteOrder>(&mut self) -> io::Result<u16> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(T::u16_from(buf))
    }
    fn read_i16<T: ByteOrder>(&mut self) -> io::Result<i16> {
        Ok(self.read_u16::<T>()? as i16)
    }
    fn read_u32<T: ByteOrder>(&mut self) -> io::Result<u32> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(T::u32_from(buf))
    }
    fn read_i32<T: ByteOrder>(&mut self) -> io::Result<i32> {
        Ok(self.read_u32::<T>()? as i32)
    }
    fn read_u64<T: ByteOrder>(&mut self) -> io::Result<u64> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(T::u64_from(buf))
    }
    fn read_i64<T: ByteOrder>(&mut self) -> io::Result<i64> {
        Ok(self.read_u64::<T>()? as i64)
    }
    fn read_f32<T: ByteOrder>(&mut self) -> io::Result<f32> {
        Ok(f32::from_bits(self.read_u32::<T>()?))
    }
    fn read_f64<T: ByteOrder>(&mut self) -> io::Result<f64> {
        Ok(f64::from_bits(self.read_u64::<T>()?))
    }
}
impl<R: io::Read + ?Sized> ReadBytesExt for R {}

pub trait WriteBytesExt: io::Write {
    fn write_u8(&mut self, value: u8) -> io::Result<()> {
        self.write_all(&[value])
    }
    fn write_i8(&mut self, value: i8) -> io::Result<()> {
        self.write_u8(value as u8)
    }
    fn write_u16<T: ByteOrder>(&mut self, value: u16) -> io::Result<()> {
        self.write_all(&T::u16_to(value))
    }
    fn write_i16<T: ByteOrder>(&mut self, value: i16) -> io::Result<()> {
        self.write_u16::<T>(value as u16)
    }
    fn write_u32<T: ByteOrder>(&mut self, value: u32) -> io::Result<()> {
        self.write_all(&T::u32_to(value))
    }
    fn write_i32<T: ByteOrder>(&mut self, value: i32) -> io::Result<()> {
        self.write_u32::<T>(value as u32)
    }
    fn write_u64<T: ByteOrder>(&mut self, value: u64) -> io::Result<()> {
        self.write_all(&T::u64_to(value))
    }
    fn write_i64<T: ByteOrder>(&mut self, value: i64) -> io::Result<()> {
        self.write_u64::<T>(value as u64)
    }
    fn write_f32<T: ByteOrder>(&mut self, value: f32) -> io::Result<()> {
        self.write_u32::<T>(value.to_bits())
    }
    fn write_f64<T: ByteOrder>(&mut self, value: f64) -> io::Result<()> {
        self.write_u64::<T>(value.to_bits())
    }
}
impl<W: io::Write + ?Sized> WriteBytesExt for W {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_little_endian_values() {
        let mut out = io::Cursor::new(alloc::vec::Vec::new());
        out.write_u32::<LE>(0xDEAD_BEEF).unwrap();
        out.write_i32::<LE>(-5).unwrap();
        out.write_f64::<LE>(1.5).unwrap();
        out.write_u16::<LE>(0x1234).unwrap();
        out.write_u8(9).unwrap();
        let bytes = out.into_inner();
        assert_eq!(&bytes[..4], &[0xEF, 0xBE, 0xAD, 0xDE]);
        let mut input = io::Cursor::new(bytes);
        assert_eq!(input.read_u32::<LE>().unwrap(), 0xDEAD_BEEF);
        assert_eq!(input.read_i32::<LE>().unwrap(), -5);
        assert_eq!(input.read_f64::<LE>().unwrap(), 1.5);
        assert_eq!(input.read_u16::<LE>().unwrap(), 0x1234);
        assert_eq!(input.read_u8().unwrap(), 9);
        assert!(input.read_u8().is_err());
    }

    #[test]
    fn big_endian_differs() {
        let mut input = io::Cursor::new(alloc::vec![1u8, 2, 3, 4]);
        assert_eq!(input.read_u32::<BE>().unwrap(), 0x0102_0304);
    }
}
