//! Platform types the runtime core sees instead of `ramen`.
//!
//! The windowing library of the original (`ramen`) is replaced by a thin state holder: the
//! host (the Horizon payload) owns the real window and EGL surface, pushes input and window
//! events through [`push_event`], and the core reads them back with `Window::poll_events`.
//! The API mirrors the subset of `ramen` that the runtime calls so game logic is unchanged.

use std::{
    collections::VecDeque,
    fmt,
    sync::Mutex,
};

/// A GameMaker virtual-key code as reported by the host (see the `gm8_input` crate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    KeyboardDown(Key),
    KeyboardUp(Key),
    MouseMove((u32, u32)),
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    ScrollUp,
    ScrollDown,
    /// Text typed on a keyboard (not produced on Horizon; kept so `process_window_events` is unchanged).
    Input(char),
    Resize((u32, u32)),
    CloseRequest,
}

/// Cursor shapes GML can request. There is no pointer on Horizon; the value is only recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Cursor {
    Arrow = 0,
    Blank,
    Cross,
    Hand,
    Help,
    IBeam,
    Progress,
    ResizeNESW,
    ResizeNS,
    ResizeNWSE,
    ResizeWE,
    ResizeAll,
    Unavailable,
    Wait,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    minimise: bool,
    maximise: bool,
}

impl Controls {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn minimise(mut self, value: bool) -> Self {
        self.minimise = value;
        self
    }
    pub fn maximise(mut self, value: bool) -> Self {
        self.maximise = value;
        self
    }
}

#[derive(Debug)]
pub struct Error(&'static str);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Error {}

static EVENTS: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

/// Called by the host to deliver an input or window event to the core.
pub fn push_event(event: Event) {
    EVENTS.lock().unwrap().push_back(event);
}

pub fn clear_events() {
    EVENTS.lock().unwrap().clear();
}

#[derive(Clone, Default)]
pub struct Connection;

impl Connection {
    pub fn new() -> Result<Self, Error> {
        Ok(Connection)
    }

    pub fn builder(&self) -> Builder {
        Builder::default()
    }
}

#[derive(Default)]
pub struct Builder {
    size: (u32, u32),
    title: String,
    visible: bool,
    borderless: bool,
    resizable: bool,
}

impl Builder {
    pub fn class_name(self, _name: &str) -> Self {
        self
    }
    pub fn visible(mut self, value: bool) -> Self {
        self.visible = value;
        self
    }
    pub fn size(mut self, size: (u32, u32)) -> Self {
        self.size = size;
        self
    }
    pub fn borderless(mut self, value: bool) -> Self {
        self.borderless = value;
        self
    }
    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = title.into();
        self
    }
    pub fn resizable(mut self, value: bool) -> Self {
        self.resizable = value;
        self
    }
    pub fn controls(self, _controls: Option<Controls>) -> Self {
        self
    }
    pub fn build(self) -> Result<Window, Error> {
        Ok(Window {
            size: self.size,
            title: self.title,
            visible: self.visible,
            borderless: self.borderless,
            resizable: self.resizable,
            cursor: Cursor::Arrow,
            buffer: Vec::new(),
        })
    }
}

/// State of the logical game window. The pixels go to the EGL surface the host owns.
pub struct Window {
    size: (u32, u32),
    title: String,
    visible: bool,
    borderless: bool,
    resizable: bool,
    cursor: Cursor,
    buffer: Vec<Event>,
}

impl Window {
    /// Moves every event the host delivered since the last call into the buffer read by `events`.
    pub fn poll_events(&mut self) {
        self.buffer.clear();
        let mut queue = EVENTS.lock().unwrap();
        self.buffer.extend(queue.drain(..));
        for event in &self.buffer {
            if let Event::Resize(size) = event {
                self.size = *size;
            }
        }
    }

    pub fn events(&self) -> impl IntoIterator<Item = &Event> {
        self.buffer.iter()
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    pub fn set_title(&mut self, title: &str) {
        self.title.clear();
        self.title.push_str(title);
    }
    pub fn set_borderless(&mut self, borderless: bool) {
        self.borderless = borderless;
    }
    pub fn set_resizable(&mut self, resizable: bool) {
        self.resizable = resizable;
    }
    pub fn set_size(&mut self, size: (u32, u32)) {
        self.size = size;
    }
    /// The window fills the Switch screen: a requested position is accepted and ignored.
    pub fn set_position(&mut self, _position: (i32, i32)) {}
    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
    }
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }
    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

/// Services the runtime needs from the host that have no `std` equivalent on Horizon.
pub mod host {
    use std::{ffi::c_void, sync::Mutex};

    /// OpenGL context services. The host (C++/libnx) creates the EGL context and surface and
    /// keeps them current on the game thread; the core only loads functions and presents.
    #[derive(Clone, Copy)]
    pub struct GlHost {
        /// `eglGetProcAddress` for a NUL-terminated name.
        pub get_proc: unsafe extern "C" fn(name: *const u8) -> *const c_void,
        /// `eglSwapBuffers` on the window surface.
        pub swap_buffers: unsafe extern "C" fn(),
        /// `eglSwapInterval`; returns non-zero on success.
        pub set_swap_interval: unsafe extern "C" fn(interval: i32) -> i32,
    }

    static GL: Mutex<Option<GlHost>> = Mutex::new(None);

