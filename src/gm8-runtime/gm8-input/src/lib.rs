#![no_std]

//! Platform-neutral Nintendo Switch controller mapping for the OpenGMK input model.
//!
//! The game-facing contract is a set of GameMaker virtual-key states. The app translates
//! `nx` HID buttons to [`SwitchButtons`], then feeds each frame into [`SwitchInputMapper`].

pub const BUTTON_A: u32 = 1 << 0;
pub const BUTTON_B: u32 = 1 << 1;
pub const BUTTON_X: u32 = 1 << 2;
pub const BUTTON_Y: u32 = 1 << 3;
pub const BUTTON_L: u32 = 1 << 4;
pub const BUTTON_R: u32 = 1 << 5;
pub const BUTTON_ZL: u32 = 1 << 6;
pub const BUTTON_ZR: u32 = 1 << 7;
pub const BUTTON_PLUS: u32 = 1 << 8;
pub const BUTTON_MINUS: u32 = 1 << 9;
pub const BUTTON_LEFT: u32 = 1 << 10;
pub const BUTTON_UP: u32 = 1 << 11;
pub const BUTTON_RIGHT: u32 = 1 << 12;
pub const BUTTON_DOWN: u32 = 1 << 13;
pub const BUTTON_STICK_L: u32 = 1 << 14;
pub const BUTTON_STICK_R: u32 = 1 << 15;

/// GameMaker/Windows virtual-key codes consumed by OpenGMK's keyboard state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Key {
    Backspace = 0x08,
    Tab = 0x09,
    Return = 0x0D,
    Shift = 0x10,
    Control = 0x11,
    Alt = 0x12,
    Escape = 0x1B,
    Space = 0x20,
    PageUp = 0x21,
    PageDown = 0x22,
    End = 0x23,
    Home = 0x24,
    Left = 0x25,
    Up = 0x26,
    Right = 0x27,
    Down = 0x28,
    Insert = 0x2D,
    Delete = 0x2E,
    F1 = 0x70,
    F2 = 0x71,
    F3 = 0x72,
    F4 = 0x73,
    F5 = 0x74,
    F6 = 0x75,
    F7 = 0x76,
    F8 = 0x77,
    F9 = 0x78,
    F10 = 0x79,
    F11 = 0x7A,
    F12 = 0x7B,
    D0 = b'0',
    D1 = b'1',
    D2 = b'2',
    D3 = b'3',
    D4 = b'4',
    D5 = b'5',
    D6 = b'6',
    D7 = b'7',
    D8 = b'8',
    D9 = b'9',
    A = b'A',
    B = b'B',
    C = b'C',
    D = b'D',
    E = b'E',
    F = b'F',
    G = b'G',
    H = b'H',
    I = b'I',
    J = b'J',
    K = b'K',
    L = b'L',
    M = b'M',
    N = b'N',
    O = b'O',
    P = b'P',
    Q = b'Q',
    R = b'R',
    S = b'S',
    T = b'T',
    U = b'U',
    V = b'V',
    W = b'W',
    X = b'X',
    Y = b'Y',
    Z = b'Z',
}

/// Which GameMaker key each Switch button produces (the d-pad and left stick always map to the arrow keys).
/// Order: A, B, X, Y, L, R, ZL, ZR, +, -, left stick click, right stick click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonMap(pub [Key; 12], pub LayerMap, pub LayerMap);

/// The keys the game wants that no single button can give (function keys, page keys, Delete...): hold the modifier
/// button (ZL by default) and press another button to send its layer key instead. Tapping the modifier alone still
/// sends its own key, a few frames long, so ZL keeps working for the game (`S`).
///
/// A [`ButtonMap`] carries two layers: the first (ZL, function and page keys) and the second (ZR, the digits and the
/// letters of menus and field moves). If both modifiers are held the first layer wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerMap {
    /// A `BUTTON_*` bit; 0 disables the layer.
    pub modifier: u32,
    /// Indexed by the bit position of the `BUTTON_*` constant (A = 0 ... STICK_R = 15).
    pub keys: [Option<Key>; 16],
}

/// `controls.txt` names of the buttons, indexed by bit position.
const LAYER_BUTTON_NAMES: [&str; 16] =
    ["A", "B", "X", "Y", "L", "R", "ZL", "ZR", "PLUS", "MINUS", "LEFT", "UP", "RIGHT", "DOWN", "STICK_L", "STICK_R"];
/// How many polls a tapped modifier presses its own key (about 70 ms at 60 polls per second).
const TAP_POLLS: u8 = 4;

impl LayerMap {
    pub const DEFAULT: LayerMap = LayerMap {
        modifier: BUTTON_ZL,
        keys: [
            Some(Key::F5),       // A: save
            Some(Key::F6),       // B: load
            Some(Key::F11),      // X: options
            Some(Key::F12),      // Y: Pokedex / info
            Some(Key::F10),      // L: sound
            Some(Key::Delete),   // R
            None,                // ZL is the modifier
            None,                // ZR
            Some(Key::Home),     // +
            Some(Key::End),      // -
            Some(Key::PageUp),   // left
            Some(Key::F1),       // up: help
            Some(Key::PageDown), // right
            Some(Key::F9),       // down: screenshot
            None,
            None,
        ],
    };

