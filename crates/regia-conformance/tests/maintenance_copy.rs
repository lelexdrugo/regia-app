//! Local only: the compaction of a copy of a real log, by hand (#471, Acceptance 2). The prototype's `moviola sync`
//! runs on the same copy before and after, outside this test (the dossier's `.regia/prove/471/copia_sync.py`), and
//! the store it writes must not change. CI has no copy, so the test is ignored there:
//!
//!     COMPACTION_COPY=<the copy's log folder> [COMPACTION_DUMP=<file>] cargo test -p regia-conformance --test maintenance_copy -- --ignored
//!
//! It refuses a folder inside the machine's own state folder: only a copy is compacted. `COMPACTION_DUMP` names a
//! file where the reader's events before the compaction are written, one `[micros, line]` a line, for the prototype's
//! `leggi` to be compared with.

use std::fs;
use std::path::PathBuf;

use regia_store::{CompactionPolicy, WallTime, compact_declarations, default_folder, is_within, read_declarations};

#[test]
#[ignore = "local only: compacts the copy named by COMPACTION_COPY"]
fn compacts_a_copy_and_the_reader_sees_the_same() {
    let folder = PathBuf::from(std::env::var_os("COMPACTION_COPY").expect("set COMPACTION_COPY to a copy's log folder"));
    if let Ok(live) = default_folder() {
        assert!(!is_within(&folder, &live), "{} is inside the machine's own state: only a copy is compacted", folder.display());
    }
    let seen = |events: Vec<regia_store::LoggedDeclaration>| events.into_iter().map(|event| (event.at.unix_micros(), event.line)).collect::<Vec<_>>();
    let before = seen(read_declarations(&folder).unwrap());
    if let Some(dump) = std::env::var_os("COMPACTION_DUMP") {
        let text: String = before.iter().map(|pair| format!("{}\n", serde_json::to_string(pair).unwrap())).collect();
        fs::write(dump, text).unwrap();
    }
    let files = fs::read_dir(&folder).unwrap().count();
    let done = compact_declarations(&folder, &CompactionPolicy::at(WallTime::now(), false)).unwrap();
    let after = seen(read_declarations(&folder).unwrap());
    eprintln!("files before {files}, after {}; {done:?}", fs::read_dir(&folder).unwrap().count());
    assert!(done.merged > 0, "nothing merged: {done:?}");
    assert_eq!(after, before, "the reader sees the same events in the same order");
}
