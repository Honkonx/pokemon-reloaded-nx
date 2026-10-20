//! ZIP import for the single-NRO build: the importer core shared with the menu app, plus a reader and a sink
//! built on `std::fs` (newlib/fsdev on Horizon, which opens write modes with Append, so growing a new file works).
//!
//! The core is `zip_import.rs`; its `nx`-only parts are
//! behind the `nx-sd` feature, which this crate does not enable. Nothing is duplicated.

#![allow(unexpected_cfgs, dead_code)]

#[path = "zip_import.rs"]
mod core_import;

pub use core_import::{ImportControl, ZipError, ZipReader, ZipSink};

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Root of the data folder on the SD card.
pub const POKENX_ROOT: &str = "sdmc:/switch/pokenx";

/// Free bytes on the volume holding `path`; `u64::MAX` when the host cannot tell (then a full disk shows up as an
/// I/O error while writing).
#[cfg(not(feature = "std"))]
fn free_bytes(path: &str) -> u64 {
    extern "C" {
        fn pokenx_fs_free_bytes(path: *const core::ffi::c_char) -> u64;
    }
    let mut c_path = path.as_bytes().to_vec();
    c_path.push(0);
    // SAFETY: `c_path` is NUL-terminated and outlives the call.
    unsafe { pokenx_fs_free_bytes(c_path.as_ptr().cast()) }
}

#[cfg(feature = "std")]
fn free_bytes(_path: &str) -> u64 {
    u64::MAX
}

pub struct StdReader(File);

impl StdReader {
    pub fn open(path: &str) -> Result<Self, ZipError> {
        File::open(path).map(Self).map_err(|_| ZipError::Io)
    }
}

impl ZipReader for StdReader {
    fn len(&mut self) -> Result<u64, ZipError> {
        self.0.metadata().map(|m| m.len()).map_err(|_| ZipError::Io)
    }
    fn seek_abs(&mut self, offset: u64) -> Result<(), ZipError> {
        self.0.seek(SeekFrom::Start(offset)).map(|_| ()).map_err(|_| ZipError::Io)
    }
    fn read_some(&mut self, out: &mut [u8]) -> Result<usize, ZipError> {
        self.0.read(out).map_err(|_| ZipError::Io)
    }
}

/// Extracts into `<root>/.game-import-staging`, validates it and swaps it into `<root>/game`. The previous game is
/// held as `<root>/game.backup.N` only during the swap (it is put back if the swap fails) and deleted once the new one
/// is in place, so an update never leaves two versions on the card.
pub struct StdGameSink {
    root: String,
    active: Option<File>,
}

