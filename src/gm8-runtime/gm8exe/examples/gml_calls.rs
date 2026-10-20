//! Lists every identifier called like a function in the GML of a GM8 game, with how many times it appears, and
//! the names of the game's own scripts (so they can be told apart from built-in functions). Prints names and counts only.
//! usage: cargo run -p gm8exe --example gml_calls -- <game.exe>
use gm8exe::{asset::{PascalString, code_action::CodeAction}};
use std::{collections::BTreeMap, env, fs, process};

fn scan(code: &str, found: &mut BTreeMap<String, usize>) {
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                i += 1;
            }
            i += 1;
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let mut j = i;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if bytes.get(j) == Some(&b'(') && (start == 0 || bytes[start - 1] != b'.') {
                *found.entry(code[start..i].to_owned()).or_default() += 1;
            }
        } else {
            i += 1;
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
        eprintln!("usage: gml_calls <game.exe>");
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
    let text = |s: &PascalString| String::from_utf8_lossy(&s.0).into_owned();
    let mut found = BTreeMap::new();
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
    for object in game.objects.iter().flatten() {
        for events in &object.events {
            for (_, list) in events {
                actions(list, &mut found);
            }
        }
    }
    for trigger in game.triggers.iter().flatten() {
        scan(&text(&trigger.condition), &mut found);
    }
    for (name, count) in &found {
        println!("CALL\t{count}\t{name}");
    }
    for script in game.scripts.iter().flatten() {
        println!("SCRIPT\t{}", text(&script.name));
    }
}
