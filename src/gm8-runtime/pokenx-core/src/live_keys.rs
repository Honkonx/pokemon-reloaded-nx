//! The keys the game uses *right now*.
//!
//! A player changes keys inside the game and the game keeps them in its saves, in a binary format that only its own scripts
//! read. Instead of decoding saves, the running game is asked: every couple of seconds the `key_<context>_<action>`
//! variables of its controller object are read and written, in the same `group|action|KEY` form as the defaults report, to
//! `cache/game-keys-live.txt` (only when something changed). The menu's "Teclas" screen shows them next to the defaults.
//! Nothing here changes the game; it only reads.

use crate::{game::Game, gml::Value, keys_report::KeyBinding};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Mutex,
    },
};

/// The object that holds the game's key variables (the game reads them as `(3).key_...`).
const KEYS_OBJECT: i32 = 3;
/// How often the variables are read, in frames (about every two seconds at the game's 60 steps per second).
const INTERVAL: u32 = 120;

/// Steps since the last read (the game's own frame counter is reset about once a second, so it cannot be used).
static STEPS: AtomicU32 = AtomicU32::new(0);
static OUTPUT: Mutex<Option<PathBuf>> = Mutex::new(None);
static LAST: Mutex<String> = Mutex::new(String::new());

/// Where to write the live report; `None` turns the capture off.
pub fn set_output(path: Option<PathBuf>) {
    if let Ok(mut slot) = OUTPUT.lock() {
        *slot = path;
    }
    if let Ok(mut last) = LAST.lock() {
        last.clear();
    }
}

/// The `controls.txt`/report name of a GameMaker virtual-key code (`#<code>` when it has no name).
pub fn key_name(code: i32) -> String {
    match code {
        48..=57 | 65..=90 => char::from(code as u8).to_string(),
        8 => "BACKSPACE".to_owned(),
        9 => "TAB".to_owned(),
        13 => "ENTER".to_owned(),
        16 => "SHIFT".to_owned(),
        17 => "CTRL".to_owned(),
        18 => "ALT".to_owned(),
        27 => "ESC".to_owned(),
        32 => "SPACE".to_owned(),
        33 => "PAGEUP".to_owned(),
        34 => "PAGEDOWN".to_owned(),
        35 => "END".to_owned(),
        36 => "HOME".to_owned(),
        37 => "LEFT".to_owned(),
        38 => "UP".to_owned(),
        39 => "RIGHT".to_owned(),
        40 => "DOWN".to_owned(),
        45 => "INSERT".to_owned(),
        46 => "DELETE".to_owned(),
        112..=123 => format!("F{}", code - 111),
        _ => format!("#{code}"),
    }
}

/// `key_<group>_<action>` -> (group, action), the split the defaults scan uses.
pub fn split_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("key_")?;
    Some(rest.split_once('_').unwrap_or(("otros", rest)))
}

/// The report text for the bindings, sorted so an unchanged set always produces the same text.
pub fn report_text(bindings: &mut Vec<KeyBinding>) -> String {
    bindings.sort_by(|a, b| (&a.group, &a.action).cmp(&(&b.group, &b.action)));
    let mut text = String::from("# PokeNX: teclas que el juego usa ahora (se regenera mientras juegas)\n");
    for binding in bindings.iter() {
        text.push_str(&format!("{}|{}|{}\n", binding.group, binding.action, binding.key));
    }
    text
}

/// Called after every game step. Reads the variables once per [`INTERVAL`] frames and writes the report when it changed.
pub fn capture(game: &Game) {
    if STEPS.fetch_add(1, Ordering::Relaxed) % INTERVAL != 0 {
        return;
    }
    let Some(path) = OUTPUT.lock().ok().and_then(|slot| slot.clone()) else { return };
    let mut iter = game.room.instance_list.iter_by_object(KEYS_OBJECT);
    let Some(handle) = iter.next(&game.room.instance_list) else { return };
    let instance = game.room.instance_list.get(handle);
    let mut bindings = Vec::new();
    for (id, field) in instance.fields.borrow().iter() {
        let Some(name) = game.compiler.field_name(*id).and_then(|name| core::str::from_utf8(name).ok()) else { continue };
        let Some((group, action)) = split_name(name) else { continue };
        if let crate::instance::Field::Single(value @ Value::Real(_)) = field {
            bindings.push(KeyBinding { group: group.to_owned(), action: action.to_owned(), key: key_name(value.round()) });
        }
    }
    if bindings.is_empty() {
        return;
    }
    let text = report_text(&mut bindings);
    let changed = LAST.lock().map(|mut last| if *last == text { false } else { *last = text.clone(); true }).unwrap_or(false);
    if changed {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_key_codes_get_the_names_controls_use() {
        assert_eq!(key_name(90), "Z");
        assert_eq!(key_name(49), "1");
        assert_eq!(key_name(32), "SPACE");
        assert_eq!(key_name(13), "ENTER");
        assert_eq!(key_name(27), "ESC");
        assert_eq!(key_name(112), "F1");
        assert_eq!(key_name(123), "F12");
        assert_eq!(key_name(33), "PAGEUP");
        assert_eq!(key_name(46), "DELETE");
        assert_eq!(key_name(220), "#220");
        assert_eq!(key_name(17), "CTRL");
    }

    #[test]
    fn variable_names_split_like_the_defaults_scan() {
        assert_eq!(split_name("key_bt_mov_arriba"), Some(("bt", "mov_arriba")));
        assert_eq!(split_name("key_captura_pantalla"), Some(("captura", "pantalla")));
        assert_eq!(split_name("key_x"), Some(("otros", "x")));
        assert_eq!(split_name("monkey_bt_x"), None);
        assert_eq!(split_name("lugar"), None);
    }

    #[test]
    fn the_report_is_sorted_and_round_trips_through_the_defaults_parser() {
        let mut bindings = vec![
            KeyBinding { group: "ex".into(), action: "hora".into(), key: "H".into() },
            KeyBinding { group: "bt".into(), action: "ataque".into(), key: "Z".into() },
        ];
        let text = report_text(&mut bindings);
        assert!(text.find("bt|ataque|Z").unwrap() < text.find("ex|hora|H").unwrap());
        let (_, _, parsed) = crate::keys_report::parse_report(&text);
        assert_eq!(parsed, bindings);
    }
}