impl StdGameSink {
    pub fn new(root: &str) -> Self {
        Self { root: root.trim_end_matches('/').to_string(), active: None }
    }
    fn staging(&self) -> String {
        format!("{}/.game-import-staging", self.root)
    }
    fn game(&self) -> String {
        format!("{}/game", self.root)
    }
    fn full_path(&self, relative: &str) -> Result<String, ZipError> {
        let path = format!("{}/{}", self.staging(), relative);
        if path.len() > 768 { Err(ZipError::InvalidName) } else { Ok(path) }
    }
    /// Removes every `<root>/game.backup.*` folder. Failures are ignored: leftovers only cost space.
    fn remove_old_versions(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("game.backup.") {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    fn select_backup(&self) -> Result<String, ZipError> {
        for suffix in 1..=1000 {
            let path = format!("{}/game.backup.{suffix}", self.root);
            if fs::metadata(&path).is_err() {
                return Ok(path);
            }
        }
        Err(ZipError::InstallFailed)
    }
}

impl ZipSink for StdGameSink {
    fn prepare_staging(&mut self, required_bytes: u64) -> Result<(), ZipError> {
        if self.active.is_some() {
            return Err(ZipError::Io);
        }
        let staging = self.staging();
        if let Ok(meta) = fs::metadata(&staging) {
            if meta.is_dir() { fs::remove_dir_all(&staging) } else { fs::remove_file(&staging) }
                .map_err(|_| ZipError::Io)?;
        }
        if fs::metadata(&self.root).is_err() {
            fs::create_dir_all(&self.root).map_err(|_| ZipError::Io)?;
        }
        let needed = required_bytes.checked_add(1024 * 1024).ok_or(ZipError::TotalTooLarge)?;
        if free_bytes(&self.root) < needed {
            return Err(ZipError::InsufficientSpace);
        }
        fs::create_dir(&staging).map_err(|_| ZipError::Io)
    }

    fn make_directory(&mut self, relative_path: &str) -> Result<(), ZipError> {
        let path = self.full_path(relative_path)?;
        match fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => Ok(()),
            Ok(_) => Err(ZipError::Io),
            Err(_) => fs::create_dir(&path).map_err(|_| ZipError::Io),
        }
    }

    fn begin_file(&mut self, relative_path: &str) -> Result<(), ZipError> {
        if self.active.is_some() {
            return Err(ZipError::Io);
        }
        let path = self.full_path(relative_path)?;
        if fs::metadata(&path).is_ok() {
            return Err(ZipError::DuplicatePath);
        }
        self.active = Some(File::create(&path).map_err(|_| ZipError::Io)?);
        Ok(())
    }

    fn write_file(&mut self, bytes: &[u8]) -> Result<(), ZipError> {
        self.active.as_mut().ok_or(ZipError::Io)?.write_all(bytes).map_err(|_| ZipError::Io)
    }

    fn finish_file(&mut self) -> Result<(), ZipError> {
        let mut file = self.active.take().ok_or(ZipError::Io)?;
        file.flush().map_err(|_| ZipError::Io)
    }

    fn validate_staging_game(&mut self) -> Result<(), ZipError> {
        let staging = self.staging();
        // Same policy as the loader: a recognised GM8.0 executable by content, plus a non-empty `data/`.
        if crate::loader::find_game_exe(&staging).is_err() {
            return Err(ZipError::InvalidGameLayout);
        }
        let data = format!("{staging}/data");
        let has_data = fs::read_dir(&data).map(|mut entries| entries.next().is_some()).unwrap_or(false);
        if has_data { Ok(()) } else { Err(ZipError::InvalidGameLayout) }
    }

    fn clear_staging(&mut self) {
        self.active.take();
        let _ = fs::remove_dir_all(self.staging());
    }

    fn install_staging(&mut self) -> Result<(), ZipError> {
        let (staging, game) = (self.staging(), self.game());
        let game_exists = fs::metadata(&game).map(|m| m.is_dir()).unwrap_or(false);
        if !game_exists {
            return fs::rename(&staging, &game).map_err(|_| ZipError::InstallFailed);
        }
        // Updating must never cost the player their saves: keep a dated copy of them, then carry them (and the
        // settings) into the new game before it takes the place of the old one. If carrying fails, nothing is swapped.
        let _ = crate::user_data::create_backup(std::path::Path::new(&self.root), crate::user_data::now_unix());
        crate::user_data::copy_user_data(std::path::Path::new(&game), std::path::Path::new(&staging))
            .map_err(|_| ZipError::InstallFailed)?;
        let backup = self.select_backup()?;
        fs::rename(&game, &backup).map_err(|_| ZipError::InstallFailed)?;
        if fs::rename(&staging, &game).is_err() {
            let _ = fs::rename(&backup, &game);
            return Err(ZipError::InstallFailed);
        }
        // The new version is in place and has the player's data: an update leaves ONE game folder, so the replaced
        // version (and any older `game.backup.N`) goes away. The saves keep their dated copy under `backups/`.
        self.remove_old_versions();
        Ok(())
    }
}

/// Progress and cancellation bridge. `report(done, total)` returns `true` to cancel.
pub struct Control<'a> {
    report: &'a dyn Fn(u64, u64) -> bool,
    total: std::cell::Cell<u64>,
    done: std::cell::Cell<u64>,
    cancelled: std::cell::Cell<bool>,
}