    /// The second layer: ZR plus a button sends a digit or a menu letter the first layer does not reach.
    pub const DEFAULT_SECOND: LayerMap = LayerMap {
        modifier: BUTTON_ZR,
        keys: [
            Some(Key::D1),     // A: party member / field move 1
            Some(Key::D2),     // B
            Some(Key::D3),     // X
            Some(Key::D4),     // Y
            Some(Key::D5),     // L
            Some(Key::D6),     // R
            None,              // ZL (first-layer modifier)
            None,              // ZR is the modifier
            Some(Key::T),      // +: talk
            Some(Key::N),      // -: use item
            Some(Key::L),      // left: areas menu
            Some(Key::Q),      // up: missions menu
            Some(Key::F),      // right: search
            Some(Key::P),      // down: trainer menu
            Some(Key::H),      // left stick click: time
            Some(Key::K),      // right stick click: mega evolution
        ],
    };

    pub fn button_bit(name: &str) -> Option<u32> {
        LAYER_BUTTON_NAMES.iter().position(|n| n.eq_ignore_ascii_case(name)).map(|bit| 1u32 << bit)
    }

    pub fn button_name(bit_position: usize) -> &'static str {
        LAYER_BUTTON_NAMES[bit_position]
    }
}

impl Default for LayerMap {
    fn default() -> Self {
        Self::DEFAULT
    }
}

pub const MAPPED_BUTTONS: [u32; 12] = [
    BUTTON_A, BUTTON_B, BUTTON_X, BUTTON_Y, BUTTON_L, BUTTON_R, BUTTON_ZL, BUTTON_ZR, BUTTON_PLUS, BUTTON_MINUS,
    BUTTON_STICK_L, BUTTON_STICK_R,
];
const BUTTON_NAMES: [&str; 12] = ["A", "B", "X", "Y", "L", "R", "ZL", "ZR", "PLUS", "MINUS", "STICK_L", "STICK_R"];

impl ButtonMap {
    pub const DEFAULT: ButtonMap = ButtonMap(
        [Key::Z, Key::X, Key::C, Key::V, Key::M, Key::B, Key::S, Key::O, Key::Space, Key::Return, Key::E, Key::Escape],
        LayerMap::DEFAULT,
        LayerMap::DEFAULT_SECOND,
    );

    pub fn pairs(&self) -> impl Iterator<Item = (u32, Key)> + '_ {
        MAPPED_BUTTONS.iter().copied().zip(self.0.iter().copied())
    }

    /// Parse a `controls.txt`: one `BUTTON=KEY` per line (`A=Z`, `PLUS=SPACE`), `#` starts a comment. Unknown names and
    /// lines are ignored, so a damaged file falls back to the default for those buttons.
    pub fn parse(text: &str) -> ButtonMap {
        let mut map = ButtonMap::DEFAULT;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((button, key)) = line.split_once('=') else { continue };
            let button = button.trim();
            // `LAYER` / `LAYER.<BUTTON>` is the first layer and `LAYER2` / `LAYER2.<BUTTON>` the second.
            if let Some((which, target)) = layer_line(button) {
                let layer = if which == 0 { &mut map.1 } else { &mut map.2 };
                let key = key.trim();
                match target {
                    None => {
                        if key.eq_ignore_ascii_case("NONE") {
                            layer.modifier = 0;
                        } else if let Some(bit) = LayerMap::button_bit(key) {
                            layer.modifier = bit;
                        }
                    }
                    Some(target) => {
                        if let Some(bit) = LayerMap::button_bit(target.trim()) {
                            let position = bit.trailing_zeros() as usize;
                            if key.eq_ignore_ascii_case("NONE") {
                                layer.keys[position] = None;
                            } else if let Some(key) = Key::from_name(key) {
                                layer.keys[position] = Some(key);
                            }
                        }
                    }
                }
                continue;
            }
            let Some(index) = BUTTON_NAMES.iter().position(|name| name.eq_ignore_ascii_case(button)) else { continue };
            if let Some(key) = Key::from_name(key.trim()) {
                map.0[index] = key;
            }
        }
        map
    }

    /// The inverse of [`ButtonMap::parse`]; used by the menu's tests and as documentation of the file format.
    pub fn button_name(index: usize) -> &'static str {
        BUTTON_NAMES[index]
    }
}

/// `LAYER` / `LAYER2` (the modifier line) or `LAYER.<BUTTON>` / `LAYER2.<BUTTON>` (a key line): which layer it edits and,
/// for a key line, the button name.
fn layer_line(text: &str) -> Option<(usize, Option<&str>)> {
    for (prefix, which) in [("LAYER2", 1), ("LAYER", 0)] {
        if text.len() < prefix.len() || !text.is_char_boundary(prefix.len()) || !text[..prefix.len()].eq_ignore_ascii_case(prefix) {
            continue;
        }
        let rest = &text[prefix.len()..];
        if rest.is_empty() {
            return Some((which, None));
        }
        if let Some(target) = rest.strip_prefix('.') {
            return Some((which, Some(target)));
        }
    }
    None
}

