//! The journal mode of the state database (#439/D4, closed in #561). The prototype never sets it, and its ADR
//! says "rollback journal, no WAL": a lock that two languages share by SQLite's write lock must not meet a
//! database in WAL that one of them never looks at. The Rust store reads `PRAGMA journal_mode` at every open
//! and puts the database back in `delete`; if SQLite cannot, that is an error of its own, never a success.
//! Every state folder is a fresh one under `target/`, never the live one.

use std::path::{Path, PathBuf};

use regia_store::{Holder, LockState, ProjectId, Store, StoreError, Ttl};
use rusqlite::Connection;

fn folder(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-journal").join(label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    std::fs::create_dir_all(&dir).expect("state folder");
    dir
}

fn mode_of(conn: &Connection) -> String {
    conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).expect("journal mode")
}

fn journal_of(dir: &Path) -> String {
    mode_of(&Connection::open(dir.join(regia_store::database_name())).expect("open"))
}

fn take(store: &Store) -> Result<LockState, StoreError> {
    let (project, holder) = (ProjectId::new("alpha").unwrap(), Holder::new("w1").unwrap());
    store.acquire_lock(&project, &holder, Ttl::from_secs(60).unwrap()).map(|outcome| outcome.state())
}

fn put_in_wal(dir: &Path) {
    let conn = Connection::open(dir.join(regia_store::database_name())).expect("create");
    assert_eq!(mode_of(&conn), "delete", "a database SQLite creates starts in the rollback journal");
    let answered: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0)).expect("set WAL");
    assert_eq!(answered, "wal");
    conn.close().expect("close");
}

/// The prototype leaves the mode alone, and a new database is `delete`: the Rust store does not change that.
#[test]
fn a_new_database_stays_in_the_rollback_journal() {
    let dir = folder("new");
    let store = Store::at(&dir).unwrap();
    assert_eq!(take(&store).unwrap(), LockState::Taken);
    assert_eq!(journal_of(&dir), "delete");
}

/// A database someone put in WAL: the mode is persistent, so after a write of the Rust store it reads `delete`,
/// and the write itself worked.
#[test]
fn a_database_in_wal_is_put_back_in_the_rollback_journal_by_a_write() {
    let dir = folder("wal-back");
    put_in_wal(&dir);
    assert_eq!(journal_of(&dir), "wal", "the mode persists in the file's header");
    let store = Store::at(&dir).unwrap();
    assert_eq!(take(&store).unwrap(), LockState::Taken);
    assert_eq!(journal_of(&dir), "delete");
    let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, [regia_store::database_name()], "no WAL file is left beside the database");
    // And it holds: the next operation finds `delete` and has nothing to change.
    assert_eq!(take(&store).unwrap(), LockState::Renewed);
    assert_eq!(journal_of(&dir), "delete");
}

/// A read puts it back too: every way in goes through the same open.
#[test]
fn a_read_of_a_database_in_wal_puts_it_back_as_well() {
    let dir = folder("wal-read");
    put_in_wal(&dir);
    let store = Store::at(&dir).unwrap();
    store.rows().expect("rows");
    assert_eq!(journal_of(&dir), "delete");
}

/// SQLite cannot leave WAL while another connection has the database open in it: it answers with the mode it
/// is still in. That is an error of its own, not a store that works in a mode nobody checked.
#[test]
fn a_database_in_wal_that_another_connection_holds_is_an_error_not_a_success() {
    let dir = folder("wal-held");
    put_in_wal(&dir);
    let holder = Connection::open(dir.join(regia_store::database_name())).expect("open");
    assert_eq!(mode_of(&holder), "wal");
    holder.execute_batch("BEGIN; SELECT count(*) FROM sqlite_master;").expect("a reader inside WAL");
    let store = Store::at(&dir).unwrap().with_busy_wait(std::time::Duration::from_millis(200));
    match take(&store) {
        Err(StoreError::JournalMode { mode, .. }) => assert_eq!(mode, "wal"),
        other => panic!("expected the journal mode error, got {other:?}"),
    }
    holder.execute_batch("COMMIT").expect("end the read");
    drop(holder);
    assert_eq!(take(&store).unwrap(), LockState::Taken, "once nobody holds it, the next open puts it back");
    assert_eq!(journal_of(&dir), "delete");
}
