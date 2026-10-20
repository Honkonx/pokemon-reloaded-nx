#![no_std]

extern crate alloc;

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InflateError {
    EmptyInput,
    ZeroOutputLimit,
    InvalidOrTruncatedStream,
    OutputLimitExceeded,
}

/// Decompresses a zlib-wrapped DEFLATE stream with a hard output-size cap.
///
/// This is the bounded equivalent needed by OpenGMK's zlib asset records. The
/// caller must validate the expected uncompressed size from the enclosing GM8
/// record and provide that size (or a stricter budget) as `max_output_bytes`.
/// The returned vector never grows beyond the supplied cap.
pub fn decompress_zlib_bounded(input: &[u8], max_output_bytes: usize) -> Result<Vec<u8>, InflateError> {
    if input.is_empty() {
        return Err(InflateError::EmptyInput);
    }
    if max_output_bytes == 0 {
        return Err(InflateError::ZeroOutputLimit);
    }
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(input, max_output_bytes).map_err(|error| {
        if error.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput {
            InflateError::OutputLimitExceeded
        } else {
            InflateError::InvalidOrTruncatedStream
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompresses_zlib_data_under_the_output_cap() {
        let input = miniz_oxide::deflate::compress_to_vec_zlib(b"GM8 synthetic asset", 6);
        let output = decompress_zlib_bounded(&input, 64).unwrap();
        assert_eq!(output, b"GM8 synthetic asset");
    }

    #[test]
    fn rejects_output_that_exceeds_the_cap() {
        let input = miniz_oxide::deflate::compress_to_vec_zlib(&[b'A'; 4096], 6);
        assert_eq!(decompress_zlib_bounded(&input, 128), Err(InflateError::OutputLimitExceeded));
    }

    #[test]
    fn rejects_invalid_truncated_and_empty_streams() {
        assert_eq!(decompress_zlib_bounded(&[], 10), Err(InflateError::EmptyInput));
        let input = miniz_oxide::deflate::compress_to_vec_zlib(b"payload", 6);
        assert_eq!(decompress_zlib_bounded(&input[..input.len() - 1], 64), Err(InflateError::InvalidOrTruncatedStream));
        assert_eq!(decompress_zlib_bounded(b"not zlib", 64), Err(InflateError::InvalidOrTruncatedStream));
    }
}
