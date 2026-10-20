//! Opt-in native Windows GL test for the adapted core. No NRO or console emulator.
//! This host is TEST ONLY; the distributable continues to use libnx/EGL.
use super::fixture;
use crate::{
    Game, PlayType,
    game::{StepResult, audio},
    platform::{self, host},
};
use core::{
    ffi::c_void,
    ptr::{null, null_mut},
};
use std::sync::atomic::{AtomicUsize, Ordering};

type Handle = *mut c_void;
#[repr(C)]
struct PixelFormat {
    size: u16,
    version: u16,
    flags: u32,
    components: [u8; 20],
    masks: [u32; 3],
}
#[link(name = "user32")]
extern "system" {
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: Handle,
    ) -> Handle;
    fn DestroyWindow(window: Handle) -> i32;
    fn GetDC(window: Handle) -> Handle;
    fn ReleaseDC(window: Handle, dc: Handle) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn ChoosePixelFormat(dc: Handle, format: *const PixelFormat) -> i32;
    fn SetPixelFormat(dc: Handle, index: i32, format: *const PixelFormat) -> i32;
    fn SwapBuffers(dc: Handle) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GetProcAddress(module: Handle, name: *const u8) -> *const c_void;
}
#[link(name = "opengl32")]
extern "system" {
    fn wglCreateContext(dc: Handle) -> Handle;
    fn wglDeleteContext(context: Handle) -> i32;
    fn wglMakeCurrent(dc: Handle, context: Handle) -> i32;
    fn wglGetCurrentDC() -> Handle;
    fn wglGetProcAddress(name: *const u8) -> *const c_void;
    fn glGetError() -> u32;
}
static SWAPS: AtomicUsize = AtomicUsize::new(0);
static SWAP_ERRORS: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" fn get_proc(name: *const u8) -> *const c_void {
    let pointer = wglGetProcAddress(name);
    if pointer.is_null() || matches!(pointer as usize, 1 | 2 | 3 | usize::MAX) {
        let dll: Vec<u16> = "opengl32.dll\0".encode_utf16().collect();
        GetProcAddress(GetModuleHandleW(dll.as_ptr()), name)
    } else {
        pointer
    }
}
unsafe extern "C" fn swap() {
    if SwapBuffers(wglGetCurrentDC()) != 0 {
        SWAPS.fetch_add(1, Ordering::Relaxed);
    } else {
        SWAP_ERRORS.fetch_add(1, Ordering::Relaxed);
    }
}
unsafe extern "C" fn interval(value: i32) -> i32 {
    let pointer = get_proc(b"wglSwapIntervalEXT\0".as_ptr());
    if pointer.is_null() {
        return 0
    }
    let set: unsafe extern "system" fn(i32) -> i32 = core::mem::transmute(pointer);
    set(value)
}
struct Context {
    window: Handle,
    dc: Handle,
    gl: Handle,
}
impl Context {
    fn new() -> Self {
        unsafe {
            assert_eq!(core::mem::size_of::<PixelFormat>(), 40);
            let mut this = Self { window: null_mut(), dc: null_mut(), gl: null_mut() };
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // WS_POPUP without WS_VISIBLE: no visible window or focus change.
            this.window = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0x80000000,
                0,
                0,
                1280,
                720,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            assert!(!this.window.is_null(), "create hidden GL test window");
            this.dc = GetDC(this.window);
            assert!(!this.dc.is_null());
            let mut format = PixelFormat { size: 40, version: 1, flags: 0x25, components: [0; 20], masks: [0; 3] };
            format.components[1] = 32;
            format.components[8] = 8;
            format.components[15] = 24;
            format.components[16] = 8;
            let index = ChoosePixelFormat(this.dc, &format);
            assert_ne!(index, 0);
            assert_ne!(SetPixelFormat(this.dc, index, &format), 0);
            this.gl = wglCreateContext(this.dc);
            assert!(!this.gl.is_null());
            assert_ne!(wglMakeCurrent(this.dc, this.gl), 0);
            let pointer = get_proc(b"wglCreateContextAttribsARB\0".as_ptr());
            assert!(!pointer.is_null(), "WGL_ARB_create_context is required");
            let create: unsafe extern "system" fn(Handle, Handle, *const i32) -> Handle = core::mem::transmute(pointer);
            let attributes = [0x2091, 3, 0x2092, 3, 0x9126, 1, 0]; // 3.3 core
            let modern = create(this.dc, null_mut(), attributes.as_ptr());
            assert!(!modern.is_null(), "OpenGL 3.3 core required");
            wglMakeCurrent(null_mut(), null_mut());
            wglDeleteContext(this.gl);
            this.gl = modern;
            assert_ne!(wglMakeCurrent(this.dc, this.gl), 0);
            host::set_gl_host(host::GlHost { get_proc, swap_buffers: swap, set_swap_interval: interval });
            this
        }
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            wglMakeCurrent(null_mut(), null_mut());
            if !self.gl.is_null() {
                wglDeleteContext(self.gl);
            }
            if !self.dc.is_null() {
                ReleaseDC(self.window, self.dc);
            }
            if !self.window.is_null() {
                DestroyWindow(self.window);
            }
        }
    }
}
struct Directory {
    original: std::path::PathBuf,
    temporary: std::path::PathBuf,
}
impl Directory {
    fn new() -> Self {
        let original = std::env::current_dir().unwrap();
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let temporary = std::env::temp_dir().join(format!("pokenx-free-fixture-{}-{unique}", std::process::id()));
        std::fs::create_dir(&temporary).unwrap();
        Self { original, temporary }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
        let _ = std::fs::remove_dir_all(&self.temporary);
    }
}

