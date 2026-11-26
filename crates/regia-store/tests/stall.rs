//! A holder that stalls halfway through a mutation, against the prototype's case of the same name (D1 of
//! `test_runtime_avversaria.py`, the dossier's adversarial review #12): one contender reads the expired
//! document and stops inside its transaction, the other arrives, and between them there must be one winner.
//!
//! The prototype stops the first one in its clock: `_adesso` is the first call after the document is read and
//! before it is written, so a patched `_adesso` that waits holds the transaction open. The Rust store reads its
//! clock at the same point (`Store::transition` calls `Clock::now` after `select`), so a clock that waits there
//! does the same, with no fault inside the database.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use regia_store::{Clock, Holder, LeaseEntry, LockOutcome, LockState, LockStatus, ProjectId, Store, SystemClock, TaskNumber, Ttl, WallTime};

/// The system's clock, except that the named thread stops at its first reading until it is told to go on.
#[derive(Debug)]
struct StallingClock {
    stalls: &'static str,
    stalled: AtomicBool,
    reached: Mutex<Sender<()>>,
    go_on: Mutex<Receiver<()>>,
}

impl Clock for StallingClock {
    fn now(&self) -> WallTime {
        let me = thread::current();
        if me.name() == Some(self.stalls) && !self.stalled.swap(true, Ordering::SeqCst) {
            self.reached.lock().unwrap().send(()).expect("the test is listening");
            self.go_on.lock().unwrap().recv_timeout(Duration::from_secs(5)).expect("the second contender did not finish");
        }
        SystemClock.now()
    }

    fn monotonic(&self) -> f64 {
        SystemClock.monotonic()
    }

    fn tag(&self) -> std::borrow::Cow<'static, str> {
        SystemClock.tag()
    }
}

fn folder(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-stall").join(label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    dir
}

fn project() -> ProjectId {
    ProjectId::new("alpha").unwrap()
}

fn holder(id: &str) -> Holder {
    Holder::new(id).unwrap()
}

/// Runs `first` on a thread that stalls inside its transaction, then `second` on this one with a short wait,
/// then lets the first go on. Returns what each saw.
fn stalled_race(
    label: &str,
    seed: impl FnOnce(&Store),
    first: impl FnOnce(&Store) -> LockOutcome + Send + 'static,
    second: impl FnOnce(&Store) -> LockOutcome,
) -> (LockOutcome, LockOutcome, Store) {
    let dir = folder(label);
    let plain = Store::at(&dir).unwrap();
    seed(&plain);
    // As the prototype's seeding helper: the document of a holder who is gone, expired for good.
    thread::sleep(Duration::from_millis(250));
    let (reached, wait_reached) = mpsc::channel();
    let (go_on, wait_go_on) = mpsc::channel();
    let stalling = StallingClock {
        stalls: "first-stalled",
        stalled: AtomicBool::new(false),
        reached: Mutex::new(reached),
        go_on: Mutex::new(wait_go_on),
    };
    let store_of_first = Arc::new(Store::at(&dir).unwrap().with_clock(stalling));
    let handle = thread::Builder::new()
        .name("first-stalled".to_owned())
        .spawn(move || first(&store_of_first))
        .expect("spawn");
    wait_reached.recv_timeout(Duration::from_secs(5)).expect("the first did not reach the stall inside its transaction");
    let other = Store::at(&dir).unwrap().with_busy_wait(Duration::from_millis(300));
    let second = second(&other);
    go_on.send(()).expect("the first is waiting");
    let first = handle.join().expect("the first thread");
    (first, second, plain)
}

fn winners(outcomes: &[&LockOutcome]) -> usize {
    outcomes.iter().filter(|outcome| outcome.is_ok()).count()
}

/// `test_d1_lock_…_non_deve_dare_due_vincitori` of `test_runtime_avversaria.py`. The sources may not hold the
/// Italian word of the name's middle (a guard of `regia-conformance`), so it is left out here.
#[test]
fn a_stalled_contender_on_a_lock_does_not_give_two_winners() {
    let ttl = Ttl::from_secs(60).unwrap();
    let (first, second, store) = stalled_race(
        "lock",
        |store| {
            store.acquire_lock(&project(), &holder("original"), Ttl::from_secs_f64(0.05).unwrap()).unwrap();
        },
        move |store| store.acquire_lock(&project(), &holder("first"), ttl).unwrap(),
        |store| store.acquire_lock(&project(), &holder("second"), ttl).unwrap(),
    );
    assert_eq!(winners(&[&first, &second]), 1, "{first:?} {second:?}");
    assert_eq!(first.state(), LockState::Stolen, "the one that held the write lock wins: {first:?}");
    assert_eq!(second.state(), LockState::Busy, "the other waits its turn and is refused: {second:?}");
    assert!(matches!(second, LockOutcome::Contended { .. }), "it gave up on the busy database, it did not see a lock: {second:?}");
    match store.lock_status(&project()).unwrap() {
        LockStatus::Held(view) => assert_eq!(view.holder(), Some("first")),
        other => panic!("{other:?}"),
    }
}

/// `test_d1_lease_…_non_deve_perdere_la_lease_del_subentrante`, same file, same guard.
#[test]
fn a_stalled_contender_on_a_lease_does_not_lose_the_lease_of_the_one_that_took_over() {
    let ttl = Ttl::from_secs(3600).unwrap();
    let task = TaskNumber::new(12);
    let (first, second, store) = stalled_race(
        "lease",
        move |store| {
            store.acquire_lease(task, &project(), &holder("original"), Ttl::from_secs_f64(0.05).unwrap()).unwrap();
        },
        move |store| store.acquire_lease(task, &project(), &holder("first"), ttl).unwrap(),
        move |store| store.acquire_lease(task, &project(), &holder("second"), ttl).unwrap(),
    );
    assert_eq!(winners(&[&first, &second]), 1, "{first:?} {second:?}");
    assert_eq!(first.state(), LockState::Stolen, "{first:?}");
    assert_eq!(second.state(), LockState::Busy, "{second:?}");
    let held: Vec<Option<String>> = store
        .leases(false)
        .unwrap()
        .iter()
        .map(|entry| match entry {
            LeaseEntry::Lease(view) => view.holder().map(str::to_owned),
            LeaseEntry::Unreadable { .. } => None,
        })
        .collect();
    assert_eq!(held, [Some("first".to_owned())]);
}