    /// Blocking native text entry, on the game thread. None means cancelled or
    /// no dialog service; callers keep the GML default in that case.
    pub type TextInputFn = fn(&str, &str) -> Result<Option<String>, String>;
    static TEXT_INPUT: Mutex<Option<TextInputFn>> = Mutex::new(None);

    pub fn set_text_input(input: Option<TextInputFn>) {
        *TEXT_INPUT.lock().unwrap() = input;
    }

    pub fn has_text_input() -> bool { TEXT_INPUT.lock().unwrap().is_some() }

    pub fn request_text_input(prompt: &str, initial: &str) -> Result<Option<String>, String> {
        // The callback may itself use platform services; do not retain this lock.
        let input = *TEXT_INPUT.lock().unwrap();
        match input {
            Some(input) => input(prompt, initial),
            None => Ok(None),
        }
    }

    pub type InputPollFn = fn() -> Option<gm8_input::InputSample>;
    static INPUT_POLL: Mutex<Option<InputPollFn>> = Mutex::new(None);
    static INPUT_MAPPER: Mutex<gm8_input::SwitchInputMapper> = Mutex::new(gm8_input::SwitchInputMapper::new());

    /// Called on the game thread, including from blocking GML dialog loops.
    static BUTTON_MAP: Mutex<gm8_input::ButtonMap> = Mutex::new(gm8_input::ButtonMap::DEFAULT);
    /// Stick dead zone in percent (0 = the mapper's default); like the button map it survives `set_input_poll`.
    static STICK_PERCENT: Mutex<u8> = Mutex::new(0);

    pub fn set_stick_threshold(percent: u8) {
        *STICK_PERCENT.lock().unwrap() = percent;
        if percent != 0 {
            INPUT_MAPPER.lock().unwrap().set_stick_threshold(percent);
        }
    }

    /// Button-to-key table from `controls.txt`; it survives `set_input_poll`, which recreates the mapper.
    pub fn set_button_map(map: gm8_input::ButtonMap) {
        *BUTTON_MAP.lock().unwrap() = map;
        INPUT_MAPPER.lock().unwrap().set_map(map);
    }

    pub fn set_input_poll(poll: Option<InputPollFn>) {
        *INPUT_POLL.lock().unwrap() = poll;
        let mut mapper = gm8_input::SwitchInputMapper::new();
        mapper.set_map(*BUTTON_MAP.lock().unwrap());
        let percent = *STICK_PERCENT.lock().unwrap();
        if percent != 0 {
            mapper.set_stick_threshold(percent);
        }
        *INPUT_MAPPER.lock().unwrap() = mapper;
    }
    pub fn has_input_poll() -> bool { INPUT_POLL.lock().unwrap().is_some() }
    pub fn poll_input() -> Option<gm8_input::InputFrame> {
        // Release the lock before invoking the host: the callback may enqueue events.
        let poll = *INPUT_POLL.lock().unwrap();
        poll.and_then(|poll| poll()).map(|sample| INPUT_MAPPER.lock().unwrap().sample(sample))
    }

    pub fn set_gl_host(host: GlHost) {
        *GL.lock().unwrap() = Some(host);
    }

    pub fn gl_host() -> Option<GlHost> {
        *GL.lock().unwrap()
    }

    /// Wall-clock hook: nanoseconds since the Unix epoch, local time already applied.
    pub type ClockFn = fn() -> u128;

    static CLOCK: Mutex<Option<ClockFn>> = Mutex::new(None);

    pub fn set_clock(clock: ClockFn) {
        *CLOCK.lock().unwrap() = Some(clock);
    }

    /// `None` when the host registered no clock (date functions then report the epoch).
    pub fn now_unix_nanos() -> Option<u128> {
        CLOCK.lock().unwrap().map(|clock| clock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_input_is_resampled_inside_dialog_polls_and_releases_after_disconnect() {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static POLLS: AtomicUsize = AtomicUsize::new(0);
        fn sample() -> Option<gm8_input::InputSample> {
            if POLLS.fetch_add(1, Ordering::Relaxed) == 0 {
                Some(gm8_input::InputSample { connected: true, buttons: gm8_input::BUTTON_A, ..Default::default() })
            } else { Some(gm8_input::InputSample::disconnected()) }
        }
        POLLS.store(0, Ordering::Relaxed);
        host::set_input_poll(Some(sample));
        assert!(host::poll_input().unwrap().keys.down.contains(gm8_input::Key::Z));
        assert!(host::poll_input().unwrap().keys.up.contains(gm8_input::Key::Z));
        host::set_input_poll(None);
        assert!(host::poll_input().is_none());
    }

    #[test]
    fn events_flow_from_host_to_window_in_order() {
        clear_events();
        let mut window = Connection::new().unwrap().builder().size((640, 480)).title("t").build().unwrap();
        push_event(Event::MouseMove((3, 4)));
        push_event(Event::Resize((1280, 720)));
        push_event(Event::CloseRequest);
        window.poll_events();
        let events: Vec<Event> = window.events().into_iter().copied().collect();
        assert_eq!(events, [Event::MouseMove((3, 4)), Event::Resize((1280, 720)), Event::CloseRequest]);
        assert_eq!(window.size(), (1280, 720));
        window.poll_events();
        assert_eq!(window.events().into_iter().count(), 0);
    }
}
