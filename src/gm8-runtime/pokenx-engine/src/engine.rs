//! Engine state and the exported C ABI.
//!
//! Two modes share one ABI: with no game loaded the engine runs a diagnostic frame (clear
//! colour reacting to the pad, tone while A is held); after `pokenx_engine_load_game` succeeds
//! it runs the GameMaker game through `pokenx-core`.

use crate::abi::*;
use crate::gl::{self, GlApi};
use crate::logging::LineBuf;
use crate::tone;
use crate::frame_clock::{FrameClock, MAX_STEPS_PER_PRESENT, PRESENTATION_HZ};
use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::ffi::{c_char, CStr};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use gm8_audio_output::convert_f32_to_i16;
use gm8_input::{InputSample, StickAxes, SwitchInputMapper, BUTTON_A};
use pokenx_core::{
    game::StepResult,
    platform::{self, host::GlHost},
    Game,
};

/// Diagnostic clear colour: dark blue, greener when a pad is connected and
/// redder for every mapped key held. Pure so it can be tested on the host.
pub fn clear_color_for(connected: bool, held_keys: usize, frame: u64) -> [f32; 4] {
    let pulse = ((frame / 30) % 2) as f32 * 0.02;
    let green = if connected { 0.16 } else { 0.04 };
    let red = (0.03 + 0.06 * held_keys as f32).min(0.9);
    [red + pulse, green, 0.14, 1.0]
}