impl Default for ButtonMap {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Key {
    /// Key from its `controls.txt` name: a letter or digit, or SPACE, ENTER/RETURN, ESC/ESCAPE, TAB, SHIFT, CTRL,
    /// BACKSPACE, LEFT, RIGHT, UP, DOWN, ALT, DELETE, INSERT, HOME, END, PAGEUP, PAGEDOWN and F1-F12 (case-insensitive).
    pub fn from_name(name: &str) -> Option<Key> {
        let upper = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        if name.len() == 1 {
            let c = name.as_bytes()[0].to_ascii_uppercase();
            return ALL_KEYS.iter().copied().find(|key| *key as u8 == c && (c.is_ascii_uppercase() || c.is_ascii_digit()));
        }
        Some(match name {
            n if upper(n, "SPACE") => Key::Space,
            n if upper(n, "ENTER") || upper(n, "RETURN") => Key::Return,
            n if upper(n, "ESC") || upper(n, "ESCAPE") => Key::Escape,
            n if upper(n, "TAB") => Key::Tab,
            n if upper(n, "SHIFT") => Key::Shift,
            n if upper(n, "CTRL") || upper(n, "CONTROL") => Key::Control,
            n if upper(n, "BACKSPACE") => Key::Backspace,
            n if upper(n, "ALT") => Key::Alt,
            n if upper(n, "DELETE") || upper(n, "DEL") || upper(n, "SUPR") => Key::Delete,
            n if upper(n, "INSERT") || upper(n, "INS") => Key::Insert,
            n if upper(n, "HOME") || upper(n, "INICIO") => Key::Home,
            n if upper(n, "END") || upper(n, "FIN") => Key::End,
            n if upper(n, "PAGEUP") || upper(n, "PGUP") => Key::PageUp,
            n if upper(n, "PAGEDOWN") || upper(n, "PGDN") => Key::PageDown,
            n if upper(n, "F1") => Key::F1,
            n if upper(n, "F2") => Key::F2,
            n if upper(n, "F3") => Key::F3,
            n if upper(n, "F4") => Key::F4,
            n if upper(n, "F5") => Key::F5,
            n if upper(n, "F6") => Key::F6,
            n if upper(n, "F7") => Key::F7,
            n if upper(n, "F8") => Key::F8,
            n if upper(n, "F9") => Key::F9,
            n if upper(n, "F10") => Key::F10,
            n if upper(n, "F11") => Key::F11,
            n if upper(n, "F12") => Key::F12,
            n if upper(n, "LEFT") => Key::Left,
            n if upper(n, "RIGHT") => Key::Right,
            n if upper(n, "UP") => Key::Up,
            n if upper(n, "DOWN") => Key::Down,
            _ => return None,
        })
    }
}

const KEY_WORDS: usize = 4;
const ANALOG_ENTER: i32 = 18_022; // 55% of the signed 16-bit stick range (the default dead zone).
const ANALOG_EXIT: i32 = 13_107; // 40%; hysteresis prevents noisy edge chatter.
/// Allowed dead-zone percentages for [`SwitchInputMapper::set_stick_threshold`].
pub const STICK_PERCENT_MIN: u8 = 25;
pub const STICK_PERCENT_MAX: u8 = 85;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeySet([u64; KEY_WORDS]);

impl KeySet {
    fn insert(&mut self, key: Key) {
        let code = key as usize;
        self.0[code / 64] |= 1u64 << (code % 64);
    }

    pub fn contains(&self, key: Key) -> bool {
        let code = key as usize;
        self.0[code / 64] & (1u64 << (code % 64)) != 0
    }

