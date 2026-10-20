//! Which room usually comes after which, learned while playing.
//!
//! Room code is compiled when a room is entered (see `asset::room::LazyCode`). The game pauses itself often (`sleep(500)` in
//! dialogs, scenes, transitions); that idle time is used to compile ahead the rooms that most often follow the current one.
//! The counts are kept in `cache/room-hints.txt` (`from to count` per line) so the next session starts with them. A stale
//! file (another game version) only makes the engine compile a few rooms nobody enters; nothing depends on it.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Mutex,
};

/// Successors kept per room and compiled ahead per pause (the most frequent first).
const MAX_SUCCESSORS: usize = 4;
/// Room changes between saves of the file (each save is one small SD write).
const SAVE_EVERY: u32 = 8;
/// Rooms with more transitions than this are trimmed when saving (keeps the file small).
const MAX_ROOMS: usize = 4000;

#[derive(Default)]
struct Hints {
    next: BTreeMap<i32, BTreeMap<i32, u32>>,
    /// Rooms whose code is fully compiled already (not worth visiting again).
    warmed: BTreeSet<i32>,
    path: Option<PathBuf>,
    unsaved: u32,
}

static STATE: Mutex<Option<Hints>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut Hints) -> R) -> Option<R> {
    let mut guard = STATE.lock().ok()?;
    Some(f(guard.get_or_insert_with(Hints::default)))
}

/// Parses the file text (lines `from to count`; anything else is ignored).
fn parse(text: &str) -> BTreeMap<i32, BTreeMap<i32, u32>> {
    let mut next: BTreeMap<i32, BTreeMap<i32, u32>> = BTreeMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(from), Some(to), Some(count)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if let (Ok(from), Ok(to), Ok(count)) = (from.parse::<i32>(), to.parse::<i32>(), count.parse::<u32>()) {
            if from >= 0 && to >= 0 && count > 0 {
                *next.entry(from).or_default().entry(to).or_insert(0) += count;
            }
        }
    }
    next
}

fn render(next: &BTreeMap<i32, BTreeMap<i32, u32>>) -> String {
    let mut text = String::from("# PokeNX: salas que suelen seguir a cada sala (origen destino veces)\n");
    for (from, targets) in next.iter().take(MAX_ROOMS) {
        let mut ordered: Vec<(&i32, &u32)> = targets.iter().collect();
        ordered.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (to, count) in ordered.into_iter().take(MAX_SUCCESSORS * 2) {
            text.push_str(&format!("{from} {to} {count}\n"));
        }
    }
    text
}

/// Where the counts are kept; `None` keeps them in memory only. Loads the file when it exists.
pub fn set_output(path: Option<PathBuf>) {
    with_state(|hints| {
        hints.next = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| parse(&t)).unwrap_or_default();
        hints.warmed.clear();
        hints.unsaved = 0;
        hints.path = path;
    });
}

/// The player went from room `from` to room `to`.
pub fn note_transition(from: i32, to: i32) {
    if from < 0 || to < 0 || from == to {
        return;
    }
    with_state(|hints| {
        *hints.next.entry(from).or_default().entry(to).or_insert(0) += 1;
        hints.unsaved += 1;
        if hints.unsaved >= SAVE_EVERY {
            save_locked(hints);
        }
    });
}

fn save_locked(hints: &mut Hints) {
    hints.unsaved = 0;
    if let Some(path) = hints.path.as_ref() {
        let _ = std::fs::write(path, render(&hints.next));
    }
}

/// Writes the counts now (end of the session).
pub fn save() {
    with_state(|hints| {
        if hints.unsaved > 0 {
            save_locked(hints);
        }
    });
}

/// The rooms that most often follow `from`, most frequent first, leaving out the ones already compiled ahead.
pub fn candidates(from: i32) -> Vec<i32> {
    with_state(|hints| {
        let Some(targets) = hints.next.get(&from) else { return Vec::new() };
        let mut ordered: Vec<(i32, u32)> = targets.iter().map(|(room, count)| (*room, *count)).collect();
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        ordered.into_iter().map(|(room, _)| room).filter(|room| !hints.warmed.contains(room)).take(MAX_SUCCESSORS).collect()
    })
    .unwrap_or_default()
}

/// The code of `room` is fully compiled.
pub fn mark_warmed(room: i32) {
    with_state(|hints| {
        hints.warmed.insert(room);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_render_round_trip() {
        let next = parse("# comentario\n3 4 10\n3 5 2\n3 4 1\nmal formado\n7 8 0\n-1 2 5\n");
        assert_eq!(next.get(&3).and_then(|t| t.get(&4)), Some(&11));
        assert_eq!(next.get(&3).and_then(|t| t.get(&5)), Some(&2));
        assert!(!next.contains_key(&7) && !next.contains_key(&-1));
        let text = render(&next);
        assert!(text.contains("3 4 11\n") && text.contains("3 5 2\n"));
        assert_eq!(parse(&text), next);
    }

    #[test]
    fn candidates_are_ordered_and_skip_compiled_rooms() {
        set_output(None);
        for _ in 0..3 {
            note_transition(1, 2);
        }
        note_transition(1, 3);
        for _ in 0..5 {
            note_transition(1, 4);
        }
        note_transition(1, 1); // staying in the same room is not a transition
        assert_eq!(candidates(1), vec![4, 2, 3]);
        mark_warmed(4);
        assert_eq!(candidates(1), vec![2, 3]);
        assert!(candidates(99).is_empty());
        set_output(None);
    }
}