impl<'a> Control<'a> {
    pub fn new(report: &'a dyn Fn(u64, u64) -> bool) -> Self {
        Self { report, total: Default::default(), done: Default::default(), cancelled: Default::default() }
    }
    fn tick(&self) {
        if (self.report)(self.done.get(), self.total.get()) {
            self.cancelled.set(true);
        }
    }
}

impl ImportControl for Control<'_> {
    fn cancelled(&self) -> bool {
        self.cancelled.get()
    }
    fn set_total(&self, bytes: u64) {
        self.total.set(bytes);
        self.tick();
    }
    fn advanced(&self, bytes: u64) {
        self.done.set(self.done.get().saturating_add(bytes));
        self.tick();
    }
}

/// Imports the game from `zip_path` into `<root>/game` (replacing the current game, which is kept as a backup).
pub fn import_game(zip_path: &str, root: &str, report: &dyn Fn(u64, u64) -> bool) -> Result<(), ZipError> {
    let mut reader = StdReader::open(zip_path)?;
    let mut sink = StdGameSink::new(root);
    core_import::import_controlled(&mut reader, &mut sink, &Control::new(report))
}

/// Short Spanish ASCII text for an import error (the console menu font has no accents).
pub fn error_text(error: ZipError) -> &'static str {
    match error {
        ZipError::InsufficientSpace => "falta espacio en la microSD",
        ZipError::Io => "error de lectura o escritura (microSD llena o ZIP danado)",
        ZipError::Integrity => "el CRC no coincide; el ZIP esta corrupto",
        ZipError::MissingEndRecord | ZipError::InvalidEndRecord | ZipError::InvalidCentralDirectory => {
            "ZIP corrupto o incompleto"
        }
        ZipError::UnsupportedZip64
        | ZipError::TooManyEntries
        | ZipError::CentralDirectoryTooLarge
        | ZipError::EntryTooLarge
        | ZipError::TotalTooLarge => "el ZIP supera los limites admitidos",
        ZipError::InvalidGameLayout => "el ZIP no contiene el .exe del juego y la carpeta data/",
        ZipError::InstallFailed => "no se pudo reemplazar la carpeta game/",
        ZipError::Cancelled => "cancelado; no se instalo nada",
        _ => "el ZIP tiene rutas o entradas no admitidas",
    }
}

/// Every import error in a fixed order, so the C host can carry them as small integers (index + 1; 0 = success).
pub const ALL_ERRORS: [ZipError; 18] = [
    ZipError::MissingEndRecord,
    ZipError::InvalidEndRecord,
    ZipError::UnsupportedZip64,
    ZipError::CentralDirectoryTooLarge,
    ZipError::InvalidCentralDirectory,
    ZipError::TooManyEntries,
    ZipError::InvalidName,
    ZipError::UnsafePath,
    ZipError::DuplicatePath,
    ZipError::UnsupportedEntry,
    ZipError::EntryTooLarge,
    ZipError::TotalTooLarge,
    ZipError::Io,
    ZipError::Integrity,
    ZipError::InvalidGameLayout,
    ZipError::InsufficientSpace,
    ZipError::InstallFailed,
    ZipError::Cancelled,
];

pub fn error_code(error: ZipError) -> i32 {
    ALL_ERRORS.iter().position(|e| *e == error).map(|i| i as i32 + 1).unwrap_or(-1)
}