    /// Write held GameMaker virtual-key codes in stable order for a runtime adapter.
    /// The output buffer holds 16 entries, the most keys a controller can hold at once (12 buttons and 4 directions).
    pub fn write_keycodes(self, output: &mut [u8; 16]) -> usize {
        let mut len = 0;
        for key in ALL_KEYS {
            if self.contains(key) && len < output.len() {
                output[len] = key as u8;
                len += 1;
            }
        }
        len
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyTransitions {
    pub held: KeySet,
    pub down: KeySet,
    pub up: KeySet,
}

/// Raw signed controller axis. nx/libnx style samples use the full `i16` range
/// (`-32768..=32767`); this contract preserves the sample without applying a
/// dead zone so gameplay and UI may use different sensitivity settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StickAxes {
    pub x: i16,
    pub y: i16,
}

/// Pixel position in the active game viewport. `None` means the platform did
/// not provide a pointer sample this frame; it does not mean `(0, 0)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerPosition {
    pub x: i32,
    pub y: i32,
}

pub const MOUSE_LEFT: u8 = 1 << 0;
pub const MOUSE_RIGHT: u8 = 1 << 1;
pub const MOUSE_MIDDLE: u8 = 1 << 2;
const MOUSE_BUTTONS: [u8; 3] = [MOUSE_LEFT, MOUSE_RIGHT, MOUSE_MIDDLE];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseTransitions {
    /// Bit mask of buttons currently held; see `MOUSE_LEFT/RIGHT/MIDDLE`.
    pub held: u8,
    pub down: u8,
    pub up: u8,
}

/// One platform-neutral input snapshot, suitable for a single runtime tick.
/// Keyboard edges are derived by [`SwitchInputMapper`]; analog samples and the
/// optional pointer are carried through unchanged. No Ramen or Horizon types
/// are exposed by this crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputFrame {
    pub keys: KeyTransitions,
    pub left_stick: StickAxes,
    pub right_stick: StickAxes,
    pub pointer: Option<PointerPosition>,
    pub mouse: MouseTransitions,
}

/// Hardware-neutral source sample consumed by [`SwitchInputMapper::sample`].
/// `connected = false` clears controller buttons and axes, producing releases;
/// pointer and mouse-button fields remain independent and are preserved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputSample {
    pub connected: bool,
    pub buttons: u32,
    pub left_stick: StickAxes,
    pub right_stick: StickAxes,
    pub pointer: Option<PointerPosition>,
    pub mouse_buttons: u8,
}

