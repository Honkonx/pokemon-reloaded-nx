//! Minimal OpenGL entry points resolved through the host's `eglGetProcAddress`. Used by the
//! diagnostic frame (no game loaded); the game's renderer loads its own functions.

use core::ffi::c_void;
use core::mem::transmute;

pub const COLOR_BUFFER_BIT: u32 = 0x0000_4000;
pub const RENDERER: u32 = 0x1F01;
pub const VERSION: u32 = 0x1F02;

type GetProc = unsafe extern "C" fn(*const u8) -> *const c_void;

#[derive(Clone, Copy)]
pub struct GlApi {
    pub viewport: unsafe extern "C" fn(i32, i32, i32, i32),
    pub clear_color: unsafe extern "C" fn(f32, f32, f32, f32),
    pub clear: unsafe extern "C" fn(u32),
    pub get_error: unsafe extern "C" fn() -> u32,
    pub get_string: unsafe extern "C" fn(u32) -> *const u8,
}

impl GlApi {
    /// # Safety
    /// `get_proc` must be a valid `eglGetProcAddress` with a current GL context.
    pub unsafe fn load(get_proc: GetProc) -> Option<Self> {
        let viewport = get_proc(c"glViewport".as_ptr().cast());
        let clear_color = get_proc(c"glClearColor".as_ptr().cast());
        let clear = get_proc(c"glClear".as_ptr().cast());
        let get_error = get_proc(c"glGetError".as_ptr().cast());
        let get_string = get_proc(c"glGetString".as_ptr().cast());
        if [viewport, clear_color, clear, get_error, get_string].iter().any(|p| p.is_null()) {
            return None;
        }
        Some(Self {
            viewport: transmute(viewport),
            clear_color: transmute(clear_color),
            clear: transmute(clear),
            get_error: transmute(get_error),
            get_string: transmute(get_string),
        })
    }
}
