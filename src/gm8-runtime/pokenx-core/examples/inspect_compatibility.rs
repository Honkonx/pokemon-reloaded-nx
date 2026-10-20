//! Read-only host inventory. Never launches Game/GL/audio, writes extracted
//! assets, or reports source snippets, user symbols, resource names or paths.
use gm8exe::{
    GameAssets,
    asset::{CodeAction, extension::FileKind},
};
use gml_parser::{
    lexer::Lexer,
    token::{Separator, Token},
};
use pokenx_core::gml::mappings::FUNCTIONS;
use std::{collections::BTreeMap, fmt::Write};

#[derive(Default)]
struct Inventory {
    fragments: u64,
    invalid_tokens: u64,
    non_builtin_calls: u64,
    builtin_calls: BTreeMap<&'static str, u64>,
    dll_files: u64,
    dll_functions: u64,
    gml_extension_files: u64,
    other_extension_files: u64,
    dnd_actions: u64,
}

impl Inventory {
    fn call(&mut self, name: &[u8]) {
        let found = std::str::from_utf8(name).ok().and_then(|name| FUNCTIONS.get_key(name).copied());
        if let Some(name) = found {
            *self.builtin_calls.entry(name).or_default() += 1;
        } else {
            self.non_builtin_calls += 1;
        }
    }

    fn code(&mut self, source: &[u8]) {
        if source.is_empty() {
            return;
        }
        self.fragments += 1;
        let mut tokens = Lexer::new(source).peekable();
        while let Some(token) = tokens.next() {
            match token {
                Token::Identifier(name) if matches!(tokens.peek(), Some(Token::Separator(Separator::ParenLeft))) => {
                    self.call(name)
                },
                Token::InvalidChar(..) => self.invalid_tokens += 1,
                _ => {},
            }
        }
    }

    fn actions(&mut self, actions: &[CodeAction]) {
        for action in actions {
            self.dnd_actions += 1;
            match action.action_kind {
                0 => {
                    match action.execution_type {
                        1 => self.call(&action.fn_name.0),
                        2 => self.code(&action.fn_code.0),
                        _ => continue,
                    }
                    for (param, kind) in action.param_strings.iter().zip(&action.param_types).take(action.param_count) {
                        // Same distinction as action::Tree::compile_params.
                        if *kind != 1 && *kind != 2 {
                            self.code(&param.0);
                        }
                    }
                },
                5 => self.code(&action.param_strings[0].0),
                6 => {
                    self.code(&action.param_strings[0].0);
                    self.code(&action.param_strings[1].0);
                },
                7 => self.code(&action.param_strings[0].0),
                _ => {},
            }
        }
    }

    fn assets(assets: &GameAssets) -> Self {
        let mut inventory = Self::default();
        for script in assets.scripts.iter().flatten() {
            inventory.code(&script.source.0);
        }
        for constant in &assets.constants {
            inventory.code(&constant.expression.0);
        }
        for code in &assets.library_init_strings {
            inventory.code(&code.0);
        }
        for trigger in assets.triggers.iter().flatten() {
            inventory.code(&trigger.condition.0);
        }
        for room in assets.rooms.iter().flatten() {
            inventory.code(&room.creation_code.0);
            for instance in &room.instances {
                inventory.code(&instance.creation_code.0);
            }
        }
        for object in assets.objects.iter().flatten() {
            for event in object.events.iter().flatten() {
                inventory.actions(&event.1);
            }
        }
        for timeline in assets.timelines.iter().flatten() {
            for moment in &timeline.moments {
                inventory.actions(&moment.1);
            }
        }
        for extension in &assets.extensions {
            for file in &extension.files {
                match file.kind {
                    FileKind::DynamicLibrary => {
                        inventory.dll_files += 1;
                        inventory.dll_functions += file.functions.len() as u64;
                    },
                    FileKind::GmlScript => {
                        inventory.gml_extension_files += 1;
                        inventory.code(&file.contents);
                    },
                    _ => inventory.other_extension_files += 1,
                }
                inventory.code(&file.initializer.0);
                inventory.code(&file.finalizer.0);
                for constant in &file.consts {
                    inventory.code(&constant.value.0);
                }
            }
        }
        inventory
    }

