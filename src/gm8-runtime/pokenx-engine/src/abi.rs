//! C ABI shared with `app/engine/pokenx_engine.h`. Keep both in sync.

use core::ffi::c_void;

pub const ABI_VERSION: u32 = 3;

/// Pad button bits are the `gm8_input::BUTTON_*` constants; the C++ side maps
/// `HidNpadButton_*` to them.
pub const PAD_CONNECTED: u32 = 1;

/// Services provided by the C++ entrypoint. All pointers must stay valid until
/// `pokenx_engine_shutdown`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PokenxHost {
    pub abi_version: u32,
    /// `eglGetProcAddress` for a NUL-terminated name.
    pub get_proc: Option<unsafe extern "C" fn(name: *const u8) -> *const c_void>,
    /// Appends one NUL-terminated line to the SD log.
    pub log: Option<unsafe extern "C" fn(line: *const u8)>,
    /// `eglSwapBuffers` on the window surface.
    pub swap_buffers: Option<unsafe extern "C" fn()>,
    /// `eglSwapInterval`; returns non-zero on success.
    pub set_swap_interval: Option<unsafe extern "C" fn(interval: i32) -> i32>,
    /// Seconds since the Unix epoch (UTC), or a negative value when unknown.
    pub unix_seconds: Option<unsafe extern "C" fn() -> i64>,
    /// Fresh HID sample, including inside GML io_handle/keyboard_wait loops.
    /// 1 = sample, 0 = exit requested, negative = unavailable. GL thread only.
    pub poll_input: Option<unsafe extern "C" fn(pad: *mut PokenxPad) -> i32>,
}

/// One HID snapshot, taken once per frame by the C++ side.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PokenxPad {
    pub flags: u32,
    pub buttons: u32,
    pub left_x: i32,
    pub left_y: i32,
    pub right_x: i32,
    pub right_y: i32,
}

/// Running totals for the host's periodic report (`pokenx_engine_stats`). Times are nanoseconds.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PokenxStats {
    /// Time the game spent in its own `sleep()` calls and how many calls.
    pub paused_ns: u64,
    pub pauses: u64,
    /// Files read from the card (cache misses) and their bytes.
    pub files_read: u64,
    pub bytes_read: u64,
    /// Room changes, the time they took in total, and the longest one since the previous call.
    pub room_changes: u64,
    pub room_ns: u64,
    pub room_max_ns: u64,
    /// Rooms compiled ahead during pauses and the time spent doing it.
    pub warmed_rooms: u64,
    pub warm_ns: u64,
}

pub const RESULT_OK: i32 = 0;
/// The core already presented the frame (`eglSwapBuffers` done); the host must not swap again.
pub const RESULT_PRESENTED: i32 = 1;
/// The game ended (game_end, window close request).
pub const RESULT_FINISHED: i32 = 2;
/// Manual-draw game: its GML owns screen_refresh; do not swap a partial buffer.
pub const RESULT_NO_PRESENT: i32 = 3;
pub const RESULT_BAD_ARGUMENT: i32 = -1;
pub const RESULT_BAD_ABI: i32 = -2;
pub const RESULT_NO_GL: i32 = -3;
pub const RESULT_NOT_INITIALIZED: i32 = -4;
pub const RESULT_GL_ERROR: i32 = -5;
/// No game could be loaded (missing folder/executable, parse or launch error; details in the log).
pub const RESULT_NO_GAME: i32 = -6;
/// The running game raised an error and stopped.
pub const RESULT_GAME_ERROR: i32 = -7;

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;
    #[test]
    fn ffi_layout_matches_the_libnx_host_header() {
        assert_eq!(core::mem::size_of::<PokenxHost>(), 56);
        assert_eq!(core::mem::offset_of!(PokenxHost, poll_input), 48);
        assert_eq!(core::mem::size_of::<PokenxPad>(), 24);
        assert_eq!(core::mem::size_of::<PokenxStats>(), 72);
    }
}
