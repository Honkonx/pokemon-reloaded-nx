//! What a player owns inside the game folder (saves, settings, screenshots) and how it survives a game update.
//!
//! The game writes these files itself; the list comes from the file calls in its GML (see
//! `docs/auditorias/ACTUALIZADOR_Y_TECLAS_2026-10-19.md`). An update installs the new game in a staging folder and calls
//! [`copy_user_data`] before swapping it in, so nothing the player made is lost. [`create_backup`] and
//! [`restore_backup`] keep dated copies of the same files under `<root>/backups/`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How a path is merged into a freshly installed game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// The player's copy replaces whatever the new game has (saves, screenshots, logs).
    Overwrite,
    /// The player's copy wins, and the new game's own file is kept next to it as `<name>.nuevo` when it differs.
    KeepOld,
    /// Only files the new game does not have are copied (data the game may also ship).
    FillMissing,
}

/// Paths relative to `game/`, with how each is merged.
pub const USER_PATHS: [(&str, Policy); 8] = [
    ("data/partidas", Policy::Overwrite),
    ("data/capturas", Policy::Overwrite),
    ("LogBattle.txt", Policy::Overwrite),
    ("data/text/config.ini", Policy::KeepOld),
    ("data/audio/config.ini", Policy::KeepOld),
    ("data/equipos", Policy::FillMissing),
    ("data/caja", Policy::FillMissing),
    ("data/char_custom", Policy::FillMissing),
];

/// The host (the menu runs before the engine registers its clock) can tell the current time here.
static UNIX_HINT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub fn set_unix_hint(seconds: i64) {
    UNIX_HINT.store(seconds, std::sync::atomic::Ordering::Relaxed);
}

/// Seconds since the Unix epoch from the engine clock, else the host hint, else 0 (unknown).
pub fn now_unix() -> i64 {
    match crate::platform::host::now_unix_nanos() {
        Some(nanos) => (nanos / 1_000_000_000) as i64,
        None => UNIX_HINT.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// `create_dir_all`/`write` for hosts that cannot name `std::fs` themselves (the engine crate on Horizon).
pub fn ensure_dir(path: &str) -> io::Result<()> {
    fs::create_dir_all(path)
}

pub fn write_file(path: &str, bytes: &[u8]) -> io::Result<()> {
    fs::write(path, bytes)
}

/// Backups kept by [`create_backup`] before the oldest are removed.
pub const BACKUPS_KEPT: usize = 5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyStats {
    pub files: u64,
    pub bytes: u64,
}

impl CopyStats {
    fn add(&mut self, other: CopyStats) {
        self.files += other.files;
        self.bytes += other.bytes;
    }
}

fn copy_file(from: &Path, to: &Path) -> io::Result<CopyStats> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = fs::copy(from, to)?;
    Ok(CopyStats { files: 1, bytes })
}

fn copy_tree(from: &Path, to: &Path, policy: Policy) -> io::Result<CopyStats> {
    let mut stats = CopyStats::default();
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            stats.add(copy_tree(&entry.path(), &to.join(entry.file_name()), policy)?);
        }
        return Ok(stats);
    }
    match policy {
        Policy::Overwrite => stats.add(copy_file(from, to)?),
        Policy::FillMissing => {
            if !to.exists() {
                stats.add(copy_file(from, to)?);
            }
        },
        Policy::KeepOld => {
            if to.exists() {
                if fs::read(from)? != fs::read(to)? {
                    let mut kept = to.as_os_str().to_owned();
                    kept.push(".nuevo");
                    fs::rename(to, PathBuf::from(kept))?;
                    stats.add(copy_file(from, to)?);
                }
            } else {
                stats.add(copy_file(from, to)?);
            }
        },
    }
    Ok(stats)
}

