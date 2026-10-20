// SPDX-License-Identifier: GPL-2.0-only
//! Shader construction and bounded driver diagnostics for the OpenGMK renderer.
use super::gl;
use gl::types::{GLint, GLuint};

fn read_log(reported: GLint, read: impl FnOnce(&mut [u8]) -> GLint) -> String {
    const LIMIT: usize = 64 * 1024;
    if reported <= 1 { return "<no driver log>".into() }
    let len = (reported as usize).min(LIMIT);
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(len).is_err() { return "<driver log allocation failed>".into() }
    bytes.resize(len, 0);
    let written = read(&mut bytes);
    if written <= 0 { return "<no driver log>".into() }
    bytes.truncate((written as usize).min(len));
    if let Some(end) = bytes.iter().position(|&byte| byte == 0) { bytes.truncate(end); }
    let mut message = String::from_utf8_lossy(&bytes).into_owned();
    if message.is_empty() { message.push_str("<no driver log>"); }
    if reported as usize > LIMIT { message.push_str("\n<driver log truncated>"); }
    message
}

unsafe fn info_log(gl: &gl::Gl, id: GLuint, program: bool) -> String {
    let mut length = 0;
    if program { gl.GetProgramiv(id, gl::INFO_LOG_LENGTH, &mut length); }
    else { gl.GetShaderiv(id, gl::INFO_LOG_LENGTH, &mut length); }
    read_log(length, |buffer| {
        let mut written = 0;
        if program { gl.GetProgramInfoLog(id, buffer.len() as _, &mut written, buffer.as_mut_ptr().cast()); }
        else { gl.GetShaderInfoLog(id, buffer.len() as _, &mut written, buffer.as_mut_ptr().cast()); }
        written
    })
}

struct Pending<'a> { gl: &'a gl::Gl, vertex: GLuint, fragment: GLuint, program: GLuint }
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        unsafe {
            if self.program != 0 { self.gl.DeleteProgram(self.program); }
            if self.vertex != 0 { self.gl.DeleteShader(self.vertex); }
            if self.fragment != 0 { self.gl.DeleteShader(self.fragment); }
        }
    }
}

unsafe fn compile(gl: &gl::Gl, id: GLuint, kind: &str, source: &str) -> Result<(), String> {
    if id == 0 { return Err(format!("Failed to create {} shader (OpenGL code {})", kind, gl.GetError())) }
    let length = i32::try_from(source.len()).map_err(|_| "Shader source exceeds GL length range")?;
    gl.ShaderSource(id, 1, &source.as_ptr().cast(), &length);
    gl.CompileShader(id);
    let mut success = 0;
    gl.GetShaderiv(id, gl::COMPILE_STATUS, &mut success);
    if success != gl::TRUE as GLint {
        let log = info_log(gl, id, false);
        return Err(format!("Failed to compile {} shader (OpenGL code 0x{:04X}): {}", kind, gl.GetError(), log))
    }
    Ok(())
}

/// A compatible GL context must be current and all entry points loaded.
/// The caller owns the returned program. Failed builds release every object.
pub(crate) unsafe fn program(gl: &gl::Gl, vertex: &str, fragment: &str) -> Result<GLuint, String> {
    macro_rules! require {
        ($($entry:ident),*) => {$(
            if !gl.$entry.is_loaded() {
                return Err(format!("OpenGL shader entry point missing: gl{}", stringify!($entry)))
            }
        )*};
    }
    require!(CreateShader, ShaderSource, CompileShader, GetShaderiv, GetShaderInfoLog,
             CreateProgram, AttachShader, LinkProgram, GetProgramiv, GetProgramInfoLog,
             DetachShader, DeleteShader, DeleteProgram, GetError);
    let mut pending = Pending { gl, vertex: gl.CreateShader(gl::VERTEX_SHADER), fragment: 0, program: 0 };
    compile(gl, pending.vertex, "vertex", vertex)?;
    pending.fragment = gl.CreateShader(gl::FRAGMENT_SHADER);
    compile(gl, pending.fragment, "fragment", fragment)?;
    pending.program = gl.CreateProgram();
    if pending.program == 0 { return Err(format!("Failed to create shader program (OpenGL code {})", gl.GetError())) }
    gl.AttachShader(pending.program, pending.vertex);
    gl.AttachShader(pending.program, pending.fragment);
    gl.LinkProgram(pending.program);
    let mut success = 0;
    gl.GetProgramiv(pending.program, gl::LINK_STATUS, &mut success);
    if success != gl::TRUE as GLint {
        let log = info_log(gl, pending.program, true);
        return Err(format!("Failed to link shader program (OpenGL code 0x{:04X}): {}", gl.GetError(), log))
    }
    gl.DetachShader(pending.program, pending.vertex);
    gl.DetachShader(pending.program, pending.fragment);
    Ok(core::mem::replace(&mut pending.program, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn driver_log_handles_empty_invalid_lengths_and_utf8() {
        for length in [i32::MIN, -1, 0, 1] {
            assert_eq!(read_log(length, |_| panic!("empty log must not call driver reader")), "<no driver log>");
        }
        assert_eq!(read_log(16, |_| 0), "<no driver log>");
        assert_eq!(read_log(16, |_| -1), "<no driver log>");
        assert_eq!(read_log(4, |out| { out.copy_from_slice(b"bad\0"); 3 }), "bad");
        assert_eq!(read_log(4, |out| { out[0] = 255; 1 }), "\u{fffd}");
        assert_eq!(read_log(4, |out| { out.copy_from_slice(b"err\0"); i32::MAX }), "err");
        assert!(read_log(i32::MAX, |out| { assert_eq!(out.len(), 65536); out[0] = b'x'; 1 }).ends_with("<driver log truncated>"));
    }

    #[test]
    fn missing_shader_entry_points_return_error_without_calling_a_null_function() {
        let gl = gl::Gl::load_with(|_| core::ptr::null());
        let error = unsafe { program(&gl, "", "") }.unwrap_err();
        assert!(error.contains("entry point missing"));
        assert!(unsafe { super::super::driver_string(&gl, gl::VERSION, "GL_VERSION") }.is_err());
    }
}
