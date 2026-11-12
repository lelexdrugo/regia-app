//! The whole files of the state folder: written by a move into place, read back, removed. Every folder is a
//! fresh one under `target/`, never the live one.

use std::path::PathBuf;

use regia_store::{read_state_file, remove_state_file, write_state_file};

fn folder(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-state-file").join(label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    dir
}

/// The folder is created, a second write replaces the first and leaves no temporary file, and a removed
/// file reads as none.
#[test]
fn a_state_file_is_written_whole_read_and_removed() {
    let dir = folder("whole").join("nested");
    assert_eq!(read_state_file(&dir, "local.json").unwrap(), None);
    write_state_file(&dir, "local.json", b"{\"a\": 1}").unwrap();
    write_state_file(&dir, "local.json", b"{\r\n \"a\": 2\r\n}").unwrap();
    assert_eq!(read_state_file(&dir, "local.json").unwrap().as_deref(), Some(&b"{\r\n \"a\": 2\r\n}"[..]));
    let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["local.json"]);
    assert!(remove_state_file(&dir, "local.json").unwrap());
    assert!(!remove_state_file(&dir, "local.json").unwrap());
    assert_eq!(read_state_file(&dir, "local.json").unwrap(), None);
}