#[test]
#[ignore = "Requires native Windows OpenGL 3.3; run alone with --ignored --test-threads=1"]
fn native_gl_fixture_runs_vm_input_render_audio_and_close() {
    let _gl = Context::new();
    // Fail real shader compilation and linkage, then prove the context still
    // builds the full renderer below. No injected failure in the game shaders.
    let shader_gl = crate::render::opengl::gl::Gl::load_with(|name| {
        let name = std::ffi::CString::new(name).unwrap();
        unsafe { get_proc(name.as_ptr().cast()) }
    });
    unsafe {
        use crate::render::opengl::{gl, shader};
        let vertex = "#version 330 core\nout vec3 color; void main(){color=vec3(1);gl_Position=vec4(0,0,0,1);}";
        let fragment = "#version 330 core\nin vec3 color; out vec4 pixel; void main(){pixel=vec4(color,1);}";
        assert!(shader::program(&shader_gl, "not valid GLSL", fragment).unwrap_err().contains("vertex"));
        assert!(shader::program(&shader_gl, vertex, "not valid GLSL").unwrap_err().contains("fragment"));
        // Uniform type disagreement is a mandatory link error. Drivers may
        // optimize unused inter-stage varyings, so do not rely on those.
        let uniform_vertex = "#version 330 core\nuniform mat4 conflict; void main(){gl_Position=conflict*vec4(0,0,0,1);}";
        let mismatch = "#version 330 core\nuniform vec4 conflict; out vec4 pixel; void main(){pixel=conflict;}";
        assert!(shader::program(&shader_gl, uniform_vertex, mismatch).unwrap_err().contains("link"));
        let program = shader::program(&shader_gl, vertex, fragment).unwrap();
        let mut attached = -1;
        shader_gl.GetProgramiv(program, gl::ATTACHED_SHADERS, &mut attached);
        assert_eq!(attached, 0, "successful program must release its shader objects");
        shader_gl.DeleteProgram(program);
        assert!(crate::render::opengl::driver_string(&shader_gl, 0, "invalid query").unwrap_err().contains("unavailable"));
        assert_eq!(shader_gl.GetError(), gl::NO_ERROR);
    }
    let directory = Directory::new();
    host::set_input_poll(None);
    SWAPS.store(0, Ordering::Relaxed);
    SWAP_ERRORS.store(0, Ordering::Relaxed);
    let bytes = fixture::executable();
    let path = directory.temporary.join("pokenx-fixture.exe");
    std::fs::create_dir(directory.temporary.join("data")).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let mut assets = gm8exe::reader::from_exe(bytes, None::<fn(&str)>, true, false).unwrap();
    assets.sounds.push(Some(Box::new(gm8exe::asset::Sound {
        name: "snd_wrapped_mp3".into(), source: "wrapped.wav".into(), extension: ".wav".into(),
        data: Some(fixture::mp3_wave_with_metadata().into_boxed_slice()),
        kind: gm8exe::asset::SoundKind::Normal, volume: 1.0, pan: 0.0, preload: true,
        fx: gm8exe::asset::sound::SoundFX {
            chorus: false, echo: false, flanger: false, gargle: false, reverb: false,
        },
    })));
    let mut game = Game::launch(
        assets,
        path,
        vec![],
        Some(directory.temporary.join("scratch")),
        encoding_rs::WINDOWS_1252,
        false,
        0,
        false,
        PlayType::Normal,
    )
    .unwrap();
    assert!(matches!(game.assets.sounds[1].as_ref().unwrap().handle,
        crate::asset::sound::FileType::Mp3(_)), "embedded MP3-in-WAVE must load at startup");
    assert!(game.renderer.upload_sprite(vec![0; 3].into_boxed_slice(), 1, 1, 0, 0).is_err());
    assert!(game.renderer.upload_sprite(Box::default(), -1, 1, 0, 0).is_err());
    let rgba = vec![9, 18, 27, 255, 0, 0, 0, 0];
    let dynamic = game.renderer.upload_sprite(rgba.clone().into_boxed_slice(), 2, 1, 0, 0).unwrap();
    assert_eq!(&*game.renderer.dump_sprite(dynamic), rgba.as_slice());
    let saved = game.renderer.dump_dynamic_textures();
    assert_eq!(saved.len(), 1, "first dynamic texture must not be mistaken for a stock atlas");
    assert!(saved[0].is_some());
    game.renderer.upload_dynamic_textures(&saved);
    assert_eq!(&*game.renderer.dump_sprite(dynamic), rgba.as_slice());
    game.renderer.delete_sprite(dynamic);
    assert!(!game.renderer.check_texture_id(dynamic.0));
    assert!(game.renderer.dump_dynamic_textures().iter().all(Option::is_none));
    for _ in 0..16 {
        let texture = game.renderer.upload_sprite(rgba.clone().into_boxed_slice(), 2, 1, 0, 0).unwrap();
        assert_eq!(texture.0, dynamic.0, "freed CPU reference must be reused too");
        assert_eq!(game.renderer.dump_dynamic_textures().len(), 1, "freed GPU slot must be reused");
        game.renderer.delete_sprite(texture);
    }
    for (w, h) in [(0, 1), (-1, 1), (i32::MAX, 1)] {
        assert!(game.renderer.create_surface(w, h, false).is_err());
    }
    let main_before = game.renderer.get_pixels(0, 0, 2, 2);
    let solid = game.renderer.create_sprite_colour(2, 2, crate::types::Colour::new(0.2, 0.4, 0.6)).unwrap();
    assert_eq!(&*game.renderer.dump_sprite(solid), [51, 102, 153, 255].repeat(4).as_slice());
    assert_eq!(game.renderer.get_pixels(0, 0, 2, 2), main_before, "filling a sprite must preserve the game framebuffer");
    game.renderer.delete_sprite(solid);
    // Default font + fixture assets occupy a tiny part of the first 4096 atlas.
    // Read a far corner through the existing FBO to verify GPU zero-initialization.
    let stock = game.assets.sprites[0].as_ref().unwrap().frames[0].atlas_ref;
    let (atlas_x, atlas_y) = game.renderer.test_texture_position(stock);
    assert_eq!(&*game.renderer.dump_sprite_part(stock, 4095 - atlas_x, 4095 - atlas_y, 1, 1), &[0; 4]);
    assert_eq!(game.start().unwrap(), StepResult::Running);
    assert_eq!(std::fs::read(directory.temporary.join("data/fixture-save.bin")).unwrap(), [42],
               "GML binary rewrite must discard pending bytes as well as old file contents");
    let ini = crate::ini::Ini::load_from_file(directory.temporary.join("data/fixture.ini")).unwrap();
    assert_eq!(ini.section(Some("save")).unwrap().get("counter"), Some("2"));
    let state = std::fs::read(directory.temporary.join("data/fixture-state.bin")).unwrap();
    assert_eq!(&state[..4], &[0x1d, 0x02, 0, 0]);
    let id = game.room.instance_list.get_by_instid(100001).unwrap();
    assert_eq!(game.room.instance_list.get(id).x.get().to_i32(), 640);
    // Create event queued WAV; consume it as the real host audio thread would.
    let mut pcm = vec![0.0f32; 4800];
    audio::render_host_audio(&mut pcm);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 0.001), "Create GML must produce PCM");
    // Exercise the real GML API and the GM8.0 Windows-1252/UTF-8 boundary.
    struct ResetTextInput;
    impl Drop for ResetTextInput {
        fn drop(&mut self) { host::set_text_input(None); }
    }
    let _reset_text_input = ResetTextInput;
    let dialog_args = [
        crate::gml::String::from(&b"\xbfNombre?"[..]).into(),
        crate::gml::String::from(&b"Jos\xe9"[..]).into(),
    ];
    fn text_bytes(value: crate::gml::Value) -> Vec<u8> {
        match value { crate::gml::Value::Str(text) => text.as_ref().to_vec(), _ => panic!("expected text") }
    }
    host::set_text_input(None);
    assert_eq!(text_bytes(game.get_string(&dialog_args).unwrap()), b"Jos\xe9");
    fn confirm(prompt: &str, initial: &str) -> Result<Option<String>, String> {
        assert_eq!(prompt, "¿Nombre?"); assert_eq!(initial, "José");
        Ok(Some("Álex".into()))
    }
    host::set_text_input(Some(confirm));
    game.update_platform_input(&[39]);
    assert_eq!(text_bytes(game.get_string(&dialog_args).unwrap()), b"\xc1lex");
    assert!(!game.input.keyboard_check(39));
    game.update_platform_input(&[39]);
    assert!(game.input.keyboard_check(39), "held controller is reconciled after the dialog");
    game.update_platform_input(&[]);
    fn cancel(_: &str, _: &str) -> Result<Option<String>, String> { Ok(None) }
    host::set_text_input(Some(cancel));
    assert_eq!(text_bytes(game.get_string(&dialog_args).unwrap()), b"Jos\xe9");
    fn empty(_: &str, _: &str) -> Result<Option<String>, String> { Ok(Some(String::new())) }
    host::set_text_input(Some(empty));
    assert_eq!(text_bytes(game.get_string(&dialog_args).unwrap()), b"Jos\xe9");
    fn unrepresentable(_: &str, _: &str) -> Result<Option<String>, String> { Ok(Some("漢".into())) }
    host::set_text_input(Some(unrepresentable));
    assert!(game.get_string(&dialog_args).is_err(), "GM8.0 encoding errors must not silently replace text");
    fn failed(_: &str, _: &str) -> Result<Option<String>, String> { Err("unavailable".into()) }
    host::set_text_input(Some(failed));
    assert!(game.get_string(&dialog_args).is_err());
    host::set_text_input(None);
    // Input queue -> real keyboard_check -> script call -> instance movement.
    platform::push_event(platform::Event::KeyboardDown(platform::Key(39)));
    for _ in 0..4 {
        assert_eq!(game.step_deferred().unwrap(), StepResult::Running);
    }
    assert_eq!(game.room.instance_list.get(id).x.get().to_i32(), 656);
    game.update_platform_input(&[39]);
    let bad_state = directory.temporary.join("data/bad-state.bin");
    std::fs::write(&bad_state, [0x1d, 0x02, 0, 0]).unwrap();
    assert!(game.load_gm_save(bad_state).is_err());
    assert!(game.input.keyboard_check(39), "rejected save must preserve held controller input");
    let previous_width = game.room.width;
    game.room.width = 0;
    let invalid = crate::game::gm_save::GMSave::from_game(&game);
    game.room.width = previous_width;
    let mut invalid_bytes = vec![0x1d, 0x02, 0, 0];
    invalid_bytes.extend(postcard::to_allocvec(&invalid).unwrap());
    let invalid_path = directory.temporary.join("data/invalid-viewport.bin");
    std::fs::write(&invalid_path, invalid_bytes).unwrap();
    game.score = 123;
    assert!(game.load_gm_save(invalid_path).is_err());
    assert_eq!(game.score, 123, "validation must precede committing save fields");
    assert_eq!(game.room.width, previous_width);
    assert!(game.input.keyboard_check(39));
    game.load_gm_save(directory.temporary.join("data/fixture-state.bin")).unwrap();
    let restored = game.room.instance_list.get_by_instid(100001).unwrap();
    assert_eq!(game.room.instance_list.get(restored).x.get().to_i32(), 640);
    assert_eq!(game.score, 0);
    game.update_platform_input(&[39]);
    assert!(game.input.keyboard_check(39), "held controller must be reconciled after a successful load");
    game.update_platform_input(&[]);
    platform::push_event(platform::Event::KeyboardUp(platform::Key(39)));
    game.step_deferred().unwrap();
    assert_eq!(game.room.instance_list.get(restored).x.get().to_i32(), 640);
    game.renderer.flush_queue();
    let pixels = game.renderer.get_pixels(0, 0, 1280, 720);
    assert!(
        pixels.chunks_exact(4).filter(|p| p[..3] == [40, 210, 170]).count() >= 16 * 16,
        "Draw GML must render the fixture sprite into the actual GL framebuffer"
    );
    assert!(pixels.chunks_exact(4).any(|p| p[..3] == [255, 255, 255]), "default font must draw text");
    for (w, h) in [(0, 720), (1280, 0), (u32::MAX, 720)] {
        assert!(game.renderer.resize_framebuffer(w, h, false).is_err());
        assert_eq!(game.renderer.get_pixels(0, 0, 1280, 720), pixels, "failed resize must keep the old framebuffer");
    }
    game.renderer.resize_framebuffer(1300, 740, true).unwrap();
    assert_eq!(game.renderer.stored_size(), (1280, 720));
    assert_eq!(game.renderer.get_pixels(0, 0, 1280, 720), pixels, "growing preserves old color pixels");
    assert_eq!(&*game.renderer.get_pixels(1299, 739, 1, 1), &[0; 4], "new area starts transparent");
    game.renderer.resize_framebuffer(1280, 720, false).unwrap();
    assert_eq!(game.renderer.get_pixels(0, 0, 1280, 720), pixels, "shrinking preserves the overlap");
    assert!(game.renderer.dump_dynamic_textures().iter().all(Option::is_none), "temporary resize surfaces must not leak ownership");
    let before = SWAPS.load(Ordering::Relaxed);
    assert!(game.present_host_frame());
    assert!(SWAPS.load(Ordering::Relaxed) > before);
    assert_eq!(SWAP_ERRORS.load(Ordering::Relaxed), 0);
    assert_eq!(unsafe { glGetError() }, 0, "GL errors after drawing/presentation");
    platform::push_event(platform::Event::CloseRequest);
    assert_eq!(game.step_deferred().unwrap(), StepResult::Finished);
    let previous_dimensions = (game.unscaled_width, game.unscaled_height);
    game.assets.rooms[0].as_mut().unwrap().width = 0;
    assert!(game.load_room(0).is_err(), "invalid room dimensions must propagate to the host");
    assert_eq!((game.unscaled_width, game.unscaled_height), previous_dimensions);
    assert_eq!(game.renderer.get_pixels(0, 0, 1280, 720), pixels);
    eprintln!("Free fixture passed VM/input/GL pixels/PCM/swap/close; native WGL only.");
}

