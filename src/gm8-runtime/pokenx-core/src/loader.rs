//! Finds the user's game folder content and launches it. This is the single entry point the
//! host uses: it reads the `.exe` as a GameMaker data container (the x86 code is never run),
//! parses it with `gm8exe` and hands the assets to `Game::launch`.

use crate::game::{Game, PlayType};
use std::path::{Path, PathBuf};

/// Scratch folder for games that use `temp_dir` / included files (created on demand).
pub const TEMP_DIR: &str = "sdmc:/switch/pokenx/cache/tmp";

/// Find a single recognized PE/i386 + standard GM8.0 container in the root or
/// its immediate subfolders if the root has none. File size and directory ordering cannot identify
/// the game: ambiguous installs are reported instead of silently picking one.
pub fn find_game_exe(dir: &str) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    let mut folders = Vec::new();
    let mut remaining = 4096usize;
    scan_folder(Path::new(dir), &mut candidates, Some(&mut folders), &mut remaining)?;
    if candidates.is_empty() {
        folders.sort();
        for folder in folders {
            scan_folder(&folder, &mut candidates, None, &mut remaining)?;
        }
    }
    candidates.sort();
    match candidates.len() {
        0 => Err(format!("no recognized standard GameMaker 8.0 executable in {dir} or its immediate subfolders")),
        1 => Ok(candidates.remove(0)),
        _ => Err(format!("multiple GameMaker executables found; keep the intended game in game/: {}",
            candidates.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>().join(", "))),
    }
}

fn scan_folder(dir: &Path, candidates: &mut Vec<PathBuf>, mut folders: Option<&mut Vec<PathBuf>>, remaining: &mut usize) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("could not read {}: {e}", dir.display()))?;
    for entry in entries {
        if *remaining == 0 { return Err("game executable search exceeded 4096 directory entries".into()) }
        *remaining -= 1;
        let path = entry.map_err(|e| format!("could not list {}: {e}", dir.display()))?.path();
        if path.is_dir() {
            if !path.file_name().and_then(|name| name.to_str()).map(|name| name.eq_ignore_ascii_case("data")).unwrap_or(false) {
                if let Some(folders) = folders.as_mut() { folders.push(path); }
            }
        } else if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false)
            && is_game_container(&path) {
            candidates.push(path);
        }
    }
    Ok(())
}

fn is_game_container(path: &Path) -> bool {
    use gm8_format_probe::{ReadAt, ProbeError};
    use std::io::{Read, Seek, SeekFrom};
    struct ProbeFile { file: std::fs::File, len: u64 }
    impl ReadAt for ProbeFile {
        fn len(&self) -> u64 { self.len }
        fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ProbeError> {
            if offset.checked_add(out.len() as u64).filter(|end| *end <= self.len).is_none() {
                return Err(ProbeError::OutOfBounds)
            }
            self.file.seek(SeekFrom::Start(offset)).map_err(|_| ProbeError::InvalidRead)?;
            self.file.read_exact(out).map_err(|_| ProbeError::InvalidRead)
        }
    }
    let Ok(file) = std::fs::File::open(path) else { return false };
    let Ok(metadata) = std::fs::metadata(path) else { return false };
    matches!(gm8_format_probe::detect_standard_gm80_executable(&mut ProbeFile { file, len: metadata.len() }), Ok(Some(_)))
}

/// The controls file a profile token selects: `default` is the factory layout (no file), `2` is `controls-2.txt`, anything else
/// (including no token) is profile 1, the original `controls.txt`.
pub fn controls_profile_file(token: &str) -> Option<&'static str> {
    match token {
        "default" => None,
        "2" => Some("controls-2.txt"),
        _ => Some("controls.txt"),
    }
}