fn clamp_axis(value: i32) -> i16 {
    value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

fn log_with(host: &PokenxHost, args: core::fmt::Arguments<'_>) {
    if let Some(log) = host.log {
        let mut line = LineBuf::new();
        let _ = line.write_fmt(args);
        // SAFETY: the host guarantees `log` stays valid until shutdown.
        unsafe { log(line.as_c_ptr().cast()) };
    }
}

pub struct Engine {
    host: PokenxHost,
    gl: GlApi,
    mapper: SwitchInputMapper,
    frame: u64,
    last_held: usize,
    game: Option<Box<Game>>,
    clock: FrameClock,
    display_frames: u64,
    catchup_limits: u64,
    clock_resets: u64,
}

static CLOCK_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static INPUT_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

fn pad_sample(pad: &PokenxPad) -> InputSample {
    InputSample {
        connected: pad.flags & PAD_CONNECTED != 0,
        buttons: pad.buttons,
        left_stick: StickAxes { x: clamp_axis(pad.left_x), y: clamp_axis(pad.left_y) },
        right_stick: StickAxes { x: clamp_axis(pad.right_x), y: clamp_axis(pad.right_y) },
        pointer: None, mouse_buttons: 0,
    }
}

fn poll_game_input() -> Option<InputSample> {
    let raw = INPUT_FN.load(Ordering::Relaxed);
    if raw.is_null() { return None }
    // Only registered from PokenxHost::poll_input; called by the GL/game thread.
    let poll = unsafe { core::mem::transmute::<*mut (), unsafe extern "C" fn(*mut PokenxPad) -> i32>(raw) };
    let mut pad = PokenxPad::default();
    match unsafe { poll(&mut pad) } {
        0 => { platform::push_event(platform::Event::CloseRequest); Some(InputSample::disconnected()) },
        n if n < 0 => None,
        _ => Some(pad_sample(&pad)),
    }
}

/// Wall clock handed to the core's date functions: host seconds as nanoseconds.
fn host_clock_nanos() -> u128 {
    let raw = CLOCK_FN.load(Ordering::Relaxed);
    if raw.is_null() {
        return 0;
    }
    // SAFETY: only ever stores `PokenxHost::unix_seconds`, an `extern "C" fn() -> i64`.
    let seconds = unsafe { core::mem::transmute::<*mut (), unsafe extern "C" fn() -> i64>(raw)() };
    if seconds < 0 { 0 } else { seconds as u128 * 1_000_000_000 }
}

impl Engine {
    fn log(&self, args: core::fmt::Arguments<'_>) {
        log_with(&self.host, args);
    }

    /// # Safety
    /// A GL context must be current and `host.get_proc` valid.
    unsafe fn new(host: PokenxHost) -> Result<Self, i32> {
        let Some(get_proc) = host.get_proc else {
            log_with(&host, format_args!("engine init: the host gave no eglGetProcAddress"));
            return Err(RESULT_BAD_ARGUMENT);
        };
        let Some(gl) = GlApi::load(get_proc) else {
            log_with(&host, format_args!("engine init: glViewport/glClear/glGetError/glGetString could not be loaded"));
            return Err(RESULT_NO_GL);
        };
        let engine = Self {
            host, gl, mapper: SwitchInputMapper::default(), frame: 0, last_held: 0, game: None,
            clock: FrameClock::default(), display_frames: 0, catchup_limits: 0, clock_resets: 0,
        };
        for (label, name) in [("GL_RENDERER", gl::RENDERER), ("GL_VERSION", gl::VERSION)] {
            let text = (engine.gl.get_string)(name);
            if text.is_null() {
                engine.log(format_args!("{label}: <null>"));
            } else {
                let text = CStr::from_ptr(text.cast::<c_char>()).to_str().unwrap_or("<no utf8>");
                engine.log(format_args!("{label}: {text}"));
            }
        }
        // Services the game's renderer and date functions need from the host.
        if let (Some(swap_buffers), Some(set_swap_interval)) = (host.swap_buffers, host.set_swap_interval) {
            platform::host::set_gl_host(GlHost { get_proc, swap_buffers, set_swap_interval });
        }
        if let Some(unix_seconds) = host.unix_seconds {
            CLOCK_FN.store(unix_seconds as *mut (), Ordering::Relaxed);
            platform::host::set_clock(host_clock_nanos);
        }
        platform::host::set_input_poll(None);
        #[cfg(target_os = "horizon")]
        platform::host::set_text_input(Some(crate::text_input::request));
        INPUT_FN.store(core::ptr::null_mut(), Ordering::Relaxed);
        if let Some(poll) = host.poll_input {
            INPUT_FN.store(poll as *mut (), Ordering::Relaxed);
            platform::host::set_input_poll(Some(poll_game_input));
        }
        Ok(engine)
    }

    fn input_frame(&mut self, pad: &PokenxPad) -> (gm8_input::InputFrame, bool) {
        let connected = pad.flags & PAD_CONNECTED != 0;
        let frame = self.mapper.sample(pad_sample(pad));
        (frame, connected)
    }

    /// Loads and starts the game found in `dir`.
    unsafe fn load_game(&mut self, dir: &str) -> i32 {
        let host = self.host;
        let log = move |message: &str| log_with(&host, format_args!("{message}"));
        let mut game = match pokenx_core::loader::launch(dir, &log) {
            Ok(game) => game,
            Err(error) => {
                log(&alloc::format!("load failed: {error}"));
                return RESULT_NO_GAME;
            }
        };
        // Startup GML may wait for input and play sound before returning to the host.
        GAME_ACTIVE.store(true, Ordering::Release);
        match game.start() {
            Ok(StepResult::Running) => {
                log("game started");
                GAME_ACTIVE.store(true, Ordering::Release);
                self.game = Some(Box::new(game));
                self.clock.reset();
                RESULT_OK
            }
            Ok(StepResult::Finished) => {
                GAME_ACTIVE.store(false, Ordering::Release);
                log("game finished during start");
                RESULT_FINISHED
            }
            Err(error) => {
                GAME_ACTIVE.store(false, Ordering::Release);
                log(&alloc::format!("start failed: {error}"));
                RESULT_GAME_ERROR
            }
        }
    }

    /// One simulation step of the loaded game.
    unsafe fn game_frame(&mut self, pad: &PokenxPad) -> i32 {
        let input = if self.host.poll_input.is_none() { Some(self.input_frame(pad).0) } else { None };
        let Some(game) = self.game.as_mut() else { return RESULT_NO_GAME };
        if let Some(input) = input { game.update_platform_frame(&input); }
        match game.step() {
            Ok(StepResult::Running) => {
                TARGET_FPS.store(game.room_speed().max(1), Ordering::Relaxed);
                self.frame += 1;
                RESULT_PRESENTED
            }
            Ok(StepResult::Finished) => RESULT_FINISHED,
            Err(error) => {
                log_with(&self.host, format_args!("game error: {error}"));
                self.game = None;
                GAME_ACTIVE.store(false, Ordering::Release);
                RESULT_GAME_ERROR
            }
        }
    }

    /// One host presentation opportunity. GameMaker events still run at the
    /// room's cadence; repeating a picture does not repeat Draw/Step events.
    unsafe fn display_frame(&mut self, pad: &PokenxPad, width: u32, height: u32, elapsed_ns: u64) -> i32 {
        if self.game.is_none() { return self.diagnostic_frame(pad, width, height) }
        let input = if self.host.poll_input.is_none() { Some(self.input_frame(pad).0) } else { None };
        if self.clock.advance(elapsed_ns) {
            self.clock_resets += 1;
            log_with(&self.host, format_args!("simulation clock reset after {} ms host interruption", elapsed_ns / 1_000_000));
        }
        let game = self.game.as_mut().unwrap();
        if let Some(input) = input { game.update_platform_frame(&input); }
        // Keep input edges and resize events fresh even on a 60 Hz presentation
        // where a slower GameMaker room has no simulation tick due yet.
        game.process_window_events();
        let mut steps = 0;
        while steps < MAX_STEPS_PER_PRESENT && (game.close_requested || self.clock.take_step(game.room_speed())) {
            match game.step_deferred() {
                Ok(StepResult::Running) => { self.frame += 1; steps += 1; },
                Ok(StepResult::Finished) => {
                    GAME_ACTIVE.store(false, Ordering::Release);
                    return RESULT_FINISHED
                },
                Err(error) => {
                    log_with(&self.host, format_args!("game error: {error}"));
                    self.game = None;
                    GAME_ACTIVE.store(false, Ordering::Release);
                    return RESULT_GAME_ERROR
                },
            }
        }
        if steps == MAX_STEPS_PER_PRESENT { self.catchup_limits += 1; }
        TARGET_FPS.store(game.room_speed().max(1), Ordering::Relaxed);
        self.display_frames += 1;
        if game.present_host_frame() { RESULT_PRESENTED } else { RESULT_NO_PRESENT }
    }

    /// Runs one diagnostic frame. Returns a `RESULT_*` code.
    unsafe fn diagnostic_frame(&mut self, pad: &PokenxPad, width: u32, height: u32) -> i32 {
        let (input, connected) = self.input_frame(pad);
        let mut codes = [0u8; 16];
        let held = input.keys.held.write_keycodes(&mut codes);
        if held != self.last_held {
            self.log(format_args!("frame {}: {} mapped keys held, pad connected: {}", self.frame, held, connected));
            self.last_held = held;
        }
        TONE_ON.store(connected && pad.buttons & BUTTON_A != 0, Ordering::Relaxed);

        let [r, g, b, a] = clear_color_for(connected, held, self.frame);
        (self.gl.viewport)(0, 0, width as i32, height as i32);
        (self.gl.clear_color)(r, g, b, a);
        (self.gl.clear)(gl::COLOR_BUFFER_BIT);
        self.frame += 1;
        let error = (self.gl.get_error)();
        if error != 0 {
            self.log(format_args!("glGetError after frame {}: 0x{:04x}", self.frame, error));
            return RESULT_GL_ERROR;
        }
        RESULT_OK
    }
}

struct EngineCell(UnsafeCell<Option<Engine>>);
// SAFETY: the engine is only touched from the C++ main thread; the audio
// thread uses the atomics below.
unsafe impl Sync for EngineCell {}
static ENGINE: EngineCell = EngineCell(UnsafeCell::new(None));
static TONE_ON: AtomicBool = AtomicBool::new(false);
static TONE_PHASE: AtomicU32 = AtomicU32::new(0);
static GAME_ACTIVE: AtomicBool = AtomicBool::new(false);
static TARGET_FPS: AtomicU32 = AtomicU32::new(60);

/// # Safety
/// `host` must point to a valid `PokenxHost` for the duration of this call, and
/// a GL context must be current on this thread.
#[no_mangle]
pub unsafe extern "C" fn pokenx_engine_init(host: *const PokenxHost) -> i32 {
    let Some(host) = host.as_ref() else { return RESULT_BAD_ARGUMENT };
    if host.abi_version != ABI_VERSION {
        return RESULT_BAD_ABI;
    }
    match Engine::new(*host) {
        Ok(engine) => {
            engine.log(format_args!("pokenx engine ABI {} initialized", ABI_VERSION));
            *ENGINE.0.get() = Some(engine);
            RESULT_OK
        }
        Err(code) => code,
    }
}

/// Loads and starts the game in `game_dir` (NUL-terminated UTF-8 path). Returns `RESULT_OK`, or
/// `RESULT_NO_GAME`/`RESULT_GAME_ERROR`/`RESULT_FINISHED` with details in the log; on failure the
/// engine stays in diagnostic mode.
///
/// # Safety
/// `game_dir` must be a valid NUL-terminated string. Call on the GL thread after init.
#[no_mangle]
pub unsafe extern "C" fn pokenx_engine_load_game(game_dir: *const c_char) -> i32 {
    let Some(engine) = (*ENGINE.0.get()).as_mut() else { return RESULT_NOT_INITIALIZED };
    if game_dir.is_null() {
        return RESULT_BAD_ARGUMENT;
    }
    let Ok(dir) = CStr::from_ptr(game_dir).to_str() else { return RESULT_BAD_ARGUMENT };
    let result = engine.load_game(dir);
    if result == RESULT_OK {
        if let Some(game) = engine.game.as_ref() {
            TARGET_FPS.store(game.room_speed().max(1), Ordering::Relaxed);
        }
    }
    result
}

/// Current simulation tick rate (room_speed). The elapsed-time host loop uses
/// `pokenx_engine_presentation_hz` for its separate display cadence.
#[no_mangle]
pub extern "C" fn pokenx_engine_target_fps() -> u32 {
    if GAME_ACTIVE.load(Ordering::Acquire) { TARGET_FPS.load(Ordering::Relaxed) } else { 60 }
}

#[no_mangle]
pub extern "C" fn pokenx_engine_presentation_hz() -> u32 { PRESENTATION_HZ }

/// Reset after suspension: elapsed time outside the game is not simulation debt.
/// # Safety
/// Call on the GL/game thread only, as with `pokenx_engine_frame`.
#[no_mangle]
pub unsafe extern "C" fn pokenx_engine_reset_timing() {
    if let Some(engine) = (*ENGINE.0.get()).as_mut() { engine.clock.reset(); }
}

/// Running total of the time the game spent in its own `sleep()` calls, in nanoseconds (cheap: the FPS meter reads it on
/// every presented frame to leave those pauses out of its numbers).
#[no_mangle]
pub extern "C" fn pokenx_engine_paused_ns() -> u64 { pokenx_core::stats::paused_ns() }

/// Fills `out` with the running totals the host logs every few seconds (pauses of the game, files read, room changes).
/// `room_max_ns` is the longest room change since the previous call.
/// # Safety
/// `out` must be null or point to a valid `PokenxStats`.
#[no_mangle]
pub unsafe extern "C" fn pokenx_engine_stats(out: *mut PokenxStats) {
    let Some(out) = out.as_mut() else { return };
    let mut snapshot = pokenx_core::stats::snapshot();
    snapshot.room_max_ns = pokenx_core::stats::take_room_max();
    *out = PokenxStats {
        paused_ns: snapshot.paused_ns,
        pauses: snapshot.pauses,
        files_read: snapshot.files_read,
        bytes_read: snapshot.bytes_read,
        room_changes: snapshot.room_changes,
        room_ns: snapshot.room_ns,
        room_max_ns: snapshot.room_max_ns,
        warmed_rooms: snapshot.warmed_rooms,
        warm_ns: snapshot.warm_ns,
    };
}

/// Tells the core the drawable size changed (docked/handheld).
#[no_mangle]
pub extern "C" fn pokenx_engine_resize(width: u32, height: u32) {
    platform::push_event(platform::Event::Resize((width, height)));
}

/// # Safety
/// `pad` must be null or point to a valid `PokenxPad`. Call from the thread that
/// owns the GL context, after a successful `pokenx_engine_init`.
#[no_mangle]
pub unsafe extern "C" fn pokenx_engine_frame(pad: *const PokenxPad, width: u32, height: u32) -> i32 {
    let Some(engine) = (*ENGINE.0.get()).as_mut() else { return RESULT_NOT_INITIALIZED };
    let default_pad = PokenxPad::default();
    let pad = pad.as_ref().unwrap_or(&default_pad);
    if engine.game.is_some() {
        engine.game_frame(pad)
    } else {
        engine.diagnostic_frame(pad, width, height)
    }
}

/// Simulation and presentation entry point used by the 60 Hz Switch host.
/// # Safety
/// Same pointer/GL-thread contract as `pokenx_engine_frame`; elapsed_ns comes
/// from the host's monotonic clock and excludes suspension.
#[no_mangle]
pub unsafe extern "C" fn pokenx_engine_frame_elapsed(pad: *const PokenxPad, width: u32, height: u32, elapsed_ns: u64) -> i32 {
    let Some(engine) = (*ENGINE.0.get()).as_mut() else { return RESULT_NOT_INITIALIZED };
    let default_pad = PokenxPad::default();
    engine.display_frame(pad.as_ref().unwrap_or(&default_pad), width, height, elapsed_ns)
}

/// Fills `frames` interleaved stereo S16 frames at 48 kHz. Safe to call from the audio thread.
///
/// # Safety
/// `out` must be valid for writes of `frames * 2` samples.
#[no_mangle]
pub unsafe extern "C" fn pokenx_engine_audio(out: *mut i16, frames: u32) -> i32 {
    if out.is_null() {
        return RESULT_BAD_ARGUMENT;
    }
    let buffer = core::slice::from_raw_parts_mut(out, frames as usize * 2);
    if GAME_ACTIVE.load(Ordering::Acquire) {
        let mut scratch = [0f32; 2048];
        let mut done = 0;
        while done < buffer.len() {
            let count = (buffer.len() - done).min(scratch.len());
            pokenx_core::game::audio::render_host_audio(&mut scratch[..count]);
            convert_f32_to_i16(&scratch[..count], &mut buffer[done..done + count]);
            done += count;
        }
    } else {
        let mut phase = TONE_PHASE.load(Ordering::Relaxed);
        tone::fill_stereo(buffer, &mut phase, TONE_ON.load(Ordering::Relaxed));
        TONE_PHASE.store(phase, Ordering::Relaxed);
    }
    RESULT_OK
}

/// # Safety
/// Call once, from the thread that called `pokenx_engine_init`.
#[no_mangle]
pub unsafe extern "C" fn pokenx_engine_shutdown() {
    pokenx_core::room_hints::save();
    platform::host::set_text_input(None);
    GAME_ACTIVE.store(false, Ordering::Release);
    platform::host::set_input_poll(None);
    INPUT_FN.store(core::ptr::null_mut(), Ordering::Relaxed);
    if let Some(mut engine) = (*ENGINE.0.get()).take() {
        // Drop the game first: its renderer still needs the GL context.
        if let Some(game) = engine.game.as_mut() {
            // An ini file only reaches the card on ini_close; do not lose it when the player quits.
            if let Some((ini, path)) = game.open_ini.take() {
                let name = game.decode_str(path.as_ref()).into_owned();
                if let Err(e) = ini.write_to_file(name.as_str()) {
                    engine.log(format_args!("ini flush on exit failed: {e}"));
                }
            }
        }
        engine.game = None;
        engine.log(format_args!("shutdown: {} simulation/diagnostic frames, {} display opportunities, {} catch-up limits, {} long interruptions",
            engine.frame, engine.display_frames, engine.catchup_limits, engine.clock_resets));
    }
    TONE_ON.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_color_reacts_to_pad_state() {
        let idle = clear_color_for(false, 0, 0);
        let connected = clear_color_for(true, 0, 0);
        let pressing = clear_color_for(true, 3, 0);
        assert!(connected[1] > idle[1]);
        assert!(pressing[0] > connected[0]);
        assert!(clear_color_for(true, 1000, 0)[0] <= 0.92);
    }

    #[test]
    fn rejects_bad_arguments_and_abi() {
        unsafe {
            assert_eq!(pokenx_engine_init(core::ptr::null()), RESULT_BAD_ARGUMENT);
            let host = PokenxHost {
                abi_version: 99,
                get_proc: None,
                log: None,
                swap_buffers: None,
                set_swap_interval: None,
                unix_seconds: None,
                poll_input: None,
            };
            assert_eq!(pokenx_engine_init(&host), RESULT_BAD_ABI);
            assert_eq!(pokenx_engine_frame(core::ptr::null(), 1, 1), RESULT_NOT_INITIALIZED);
            assert_eq!(pokenx_engine_load_game(c"nowhere".as_ptr()), RESULT_NOT_INITIALIZED);
            assert_eq!(pokenx_engine_audio(core::ptr::null_mut(), 4), RESULT_BAD_ARGUMENT);
        }
    }

    #[test]
    fn audio_is_silent_until_a_is_held() {
        let mut buffer = [7i16; 16];
        unsafe { assert_eq!(pokenx_engine_audio(buffer.as_mut_ptr(), 8), RESULT_OK) };
        assert!(buffer.iter().all(|&s| s == 0));
    }

    #[test]
    fn target_fps_defaults_to_sixty() {
        assert_eq!(pokenx_engine_target_fps(), 60);
        assert_eq!(pokenx_engine_presentation_hz(), 60);
    }
}