    fn report(&self) -> String {
        let mut text = String::from(concat!(
            "PokeNX lexical API inventory v1\n",
            "Not execution, compilation, reachability or compatibility proof.\n",
            "Only public builtin names and aggregate counts.\n",
        ));
        for (key, value) in [
            ("fragments", self.fragments),
            ("invalid_tokens", self.invalid_tokens),
            ("non_builtin_calls", self.non_builtin_calls),
            ("dnd_actions", self.dnd_actions),
            ("dll_files", self.dll_files),
            ("dll_functions", self.dll_functions),
            ("gml_extension_files", self.gml_extension_files),
            ("other_extension_files", self.other_extension_files),
        ] {
            writeln!(text, "{key}={value}").unwrap();
        }
        text.push_str("builtin\tpossible_calls\tsource_review_marker\n");
        for (name, count) in &self.builtin_calls {
            // This is a review hint from the actual source, not a support score.
            let marker = format!("[gml] no implementada: {name}\"");
            let marked = include_str!("../src/gml/kernel.rs").contains(&marker);
            writeln!(text, "{name}\t{count}\t{}", if marked { "review" } else { "none" }).unwrap();
        }
        text
    }
}

fn run() -> Result<(), &'static str> {
    let path = std::env::args_os().nth(1).ok_or("provide a local GM8 data container path")?;
    let bytes = std::fs::read(path).map_err(|_| "cannot read the container")?;
    let assets = gm8exe::reader::from_exe(bytes, None::<fn(&str)>, true, false)
        .map_err(|_| "GM8 parser rejected the container; no private error details printed")?;
    print!("{}", Inventory::assets(&assets).report());
    if std::env::args().skip(2).any(|arg| arg == "--compile") {
        // Rust debug compiler frames can exceed the Windows main stack with
        // large title scripts. This is an analysis worker, not a Switch stack
        // guarantee or a reason to run the game on the host.
        let stack_kib = std::env::var("POKENX_COMPILE_STACK_KIB")
            .ok()
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| "invalid static compiler stack size")?
            .unwrap_or(16 * 1024);
        if !(64..=64 * 1024).contains(&stack_kib) {
            return Err("static compiler stack size out of range");
        }
        // A parser/compiler panic may contain private identifiers. Never print
        // its payload; the caller only receives a fixed diagnostic.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| eprintln!("static compiler panic; private details suppressed")));
        let worker = std::thread::Builder::new()
            .stack_size(stack_kib * 1024)
            .spawn(move || pokenx_core::compatibility::compile_assets(&assets))
            .map_err(|_| "cannot start static compiler worker");
        let summary = worker.and_then(|worker| worker.join().map_err(|_| "static compiler failed"));
        std::panic::set_hook(previous_hook);
        let summary = summary?;
        println!("static_compiler_stack_kib={stack_kib}");
        println!("static_compilation={summary:?}");
        println!("Compilation does not evaluate constants, run GML or inspect dynamic files.");
    }
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[path = "../../../gm8exe/tests/support/fixture.rs"]
    mod fixture;

    #[test]
    fn lexer_ignores_strings_comments_and_does_not_report_private_symbols() {
        let mut inventory = Inventory::default();
        inventory.code(
            concat!(
                "// get_integer(1)\n/* sound_play(0) */ private_script(); ",
                "draw_text(1,2,'get_integer(0)'); get_integer('private text',0);",
            )
            .as_bytes(),
        );
        assert_eq!(inventory.builtin_calls.len(), 2);
        assert_eq!(inventory.builtin_calls["draw_text"], 1);
        assert_eq!(inventory.builtin_calls["get_integer"], 1);
        assert_eq!(inventory.non_builtin_calls, 1);
        let report = inventory.report();
        assert!(report.contains("get_integer\t1\treview"));
        assert!(!report.contains("private_script"));
        assert!(!report.contains("private text"));
    }

    #[test]
    fn complete_synthetic_container_scans_without_launch_or_extraction() {
        let assets = gm8exe::reader::from_exe(fixture::executable(), None::<fn(&str)>, true, false).unwrap();
        let inventory = Inventory::assets(&assets);
        assert!(inventory.builtin_calls.contains_key("game_save"));
        assert!(inventory.builtin_calls.contains_key("keyboard_check"));
        assert!(inventory.builtin_calls.contains_key("draw_sprite"));
        assert_eq!(inventory.dll_files, 0);
        assert_eq!(inventory.invalid_tokens, 0);
        assert!(!inventory.report().contains("fixture_clamp"));
        assert!(!inventory.report().contains("obj_fixture"));
    }
}
