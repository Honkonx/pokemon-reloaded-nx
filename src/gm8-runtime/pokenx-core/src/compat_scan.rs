//! Which built-in GML functions a version of the game calls, and which of them this engine does not run (or runs in a
//! simplified way). It reads the game's GML text once, the way `keys_report` does, and keeps only function names and counts:
//! no script text. The report is written to the SD card (`cache/game-compat.txt`) for the menu's "Compatibilidad" screen, so a
//! new version of the game can be checked right after it is installed.
//!
//! This is a static count, not a test: it cannot see calls built at run time (`script_execute` with a computed script,
//! `execute_string`), and a function that exists may still behave differently from GameMaker 8. It answers one question:
//! "does the new version call something the engine has no function for?".

use alloc::collections::{BTreeMap, BTreeSet};
use gm8exe::asset::{code_action::CodeAction, PascalString};

/// Functions the engine has but runs in a simplified way (or not at all), and what that means for the game. The list comes
/// from reading `gml/kernel.rs` against what the game calls; a name here is reported only when the game calls it.
const SIMPLIFIED: &[(&str, &str)] = &[
    ("message_alpha", "solo el aspecto de los cuadros de mensaje"),
    ("message_background", "solo el aspecto de los cuadros de mensaje"),
    ("message_button", "solo el aspecto de los cuadros de mensaje"),
    ("message_button_font", "solo el aspecto de los cuadros de mensaje"),
    ("message_text_font", "solo el aspecto de los cuadros de mensaje"),
    ("window_set_fullscreen", "irrelevante en la Switch (siempre pantalla completa)"),
    ("window_get_fullscreen", "irrelevante en la Switch (siempre pantalla completa)"),
    ("chr", "solo importa con texto fuera de ASCII y Latin-1"),
    ("window_set_cursor", "sin efecto visible en la consola"),
    ("get_open_filename", "función del PC; el juego ya maneja que se cancele"),
];

/// Words the GML grammar allows before a parenthesis that are not function calls.
const KEYWORDS: &[&str] = &[
    "if", "while", "for", "repeat", "switch", "with", "until", "return", "exit", "var", "do", "else", "case", "default", "break", "continue",
    "globalvar", "not", "and", "or", "xor", "div", "mod", "begin", "end", "then", "self", "other", "all", "noone", "global", "local",
];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Distinct built-in functions the game calls.
    pub builtin_calls: usize,
    /// Scripts the game defines itself.
    pub scripts: usize,
    /// Called built-in names the engine has no function for, with how many times.
    pub missing: Vec<(String, usize)>,
    /// Called names the engine runs in a simplified way, with how many times and what that means.
    pub simplified: Vec<(String, usize, String)>,
}