/// Parses the game found in `dir` and prepares it. Call `Game::start` afterwards.
pub fn launch(dir: &str, log: &dyn Fn(&str)) -> Result<Game, String> {
    let exe = find_game_exe(dir)?;
    log(&format!("game executable: {}", exe.display()));
    // The game's current keys (what the player changed inside the game) are written to cache/game-keys-live.txt for the menu's
    // "Teclas" screen, only where the app's cache folder exists.
    let live_keys = std::path::Path::new(dir).parent().map(|root| root.join("cache")).filter(|cache| cache.is_dir());
    crate::live_keys::set_output(live_keys.as_ref().map(|cache| cache.join("game-keys-live.txt")));
    // Which room usually follows which, so pauses of the game can be used to compile the likely next rooms ahead of time.
    crate::room_hints::set_output(live_keys.map(|cache| cache.join("room-hints.txt")));
    // The controls profile (profile.txt: default, 1 or 2) picks the file next to game/ that the menu's "Mandos" screen writes.
    if let Some(root) = std::path::Path::new(dir).parent() {
        let profile = std::fs::read_to_string(root.join("profile.txt")).unwrap_or_default();
        match controls_profile_file(profile.trim()) {
            Some(name) => {
                if let Ok(text) = std::fs::read_to_string(root.join(name)) {
                    crate::platform::host::set_button_map(gm8_input::ButtonMap::parse(&text));
                    log(&format!("{name} applied"));
                }
            },
            None => {
                crate::platform::host::set_button_map(gm8_input::ButtonMap::DEFAULT);
                log("controls: default profile");
            },
        }
    }
    // stickzone.txt (written by the menu's "Zona muerta del palo"): small, normal or large.
    if let Some(token) = std::path::Path::new(dir).parent().map(|root| root.join("stickzone.txt")).and_then(|path| std::fs::read_to_string(path).ok()) {
        let percent = match token.trim() {
            "small" => 35,
            "large" => 70,
            _ => 0,
        };
        crate::platform::host::set_stick_threshold(percent);
        log(&format!("stick dead zone: {}", token.trim()));
    }
    match exe.parent().map(|folder| folder.join("data")) {
        Some(data) if data.is_dir() => log(&format!("game data folder: {}", data.display())),
        _ => log("no data/ folder next to the executable; the game will only see what is inside the .exe"),
    }
    let size = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
    let assets = match crate::preload::take(&exe, size) {
        Some(assets) => {
            log(&format!("using the {size} bytes of game data the menu parsed in advance"));
            assets
        },
        None => {
            let bytes = std::fs::read(&exe).map_err(|e| format!("could not read {}: {}", exe.display(), e))?;
            log(&format!("read {} bytes; parsing GameMaker data", bytes.len()));
            gm8exe::reader::from_exe(bytes, Some(|message: &str| log(message)), false, false)
                .map_err(|e| format!("could not parse the GameMaker data: {e}"))?
        },
    };
    log("game data parsed");

    std::fs::create_dir_all(TEMP_DIR).map_err(|e| format!("could not create {TEMP_DIR}: {e}"))?;
    Game::launch(
        assets,
        exe,
        Vec::new(),
        Some(PathBuf::from(TEMP_DIR)),
        encoding_rs::WINDOWS_1252,
        true,
        0,
        false,
        PlayType::Normal,
    )
    .map_err(|e| format!("could not launch the game: {e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn profile_tokens_pick_the_controls_file() {
        use super::controls_profile_file;
        assert_eq!(controls_profile_file("default"), None);
        assert_eq!(controls_profile_file("1"), Some("controls.txt"));
        assert_eq!(controls_profile_file("2"), Some("controls-2.txt"));
        assert_eq!(controls_profile_file(""), Some("controls.txt"), "no token: the original controls.txt keeps working");
        assert_eq!(controls_profile_file("basura"), Some("controls.txt"));
    }

    use super::*;

    // A synthetic PE with the standard GM8.0 loader signature and an empty
    // prelude. It identifies the container; it deliberately holds no game data.
    fn container_fixture() -> Vec<u8> {
        let mut bytes = vec![0u8; 0x145100];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x86].copy_from_slice(b"PE\0\0\x4c\x01");
        bytes[0xa49be..0xa49c6].copy_from_slice(&[0x8b, 0x45, 0xf4, 0xe8, 0x2a, 0xbd, 0xfd, 0xff]);
        bytes[0xa49c6] = 0x90; // supported patched-out magic check
        bytes[0x144ac0..0x144ac4].copy_from_slice(&0x145000u32.to_le_bytes());
        bytes
    }

    #[test]
    fn finds_the_gm8_container_instead_of_a_larger_non_game_exe() {
        let root = std::env::temp_dir().join(format!("pokenx-loader-test-{}", std::process::id()));
        let inner = root.join("Game");
        std::fs::create_dir_all(&inner).unwrap();
        let mut installer = vec![0u8; 2 * 1024 * 1024];
        installer[..2].copy_from_slice(b"MZ");
        std::fs::write(root.join("installer.exe"), installer).unwrap();
        let bytes = container_fixture();
        std::fs::write(inner.join("Game.EXE"), &bytes).unwrap();
        let found = find_game_exe(root.to_str().unwrap()).unwrap();
        assert_eq!(found.file_name().unwrap(), "Game.EXE");
        std::fs::write(inner.join("another-game.exe"), bytes).unwrap();
        assert!(find_game_exe(root.to_str().unwrap()).unwrap_err().contains("multiple"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_folder_has_no_game() {
        assert!(find_game_exe("definitely/not/a/real/folder").is_err());
        assert!(launch("definitely/not/a/real/folder", &|_| {}).is_err());
    }
}
