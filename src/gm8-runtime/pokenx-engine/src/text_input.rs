//! Small owned UTF-8 bridge to the statically linked libnx host service.
//! Independent from the versioned Engine/GL ABI, like the other native helpers.
use alloc::{
    ffi::CString,
    string::{String, ToString},
};
use core::ffi::c_char;

const OUTPUT_BYTES: usize = 1025;
type NativeKeyboard = unsafe extern "C" fn(*const c_char, *const c_char, *mut u8, usize) -> i32;

#[cfg(target_os = "horizon")]
unsafe extern "C" {
    fn pokenx_host_text_input(prompt: *const c_char, initial: *const c_char, out: *mut u8, capacity: usize) -> i32;
}

#[cfg(target_os = "horizon")]
pub fn request(prompt: &str, initial: &str) -> Result<Option<String>, String> {
    request_with(pokenx_host_text_input, prompt, initial)
}

#[cfg_attr(not(any(test, target_os = "horizon")), allow(dead_code))]
fn request_with(native: NativeKeyboard, prompt: &str, initial: &str) -> Result<Option<String>, String> {
    let prompt = CString::new(prompt).map_err(|_| "dialog prompt contains NUL".to_string())?;
    let initial = CString::new(initial).map_err(|_| "dialog default contains NUL".to_string())?;
    let mut output = [0xffu8; OUTPUT_BYTES];
    // Native writes only within this capacity, and never retains these pointers.
    match unsafe { native(prompt.as_ptr(), initial.as_ptr(), output.as_mut_ptr(), output.len()) } {
        0 => Ok(None),
        1 => {
            let end = output
                .iter()
                .position(|byte| *byte == 0)
                .ok_or_else(|| "system keyboard returned unterminated text".to_string())?;
            let text = core::str::from_utf8(&output[..end])
                .map_err(|_| "system keyboard returned invalid UTF-8".to_string())?;
            Ok(Some(text.to_string()))
        },
        _ => Err("system keyboard unavailable or failed".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn native(prompt: *const c_char, initial: *const c_char, out: *mut u8, capacity: usize) -> i32 {
        assert_eq!(core::ffi::CStr::from_ptr(prompt).to_bytes(), b"Name?");
        assert_eq!(core::ffi::CStr::from_ptr(initial).to_bytes(), b"Player");
        let bytes = "José\0".as_bytes();
        assert!(capacity > bytes.len());
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
        1
    }
    #[test]
    fn confirmed_unicode_is_owned_and_nul_inputs_are_rejected() {
        assert_eq!(request_with(native, "Name?", "Player").unwrap(), Some("José".into()));
        assert!(request_with(native, "Name?\0hidden", "Player").is_err());
    }
    #[test]
    fn cancel_error_and_malformed_native_results_are_distinct() {
        unsafe extern "C" fn cancel(_: *const c_char, _: *const c_char, _: *mut u8, _: usize) -> i32 {
            0
        }
        unsafe extern "C" fn failure(_: *const c_char, _: *const c_char, _: *mut u8, _: usize) -> i32 {
            -1
        }
        unsafe extern "C" fn unterminated(_: *const c_char, _: *const c_char, _: *mut u8, _: usize) -> i32 {
            1
        }
        unsafe extern "C" fn bad_utf8(_: *const c_char, _: *const c_char, out: *mut u8, _: usize) -> i32 {
            *out = 0xff;
            *out.add(1) = 0;
            1
        }
        assert_eq!(request_with(cancel, "", "").unwrap(), None);
        for native in [failure as NativeKeyboard, unterminated, bad_utf8] {
            assert!(request_with(native, "", "").is_err());
        }
    }
}
