//! The sessions' log of declarations (#445), against the prototype's `test_dichiarazioni.py`: a line per
//! write, appended to a file per session and process, nothing without a session, and the threads of a
//! process never mixing their lines. Every folder is a fresh one under `target/`.

use std::fs;
use std::path::PathBuf;

use regia_store::{Declaration, DeclarationError, SessionIds, WallTime, write_declaration};
use serde_json::Value;

/// 2026-01-10T09:00:00Z, the instant of the fixtures.
const T0_MICROS: i64 = 1_768_035_600_000_000;

fn folder(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-declarations").join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    dir
}

fn claude(id: &str) -> SessionIds {
    let id = id.to_owned();
    SessionIds::from_lookup(move |name| (name == "CLAUDE_CODE_SESSION_ID").then(|| id.clone()))
}

fn at() -> WallTime {
    WallTime::from_unix_micros(T0_MICROS + 999_999).unwrap()
}

/// The fourth of the prototype's `TestDichiara`: without a session nothing is written, not even the folder.
#[test]
fn without_a_session_nothing_is_written() {
    let folder = folder("no-session");
    let declaration = Declaration { command: "c", project: Some("p"), task: None, role: None };
    let refused = write_declaration(&folder, &SessionIds::default(), &declaration, at());
    assert!(matches!(refused, Err(DeclarationError::NoSession)), "{refused:?}");
    assert!(!folder.exists(), "nothing is created");
}

/// A line per write, in the file of the session and the process, at the instant truncated to the second;
/// two sessions, two files.
#[test]
fn each_write_appends_a_line_to_the_session_s_file() {
    let folder = folder("lines");
    let declaration = Declaration { command: "c d", project: Some("alpha"), task: Some(146), role: Some("worker") };
    let first = write_declaration(&folder, &claude("s-1"), &declaration, at()).unwrap();
    let second = write_declaration(&folder, &claude("s-1"), &Declaration { task: None, ..declaration }, at()).unwrap();
    assert_eq!((first.file.clone(), first.ts.as_str(), first.session.as_str()), (second.file.clone(), "2026-01-10T09:00:00Z", "s-1"));
    assert_eq!(first.file.file_name().unwrap().to_string_lossy(), format!("s-1.{}.jsonl", std::process::id()));
    let text = fs::read_to_string(&first.file).unwrap();
    assert_eq!(text, format!("{}{}", first.line, second.line));
    let lines: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!((lines[0]["task"].clone(), lines[1].get("task")), (Value::from(146), None));
    write_declaration(&folder, &claude("s-2"), &declaration, at()).unwrap();
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 2, "a file per session");
}

/// `test_i_fili_dello_stesso_processo_non_si_mescolano`: the threads of a process write to one file and
/// never mix their lines.
#[test]
fn threads_of_a_process_do_not_mix_their_lines() {
    let folder = folder("threads");
    let handles: Vec<_> = (0..6)
        .map(|thread| {
            let folder = folder.clone();
            std::thread::spawn(move || {
                for task in 0..60 {
                    let declaration = Declaration { command: "c", project: Some("alpha"), task: Some(thread * 100 + task), role: None };
                    write_declaration(&folder, &claude("t"), &declaration, at()).unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let files: Vec<PathBuf> = fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().path()).collect();
    assert_eq!(files.len(), 1, "one file for the process");
    let text = fs::read_to_string(&files[0]).unwrap();
    let lines: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.len(), 360);
}
