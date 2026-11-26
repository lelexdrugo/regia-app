//! The operations of a lock's holder (#468): each checks, in the transaction that reads and writes, that the lock
//! is still the holder's (the shape of `step start` for D3, and of the dossier's #507 for the steps registry).
//! Every state folder is a fresh one under `target/`.

use std::path::PathBuf;

use regia_store::{CursorName, Fenced, Holder, ManualClock, ProjectId, Store, Ttl, WallTime};
use serde_json::{Value, json};

/// 2026-01-10T09:00:00Z, the instant of the fixtures.
const T0_MICROS: i64 = 1_768_035_600_000_000;

fn store(label: &str) -> (Store, ManualClock) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-held").join(label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    let clock = ManualClock::new(WallTime::from_unix_micros(T0_MICROS).unwrap(), 1000.0, "rust/manual");
    (Store::at(dir).unwrap().with_clock(clock.clone()), clock)
}

fn ids() -> (ProjectId, ProjectId, CursorName) {
    (ProjectId::new("controller").unwrap(), ProjectId::new("alpha").unwrap(), CursorName::new("signal").unwrap())
}

fn take(store: &Store, lock: &ProjectId, holder: &str) -> Holder {
    let holder = Holder::new(holder).unwrap();
    assert!(store.acquire_lock(lock, &holder, Ttl::from_secs(600).unwrap()).unwrap().is_ok());
    holder
}

#[test]
fn the_holder_reads_and_writes_and_sees_the_stored_value() {
    let (store, _) = store("writes");
    let (lock, project, name) = ids();
    let holder = take(&store, &lock, "c1");
    let held = store.as_holder(&lock, &holder);
    assert_eq!(held.cursor(&project, &name).unwrap(), Fenced::Done(Value::Null), "no row: null");
    let seen = held.write_cursor(&project, &name, |stored| (Some(json!({"n": 1})), stored)).unwrap();
    assert_eq!(seen, Fenced::Done(Value::Null));
    let seen = held.write_cursor(&project, &name, |stored| (Some(json!({"n": stored["n"].as_i64().unwrap() + 1})), stored)).unwrap();
    assert_eq!(seen, Fenced::Done(json!({"n": 1})));
    assert_eq!(store.cursor(&project, &name).unwrap(), json!({"n": 2}));
    let untouched = held.write_cursor(&project, &name, |_| (None, ())).unwrap();
    assert_eq!((untouched, store.cursor(&project, &name).unwrap()), (Fenced::Done(()), json!({"n": 2})), "None writes nothing");
}

#[test]
fn another_holder_s_lock_or_none_writes_nothing() {
    let (store, clock) = store("lost");
    let (lock, project, name) = ids();
    let first = take(&store, &lock, "c1");
    store.write_cursor(&project, &name, json!("before")).unwrap();
    clock.advance(601.0);
    let thief = take(&store, &lock, "thief");
    let mut ran = false;
    let outcome = store
        .as_holder(&lock, &first)
        .write_cursor(&project, &name, |_| {
            ran = true;
            (Some(json!("after")), ())
        })
        .unwrap();
    assert_eq!(outcome, Fenced::LockLost { holder: Some(json!("thief")) });
    assert!(!ran, "nothing is read for a holder that lost its lock");
    assert_eq!(store.cursor(&project, &name).unwrap(), json!("before"));
    assert_eq!(store.as_holder(&lock, &first).cursor(&project, &name).unwrap(), Fenced::LockLost { holder: Some(json!("thief")) });

    assert!(store.release_lock(&lock, &thief).unwrap().is_ok());
    let outcome = store.as_holder(&lock, &first).write_cursor(&project, &name, |_| (Some(json!("after")), ())).unwrap();
    assert_eq!(outcome, Fenced::LockLost { holder: None }, "free is not the holder's either");
    assert_eq!(store.cursor(&project, &name).unwrap(), json!("before"));
}

/// The owner is checked, not the expiry: a lock that expired and that nobody took is still the holder's.
#[test]
fn an_expired_lock_nobody_took_is_still_the_holder_s() {
    let (store, clock) = store("expired");
    let (lock, project, name) = ids();
    let holder = take(&store, &lock, "c1");
    clock.advance(3600.0);
    assert!(matches!(store.lock_status(&lock).unwrap(), regia_store::LockStatus::Expired(_)));
    let outcome = store.as_holder(&lock, &holder).write_cursor(&project, &name, |_| (Some(json!(1)), ())).unwrap();
    assert_eq!(outcome, Fenced::Done(()));
    assert_eq!(store.cursor(&project, &name).unwrap(), json!(1));
}
