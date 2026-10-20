//! Host tests of the Horizon shim: its file system calls go to `host::fake` (real `std` under
//! the hood), so everything else (paths, buffering, directory walks, sync) runs for real.

use alloc_crate::{boxed::Box, format, string::{String, ToString}, vec, vec::Vec};
use super::{
    collections::{HashMap, HashSet},
    env, ffi,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{mpsc, Mutex, RwLock},
    time::{Duration, Instant},
};

fn scratch(name: &str) -> PathBuf {
    let dir = real_std::env::temp_dir().join(format!("pokenx-std-test-{}-{}", name, real_std::process::id()));
    let _ = real_std::fs::remove_dir_all(&dir);
    real_std::fs::create_dir_all(&dir).unwrap();
    PathBuf::from(dir.to_str().unwrap().replace('\\', "/"))
}

#[test]
fn path_components_and_names() {
    let p = Path::new("sdmc:/switch/pokenx/game/data/hero.png");
    assert_eq!(p.file_name().unwrap().to_str(), Some("hero.png"));
    assert_eq!(p.file_stem().unwrap().to_str(), Some("hero"));
    assert_eq!(p.extension().unwrap().to_str(), Some("png"));
    assert_eq!(p.parent().unwrap().to_str(), Some("sdmc:/switch/pokenx/game/data"));
    assert!(p.is_absolute());
    assert!(!Path::new("data/hero.png").is_absolute());
    assert_eq!(Path::new("a/b.txt").with_extension("bin").to_str(), Some("a/b.bin"));
    assert_eq!(Path::new("a/b").with_file_name("c").to_str(), Some("a/c"));
    assert_eq!(Path::new(".hidden").extension(), None);
    assert_eq!(Path::new("archive.tar.gz").file_stem().unwrap().to_str(), Some("archive.tar"));
    assert_eq!(Path::new("sdmc:/").parent(), None);
    assert_eq!(Path::new("/").parent(), None);
}

#[test]
fn path_join_push_pop_and_prefixes() {
    let mut buf = PathBuf::from("sdmc:/switch");
    buf.push("pokenx");
    buf.push("game/data");
    assert_eq!(buf.to_str(), Some("sdmc:/switch/pokenx/game/data"));
    assert!(buf.pop());
    assert_eq!(buf.to_str(), Some("sdmc:/switch/pokenx/game"));
    buf.push("/absolute/wins");
    assert_eq!(buf.to_str(), Some("/absolute/wins"));
    assert_eq!(Path::new("sdmc:/").join("x").to_str(), Some("sdmc:/x"));
    assert!(Path::new("sdmc:/switch/pokenx").starts_with("sdmc:/switch"));
    assert!(!Path::new("sdmc:/switchX").starts_with("sdmc:/switch"));
    assert!(Path::new("a/b/c.txt").ends_with("b/c.txt"));
    assert_eq!(Path::new("sdmc:/a/b").strip_prefix("sdmc:/a").unwrap().to_str(), Some("b"));
    assert!(Path::new("sdmc:/a/b").strip_prefix("sdmc:/z").is_err());
    let collected: PathBuf = ["a", "b", "c"].iter().collect();
    assert_eq!(collected.to_str(), Some("a/b/c"));
}

#[test]
fn path_components_iterate() {
    let parts: Vec<Component> = Path::new("/a/./b/../c").components().collect();
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[0], Component::RootDir);
    assert_eq!(parts[3], Component::ParentDir);
    assert_eq!(parts[4].as_os_str().to_str(), Some("c"));
}

#[test]
fn files_round_trip_and_seek() {
    let dir = scratch("files");
    let file = dir.join("data.bin");
    fs::write(&file, b"hello world").unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"hello world");
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello world");
    assert_eq!(fs::metadata(&file).unwrap().len(), 11);
    assert!(file.exists() && file.is_file() && !file.is_dir());

    let mut handle = fs::File::open(&file).unwrap();
    handle.seek(SeekFrom::Start(6)).unwrap();
    let mut tail = String::new();
    handle.read_to_string(&mut tail).unwrap();
    assert_eq!(tail, "world");
    assert_eq!(handle.seek(SeekFrom::End(-5)).unwrap(), 6);
    assert_eq!(handle.metadata().unwrap().len(), 11);

    let mut out = OpenOptions::new().read(true).write(true).open(&file).unwrap();
    out.seek(SeekFrom::Start(0)).unwrap();
    out.write_all(b"J").unwrap();
    out.set_len(14).unwrap();
    drop(out);
    let data = fs::read(&file).unwrap();
    assert_eq!(&data[..11], b"Jello world");
    assert_eq!(data.len(), 14);
    assert!(OpenOptions::new().read(true).open(dir.join("missing")).is_err());
    assert!(OpenOptions::new().write(true).create_new(true).open(&file).is_err());
}

