//! OpenGL context backend for Horizon: the host owns EGL (see `platform::host::GlHost`); this
//! type only exposes the entry points the renderer expects from a platform backend.

use crate::platform::{Connection, Window, host};
use std::ffi::{CString, c_void};

pub struct PlatformImpl {
    host: host::GlHost,
}

impl PlatformImpl {
    /// # Safety
    /// A GL context created by the host must be current on the calling thread.
    pub unsafe fn new(_connection: &Connection, _window: &Window) -> Result<Self, String> {
        match host::gl_host() {
            Some(host) => Ok(Self { host }),
            None => Err("no OpenGL host registered (platform::host::set_gl_host)".into()),
        }
    }

    pub unsafe fn get_function_loader() -> Result<Box<dyn FnMut(&'static str) -> *const c_void>, String> {
        let host = host::gl_host().ok_or_else(|| String::from("no OpenGL host registered"))?;
        Ok(Box::new(move |name: &'static str| {
            let cname = CString::new(name).expect("GL function names contain no NUL");
            // SAFETY: the host guarantees `get_proc` is a valid `eglGetProcAddress`.
            let pointer = unsafe { (host.get_proc)(cname.as_ptr().cast()) };
            if pointer.is_null() {
                // The first call through a null pointer would crash without a trace; leave the name in the log.
                println!("GL function missing: {name}");
            }
            pointer
        }))
    }

    pub unsafe fn clean_function_loader() {}

    pub unsafe fn swap_buffers(&self) {
        (self.host.swap_buffers)();
    }

    pub unsafe fn set_swap_interval(&self, n: u32) -> bool {
        (self.host.set_swap_interval)(n as i32) != 0
    }

    pub unsafe fn get_swap_interval(&self) -> u32 {
        // EGL has no query for the swap interval; the renderer only needs a plausible value.
        1
    }

    pub unsafe fn wait_vsync(&self) {
        // `eglSwapBuffers` already paces to the display when the interval is 1.
    }
}