pub fn error_from_code(code: i32) -> Option<ZipError> {
    usize::try_from(code - 1).ok().and_then(|i| ALL_ERRORS.get(i).copied())
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    // Stored (uncompressed) ZIP with `Game.exe` (an MZ header only) and `data/a.bin`. The synthetic executable is not a
    // GM8.0 container, so the layout validation must reject it and leave `game/` untouched.
    fn tiny_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let crc = {
                let mut c = !0u32;
                for &b in data.iter() {
                    c ^= u32::from(b);
                    for _ in 0..8 {
                        c = (c >> 1) ^ (0xEDB8_8320 & (!(c & 1)).wrapping_add(1));
                    }
                }
                !c
            };
            out.extend_from_slice(&[0x50, 0x4b, 3, 4, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            central.extend_from_slice(&[0x50, 0x4b, 1, 2, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let central_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&central_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pokenx-zipsd-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_zip_without_a_gm8_executable_is_rejected_and_the_current_game_is_kept() {
        let dir = scratch("reject");
        let root = dir.to_str().unwrap().replace('\\', "/");
        fs::create_dir_all(dir.join("game/data")).unwrap();
        fs::write(dir.join("game/keep.txt"), b"old").unwrap();
        let zip_path = dir.join("g.zip");
        fs::write(&zip_path, tiny_zip(&[("Game.exe", b"MZ-not-a-gm8"), ("data/a.bin", b"1")])).unwrap();
        let result = import_game(zip_path.to_str().unwrap(), &root, &|_, _| false);
        assert_eq!(result, Err(ZipError::InvalidGameLayout));
        assert_eq!(fs::read(dir.join("game/keep.txt")).unwrap(), b"old", "the current game must stay in place");
        assert!(!dir.join(".game-import-staging").exists(), "staging must be cleaned");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancelling_from_the_progress_callback_stops_without_touching_the_game() {
        let dir = scratch("cancel");
        let root = dir.to_str().unwrap().replace('\\', "/");
        let zip_path = dir.join("g.zip");
        fs::write(&zip_path, tiny_zip(&[("Game.exe", b"MZ-x"), ("data/a.bin", b"1")])).unwrap();
        let result = import_game(zip_path.to_str().unwrap(), &root, &|_, _| true);
        assert_eq!(result, Err(ZipError::Cancelled));
        assert!(!dir.join("game").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn installing_an_update_carries_the_saves_over_and_leaves_a_single_game_folder() {
        let dir = scratch("update");
        let root = dir.to_str().unwrap().replace('\\', "/");
        fs::create_dir_all(dir.join("game/data/partidas")).unwrap();
        fs::write(dir.join("game/data/partidas/reloaded1.data"), "mi partida").unwrap();
        fs::write(dir.join("game/viejo.txt"), "version vieja").unwrap();
        fs::create_dir_all(dir.join(".game-import-staging/data")).unwrap();
        fs::write(dir.join(".game-import-staging/nuevo.txt"), "version nueva").unwrap();
        StdGameSink::new(&root).install_staging().unwrap();
        assert_eq!(fs::read_to_string(dir.join("game/data/partidas/reloaded1.data")).unwrap(), "mi partida");
        assert!(dir.join("game/nuevo.txt").exists() && !dir.join("game/viejo.txt").exists());
        assert!(!dir.join("game.backup.1").exists(), "the replaced version is deleted: one game folder only");
        fs::create_dir_all(dir.join("game.backup.7")).unwrap();
        fs::write(dir.join("game.backup.7/resto.txt"), "de una importacion antigua").unwrap();
        fs::create_dir_all(dir.join(".game-import-staging")).unwrap();
        fs::write(dir.join(".game-import-staging/otra.txt"), "version aun mas nueva").unwrap();
        StdGameSink::new(&root).install_staging().unwrap();
        assert!(!dir.join("game.backup.7").exists(), "older leftovers are cleaned too");
        assert!(dir.join("game/otra.txt").exists() && dir.join("game/data/partidas/reloaded1.data").exists());
        assert_eq!(crate::user_data::list_backups(Path::new(&root)).len(), 2, "each install made a dated copy of the saves");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn error_codes_round_trip() {
        for (i, error) in ALL_ERRORS.iter().enumerate() {
            assert_eq!(error_code(*error), i as i32 + 1);
            assert_eq!(error_from_code(i as i32 + 1), Some(*error));
        }
        assert_eq!(error_from_code(0), None);
        assert_eq!(error_from_code(99), None);
    }

    #[test]
    fn errors_have_plain_text() {
        assert!(error_text(ZipError::InsufficientSpace).contains("espacio"));
        assert!(error_text(ZipError::Cancelled).contains("cancelado"));
    }
}
