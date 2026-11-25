//! The prototype's cases that need a fault inside the database: `test_runtime_avversaria.py` patches
//! `sqlite3.connect` there, and the Rust store has a test-only injection point in its own `db::Database`
//! (`db::faults`, compiled only for tests) in its place. They live here, not under `tests/`, because that
//! point is not part of the crate's API: a test in `tests/` is built against the crate as a user sees it.
//!
//! The cases of the same file that need no fault are in `tests/state.rs` and `tests/stall.rs`.

use std::path::PathBuf;

use crate::{Holder, LeaseEntry, LockState, ProjectId, Store, StoreError, TaskNumber, Ttl};

fn store(label: &str) -> Store {
    // The unit test's executable is in `target/<profile>/deps`: a folder the temporary-folder refusal lets in.
    let exe = std::env::current_exe().expect("the test's executable");
    let dir: PathBuf = exe.parent().expect("deps folder").join("store-faults").join(label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    Store::at(dir).expect("store")
}

fn project() -> ProjectId {
    ProjectId::new("alpha").unwrap()
}

fn holder(id: &str) -> Holder {
    Holder::new(id).unwrap()
}

fn secs(secs: u64) -> Ttl {
    Ttl::from_secs(secs).unwrap()
}

fn held_ttl(store: &Store) -> Option<serde_json::Value> {
    let status = store.lock_status(&project()).expect("status");
    status.lock().and_then(|view| view.field(crate::ViewField::TtlS).cloned())
}

/// `test_errore_transitorio_di_apertura_viene_ritentato`: "unable to open database file" twice, as a file an
/// antivirus holds, and the operation still succeeds, on the third attempt.
#[test]
fn a_transient_error_at_open_is_retried() {
    let store = store("transient-open");
    assert_eq!(store.acquire_lock(&project(), &holder("a"), secs(60)).unwrap().state(), LockState::Taken);
    let before = store.db.faults().opens();
    store.db.faults().fail_next_opens(2);
    let renewed = store.renew_lock(&project(), &holder("a"), secs(120)).unwrap();
    assert_eq!(renewed.state(), LockState::Renewed);
    assert_eq!(store.db.faults().opens() - before, 3, "two failed attempts and the one that worked");
    assert_eq!(held_ttl(&store), Some(serde_json::json!(120)));
}

/// The same fault without end is the error of the store after its 40 attempts, not a loop and not a success.
#[test]
fn a_persistent_error_at_open_is_an_error_after_the_attempts() {
    let store = store("persistent-open");
    store.db.faults().fail_next_opens(1000);
    let error = store.acquire_lock(&project(), &holder("a"), secs(60)).expect_err("an error, not an outcome");
    assert!(matches!(error, StoreError::Unavailable { .. }), "{error:?}");
    assert_eq!(store.db.faults().opens(), 40, "the prototype's RETRY_IO");
}

/// `test_errore_persistente_di_commit_conserva_il_documento_precedente`: the COMMIT fails every time. The
/// renewal is an error, the document before it is still the stored one, and nothing is left in the folder
/// but the database.
#[test]
fn a_persistent_commit_error_keeps_the_previous_document() {
    let store = store("persistent-commit");
    assert_eq!(store.acquire_lock(&project(), &holder("a"), secs(60)).unwrap().state(), LockState::Taken);
    store.db.faults().fail_commits(true);
    let error = store.renew_lock(&project(), &holder("a"), secs(120)).expect_err("a failed commit is an error");
    assert!(matches!(error, StoreError::WriteFailed(_)), "{error:?}");
    store.db.faults().fail_commits(false);
    assert_eq!(held_ttl(&store), Some(serde_json::json!(60)));
    let status = store.lock_status(&project()).unwrap();
    assert_eq!(status.lock().and_then(|view| view.holder()), Some("a"));
    let mut left: Vec<String> =
        std::fs::read_dir(store.folder()).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, [crate::database_name()]);
}

/// `test_d3_scrittura_non_persistita_non_deve_confermare_una_lease`: the disk does not confirm the write. The
/// first holder is not told it holds the lease, and what failed leaves nothing that blocks the second.
#[test]
fn a_write_that_is_not_persisted_does_not_confirm_a_lease() {
    let store = store("not-persisted");
    let task = TaskNumber::new(12);
    store.db.faults().fail_commits(true);
    let first = store.acquire_lease(task, &project(), &holder("first"), secs(3600));
    assert!(first.as_ref().map_or(true, |outcome| !outcome.is_ok()), "success was declared for a lease never stored: {first:?}");
    store.db.faults().fail_commits(false);
    assert!(store.leases(false).unwrap().is_empty(), "the failed write left a row");
    let second = store.acquire_lease(task, &project(), &holder("second"), secs(3600)).unwrap();
    assert_eq!(second.state(), LockState::Taken, "the failed write must not block the next holder");
    let holders: Vec<Option<String>> = store
        .leases(false)
        .unwrap()
        .iter()
        .map(|entry| match entry {
            LeaseEntry::Lease(view) => view.holder().map(str::to_owned),
            LeaseEntry::Unreadable { .. } => None,
        })
        .collect();
    assert_eq!(holders, [Some("second".to_owned())]);
}

/// `test_d6_errore_dopo_il_commit_non_deve_trasformare_successo_in_errore`: closing the connection raises after
/// the commit. What is committed is a success, whatever the close says.
#[test]
fn an_error_after_the_commit_does_not_turn_the_success_into_an_error() {
    let store = store("after-commit");
    assert_eq!(store.acquire_lock(&project(), &holder("a"), secs(60)).unwrap().state(), LockState::Taken);
    store.db.faults().fail_closes(true);
    let renewed = store.renew_lock(&project(), &holder("a"), secs(120)).expect("the commit happened: not an error");
    assert!(renewed.is_ok(), "{renewed:?}");
    assert_eq!(renewed.state(), LockState::Renewed);
    store.db.faults().fail_closes(false);
    assert_eq!(held_ttl(&store), Some(serde_json::json!(120)));
}