/// Opt-in smoke test against a real game folder on the developer's PC (nothing is copied or stored):
/// `POKENX_REAL_GAME=<folder> [POKENX_REAL_FRAMES=1200] cargo test --release real_game_smoke -- --ignored --nocapture`.
/// It loads the game through the same discovery/launch path as the console, steps frames with no input and
/// reports the first error or panic with the frame and room, which is how GML failures are found without a Switch.
#[test]
#[ignore = "Needs POKENX_REAL_GAME and native Windows OpenGL 3.3"]
fn real_game_smoke() {
    let Some(folder) = std::env::var_os("POKENX_REAL_GAME") else { return };
    let frames: usize = std::env::var("POKENX_REAL_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(1200);
    let _gl = Context::new();
    host::set_input_poll(None);
    // POKENX_LIVE_KEYS_OUT=<file> turns on the live key capture (what the menu's Teclas screen reads).
    if let Some(path) = std::env::var_os("POKENX_LIVE_KEYS_OUT") {
        crate::live_keys::set_output(Some(path.into()));
    }
    // POKENX_ROOM_HINTS=<file>: which room follows which (read at the start, written at the end), as the app keeps it in cache/.
    if let Some(path) = std::env::var_os("POKENX_ROOM_HINTS") {
        crate::room_hints::set_output(Some(path.into()));
    }
    let folder = folder.to_string_lossy().into_owned();
    let exe = crate::loader::find_game_exe(&folder).expect("no game executable found");
    println!("exe: {}", exe.display());
    let bytes = std::fs::read(&exe).unwrap();
    let assets = gm8exe::reader::from_exe(bytes, None::<fn(&str)>, false, false).expect("parse");
    // POKENX_REAL_KEYS_TEST=1: after start, give the game its controller object and the default key assignment (what a
    // loaded save does), so the live key capture has something to read.
    let key_assignment = std::env::var_os("POKENX_REAL_KEYS_TEST").and_then(|_| {
        assets.scripts.iter().flatten().find(|script| script.name.0.as_ref() == b"DefaultKeyAssignment").map(|script| script.source.0.clone())
    });
    let scratch = std::env::temp_dir().join(format!("pokenx-real-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let original = std::env::current_dir().unwrap();
    apply_test_date();
    let started = std::time::Instant::now();
    let mut game = Game::launch(
        assets, exe, vec![], Some(scratch.clone()), encoding_rs::WINDOWS_1252, false, 0, false, PlayType::Normal,
    )
    .expect("launch");
    println!("launched in {:?}", started.elapsed());
    println!("game clock: {}-{:02}-{:02}", game.clock.measure().year(), game.clock.measure().month(), game.clock.measure().day());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Poor-man's sampler (POKENX_REAL_PROFILE=1): which script/builtin was last seen every ~2 ms.
        let profile_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let profile_data = std::sync::Arc::new(std::sync::Mutex::new((
            std::collections::HashMap::<i64, u32>::new(),
            std::collections::HashMap::<i64, u32>::new(),
            std::collections::HashMap::<&'static str, u32>::new(),
        )));
        let profiler = std::env::var_os("POKENX_REAL_PROFILE").map(|_| {
            let (stop, data) = (profile_stop.clone(), profile_data.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    let mut d = data.lock().unwrap();
                    *d.0.entry(crate::Game::debug_last_script()).or_default() += 1;
                    *d.1.entry(crate::Game::debug_last_exec()).or_default() += 1;
                    *d.2.entry(crate::Game::debug_last_function()).or_default() += 1;
                }
            })
        });
        match game.start() {
            Ok(_) => println!("start ok after {:?}", started.elapsed()),
            Err(e) => return Err(format!("start failed: {e}")),
        }
        // The holder lists the game's own `object_event_add`/`object_event_clear` calls kept up to date one event at a time
        // must equal a full rebuild.
        {
            let incremental = game.event_holders_snapshot();
            game.refresh_event_holders();
            let rebuilt = game.event_holders_snapshot();
            if incremental != rebuilt {
                return Err(format!("event holders differ after start: {} lists incremental, {} rebuilt", incremental.len(), rebuilt.len()));
            }
            println!("event holders: {} lists, identical to a full rebuild", rebuilt.len());
        }
        if let Some(source) = &key_assignment {
            let code = format!("instance_create(0,16,(3)); with((3)){{ {} }}", String::from_utf8_lossy(source));
            let instructions = game.compiler.compile(code.as_bytes()).map_err(|e| format!("key test compile: {e}"))?;
            let dummy = game.room.instance_list.insert_dummy(crate::instance::Instance::new_dummy(None));
            game.execute(&instructions, &mut crate::gml::Context::with_single_instance(dummy)).map_err(|e| format!("key test run: {e}"))?;
            game.room.instance_list.remove_dummy(dummy);
        }
        // Optional scripted pulses (`POKENX_REAL_PRESS=A,Down`): the buttons are held for 4 of every 24 polls, and
        // the host input poll is the same one the game's own io_handle dialog loops use on the console.
        // Sequence: steps separated by `;`, each step a `+`-joined button set (e.g. `A;Down;A+Right`). Each step is
        // held for 4 polls and released for 20, and the sequence repeats.
        let mut steps = Vec::new();
        for step in std::env::var("POKENX_REAL_PRESS").unwrap_or_default().split(';') {
            let mut mask = 0u32;
            for name in step.split('+') {
                mask |= match name.trim() {
                    "A" => gm8_input::BUTTON_A, "B" => gm8_input::BUTTON_B, "X" => gm8_input::BUTTON_X, "Y" => gm8_input::BUTTON_Y,
                    "Plus" => gm8_input::BUTTON_PLUS, "Minus" => gm8_input::BUTTON_MINUS,
                    "Down" => gm8_input::BUTTON_DOWN, "Up" => gm8_input::BUTTON_UP,
                    "Left" => gm8_input::BUTTON_LEFT, "Right" => gm8_input::BUTTON_RIGHT,
                    _ => 0,
                };
            }
            if mask != 0 || step.trim() == "-" { steps.push(mask); } // `-` = wait one step
        }
        let fuzz: usize = std::env::var("POKENX_REAL_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        FUZZ_SEED.store(fuzz, Ordering::Relaxed);
        let scripted = !steps.is_empty() || fuzz != 0;
        *PRESS_STEPS.lock().unwrap() = steps;
        if scripted {
            host::set_input_poll(Some(pulse_sample));
        }
        // `POKENX_REAL_SHOTS=400,700` + `POKENX_REAL_SHOT_DIR=<folder>`: save the rendered game frame as PNG.
        let shots: Vec<usize> = std::env::var("POKENX_REAL_SHOTS").unwrap_or_default().split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let shot_dir = std::env::var("POKENX_REAL_SHOT_DIR").unwrap_or_default();
        let mut last_room = game.room.id;
        // Watchdog: if no frame completes for 25 s, say which script was running and stop the process.
        static PROGRESS: AtomicUsize = AtomicUsize::new(0);
        std::thread::spawn(|| {
            let mut seen = usize::MAX;
            let mut stalled = 0;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                // A modal GML dialog loops inside one frame but keeps polling the pad, so polls count as progress.
                let now = PROGRESS.load(Ordering::Relaxed).wrapping_add(PULSE_POLLS.load(Ordering::Relaxed));
                if now == seen { stalled += 1 } else { stalled = 0; seen = now }
                if stalled >= 5 {
                    println!("WATCHDOG: no progress since frame {now}; last script id {}, last ex() target {}, last builtin {}", crate::Game::debug_last_script(), crate::Game::debug_last_exec(), crate::Game::debug_last_function());
                    println!("trail: {:?}", crate::Game::debug_function_trail());
                    std::process::exit(3);
                }
            }
        });
        for frame in 0..frames {
            PROGRESS.store(frame, Ordering::Relaxed);
            if game.room.id != last_room {
                println!("room change at frame {frame}: {last_room} -> {}", game.room.id);
                last_room = game.room.id;
                PULSE_POLLS.store(0, Ordering::Relaxed); // the script restarts in every room
            }
            let files_before = (crate::gml::file::READ_FILES.load(Ordering::Relaxed), crate::gml::file::READ_BYTES.load(Ordering::Relaxed));
            let frame_started = std::time::Instant::now();
            let step_result = game.step_deferred();
            if frame_started.elapsed() > std::time::Duration::from_millis(100) {
                let files_now = (crate::gml::file::READ_FILES.load(Ordering::Relaxed), crate::gml::file::READ_BYTES.load(Ordering::Relaxed));
                println!("SLOW frame {frame} room {}: {:?} (files read: {}, {} bytes)", game.room.id, frame_started.elapsed(), files_now.0 - files_before.0, files_now.1 - files_before.1);
            }
            match step_result {
                Ok(StepResult::Running) => {}
                Ok(other) => return Err(format!("stopped at frame {frame}: {other:?} (last script id {})", crate::Game::debug_last_script())),
                Err(e) => return Err(format!("error at frame {frame} (room {}): {e}", game.room.id)),
            }
            if shots.contains(&frame) {
                let (w, h) = (game.unscaled_width as i32, game.unscaled_height as i32);
                let mut px = game.renderer.get_pixels(0, 0, w, h).to_vec();
                // `get_pixels` already returns top-down rows; the framebuffer is BGRA, so swap to RGBA.
                for c in px.chunks_exact_mut(4) { c.swap(0, 2); }
                if let Some(image) = crate::image::RgbaImage::from_vec(w as u32, h as u32, px) {
                    let path = format!("{shot_dir}/frame_{frame}.png");
                    std::fs::write(&path, crate::image::codec::encode_png(&image)).unwrap();
                    println!("screenshot {path} ({w}x{h}) room {}", game.room.id);
                }
            }
            if frame % 120 == 0 {
                println!("frame {frame} room {}", game.room.id);
            }
        }
        crate::room_hints::save();
        let stats = crate::stats::snapshot();
        println!(
            "STATS paused {} ms in {} pauses; {} room changes took {} ms (longest {} ms); {} rooms compiled ahead in {} ms; {} files read",
            stats.paused_ns / 1_000_000, stats.pauses, stats.room_changes, stats.room_ns / 1_000_000, stats.room_max_ns / 1_000_000,
            stats.warmed_rooms, stats.warm_ns / 1_000_000, stats.files_read
        );
        if let Some(handle) = profiler {
            profile_stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
            let d = profile_data.lock().unwrap();
            let name = |id: i64| game.assets.scripts.get(id as usize).and_then(|s| s.as_ref()).map(|s| s.name.to_string()).unwrap_or_else(|| format!("#{id}"));
            let top = |m: &std::collections::HashMap<i64, u32>, title: &str| {
                let mut v: Vec<_> = m.iter().collect();
                v.sort_by(|a, b| b.1.cmp(a.1));
                println!("PROFILE {title} (samples of 2 ms):");
                for (id, n) in v.into_iter().take(15) { println!("  {n:6}  {}", name(*id)); }
            };
            top(&d.0, "last script");
            top(&d.1, "last ex() target");
            let mut f: Vec<_> = d.2.iter().collect();
            f.sort_by(|a, b| b.1.cmp(a.1));
            println!("PROFILE last builtin:");
            for (n, c) in f.into_iter().take(15) { println!("  {c:6}  {n}"); }
        }
        Ok(())
    }));
    let _ = std::env::set_current_dir(original);
    let _ = std::fs::remove_dir_all(&scratch);
    match outcome {
        Ok(Ok(())) => println!("RESULT: {frames} frames without error in {:?}", started.elapsed()),
        Ok(Err(message)) => println!("RESULT: {message}"),
        Err(_) => println!("RESULT: PANIC (see message above)"),
    }
}