#[test]
fn binary_rewrite_truncates_and_preserves_the_stream_position() {
    let dir = scratch("truncate");
    let path = dir.join("save.bin");
    fs::write(&path, b"old saved data").unwrap();
    let mut file = OpenOptions::new().read(true).write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.set_len(0).unwrap(); // file_bin_rewrite's exact operation
    file.write_all(b"new").unwrap();
    file.sync_all().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"new");
    file.seek(SeekFrom::Start(2)).unwrap();
    file.set_len(1).unwrap();
    assert_eq!(file.seek(SeekFrom::Current(0)).unwrap(), 2);
    file.set_len(5).unwrap();
    assert_eq!(file.seek(SeekFrom::Current(0)).unwrap(), 2);
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"n\0\0\0\0");
    assert!(file.set_len(u64::MAX).is_err());
}

#[test]
fn stream_read_error_is_not_reported_as_eof() {
    let dir = scratch("read-error");
    let path = dir.join("data.bin");
    let mut write_only = fs::File::create(&path).unwrap();
    write_only.write_all(b"data").unwrap();
    let mut byte = [0];
    assert!(write_only.read(&mut byte).is_err());
    assert_eq!(write_only.read(&mut []).unwrap(), 0);
    drop(write_only);
    let mut read = fs::File::open(&path).unwrap();
    let mut bytes = Vec::new();
    read.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"data");
    assert_eq!(read.read(&mut byte).unwrap(), 0);
}

#[test]
fn explicit_sync_and_file_helpers_report_late_io_errors() {
    let dir = scratch("late-errors");
    let path = dir.join("save.bin");
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(b"saved").unwrap();
    super::host::fail_next_sync();
    assert!(file.sync_all().is_err(), "sync_all must report descriptor sync failure");
    file.sync_all().unwrap();
    drop(file);
    super::host::fail_next_flush();
    assert!(fs::write(&path, b"new").is_err(), "write must not discard stdio flush errors");
    super::host::fail_next_close();
    assert!(fs::write(&path, b"new").is_err(), "write must not discard close errors");
    fs::write(&path, b"complete").unwrap();
    super::host::fail_next_close();
    assert!(fs::copy(&path, dir.join("copy.bin")).is_err());
    assert_eq!(fs::copy(&path, dir.join("copy.bin")).unwrap(), 8);
    assert_eq!(fs::read(dir.join("copy.bin")).unwrap(), b"complete");
}

#[test]
fn exclusive_creation_never_truncates_an_existing_file() {
    let dir = scratch("exclusive");
    let path = dir.join("save.tmp");
    let mut file = OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
    file.write_all(b"preserve").unwrap();
    file.sync_all().unwrap();
    assert_eq!(OpenOptions::new().write(true).create_new(true).open(&path).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&path).unwrap(), b"preserve");
    assert!(OpenOptions::new().read(true).create_new(true).open(dir.join("invalid")).is_err());
}

#[test]
fn append_and_truncate_modes() {
    let dir = scratch("modes");
    let file = dir.join("log.txt");
    OpenOptions::new().append(true).create(true).open(&file).unwrap().write_all(b"one\n").unwrap();
    OpenOptions::new().append(true).open(&file).unwrap().write_all(b"two\n").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\ntwo\n");
    OpenOptions::new().write(true).truncate(true).open(&file).unwrap().write_all(b"x").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "x");
}

