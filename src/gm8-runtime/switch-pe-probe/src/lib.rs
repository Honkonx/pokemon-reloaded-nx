#![no_std]

//! A small, allocation-free PE envelope check for the Switch porting experiment.
//!
//! This deliberately does not parse GameMaker data. A successful result only says that
//! the input has the DOS marker and an i386 PE/COFF header in the shape expected by the
//! first checks in OpenGMK's `gm8exe::reader::from_exe`.

const DOS_PE_OFFSET_FIELD: usize = 0x3c;
const PE_SIGNATURE: &[u8; 4] = b"PE\0\0";
const IMAGE_FILE_MACHINE_I386: u16 = 0x014c;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeHeader {
    pub pe_offset: u32,
    pub machine: u16,
    pub section_count: u16,
    pub optional_header_size: u16,
    pub characteristics: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    MissingMzSignature,
    MissingPeOffset,
    PeOffsetOutOfBounds,
    MissingPeSignature,
    MissingCoffHeader,
    NotI386,
}

/// Parse the DOS/PE/COFF envelope without allocation or operating-system APIs.
///
/// This is a portability probe, not the GameMaker 8 reader and not a validator of
/// executable safety. It never executes code from the supplied PE image.
pub fn parse_pe_header(bytes: &[u8]) -> Result<PeHeader, ParseError> {
    let pe_offset = read_pe_offset(bytes)?;
    let pe = usize::try_from(pe_offset).map_err(|_| ParseError::PeOffsetOutOfBounds)?;
    let header = bytes.get(pe..).ok_or(ParseError::PeOffsetOutOfBounds)?;
    parse_pe_header_parts(pe_offset, bytes, header)
}

/// Parse the 64-byte DOS prefix and a separate PE-signature/COFF-header window.
/// This lets a filesystem caller inspect a large executable with two bounded reads.
pub fn parse_pe_header_parts(
    pe_offset: u32,
    dos_header: &[u8],
    pe_header: &[u8],
) -> Result<PeHeader, ParseError> {
    if dos_header.get(..2) != Some(b"MZ") {
        return Err(ParseError::MissingMzSignature);
    }
    if read_pe_offset(dos_header)? != pe_offset {
        return Err(ParseError::PeOffsetOutOfBounds);
    }
    if pe_header.get(..PE_SIGNATURE.len()) != Some(PE_SIGNATURE) {
        return Err(ParseError::MissingPeSignature);
    }
    // COFF IMAGE_FILE_HEADER immediately follows the four-byte PE signature.
    let header = pe_header
        .get(PE_SIGNATURE.len()..PE_SIGNATURE.len() + 20)
        .ok_or(ParseError::MissingCoffHeader)?;
    let read_u16 = |offset: usize| u16::from_le_bytes([header[offset], header[offset + 1]]);
    let machine = read_u16(0);
    if machine != IMAGE_FILE_MACHINE_I386 {
        return Err(ParseError::NotI386);
    }

    Ok(PeHeader {
        pe_offset,
        machine,
        section_count: read_u16(2),
        optional_header_size: read_u16(16),
        characteristics: read_u16(18),
    })
}

/// Read `e_lfanew` from a DOS header without requiring the full executable image.
pub fn read_pe_offset(dos_header: &[u8]) -> Result<u32, ParseError> {
    if dos_header.get(..2) != Some(b"MZ") {
        return Err(ParseError::MissingMzSignature);
    }
    let pe_offset_bytes = dos_header
        .get(DOS_PE_OFFSET_FIELD..DOS_PE_OFFSET_FIELD + 4)
        .ok_or(ParseError::MissingPeOffset)?;
    Ok(u32::from_le_bytes([
        pe_offset_bytes[0],
        pe_offset_bytes[1],
        pe_offset_bytes[2],
        pe_offset_bytes[3],
    ]))
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;

    fn synthetic_i386_pe() -> std::vec::Vec<u8> {
        let pe_offset = 0x40usize;
        let mut image = vec![0u8; pe_offset + 4 + 20];
        image[..2].copy_from_slice(b"MZ");
        image[DOS_PE_OFFSET_FIELD..DOS_PE_OFFSET_FIELD + 4]
            .copy_from_slice(&(pe_offset as u32).to_le_bytes());
        image[pe_offset..pe_offset + 4].copy_from_slice(PE_SIGNATURE);
        let coff = pe_offset + 4;
        image[coff..coff + 2].copy_from_slice(&IMAGE_FILE_MACHINE_I386.to_le_bytes());
        image[coff + 2..coff + 4].copy_from_slice(&3u16.to_le_bytes());
        image[coff + 16..coff + 18].copy_from_slice(&0xe0u16.to_le_bytes());
        image[coff + 18..coff + 20].copy_from_slice(&0x010fu16.to_le_bytes());
        image
    }

    #[test]
    fn accepts_synthetic_i386_pe_envelope() {
        let parsed = parse_pe_header(&synthetic_i386_pe()).unwrap();
        assert_eq!(parsed.pe_offset, 0x40);
        assert_eq!(parsed.machine, IMAGE_FILE_MACHINE_I386);
        assert_eq!(parsed.section_count, 3);
        assert_eq!(parsed.optional_header_size, 0xe0);
    }

    #[test]
    fn accepts_bounded_dos_and_pe_windows() {
        let image = synthetic_i386_pe();
        let parsed = parse_pe_header_parts(0x40, &image[..64], &image[0x40..0x40 + 24]).unwrap();
        assert_eq!(parsed.machine, IMAGE_FILE_MACHINE_I386);
        assert_eq!(parsed.section_count, 3);
        assert_eq!(parse_pe_header_parts(0x80, &image[..64], &image[0x40..0x40 + 24]), Err(ParseError::PeOffsetOutOfBounds));
        assert_eq!(parse_pe_header_parts(0x40, &image[..64], &image[0x40..0x40 + 23]), Err(ParseError::MissingCoffHeader));
    }

    #[test]
    fn rejects_truncated_or_non_i386_images() {
        assert_eq!(parse_pe_header(b"MZ"), Err(ParseError::MissingPeOffset));
        let mut wrong_machine = synthetic_i386_pe();
        wrong_machine[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes());
        assert_eq!(parse_pe_header(&wrong_machine), Err(ParseError::NotI386));
    }
}
