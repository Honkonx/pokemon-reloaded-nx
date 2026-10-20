//! The key bindings a GameMaker 8 game sets for itself.
//!
//! A game of this kind keeps its keyboard layout in variables (`key_<context>_<action> = ord('Z');`, `... = vk_f5;`)
//! that its own scripts fill in. This module reads those literal assignments from the game's GML so the menu can show
//! which key each action uses by default and whether a Switch button can produce it. Only the keys are read: no script
//! text is kept, and the report is written to the SD (never into the repository).

use gm8exe::asset::{code_action::CodeAction, PascalString};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    /// The context the game groups it under (battle, boxes, global...), as the game names it.
    pub group: String,
    pub action: String,
    /// Key name in `controls.txt` vocabulary (`Z`, `UP`, `F5`, `PAGEDOWN`...), or `#<code>` for a raw virtual-key code.
    pub key: String,
}

/// `vk_enter` -> `ENTER`, `vk_f5` -> `F5`, `vk_control` -> `CTRL`...
fn vk_name(name: &str) -> String {
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "RETURN" => "ENTER".to_owned(),
        "ESCAPE" => "ESC".to_owned(),
        "CONTROL" => "CTRL".to_owned(),
        "PGUP" => "PAGEUP".to_owned(),
        "PGDN" => "PAGEDOWN".to_owned(),
        _ => upper,
    }
}

/// The key a literal value names, or `None` when it is an expression (`argument1`, a function call...).
fn literal_key(value: &str) -> Option<String> {
    let value = value.trim();
    let end = value.find(|c: char| c == ';' || c == '\n' || c == '}' || c == '\r').unwrap_or(value.len());
    let value = value[..end].trim();
    if let Some(rest) = value.strip_prefix("ord(") {
        let mut chars = rest.chars();
        let quote = chars.next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        let letter = chars.next()?;
        if chars.next()? != quote || chars.next()? != ')' || chars.next().is_some() {
            return None;
        }
        return Some(letter.to_ascii_uppercase().to_string());
    }
    if let Some(rest) = value.strip_prefix("vk_") {
        return rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_').then(|| vk_name(rest));
    }
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        let code: u32 = value.parse().ok()?;
        return Some(match code {
            48..=57 | 65..=90 => char::from(code as u8).to_string(),
            _ => format!("#{code}"),
        });
    }
    None
}

/// Records `key_xxx = <literal>` assignments found in `code`; the first literal seen for a name wins.
fn scan_code(code: &str, found: &mut Vec<KeyBinding>) {
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
        let Some(value) = rest.strip_prefix('=').filter(|after| !after.starts_with('=')) else { continue };
        let Some(key) = literal_key(value) else { continue };
        let name = &code[start + 4..end];
        let (group, action) = name.split_once('_').unwrap_or(("otros", name));
        if found.iter().any(|binding| binding.group == group && binding.action == action) {
            continue;
        }
        found.push(KeyBinding { group: group.to_owned(), action: action.to_owned(), key });
    }
}

fn scan_actions(list: &[CodeAction], found: &mut Vec<KeyBinding>) {
    for action in list {
        for text in action.param_strings.iter().chain(std::iter::once(&action.fn_code)) {
            scan_code(&String::from_utf8_lossy(&text.0), found);
        }
    }
}

/// Every literal key binding in the game's GML, in the order the game's assets list them.
pub fn scan_assets(assets: &gm8exe::GameAssets) -> Vec<KeyBinding> {
    let text = |s: &PascalString| String::from_utf8_lossy(&s.0).into_owned();
    let mut found = Vec::new();
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
    found
}

/// The text written to `cache/game-keys.txt`: two header lines naming the executable it came from, then one
/// `group|action|KEY` line per binding.
pub fn report_text(exe_name: &str, exe_size: u64, bindings: &[KeyBinding]) -> String {
    let mut text = format!("# PokeNX: teclas por defecto leidas del juego (se regenera)\nexe={exe_name}\nsize={exe_size}\n");
    for binding in bindings {
        text.push_str(&format!("{}|{}|{}\n", binding.group, binding.action, binding.key));
    }
    text
}