/// Copies the player's files from the game folder `from` into the game folder `to`, merging as each path's policy says.
/// Missing sources are skipped. The source is never modified.
pub fn copy_user_data(from: &Path, to: &Path) -> io::Result<CopyStats> {
    let mut stats = CopyStats::default();
    for (relative, policy) in USER_PATHS {
        let source = from.join(relative);
        if source.exists() {
            stats.add(copy_tree(&source, &to.join(relative), policy)?);
        }
    }
    Ok(stats)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupInfo {
    pub name: String,
    pub files: u64,
    pub bytes: u64,
    /// Seconds since the Unix epoch, 0 when the console clock was unknown.
    pub unix: i64,
}

fn backups_dir(root: &Path) -> PathBuf {
    root.join("backups")
}

fn manifest_text(info: &BackupInfo) -> String {
    format!("files={}\nbytes={}\nunix={}\n", info.files, info.bytes, info.unix)
}

fn read_manifest(dir: &Path, name: &str) -> BackupInfo {
    let mut info = BackupInfo { name: name.to_owned(), files: 0, bytes: 0, unix: 0 };
    if let Ok(text) = fs::read_to_string(dir.join("manifest.txt")) {
        for line in text.lines() {
            match line.split_once('=') {
                Some(("files", value)) => info.files = value.trim().parse().unwrap_or(0),
                Some(("bytes", value)) => info.bytes = value.trim().parse().unwrap_or(0),
                Some(("unix", value)) => info.unix = value.trim().parse().unwrap_or(0),
                _ => {},
            }
        }
    }
    info
}

fn backup_number(name: &str) -> Option<u32> {
    name.strip_prefix("copia-").and_then(|digits| digits.parse().ok())
}

/// The backups under `<root>/backups`, newest first.
pub fn list_backups(root: &Path) -> Vec<BackupInfo> {
    let mut found: Vec<(u32, BackupInfo)> = Vec::new();
    if let Ok(entries) = fs::read_dir(backups_dir(root)) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let (Some(number), true) = (backup_number(&name), entry.path().is_dir()) {
                found.push((number, read_manifest(&entry.path(), &name)));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, info)| info).collect()
}

/// Copies the player's files from `<root>/game` into a new `<root>/backups/copia-NNN` and removes the oldest copies
/// beyond [`BACKUPS_KEPT`]. `now_unix` is recorded in the manifest (0 when unknown).
pub fn create_backup(root: &Path, now_unix: i64) -> io::Result<BackupInfo> {
    create_backup_keeping(root, now_unix, BACKUPS_KEPT)
}

fn create_backup_keeping(root: &Path, now_unix: i64, keep: usize) -> io::Result<BackupInfo> {
    let game = root.join("game");
    let directory = backups_dir(root);
    fs::create_dir_all(&directory)?;
    let next = list_backups(root).iter().filter_map(|info| backup_number(&info.name)).max().map_or(1, |n| n + 1);
    let name = format!("copia-{next:03}");
    let target = directory.join(&name);
    let stats = match copy_user_data(&game, &target) {
        Ok(stats) => stats,
        Err(error) => {
            let _ = fs::remove_dir_all(&target);
            return Err(error);
        },
    };
    fs::create_dir_all(&target)?;
    let info = BackupInfo { name, files: stats.files, bytes: stats.bytes, unix: now_unix };
    fs::write(target.join("manifest.txt"), manifest_text(&info))?;
    for old in list_backups(root).into_iter().skip(keep) {
        let _ = fs::remove_dir_all(directory.join(old.name));
    }
    Ok(info)
}

