//! Prints the name and source of every script of a GM8 game whose source contains `needle` (a code-reading aid; the
//! output goes to the terminal and is never stored by the project).
//! usage: cargo run -p gm8exe --example script_grep -- <game.exe> <needle> [max-scripts]
use std::{env, fs, process};

fn main() {
    let mut args = env::args_os().skip(1);
    let (Some(path), Some(needle)) = (args.next(), args.next()) else {
        eprintln!("usage: script_grep <game.exe> <needle> [max-scripts]");
        process::exit(2);
    };
    let needle = needle.to_string_lossy().into_owned();
    let max: usize = args.next().and_then(|v| v.to_string_lossy().parse().ok()).unwrap_or(5);
    let mut bytes = fs::read(&path).unwrap_or_else(|e| {
        eprintln!("read failed: {e}");
        process::exit(2);
    });
    let game = gm8exe::reader::from_exe(&mut bytes, None::<fn(&str)>, false, false).unwrap_or_else(|e| {
        eprintln!("parse failed: {e}");
        process::exit(1);
    });
    let mut shown = 0;
    for script in game.scripts.iter().flatten() {
        let source = String::from_utf8_lossy(&script.source.0);
        if source.contains(&needle) {
            println!("=== {}\n{}\n", String::from_utf8_lossy(&script.name.0), source);
            shown += 1;
            if shown >= max {
                break;
            }
        }
    }
    eprintln!("{shown} script(s) shown");
}
