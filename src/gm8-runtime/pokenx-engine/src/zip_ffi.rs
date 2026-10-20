//! C ABI for importing the game ZIP from the host's console menu.
//!
//! The menu (C++) calls `pokenx_import_zip`; progress and cancellation travel through one callback. The work and the
//! validation are the shared importer in `pokenx_core::zip_sd` (same code the Rust menu app uses).

use core::ffi::{c_char, c_void, CStr};
use pokenx_core::zip_sd;

/// Called with the bytes written so far and the total (0 until known). Return non-zero to cancel.
pub type ProgressFn = Option<unsafe extern "C" fn(done: u64, total: u64, user: *mut c_void) -> i32>;

/// Imports the game from `zip_path` (a `sdmc:/...` path) into `sdmc:/switch/pokenx/game`, replacing the current game
/// (the replaced version is deleted once the new one is in place; the player's saves are carried over). Returns 0 on success, otherwise an error code for `pokenx_import_error_text`.
///
/// # Safety
/// `zip_path` must be a valid NUL-terminated string; `progress` (if any) must be callable with `user`.
#[no_mangle]
pub unsafe extern "C" fn pokenx_import_zip(zip_path: *const c_char, progress: ProgressFn, user: *mut c_void) -> i32 {
    if zip_path.is_null() {
        return -1;
    }
    let Ok(path) = CStr::from_ptr(zip_path).to_str() else { return -1 };
    let report = |done: u64, total: u64| match progress {
        // SAFETY: the host promised the callback is valid for the duration of the call.
        Some(callback) => unsafe { callback(done, total, user) != 0 },
        None => false,
    };
    match zip_sd::import_game(path, zip_sd::POKENX_ROOT, &report) {
        Ok(()) => 0,
        Err(error) => zip_sd::error_code(error),
    }
}

/// Copies the NUL-terminated Spanish text for an import error code into `out` (at most `capacity` bytes).
///
/// # Safety
/// `out` must be valid for `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn pokenx_import_error_text(code: i32, out: *mut c_char, capacity: usize) {
    if out.is_null() || capacity == 0 {
        return;
    }
    let text = match zip_sd::error_from_code(code) {
        Some(error) => zip_sd::error_text(error),
        None => "ruta de ZIP no valida",
    };
    let bytes = text.as_bytes();
    let count = bytes.len().min(capacity - 1);
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), count);
    *out.add(count) = 0;
}

/// Looks for the game in `dir` (the `game/` folder) exactly as the loader does, by the content of the executable and
/// never by its name. Returns 0 when found and writes the executable's path into `out`; returns non-zero otherwise
/// and writes the reason (Spanish, ASCII) into `out`.
///
/// # Safety
/// `dir` must be a valid NUL-terminated string and `out` valid for `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn pokenx_find_game(dir: *const c_char, out: *mut c_char, capacity: usize) -> i32 {
    if dir.is_null() || out.is_null() || capacity == 0 {
        return -1;
    }
    let Ok(dir) = CStr::from_ptr(dir).to_str() else { return -1 };
    let (code, text) = match pokenx_core::loader::find_game_exe(dir) {
        Ok(path) => (0, path.to_string_lossy().into_owned()),
        Err(reason) => (1, reason),
    };
    let bytes = text.as_bytes();
    let count = bytes.len().min(capacity - 1);
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), count);
    *out.add(count) = 0;
    code
}

fn write_c_string(text: &str, out: *mut c_char, capacity: usize) {
    if out.is_null() || capacity == 0 {
        return;
    }
    let bytes = text.as_bytes();
    let count = bytes.len().min(capacity - 1);
    // SAFETY: the caller guarantees `out` is valid for `capacity` bytes; `count < capacity`.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), count);
        *out.add(count) = 0;
    }
}

/// Tells the engine the current time (seconds since the Unix epoch) before its own clock exists, so backups made from the
/// menu carry a date.
#[no_mangle]
pub extern "C" fn pokenx_set_unix_seconds(seconds: i64) {
    pokenx_core::user_data::set_unix_hint(seconds);
}

/// Reads the key bindings the game sets for itself (see `pokenx_core::keys_report`) and writes the report to `out_path`.
/// Returns the number of bindings found, or a negative number: -1 bad arguments, -2 no game, -3 the data could not be
/// parsed, -4 the report could not be written. Parsing the game takes several seconds and a few hundred MiB.
///
/// # Safety
/// Both paths must be valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn pokenx_scan_game_keys(game_dir: *const c_char, out_path: *const c_char) -> i32 {
    if game_dir.is_null() || out_path.is_null() {
        return -1;
    }
    let (Ok(dir), Ok(path)) = (CStr::from_ptr(game_dir).to_str(), CStr::from_ptr(out_path).to_str()) else { return -1 };
    let text = match pokenx_core::keys_report::scan_game(dir) {
        Ok(text) => text,
        Err(pokenx_core::keys_report::ScanError::NoGame(_)) => return -2,
        Err(pokenx_core::keys_report::ScanError::Unreadable(_)) => return -3,
    };
    let count = pokenx_core::keys_report::parse_report(&text).2.len() as i32;
    if let Some(parent) = core::str::from_utf8(path.as_bytes()).ok().and_then(|p| p.rfind('/').map(|at| &p[..at])) {
        let _ = std_create_dir_all(parent);
    }
    match std_write(path, text.as_bytes()) {
        Ok(()) => count,
        Err(_) => -4,
    }
}