/// Restores backup `name` into `<root>/game`. A new backup of the current files is made first, so a restore can be
/// undone. Names with path separators or `..` are refused.
pub fn restore_backup(root: &Path, name: &str, now_unix: i64) -> io::Result<CopyStats> {
    let valid = backup_number(name).is_some() && !name.contains(['/', '\\']) && !name.contains("..");
    if !valid {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid backup name"));
    }
    let source = backups_dir(root).join(name);
    if !source.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "backup not found"));
    }
    // Keep one more than usual so this safety copy can never rotate out the backup being restored.
    let _ = create_backup_keeping(root, now_unix, BACKUPS_KEPT + 1);
    // The backup folder has the same layout as `game/`, so it restores with the same merge rules (the player's
    // files from the backup win).
    let mut stats = CopyStats::default();
    for (relative, _) in USER_PATHS {
        let from = source.join(relative);
        if from.exists() {
            stats.add(copy_tree(&from, &root.join("game").join(relative), Policy::Overwrite)?);
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pokenx-user-data-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn an_update_keeps_saves_and_settings_and_leaves_the_rest_to_the_new_game() {
        let root = scratch("update");
        let (old, new) = (root.join("game"), root.join("staging"));
        write(&old.join("data/partidas/reloaded1.data"), "mi partida");
        write(&old.join("data/partidas/reloaded1.nfo"), "info");
        write(&old.join("data/text/config.ini"), "teclas del jugador");
        write(&old.join("data/audio/config.ini"), "volumen");
        write(&old.join("data/caja/propia.bin"), "caja");
        write(&old.join("data/pkmn/viejo.png"), "no es del jugador");
        write(&new.join("data/text/config.ini"), "valores nuevos");
        write(&new.join("data/audio/config.ini"), "volumen");
        write(&new.join("data/caja/propia.bin"), "caja de fabrica");
        write(&new.join("data/pkmn/nuevo.png"), "grafico nuevo");

        let stats = copy_user_data(&old, &new).unwrap();
        assert_eq!(fs::read_to_string(new.join("data/partidas/reloaded1.data")).unwrap(), "mi partida");
        assert_eq!(fs::read_to_string(new.join("data/text/config.ini")).unwrap(), "teclas del jugador");
        assert_eq!(fs::read_to_string(new.join("data/text/config.ini.nuevo")).unwrap(), "valores nuevos");
        assert!(!new.join("data/audio/config.ini.nuevo").exists(), "equal files leave no .nuevo");
        assert_eq!(fs::read_to_string(new.join("data/caja/propia.bin")).unwrap(), "caja de fabrica", "FillMissing never overwrites");
        assert!(!new.join("data/pkmn/viejo.png").exists() && new.join("data/pkmn/nuevo.png").exists());
        assert!(old.join("data/partidas/reloaded1.data").exists(), "the source stays untouched");
        assert!(stats.files >= 3);
    }

    #[test]
    fn backups_are_numbered_rotated_listed_and_restorable() {
        let root = scratch("backup");
        write(&root.join("game/data/partidas/reloaded1.data"), "uno");
        for round in 1..=7 {
            let info = create_backup(&root, 1_000 + round).unwrap();
            assert_eq!(info.name, format!("copia-{round:03}"));
            assert_eq!(info.files, 1);
        }
        let listed = list_backups(&root);
        assert_eq!(listed.len(), BACKUPS_KEPT);
        assert_eq!(listed[0].name, "copia-007");
        assert_eq!(listed[0].unix, 1_007);
        assert_eq!(listed[BACKUPS_KEPT - 1].name, "copia-003");

        write(&root.join("game/data/partidas/reloaded1.data"), "estropeada");
        let restored = restore_backup(&root, "copia-007", 2_000).unwrap();
        assert_eq!(restored.files, 1);
        assert_eq!(fs::read_to_string(root.join("game/data/partidas/reloaded1.data")).unwrap(), "uno");
        assert!(list_backups(&root).iter().any(|info| info.name == "copia-008"), "the restore backed up the broken state first");
    }

    #[test]
    fn restore_refuses_names_that_could_escape_the_backup_folder() {
        let root = scratch("names");
        for name in ["../game", "copia-1/../../x", "otra", "copia-001\\x", ""] {
            assert!(restore_backup(&root, name, 0).is_err(), "{name}");
        }
        assert!(restore_backup(&root, "copia-009", 0).is_err(), "unknown backup");
    }
}
