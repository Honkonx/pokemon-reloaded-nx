//! Lists which keyboard/mouse/joystick inputs a GM8 game reads: key events of objects and calls to
//! `keyboard_*`, `mouse_*`, `joystick_*` in every piece of GML. Prints counts only (no script text).
//! usage: cargo run -p gm8exe --example input_usage -- <game.exe>
use gm8exe::asset::{PascalString, code_action::CodeAction};
use std::{collections::BTreeMap, env, fs, process};

const PREFIXES: [&str; 14] = [
    "keyboard_", "mouse_", "joystick_", "io_clear", "game_end", "window_set_fullscreen", "file_text_open_write",
    "file_text_open_append", "file_bin_open", "ini_open", "screen_save", "sprite_save", "file_delete", "directory_create",
];

fn scan(code: &str, found: &mut BTreeMap<String, usize>) {
    scan_assignments(code, found);
    let bytes = code.as_bytes();
    for prefix in PREFIXES {
        let mut from = 0;
        while let Some(at) = code[from..].find(prefix) {
            let start = from + at;
            from = start + prefix.len();
            if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
                continue;
            }
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            let name = &code[start..end];
            let mut arg = String::new();
            if code[end..].starts_with('(') {
                let mut depth = 0;
                for ch in code[end..].chars() {
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                        if depth == 0 {
                            arg.push(ch);
                            break;
                        }
                    }
                    if !ch.is_whitespace() {
                        arg.push(ch);
                    }
                    if arg.len() > 70 {
                        break;
                    }
                }
            }
            *found.entry(format!("{name}{arg}")).or_default() += 1;
        }
    }
}

/// Records `key_xxx = value;` assignments (default key bindings the game sets itself).
fn scan_assignments(code: &str, found: &mut BTreeMap<String, usize>) {
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(at) = code[from..].find("key_") {
        let start = from + at;
        from = start + 4;
        if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            continue;
        }
        let mut end = start;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        let rest = code[end..].trim_start();
        if rest.starts_with('=') && !rest.starts_with("==") {
            let value: String = rest[1..].chars().take_while(|c| *c != ';' && *c != '\n').filter(|c| !c.is_whitespace()).take(40).collect();
            *found.entry(format!("ASSIGN {} = {}", &code[start..end], value)).or_default() += 1;
        }
    }
}

fn actions(list: &[CodeAction], found: &mut BTreeMap<String, usize>) {
    for action in list {
        for text in action.param_strings.iter().chain(std::iter::once(&action.fn_code)) {
            scan(&String::from_utf8_lossy(&text.0), found);
        }
    }
}

fn main() {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!("usage: input_usage <game.exe>");
        process::exit(2);
    };
    let mut bytes = fs::read(&path).unwrap_or_else(|e| {
        eprintln!("read failed: {e}");
        process::exit(2);
    });
    let game = gm8exe::reader::from_exe(&mut bytes, None::<fn(&str)>, false, false).unwrap_or_else(|e| {
        eprintln!("parse failed: {e}");
        process::exit(1);
    });
    let mut found = BTreeMap::new();
    let text = |s: &PascalString| String::from_utf8_lossy(&s.0).into_owned();
    for script in game.scripts.iter().flatten() {
        scan(&text(&script.source), &mut found);
    }
    for room in game.rooms.iter().flatten() {
        scan(&text(&room.creation_code), &mut found);
        for instance in &room.instances {
            scan(&text(&instance.creation_code), &mut found);
        }
    }
    for timeline in game.timelines.iter().flatten() {
        for (_, list) in &timeline.moments {
            actions(list, &mut found);
        }
    }
    let mut key_events: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for object in game.objects.iter().flatten() {
        for (kind, events) in object.events.iter().enumerate() {
            for (sub, list) in events {
                if matches!(kind, 5 | 9 | 10) {
                    *key_events.entry((kind as u32, *sub)).or_default() += 1;
                }
                actions(list, &mut found);
            }
        }
    }
    for trigger in game.triggers.iter().flatten() {
        scan(&text(&trigger.condition), &mut found);
    }
    println!("== key events (kind 5 keyboard, 9 key press, 10 key release): code -> objects");
    for ((kind, key), count) in &key_events {
        println!("{kind}\t{key}\t{count}");
    }
    println!("== calls (name+argument -> occurrences)");
    for (call, count) in &found {
        println!("{count}\t{call}");
    }
}