/// Parses a report written by [`report_text`]: (executable name, executable size, bindings).
pub fn parse_report(text: &str) -> (String, u64, Vec<KeyBinding>) {
    let (mut exe, mut size, mut bindings) = (String::new(), 0, Vec::new());
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("exe=") {
            exe = value.to_owned();
        } else if let Some(value) = line.strip_prefix("size=") {
            size = value.trim().parse().unwrap_or(0);
        } else if !line.starts_with('#') {
            let mut parts = line.splitn(3, '|');
            if let (Some(group), Some(action), Some(key)) = (parts.next(), parts.next(), parts.next()) {
                bindings.push(KeyBinding { group: group.to_owned(), action: action.to_owned(), key: key.to_owned() });
            }
        }
    }
    (exe, size, bindings)
}

/// Why a scan could not produce a report.
#[derive(Debug, PartialEq, Eq)]
pub enum ScanError {
    /// No GameMaker 8.0 executable in the folder (the loader's reason).
    NoGame(String),
    /// The executable could not be read or parsed.
    Unreadable(String),
}

/// Parses the game in `dir` (the `game/` folder) and returns its report text.
pub fn scan_game(dir: &str) -> Result<String, ScanError> {
    scan_game_reports(dir).map(|(keys, _)| keys)
}

/// Parses the game in `dir` once and returns both reports: the key bindings and the function compatibility.
pub fn scan_game_reports(dir: &str) -> Result<(String, String), ScanError> {
    crate::loader::find_game_exe(dir).map_err(ScanError::NoGame)?;
    crate::preload::with_assets(dir, |assets, name, size| {
        (report_text(name, size, &scan_assets(assets)), crate::compat_scan::report_text(name, size, &crate::compat_scan::scan_assets(assets)))
    })
    .map_err(ScanError::Unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(code: &str) -> Vec<(String, String, String)> {
        let mut found = Vec::new();
        scan_code(code, &mut found);
        found.into_iter().map(|b| (b.group, b.action, b.key)).collect()
    }

    #[test]
    fn reads_literal_assignments_and_ignores_expressions() {
        let found = scan(
            "key_bt_Ataque1 = ord('Z');\nkey_bt_mov_arriba = vk_up;\nkey_global_guardar=vk_f5;\n\
             key_ini_entrar = vk_enter ; key_ex_hab = 56;key_bt_show_pkmn = 220;\n\
             key_bt_cambio = argument1; key_bt_x = ex(8462,1); if (key_bt_y == ord('A')) {} key_random = ord(\"C\");",
        );
        assert_eq!(found, vec![
            ("bt".into(), "Ataque1".into(), "Z".into()),
            ("bt".into(), "mov_arriba".into(), "UP".into()),
            ("global".into(), "guardar".into(), "F5".into()),
            ("ini".into(), "entrar".into(), "ENTER".into()),
            ("ex".into(), "hab".into(), "8".into()),
            ("bt".into(), "show_pkmn".into(), "#220".into()),
            ("otros".into(), "random".into(), "C".into()),
        ]);
    }

    #[test]
    fn the_first_literal_for_an_action_wins_and_prefixes_are_not_matched() {
        let found = scan("key_bt_ataque = ord('Z'); key_bt_ataque = ord('X'); monkey_bt_otro = ord('Q');");
        assert_eq!(found, vec![("bt".into(), "ataque".into(), "Z".into())]);
    }

    /// Optional check against a real game: POKENX_REAL_GAME=<folder> cargo test --release real_game_keys -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_game_keys() {
        let Ok(dir) = std::env::var("POKENX_REAL_GAME") else { return };
        let started = std::time::Instant::now();
        let text = scan_game(&dir).expect("scan");
        let (exe, size, bindings) = parse_report(&text);
        let groups: std::collections::BTreeSet<&str> = bindings.iter().map(|b| b.group.as_str()).collect();
        println!("{exe} ({size} bytes): {} bindings in {} groups, {:?}", bindings.len(), groups.len(), started.elapsed());
        if let Ok(out) = std::env::var("POKENX_REAL_KEYS_OUT") {
            std::fs::write(out, &text).unwrap();
        }
        assert!(bindings.len() > 100);
    }

    #[test]
    fn report_round_trips() {
        let bindings = vec![
            KeyBinding { group: "bt".into(), action: "mov_arriba".into(), key: "UP".into() },
            KeyBinding { group: "global".into(), action: "guardar".into(), key: "F5".into() },
        ];
        let text = report_text("juego.exe", 123, &bindings);
        assert_eq!(parse_report(&text), ("juego.exe".to_owned(), 123, bindings));
    }
}
