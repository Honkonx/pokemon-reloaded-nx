//! Parsing the game while the player is still in the menu.
//!
//! Reading the `.exe` and parsing its ≈ 20 000 resources takes about 6 s on the console. The menu starts it on a worker
//! thread as soon as it opens; `loader::launch` then takes the result instead of doing the work again. The result is
//! used only if it is for the same executable (path and length), so installing another version in between just falls back
//! to the normal path.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

struct Preloaded {
    exe: PathBuf,
    size: u64,
    assets: gm8exe::GameAssets,
}

static SLOT: Mutex<Option<Preloaded>> = Mutex::new(None);

/// Reads and parses the game in `dir` (the `game/` folder) and keeps the result for [`take`]. Blocks until done.
pub fn preload(dir: &str) -> Result<(), String> {
    let exe = crate::loader::find_game_exe(dir)?;
    let bytes = std::fs::read(&exe).map_err(|e| format!("could not read {}: {e}", exe.display()))?;
    let size = bytes.len() as u64;
    let assets = gm8exe::reader::from_exe(bytes, None::<fn(&str)>, false, false)
        .map_err(|e| format!("could not parse the GameMaker data: {e}"))?;
    if let Ok(mut slot) = SLOT.lock() {
        *slot = Some(Preloaded { exe, size, assets });
    }
    Ok(())
}

/// The preloaded data when it belongs to `exe` with this length; it is removed from the slot either way.
pub fn take(exe: &Path, size: u64) -> Option<gm8exe::GameAssets> {
    let preloaded = SLOT.lock().ok()?.take()?;
    (preloaded.exe.as_path() == exe && preloaded.size == size).then_some(preloaded.assets)
}

/// Runs `f` on the parsed game in `dir` (the `game/` folder) with its executable's name and length. The preloaded data is
/// used when it is for the same executable and stays in its slot for the launch; otherwise the game is parsed afresh and
/// dropped afterwards.
pub fn with_assets<T>(dir: &str, f: impl FnOnce(&gm8exe::GameAssets, &str, u64) -> T) -> Result<T, String> {
    let exe = crate::loader::find_game_exe(dir)?;
    let size = std::fs::metadata(&exe).map(|metadata| metadata.len()).map_err(|e| format!("could not read {}: {e}", exe.display()))?;
    let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if let Ok(slot) = SLOT.lock() {
        if let Some(preloaded) = slot.as_ref().filter(|p| p.exe.as_path() == exe.as_path() && p.size == size) {
            return Ok(f(&preloaded.assets, &name, size));
        }
    }
    let bytes = std::fs::read(&exe).map_err(|e| format!("could not read {}: {e}", exe.display()))?;
    let assets = gm8exe::reader::from_exe(bytes, None::<fn(&str)>, false, false)
        .map_err(|e| format!("could not parse the GameMaker data: {e}"))?;
    Ok(f(&assets, &name, size))
}

/// Frees whatever was preloaded (the game changed, or the player is leaving).
pub fn clear() {
    if let Ok(mut slot) = SLOT.lock() {
        *slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_without_a_preload_gives_nothing() {
        clear();
        assert!(take(Path::new("x.exe"), 1).is_none());
        assert!(preload("this folder does not exist").is_err());
    }
}