#[test]
fn directories_copy_rename_and_remove() {
    let dir = scratch("dirs");
    fs::create_dir_all(dir.join("a/b/c")).unwrap();
    assert!(dir.join("a/b/c").is_dir());
    fs::create_dir_all(dir.join("a/b/c")).unwrap(); // idempotent
    fs::write(dir.join("a/one.txt"), b"1").unwrap();
    fs::write(dir.join("a/b/two.txt"), b"22").unwrap();
    assert_eq!(fs::copy(dir.join("a/one.txt"), dir.join("copy.txt")).unwrap(), 1);
    fs::rename(dir.join("copy.txt"), dir.join("moved.txt")).unwrap();
    assert!(!dir.join("copy.txt").exists() && dir.join("moved.txt").exists());

    let mut names: Vec<String> = fs::read_dir(dir.join("a"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["b", "one.txt"]);
    let kinds: Vec<bool> = fs::read_dir(dir.join("a")).unwrap().map(|e| e.unwrap().metadata().unwrap().is_dir()).collect();
    assert_eq!(kinds.iter().filter(|d| **d).count(), 1);

    fs::remove_file(dir.join("moved.txt")).unwrap();
    fs::remove_dir_all(dir.join("a")).unwrap();
    assert!(!dir.join("a").exists());
    assert!(fs::read_dir(dir.join("a")).is_err());
}

#[test]
fn buffered_io_reads_lines_and_flushes_on_drop() {
    let dir = scratch("buffered");
    let file = dir.join("lines.txt");
    {
        let mut writer = BufWriter::new(fs::File::create(&file).unwrap());
        writeln!(writer, "alpha").unwrap();
        write!(writer, "beta\ngamma").unwrap();
    } // dropped: must flush
    let mut reader = BufReader::new(fs::File::open(&file).unwrap());
    let mut line = String::new();
    assert_eq!(reader.read_line(&mut line).unwrap(), 6);
    assert_eq!(line, "alpha\n");
    line.clear();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "beta\n");
    line.clear();
    assert_eq!(reader.read_line(&mut line).unwrap(), 5);
    assert_eq!(line, "gamma");
    line.clear();
    assert_eq!(reader.read_line(&mut line).unwrap(), 0);

    let bytes: Vec<u8> = fs::File::open(&file).unwrap().bytes().map(|b| b.unwrap()).collect();
    assert_eq!(bytes.len(), 16);
}

#[test]
fn working_directory_round_trips() {
    let dir = scratch("cwd");
    let previous = env::current_dir().unwrap();
    env::set_current_dir(&dir).unwrap();
    fs::write("relative.txt", b"r").unwrap();
    assert!(dir.join("relative.txt").exists());
    env::set_current_dir(&previous).unwrap();
    assert_eq!(env::temp_dir().to_str(), Some(env::TEMP_DIR));
    assert!(env::var("POKENX_DOES_NOT_EXIST").is_err());
}

#[test]
fn ffi_strings_behave_like_text() {
    let owned = ffi::OsString::from("sdmc:/x");
    assert_eq!(owned.to_str(), Some("sdmc:/x"));
    assert_eq!(ffi::OsStr::new("a"), ffi::OsStr::new("a"));
    assert!(ffi::OsStr::new("abc") < ffi::OsStr::new("abd"));
    assert_eq!(owned.into_string().unwrap(), "sdmc:/x");
    assert!(ffi::CString::new("with\0nul").is_err());
}

#[test]
fn collections_hash_and_order() {
    let mut map: HashMap<u32, &str> = HashMap::new();
    map.insert(1, "a");
    map.insert(2, "b");
    assert_eq!(map.get(&2), Some(&"b"));
    *map.entry(3).or_insert("c") = "C";
    assert_eq!(map.len(), 3);
    let set: HashSet<i32> = [1, 2, 2, 3].into_iter().collect();
    assert_eq!(set.len(), 3);
}

#[test]
fn locks_and_channels() {
    let mutex = Mutex::new(5);
    *mutex.lock().unwrap() += 1;
    assert_eq!(*mutex.lock().unwrap(), 6);
    assert!(mutex.try_lock().is_ok());
    let rw = RwLock::new(vec![1]);
    rw.write().unwrap().push(2);
    assert_eq!(rw.read().unwrap().len(), 2);

    let (tx, rx) = mpsc::channel();
    tx.send(1).unwrap();
    tx.send(2).unwrap();
    assert_eq!(rx.try_recv(), Ok(1));
    assert_eq!(rx.recv(), Ok(2));
    assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    assert_eq!(rx.recv_timeout(Duration::from_millis(2)), Err(mpsc::RecvTimeoutError::Timeout));
    drop(tx);
    assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    assert_eq!(rx.recv(), Err(mpsc::RecvError));
}

#[test]
fn clock_is_monotonic() {
    let a = Instant::now();
    super::thread::sleep(Duration::from_millis(2));
    let b = Instant::now();
    assert!(b > a);
    assert!(b.duration_since(a) >= Duration::from_millis(1));
    assert!(a + Duration::from_secs(1) > b);
}

#[test]
fn errors_display_like_std() {
    let error = io::Error::new(io::ErrorKind::NotFound, "no such file");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(fs::read("definitely/not/here").is_err());
}