/// Like `pokenx_scan_game_keys`, and also writes the function-compatibility report of the game (which built-in GML
/// functions it calls that the engine lacks or simplifies). The game is parsed once, or not at all when the menu already
/// preloaded it. Returns the number of key bindings or a negative number as above.
///
/// # Safety
/// The three paths must be valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn pokenx_scan_game_reports(game_dir: *const c_char, keys_path: *const c_char, compat_path: *const c_char) -> i32 {
    if game_dir.is_null() || keys_path.is_null() || compat_path.is_null() {
        return -1;
    }
    let (Ok(dir), Ok(keys_path), Ok(compat_path)) =
        (CStr::from_ptr(game_dir).to_str(), CStr::from_ptr(keys_path).to_str(), CStr::from_ptr(compat_path).to_str())
    else {
        return -1;
    };
    let (keys, compat) = match pokenx_core::keys_report::scan_game_reports(dir) {
        Ok(texts) => texts,
        Err(pokenx_core::keys_report::ScanError::NoGame(_)) => return -2,
        Err(pokenx_core::keys_report::ScanError::Unreadable(_)) => return -3,
    };
    let count = pokenx_core::keys_report::parse_report(&keys).2.len() as i32;
    for (path, text) in [(keys_path, &keys), (compat_path, &compat)] {
        if let Some(parent) = path.rfind('/').map(|at| &path[..at]) {
            let _ = std_create_dir_all(parent);
        }
        if std_write(path, text.as_bytes()).is_err() {
            return -4;
        }
    }
    count
}

fn std_create_dir_all(path: &str) -> Result<(), ()> {
    pokenx_core::user_data::ensure_dir(path).map_err(|_| ())
}

fn std_write(path: &str, bytes: &[u8]) -> Result<(), ()> {
    pokenx_core::user_data::write_file(path, bytes).map_err(|_| ())
}

/// Copies the player's saves and settings into a new dated backup under `<root>/backups`. Returns the number of files
/// copied (the backup's name goes to `name_out`), or -1 on a bad argument and -2 when the copy failed.
///
/// # Safety
/// `root` must be a valid NUL-terminated string and `name_out` valid for `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn pokenx_backup_create(root: *const c_char, name_out: *mut c_char, capacity: usize) -> i32 {
    if root.is_null() {
        return -1;
    }
    let Ok(root) = CStr::from_ptr(root).to_str() else { return -1 };
    match pokenx_core::user_data::create_backup(root.as_ref(), pokenx_core::user_data::now_unix()) {
        Ok(info) => {
            write_c_string(&info.name, name_out, capacity);
            info.files as i32
        },
        Err(_) => -2,
    }
}

/// Lists the backups, newest first, one per line as `name<TAB>files<TAB>bytes<TAB>unix_seconds`. Returns how many there are.
///
/// # Safety
/// `root` must be a valid NUL-terminated string and `out` valid for `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn pokenx_backup_list(root: *const c_char, out: *mut c_char, capacity: usize) -> i32 {
    if root.is_null() {
        return -1;
    }
    let Ok(root) = CStr::from_ptr(root).to_str() else { return -1 };
    let backups = pokenx_core::user_data::list_backups(root.as_ref());
    let mut text = alloc::string::String::new();
    for info in &backups {
        text.push_str(&alloc::format!("{}\t{}\t{}\t{}\n", info.name, info.files, info.bytes, info.unix));
    }
    write_c_string(&text, out, capacity);
    backups.len() as i32
}

/// Restores backup `name` into `<root>/game` (a new backup of the current files is made first). Returns the number of files
/// restored, -1 for a bad argument and -2 when it failed.
///
/// # Safety
/// Both strings must be valid and NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn pokenx_backup_restore(root: *const c_char, name: *const c_char) -> i32 {
    if root.is_null() || name.is_null() {
        return -1;
    }
    let (Ok(root), Ok(name)) = (CStr::from_ptr(root).to_str(), CStr::from_ptr(name).to_str()) else { return -1 };
    match pokenx_core::user_data::restore_backup(root.as_ref(), name, pokenx_core::user_data::now_unix()) {
        Ok(stats) => stats.files as i32,
        Err(_) => -2,
    }
}

/// Reads and parses the game in `game_dir` so that the next `pokenx_engine_load_game` can skip that work. Blocks until
/// done: the menu runs it on a worker thread while the player is still choosing. Returns 0 on success, negative on failure.
///
/// # Safety
/// `game_dir` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pokenx_preload_game(game_dir: *const c_char) -> i32 {
    if game_dir.is_null() {
        return -1;
    }
    let Ok(dir) = CStr::from_ptr(game_dir).to_str() else { return -1 };
    match pokenx_core::preload::preload(dir) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Frees the game data kept by `pokenx_preload_game` (the game was replaced or will not be played).
#[no_mangle]
pub extern "C" fn pokenx_preload_clear() {
    pokenx_core::preload::clear();
}