static PRESS_STEPS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
static PULSE_POLLS: AtomicUsize = AtomicUsize::new(0);
static FUZZ_SEED: AtomicUsize = AtomicUsize::new(0);
fn pulse_sample() -> Option<gm8_input::InputSample> {
    let poll = PULSE_POLLS.fetch_add(1, Ordering::Relaxed);
    let seed = FUZZ_SEED.load(Ordering::Relaxed);
    if seed != 0 {
        // `POKENX_REAL_FUZZ=<seed>`: every 24 polls pick a pseudo-random button set, held for 6 polls.
        const BUTTONS: [u32; 10] = [
            gm8_input::BUTTON_A, gm8_input::BUTTON_B, gm8_input::BUTTON_X, gm8_input::BUTTON_Y,
            gm8_input::BUTTON_PLUS, gm8_input::BUTTON_MINUS, gm8_input::BUTTON_UP, gm8_input::BUTTON_DOWN,
            gm8_input::BUTTON_LEFT, gm8_input::BUTTON_RIGHT,
        ];
        let mut x = (poll / 24) as u64 ^ (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        x = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let pick = BUTTONS[(x >> 32) as usize % BUTTONS.len()];
        let held = poll % 24 < 6;
        return Some(gm8_input::InputSample { connected: true, buttons: if held { pick } else { 0 }, ..Default::default() });
    }
    let steps = PRESS_STEPS.lock().unwrap();
    let held = poll % 24 < 4 && !steps.is_empty();
    Some(gm8_input::InputSample {
        connected: true,
        buttons: if held { steps[(poll / 24) % steps.len()] } else { 0 },
        ..Default::default()
    })
}


fn apply_test_date() {
    // Test-only clock for exploring a dated build: `POKENX_REAL_DATE=2026-06-15` (local time, noon).
    if let Ok(date) = std::env::var("POKENX_REAL_DATE") {
        let parts: Vec<i64> = date.split('-').filter_map(|v| v.parse().ok()).collect();
        if let [y, m, d] = parts[..] {
            // days since 1970-01-01 (civil-from-days inverse, Howard Hinnant's algorithm)
            let y = if m <= 2 { y - 1 } else { y };
            let era = y.div_euclid(400);
            let yoe = y - era * 400;
            let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
            let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
            FIXED_DAYS.store(era * 146097 + doe - 719468, Ordering::Relaxed);
            host::set_clock(fixed_clock);
        }
    }
}

static FIXED_DAYS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
fn fixed_clock() -> u128 {
    let seconds = FIXED_DAYS.load(Ordering::Relaxed) as u128 * 86_400 + 12 * 3_600;
    seconds * 1_000_000_000
}