/// Every name used like a function call in `code`, with strings and `//` comments skipped.
fn scan_code(code: &str, found: &mut BTreeMap<String, usize>) {
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

fn scan_actions(list: &[CodeAction], found: &mut BTreeMap<String, usize>) {
    for action in list {
        for text in action.param_strings.iter().chain(core::iter::once(&action.fn_code)) {
            scan_code(&String::from_utf8_lossy(&text.0), found);
        }
    }
}

/// Counts the calls in the game's GML and sorts them into built-in functions the engine has, has simplified, or lacks.
pub fn scan_assets(assets: &gm8exe::GameAssets) -> Report {
    let text = |s: &PascalString| String::from_utf8_lossy(&s.0).into_owned();
    let mut found = BTreeMap::new();
    for script in assets.scripts.iter().flatten() {
        scan_code(&text(&script.source), &mut found);
    }
    for room in assets.rooms.iter().flatten() {
        scan_code(&text(&room.creation_code), &mut found);
        for instance in &room.instances {
            scan_code(&text(&instance.creation_code), &mut found);
        }
    }
    for object in assets.objects.iter().flatten() {
        for events in &object.events {
            for (_, list) in events {
                scan_actions(list, &mut found);
            }
        }
    }
    for timeline in assets.timelines.iter().flatten() {
        for (_, list) in &timeline.moments {
            scan_actions(list, &mut found);
        }
    }
    for trigger in assets.triggers.iter().flatten() {
        scan_code(&text(&trigger.condition), &mut found);
    }
    // Names that are not built-in functions: the game's own scripts and the functions of its extensions.
    let mut own: BTreeSet<String> = assets.scripts.iter().flatten().map(|script| text(&script.name)).collect();
    let scripts = own.len();
    for extension in assets.extensions.iter() {
        for file in &extension.files {
            for function in &file.functions {
                own.insert(text(&function.name));
            }
        }
    }
    let mut report = Report { scripts, ..Report::default() };
    for (name, count) in found {
        if own.contains(&name) || KEYWORDS.contains(&name.as_str()) {
            continue;
        }
        report.builtin_calls += 1;
        if !crate::gml::mappings::FUNCTIONS.contains_key(name.as_str()) {
            report.missing.push((name, count));
        } else if let Some((_, impact)) = SIMPLIFIED.iter().find(|(simplified, _)| *simplified == name) {
            report.simplified.push((name, count, (*impact).to_owned()));
        }
    }
    report
}

/// The text written to `cache/game-compat.txt`.
pub fn report_text(exe_name: &str, exe_size: u64, report: &Report) -> String {
    let mut text = format!(
        "# PokeNX: funciones de GameMaker que usa el juego frente al motor (se regenera)\nexe={exe_name}\nsize={exe_size}\nscripts={}\nbuiltin={}\n",
        report.scripts, report.builtin_calls
    );
    for (name, count) in &report.missing {
        text.push_str(&format!("missing|{name}|{count}\n"));
    }
    for (name, count, impact) in &report.simplified {
        text.push_str(&format!("simplified|{name}|{count}|{impact}\n"));
    }
    text
}

/// Parses [`report_text`]: (executable name, executable size, report).
pub fn parse_report(text: &str) -> (String, u64, Report) {
    let (mut exe, mut size, mut report) = (String::new(), 0, Report::default());
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("exe=") {
            exe = value.to_owned();
        } else if let Some(value) = line.strip_prefix("size=") {
            size = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("scripts=") {
            report.scripts = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("builtin=") {
            report.builtin_calls = value.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("missing|") {
            let mut parts = rest.splitn(2, '|');
            if let (Some(name), Some(count)) = (parts.next(), parts.next()) {
                report.missing.push((name.to_owned(), count.trim().parse().unwrap_or(0)));
            }
        } else if let Some(rest) = line.strip_prefix("simplified|") {
            let mut parts = rest.splitn(3, '|');
            if let (Some(name), Some(count), Some(impact)) = (parts.next(), parts.next(), parts.next()) {
                report.simplified.push((name.to_owned(), count.trim().parse().unwrap_or(0), impact.to_owned()));
            }
        }
    }
    (exe, size, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calls(code: &str) -> Vec<(String, usize)> {
        let mut found = BTreeMap::new();
        scan_code(code, &mut found);
        found.into_iter().collect()
    }

    #[test]
    fn finds_calls_but_not_strings_comments_or_member_access() {
        let found = calls("x=sprite_add(\"a(b).png\",1,0,0,0,0); // draw_text(1)\nobj.show_message(2); show_message ( 'x(' ); y=a1(2)+b_2(3,4);");
        assert_eq!(found, vec![("a1".into(), 1), ("b_2".into(), 1), ("show_message".into(), 1), ("sprite_add".into(), 1)]);
    }

    #[test]
    fn the_report_round_trips() {
        let report = Report {
            builtin_calls: 3,
            scripts: 9,
            missing: vec![("made_up_function".into(), 2)],
            simplified: vec![("chr".into(), 26, "solo importa con texto fuera de ASCII".into())],
        };
        let text = report_text("juego.exe", 77, &report);
        assert_eq!(parse_report(&text), ("juego.exe".to_owned(), 77, report));
    }

    #[test]
    fn every_simplified_function_is_one_the_engine_maps() {
        for (name, _) in SIMPLIFIED {
            assert!(crate::gml::mappings::FUNCTIONS.contains_key(*name), "{name} is not in the engine's function table");
        }
    }

    /// Optional check against a real game: POKENX_REAL_GAME=<folder> cargo test --release real_game_compat -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_game_compat() {
        let Ok(dir) = std::env::var("POKENX_REAL_GAME") else { return };
        let exe = crate::loader::find_game_exe(&dir).expect("game");
        let bytes = std::fs::read(&exe).unwrap();
        let assets = gm8exe::reader::from_exe(bytes, None::<fn(&str)>, false, false).expect("parse");
        let started = std::time::Instant::now();
        let report = scan_assets(&assets);
        println!(
            "{} scripts, {} built-in functions called, {} missing {:?}, {} simplified, in {:?}",
            report.scripts, report.builtin_calls, report.missing.len(), report.missing, report.simplified.len(), started.elapsed()
        );
    }
}