impl InputSample {
    pub const fn disconnected() -> Self {
        Self {
            connected: false,
            buttons: 0,
            left_stick: StickAxes { x: 0, y: 0 },
            right_stick: StickAxes { x: 0, y: 0 },
            pointer: None,
            mouse_buttons: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SwitchInputMapper {
    previous: KeySet,
    stick_directions: u8,
    previous_mouse: u8,
    map: ButtonMap,
    /// Per layer: its modifier was held on the previous poll / a layer key was sent during this hold / polls of tap left.
    modifier_held: [bool; 2],
    layer_used: [bool; 2],
    tap_polls: [u8; 2],
    stick_enter: i32,
    stick_exit: i32,
}

/// Select the more intentional left-stick sample when Horizon reports both the
/// handheld and player-one devices. A tiny neutral drift on one device must not
/// mask deliberate movement on the other.
pub fn select_left_stick(handheld: (i32, i32), player_one: (i32, i32)) -> (i32, i32) {
    let magnitude = |(x, y): (i32, i32)| i64::from(x) * i64::from(x) + i64::from(y) * i64::from(y);
    if magnitude(handheld) >= magnitude(player_one) { handheld } else { player_one }
}

impl Default for SwitchInputMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl SwitchInputMapper {
    pub const fn new() -> Self {
        Self {
            previous: KeySet([0; KEY_WORDS]),
            stick_directions: 0,
            previous_mouse: 0,
            map: ButtonMap::DEFAULT,
            modifier_held: [false; 2],
            layer_used: [false; 2],
            tap_polls: [0; 2],
            stick_enter: ANALOG_ENTER,
            stick_exit: ANALOG_EXIT,
        }
    }

    /// Dead zone of the left stick as a percentage of its range (the default is 55). The release point keeps the same
    /// ratio to the press point, so the stick still does not chatter at the edge. Values are clamped to
    /// `STICK_PERCENT_MIN..=STICK_PERCENT_MAX`.
    pub fn set_stick_threshold(&mut self, percent: u8) {
        let percent = i32::from(percent.clamp(STICK_PERCENT_MIN, STICK_PERCENT_MAX));
        self.stick_enter = 32_768 * percent / 100;
        self.stick_exit = self.stick_enter * ANALOG_EXIT / ANALOG_ENTER;
    }

    /// Replace the button-to-key table (held keys are re-evaluated on the next sample).
    pub fn set_map(&mut self, map: ButtonMap) {
        self.map = map;
    }

    /// Convert one hardware-neutral sample into a frame and calculate key and
    /// mouse edges. Controller disconnection releases every previously held
    /// mapped key. Mouse buttons are independent of controller connection.
    pub fn sample(&mut self, sample: InputSample) -> InputFrame {
        let connected = sample.connected;
        let (buttons, left) = if connected { (sample.buttons, sample.left_stick) } else { (0, StickAxes::default()) };
        let keys = self.update(buttons, left.x as i32, left.y as i32);
        let mouse_held = sample.mouse_buttons & (MOUSE_LEFT | MOUSE_RIGHT | MOUSE_MIDDLE);
        let mouse = transitions(self.previous_mouse, mouse_held);
        self.previous_mouse = mouse_held;
        InputFrame {
            keys,
            left_stick: if connected { left } else { StickAxes::default() },
            right_stick: if connected { sample.right_stick } else { StickAxes::default() },
            pointer: sample.pointer,
            mouse,
        }
    }

    /// Map one controller sample. `stick_x/y` use the `nx` HID signed range `-32768..=32767`.
    /// Use `(0, 0)` and no buttons when the controller disconnects; held keys then emit releases.
    pub fn update(&mut self, buttons: u32, stick_x: i32, stick_y: i32) -> KeyTransitions {
        self.update_stick(stick_x, stick_y);

        let mut held = KeySet::default();
        let layers = [self.map.1, self.map.2];
        let modifier_now = layers.map(|layer| layer.modifier != 0 && buttons & layer.modifier != 0);
        let mut buttons = buttons;
        // The first layer whose modifier is held is the one in use; the other modifier is ignored meanwhile.
        let active = modifier_now.iter().position(|held| *held);
        if let Some(index) = active {
            // While a modifier is held it sends nothing itself, and every button with a layer key sends that key.
            let layer = layers[index];
            let pressed = buttons & !layer.modifier;
            buttons &= !(layers[0].modifier | layers[1].modifier);
            for (position, entry) in layer.keys.iter().enumerate() {
                let bit = 1u32 << position;
                if let (Some(key), true) = (entry, pressed & bit != 0) {
                    held.insert(*key);
                    buttons &= !bit;
                    self.layer_used[index] = true;
                }
            }
        }
        for index in 0..2 {
            if !modifier_now[index] {
                if self.modifier_held[index] && !self.layer_used[index] {
                    self.tap_polls[index] = TAP_POLLS;
                }
                self.layer_used[index] = false;
            }
            self.modifier_held[index] = modifier_now[index];
            if self.tap_polls[index] > 0 {
                self.tap_polls[index] -= 1;
                if let Some((_, key)) = self.map.pairs().find(|(button, _)| *button == layers[index].modifier) {
                    held.insert(key);
                }
            }
        }
        for (button, key) in self.map.pairs() {
            if buttons & button != 0 {
                held.insert(key);
            }
        }

        for (button, direction, key) in [
            (BUTTON_UP, 1, Key::Up),
            (BUTTON_DOWN, 2, Key::Down),
            (BUTTON_LEFT, 4, Key::Left),
            (BUTTON_RIGHT, 8, Key::Right),
        ] {
            if buttons & button != 0 || self.stick_directions & direction != 0 {
                held.insert(key);
            }
        }

        let mut down = KeySet::default();
        let mut up = KeySet::default();
        for key in ALL_KEYS {
            let was_held = self.previous.contains(key);
            let is_held = held.contains(key);
            if is_held && !was_held {
                down.insert(key);
            } else if was_held && !is_held {
                up.insert(key);
            }
        }

        self.previous = held;
        KeyTransitions { held, down, up }
    }

    fn update_stick(&mut self, x: i32, y: i32) {
        let (enter, exit) = (self.stick_enter, self.stick_exit);
        self.stick_directions = update_axis_direction(self.stick_directions, 1, 2, y, true, enter, exit)
            | update_axis_direction(self.stick_directions, 4, 8, x, false, enter, exit);
    }
}

fn transitions(previous: u8, held: u8) -> MouseTransitions {
    let mut down = 0;
    let mut up = 0;
    for button in MOUSE_BUTTONS {
        if held & button != 0 && previous & button == 0 {
            down |= button;
        } else if held & button == 0 && previous & button != 0 {
            up |= button;
        }
    }
    MouseTransitions { held, down, up }
}

/// Every key the mapper can produce (a remapped button may target any of them).
const ALL_KEYS: [Key; 66] = [
    Key::Backspace,
    Key::Tab,
    Key::Return,
    Key::Shift,
    Key::Control,
    Key::Alt,
    Key::Escape,
    Key::Space,
    Key::PageUp,
    Key::PageDown,
    Key::End,
    Key::Home,
    Key::Left,
    Key::Up,
    Key::Right,
    Key::Down,
    Key::Insert,
    Key::Delete,
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
    Key::D0,
    Key::D1,
    Key::D2,
    Key::D3,
    Key::D4,
    Key::D5,
    Key::D6,
    Key::D7,
    Key::D8,
    Key::D9,
    Key::A,
    Key::B,
    Key::C,
    Key::D,
    Key::E,
    Key::F,
    Key::G,
    Key::H,
    Key::I,
    Key::J,
    Key::K,
    Key::L,
    Key::M,
    Key::N,
    Key::O,
    Key::P,
    Key::Q,
    Key::R,
    Key::S,
    Key::T,
    Key::U,
    Key::V,
    Key::W,
    Key::X,
    Key::Y,
    Key::Z,
];

fn update_axis_direction(current: u8, negative_bit: u8, positive_bit: u8, value: i32, invert: bool, enter: i32, exit: i32) -> u8 {
    let (negative, positive) = if invert { (value > enter, value < -enter) } else { (value < -enter, value > enter) };
    let negative_held = current & negative_bit != 0;
    let positive_held = current & positive_bit != 0;
    let negative = negative || (negative_held && if invert { value > exit } else { value < -exit });
    let positive = positive || (positive_held && if invert { value < -exit } else { value > exit });
    (if negative { negative_bit } else { 0 }) | (if positive { positive_bit } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_switch_face_buttons_and_emits_edges() {
        let mut mapper = SwitchInputMapper::default();
        let pressed = mapper.update(BUTTON_A | BUTTON_B | BUTTON_PLUS, 0, 0);
        assert!(pressed.held.contains(Key::Z));
        assert!(pressed.down.contains(Key::Z));
        assert!(pressed.held.contains(Key::X));
        assert!(pressed.held.contains(Key::Space));

        let held = mapper.update(BUTTON_A | BUTTON_B | BUTTON_PLUS, 0, 0);
        assert!(held.down == KeySet::default());
        assert!(held.up == KeySet::default());

        let released = mapper.update(0, 0, 0);
        assert!(released.up.contains(Key::Z));
        assert!(released.up.contains(Key::X));
        assert!(released.up.contains(Key::Space));
    }

    #[test]
    fn analog_stick_uses_deadzone_and_hysteresis() {
        let mut mapper = SwitchInputMapper::default();
        assert!(!mapper.update(0, 17_000, 0).held.contains(Key::Right));
        assert!(mapper.update(0, 18_100, 0).down.contains(Key::Right));
        assert!(mapper.update(0, 15_000, 0).held.contains(Key::Right));
        assert!(mapper.update(0, 13_000, 0).up.contains(Key::Right));
    }

    #[test]
    fn the_stick_dead_zone_can_be_changed_and_is_clamped() {
        let mut small = SwitchInputMapper::default();
        small.set_stick_threshold(30);
        assert!(small.update(0, 11_000, 0).down.contains(Key::Right), "a light push counts with a small dead zone");
        let mut large = SwitchInputMapper::default();
        large.set_stick_threshold(75);
        assert!(!large.update(0, 20_000, 0).held.contains(Key::Right), "the default press point is not enough with a large one");
        assert!(large.update(0, 26_000, 0).down.contains(Key::Right));
        let mut clamped = SwitchInputMapper::default();
        clamped.set_stick_threshold(0);
        assert!(!clamped.update(0, 6_000, 0).held.contains(Key::Right), "never below the minimum");
        clamped.set_stick_threshold(255);
        assert!(clamped.update(0, 32_000, 0).held.contains(Key::Right), "never above the maximum");
    }

    #[test]
    fn dpad_keeps_key_held_when_stick_releases() {
        let mut mapper = SwitchInputMapper::default();
        mapper.update(0, 20_000, 0);
        let overlap = mapper.update(BUTTON_RIGHT, 0, 0);
        assert!(overlap.held.contains(Key::Right));
        assert!(!overlap.up.contains(Key::Right));
        assert!(mapper.update(0, 0, 0).up.contains(Key::Right));
    }

    #[test]
    fn a_disconnected_controller_releases_held_input() {
        let mut mapper = SwitchInputMapper::default();
        mapper.update(BUTTON_STICK_L, 0, 0);
        let disconnected = mapper.update(0, 0, 0);
        assert!(disconnected.up.contains(Key::E));
    }

    #[test]
    fn held_keys_export_as_opengmk_virtual_key_codes() {
        let mut mapper = SwitchInputMapper::default();
        let held = mapper.update(BUTTON_A | BUTTON_LEFT | BUTTON_PLUS, 0, 0).held;
        let mut codes = [0; 16];
        let len = held.write_keycodes(&mut codes);
        assert_eq!(&codes[..len], &[0x20, 0x25, b'Z']);
    }

    #[test]
    fn slight_handheld_drift_does_not_mask_player_one_stick() {
        assert_eq!(select_left_stick((1_500, -800), (24_000, 0)), (24_000, 0));
    }

    #[test]
    fn stick_selection_uses_strongest_vector_and_handles_zero() {
        assert_eq!(select_left_stick((0, 0), (-12_000, 9_000)), (-12_000, 9_000));
        assert_eq!(select_left_stick((0, 0), (0, 0)), (0, 0));
    }

    #[test]
    fn input_frame_preserves_both_raw_stick_ranges_and_pointer_coordinates() {
        let mut mapper = SwitchInputMapper::default();
        let frame = mapper.sample(InputSample {
            connected: true,
            left_stick: StickAxes { x: i16::MIN, y: i16::MAX },
            right_stick: StickAxes { x: -12_345, y: 23_456 },
            pointer: Some(PointerPosition { x: 1279, y: 719 }),
            ..InputSample::default()
        });
        assert_eq!(frame.left_stick, StickAxes { x: -32768, y: 32767 });
        assert_eq!(frame.right_stick, StickAxes { x: -12_345, y: 23_456 });
        assert_eq!(frame.pointer, Some(PointerPosition { x: 1279, y: 719 }));
    }

    #[test]
    fn mouse_buttons_emit_held_down_and_up_edges() {
        let mut mapper = SwitchInputMapper::default();
        let pressed = mapper.sample(InputSample { mouse_buttons: MOUSE_LEFT | MOUSE_RIGHT, ..InputSample::default() });
        assert_eq!(pressed.mouse, MouseTransitions {
            held: MOUSE_LEFT | MOUSE_RIGHT,
            down: MOUSE_LEFT | MOUSE_RIGHT,
            up: 0
        });
        let held = mapper.sample(InputSample { mouse_buttons: MOUSE_LEFT | MOUSE_RIGHT, ..InputSample::default() });
        assert_eq!(held.mouse.down, 0);
        let released = mapper.sample(InputSample { mouse_buttons: MOUSE_RIGHT, ..InputSample::default() });
        assert_eq!(released.mouse.held, MOUSE_RIGHT);
        assert_eq!(released.mouse.up, MOUSE_LEFT);
    }

    #[test]
    fn disconnect_releases_keys_and_zeros_axes_but_keeps_independent_pointer() {
        let mut mapper = SwitchInputMapper::default();
        mapper.sample(InputSample {
            connected: true,
            buttons: BUTTON_A,
            left_stick: StickAxes { x: 22_000, y: 0 },
            ..InputSample::default()
        });
        let frame = mapper
            .sample(InputSample { pointer: Some(PointerPosition { x: 10, y: 20 }), ..InputSample::disconnected() });
        assert!(frame.keys.up.contains(Key::Z));
        assert!(frame.keys.up.contains(Key::Right));
        assert_eq!(frame.left_stick, StickAxes::default());
        assert_eq!(frame.right_stick, StickAxes::default());
        assert_eq!(frame.pointer, Some(PointerPosition { x: 10, y: 20 }));
    }

    #[test]
    fn gm_action_mapping_is_explicit_and_dpad_and_left_stick_share_edges() {
        let mut mapper = SwitchInputMapper::default();
        let frame = mapper.sample(InputSample {
            connected: true,
            buttons: BUTTON_A | BUTTON_B | BUTTON_X | BUTTON_Y | BUTTON_PLUS,
            ..InputSample::default()
        });
        for key in [Key::Z, Key::X, Key::C, Key::V, Key::Space] {
            assert!(frame.keys.held.contains(key), "expected mapped action {key:?}");
            assert!(frame.keys.down.contains(key));
        }
        mapper.sample(InputSample {
            connected: true,
            left_stick: StickAxes { x: 22_000, y: 0 },
            ..InputSample::default()
        });
        let overlap = mapper.sample(InputSample { connected: true, buttons: BUTTON_RIGHT, ..InputSample::default() });
        assert!(overlap.keys.held.contains(Key::Right));
        assert!(!overlap.keys.up.contains(Key::Right));
    }
}

#[cfg(test)]
mod button_map_tests {
    use super::*;

    #[test]
    fn default_map_is_the_original_layout() {
        let mut mapper = SwitchInputMapper::new();
        let held = mapper.update(BUTTON_A | BUTTON_PLUS, 0, 0).held;
        assert!(held.contains(Key::Z) && held.contains(Key::Space));
    }

    #[test]
    fn controls_file_remaps_buttons_and_ignores_garbage() {
        let map = ButtonMap::parse("# my layout\nA = x\nb=ENTER\nZR=q  # shoulder\nnonsense\nPLUS=NOPE\nR=7\nLAYER2=NONE\n");
        assert_eq!(map.0[0], Key::X);
        assert_eq!(map.0[1], Key::Return);
        assert_eq!(map.0[7], Key::Q);
        assert_eq!(map.0[8], Key::Space, "unknown key leaves the default");
        assert_eq!(map.0[5], Key::D7);
        let mut mapper = SwitchInputMapper::new();
        mapper.set_map(map);
        let held = mapper.update(BUTTON_A | BUTTON_ZR, 0, 0).held;
        assert!(held.contains(Key::X) && held.contains(Key::Q) && !held.contains(Key::Z));
    }

    #[test]
    fn function_and_page_keys_have_names_and_codes() {
        for (name, code) in [("F1", 0x70u8), ("f5", 0x74), ("F12", 0x7B), ("Delete", 0x2E), ("supr", 0x2E), ("Insert", 0x2D), ("HOME", 0x24),
            ("end", 0x23), ("PageUp", 0x21), ("pgdn", 0x22), ("ALT", 0x12)] {
            assert_eq!(Key::from_name(name).map(|key| key as u8), Some(code), "{name}");
        }
        assert!(Key::from_name("F13").is_none());
    }

    #[test]
    fn holding_the_modifier_sends_layer_keys_instead_of_the_normal_ones() {
        let mut mapper = SwitchInputMapper::new();
        let layered = mapper.update(BUTTON_ZL | BUTTON_A | BUTTON_UP, 0, 0);
        assert!(layered.held.contains(Key::F5) && layered.held.contains(Key::F1));
        assert!(!layered.held.contains(Key::Z) && !layered.held.contains(Key::S), "base keys stay quiet under the layer");
        assert!(!layered.held.contains(Key::Up), "the d-pad direction is replaced too");
        let released = mapper.update(0, 0, 0);
        assert!(released.up.contains(Key::F5) && released.up.contains(Key::F1));
        assert!(!released.down.contains(Key::S), "using the layer must not tap the modifier's own key");
        for _ in 0..8 {
            assert!(!mapper.update(0, 0, 0).held.contains(Key::S));
        }
    }

    #[test]
    fn a_lone_modifier_tap_still_sends_its_own_key_for_a_few_polls() {
        let mut mapper = SwitchInputMapper::new();
        assert!(!mapper.update(BUTTON_ZL, 0, 0).held.contains(Key::S), "nothing is sent while it may still be a layer press");
        let mut presses = 0;
        for _ in 0..8 {
            if mapper.update(0, 0, 0).held.contains(Key::S) {
                presses += 1;
            }
        }
        assert_eq!(presses, TAP_POLLS as usize);
    }

    #[test]
    fn controls_file_edits_the_layer() {
        let map = ButtonMap::parse("LAYER=R\nLAYER.A=F9\nlayer.up=none\nLAYER.LEFT=ins\nLAYER.B=zzz\n");
        assert_eq!(map.1.modifier, BUTTON_R);
        assert_eq!(map.1.keys[0], Some(Key::F9));
        assert_eq!(map.1.keys[11], None);
        assert_eq!(map.1.keys[10], Some(Key::Insert));
        assert_eq!(map.1.keys[1], Some(Key::F6), "unknown key leaves the default");
        assert_eq!(ButtonMap::parse("LAYER=NONE\n").1.modifier, 0);
        let mut mapper = SwitchInputMapper::new();
        mapper.set_map(ButtonMap::parse("LAYER=NONE\n"));
        assert!(mapper.update(BUTTON_ZL | BUTTON_A, 0, 0).held.contains(Key::Z), "no layer: ZL and A behave as before");
    }

    #[test]
    fn the_second_layer_sends_digits_and_letters_under_zr() {
        let mut mapper = SwitchInputMapper::new();
        let layered = mapper.update(BUTTON_ZR | BUTTON_A | BUTTON_STICK_R, 0, 0);
        assert!(layered.held.contains(Key::D1) && layered.held.contains(Key::K));
        assert!(!layered.held.contains(Key::Z) && !layered.held.contains(Key::O) && !layered.held.contains(Key::Escape));
        let released = mapper.update(0, 0, 0);
        assert!(released.up.contains(Key::D1));
        for _ in 0..8 {
            assert!(!mapper.update(0, 0, 0).held.contains(Key::O), "using the second layer must not tap ZR's own key");
        }
        let mut tap = SwitchInputMapper::new();
        tap.update(BUTTON_ZR, 0, 0);
        let presses = (0..8).filter(|_| tap.update(0, 0, 0).held.contains(Key::O)).count();
        assert_eq!(presses, TAP_POLLS as usize, "a lone ZR tap still sends O");
    }

    #[test]
    fn the_first_layer_wins_when_both_modifiers_are_held() {
        let mut mapper = SwitchInputMapper::new();
        let keys = mapper.update(BUTTON_ZL | BUTTON_ZR | BUTTON_A, 0, 0);
        assert!(keys.held.contains(Key::F5) && !keys.held.contains(Key::D1));
        assert!(!keys.held.contains(Key::O) && !keys.held.contains(Key::S));
    }

    #[test]
    fn controls_file_edits_the_second_layer_without_touching_the_first() {
        let map = ButtonMap::parse("LAYER2=L\nLAYER2.A=F\nlayer2.up=none\nLAYER.A=F9\nLAYER2.B=zzz\nlayer2=bogus\n");
        assert_eq!(map.2.modifier, BUTTON_L);
        assert_eq!(map.2.keys[0], Some(Key::F));
        assert_eq!(map.2.keys[11], None);
        assert_eq!(map.2.keys[1], Some(Key::D2), "unknown key leaves the default");
        assert_eq!((map.1.modifier, map.1.keys[0]), (BUTTON_ZL, Some(Key::F9)), "LAYER. lines still edit the first layer");
        assert_eq!(ButtonMap::parse("LAYER2=NONE\n").2.modifier, 0);
        let mut mapper = SwitchInputMapper::new();
        mapper.set_map(ButtonMap::parse("LAYER2=NONE\n"));
        assert!(mapper.update(BUTTON_ZR | BUTTON_A, 0, 0).held.contains(Key::Z), "no second layer: ZR and A behave as before");
        assert_eq!(ButtonMap::parse("").2, LayerMap::DEFAULT_SECOND);
    }

    #[test]
    fn key_names_round_trip() {
        for name in ["a", "Z", "0", "space", "Esc", "ctrl", "tab", "shift", "backspace", "left", "DOWN"] {
            assert!(Key::from_name(name).is_some(), "{name}");
        }
        assert!(Key::from_name("F13").is_none() && Key::from_name("").is_none() && Key::from_name("é").is_none());
    }
}
