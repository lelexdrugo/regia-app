//! Leases, cursors and the orchestrator's step against the prototype's cases (`tools/tests/test_runtime.py`
//! on leases and cursors, its tests of the step's start and end, D3 of #417), on a manual clock. Every
//! state folder is a fresh one under `target/`, never the live one. The conformance tests name those
//! cases one by one.

use std::collections::BTreeMap;
use std::path::PathBuf;

use regia_store::{
    CursorName, EventsCursor, EventsParts, Holder, LeaseEntry, LockOutcome, LockState, ManualClock, OwnComment, OwnParts, ProjectId,
    StepClosing, StepOpening, Store, StoreError, TaskNumber, Ttl, WallTime, generated_holder, reports_folder, step_commit_message,
};
use serde_json::{Value, json};

/// 2026-01-10T09:00:00Z, the instant of the fixtures.
const T0_MICROS: i64 = 1_768_035_600_000_000;
const T0: &str = "2026-01-10T09:00:00Z";

fn folder(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-state").join(label);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    dir
}

fn store(label: &str) -> (Store, ManualClock) {
    let clock = ManualClock::new(WallTime::from_unix_micros(T0_MICROS).unwrap(), 1000.0, "rust/manual");
    (Store::at(folder(label)).unwrap().with_clock(clock.clone()), clock)
}

fn project(id: &str) -> ProjectId {
    ProjectId::new(id).unwrap()
}

fn holder(id: &str) -> Holder {
    Holder::new(id).unwrap()
}

fn secs(secs: u64) -> Ttl {
    Ttl::from_secs(secs).unwrap()
}

fn task(number: u64) -> TaskNumber {
    TaskNumber::new(number)
}

/// The names of format v1, as the rows hold them.
fn v1() -> toml::Table {
    toml::from_str(include_str!("../formats/store_v1.toml")).unwrap()
}

fn name(section: &str, key: &str) -> String {
    v1()[section][key].as_str().unwrap().to_owned()
}

/// A row written as another process left it: `text` as the document, with no stamp.
fn raw_row(store: &Store, kind: &str, key: &str, text: &str) {
    // The store creates its table when it first opens the database.
    store.rows().unwrap();
    let format = v1();
    let column = |name: &str| format["table"][name].as_str().unwrap().to_owned();
    let [table, kind_column, key_column, generation, document] = ["name", "kind", "key", "generation", "document"].map(column);
    let conn = rusqlite::Connection::open(store.database()).unwrap();
    conn.execute(
        &format!("INSERT INTO {table} ({kind_column}, {key_column}, {generation}, {document}) VALUES (?1, ?2, 1, ?3)"),
        (&name("kinds", kind), key, text),
    )
    .unwrap();
}

fn view_holder(outcome: &LockOutcome) -> Option<String> {
    outcome.lock().and_then(|view| view.holder().map(str::to_owned))
}

fn lease_tasks(entries: &[LeaseEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| match entry {
            LeaseEntry::Lease(view) => view.task().cloned().unwrap_or(Value::Null),
            LeaseEntry::Unreadable { task } => task.clone(),
        })
        .collect()
}

/// The key of the step cursor's row of `project`.
fn step_key(project: &str) -> String {
    format!("{project}:{}", name("cursor", "step"))
}

/// A step cursor's document as the prototype writes it, with the fields given (names of format v1).
fn step_row(fields: &[(&str, Value)]) -> String {
    let value: serde_json::Map<String, Value> = fields.iter().map(|(key, value)| (name("step", key), value.clone())).collect();
    json!({name("cursor", "value"): value, name("cursor", "updated"): "x"}).to_string()
}

/// The stored document of a row, parsed.
fn stored(store: &Store, kind: &str, key: &str) -> Option<Value> {
    let kind = name("kinds", kind);
    store.rows().unwrap().into_iter().find(|row| row.kind == kind && row.key == key).map(|row| serde_json::from_str(&row.document).unwrap())
}

#[test]
fn a_heartbeat_extends_the_lease() {
    let (store, clock) = store("heartbeat");
    let taken = store.acquire_lease(task(7), &project("alpha"), &holder("w1"), secs(30)).unwrap();
    assert_eq!(taken.state(), LockState::Taken);
    let before = taken.lock().unwrap().expires_at().unwrap().to_owned();
    clock.advance(0.05);
    let beat = store.heartbeat_lease(task(7), &holder("w1"), None).unwrap();
    assert_eq!((beat.state(), beat.exit_code()), (LockState::Heartbeat, 0));
    let after = beat.lock().unwrap().expires_at().unwrap().to_owned();
    assert!(WallTime::parse_iso(&after) > WallTime::parse_iso(&before), "{after} after {before}");
    assert_eq!(beat.lock().unwrap().heartbeat_at(), Some("2026-01-10T09:00:00.050+00:00"));
    // Without a ttl, the one it was taken with, in the form it was stored.
    assert_eq!(beat.lock().unwrap().fields()["ttl_s"], json!(30));
    assert!(store.leases(true).unwrap().is_empty());
}

#[test]
fn an_expired_lease_lists_with_the_expired_and_every_lease_without_the_filter() {
    let (store, clock) = store("expired-listed");
    store.acquire_lease(task(9), &project("alpha"), &holder("gone"), secs(60)).unwrap();
    clock.advance(120.0);
    store.acquire_lease(task(10), &project("alpha"), &holder("alive"), secs(60)).unwrap();
    assert_eq!(lease_tasks(&store.leases(true).unwrap()), [json!(9)]);
    assert_eq!(lease_tasks(&store.leases(false).unwrap()), [json!(9), json!(10)]);
}

#[test]
fn a_lease_expires_in_time_and_passes_to_another() {
    let (store, clock) = store("expires");
    store.acquire_lease(task(11), &project("alpha"), &holder("w1"), Ttl::from_secs_f64(0.2).unwrap()).unwrap();
    clock.advance(0.4);
    assert_eq!(lease_tasks(&store.leases(true).unwrap()), [json!(11)]);
    assert!(!store.heartbeat_lease(task(11), &holder("w2"), None).unwrap().is_ok());
    let stolen = store.acquire_lease(task(11), &project("alpha"), &holder("w2"), secs(60)).unwrap();
    assert_eq!(stolen.state(), LockState::Stolen);
    assert_eq!(stolen.lock().unwrap().replaced(), Some(&json!("w1")));
    assert_eq!(stolen.lock().unwrap().stolen_at(), Some("2026-01-10T09:00:00.400+00:00"));
}

#[test]
fn a_task_has_one_valid_lease_and_another_holder_neither_beats_nor_releases_it() {
    let (store, _) = store("one-lease");
    assert!(store.acquire_lease(task(5), &project("alpha"), &holder("w1"), secs(60)).unwrap().is_ok());
    let refused = store.acquire_lease(task(5), &project("alpha"), &holder("w2"), secs(60)).unwrap();
    assert_eq!((refused.state(), refused.exit_code(), view_holder(&refused).as_deref()), (LockState::Busy, 3, Some("w1")));
    for outcome in [store.heartbeat_lease(task(5), &holder("w2"), None).unwrap(), store.release_lease(task(5), &holder("w2")).unwrap()] {
        assert_eq!((outcome.state(), view_holder(&outcome).as_deref()), (LockState::Foreign, Some("w1")));
    }
    assert!(stored(&store, "lease", "5").is_some());
    assert_eq!(store.release_lease(task(5), &holder("w1")).unwrap().state(), LockState::Released);
    assert!(stored(&store, "lease", "5").is_none());
    // Nothing to release is already free; nothing to beat is absent, a refusal.
    assert_eq!(store.release_lease(task(5), &holder("w1")).unwrap().state(), LockState::Free);
    let absent = store.heartbeat_lease(task(5), &holder("w1"), None).unwrap();
    assert_eq!((absent.state(), absent.exit_code()), (LockState::Absent, 3));
}

/// The prototype renews its own live lease with a heartbeat: `heartbeat_at` moves, `renewed_at` is never
/// written, and the lease moves to the project given.
#[test]
fn renewing_one_s_own_lease_beats_and_moves_it_to_the_project() {
    let (store, clock) = store("own-renew");
    store.acquire_lease(task(7), &project("alpha"), &holder("w1"), secs(900)).unwrap();
    clock.advance(60.0);
    let renewed = store.acquire_lease(task(7), &project("beta"), &holder("w1"), Ttl::from_secs_f64(300.0).unwrap()).unwrap();
    assert_eq!(renewed.state(), LockState::Renewed);
    let doc = stored(&store, "lease", "7").unwrap();
    assert_eq!(doc[name("lease", "project")], json!("beta"));
    assert_eq!(doc[name("lease", "taken_at")], json!("2026-01-10T09:00:00.000+00:00"));
    assert_eq!(doc[name("lease", "heartbeat_at")], json!("2026-01-10T09:01:00.000+00:00"));
    assert_eq!(doc[name("lease", "ttl_s")], json!(300.0), "a ttl given on the command line is a float");
    assert!(doc.get(name("lock", "renewed_at")).is_none());
    assert_eq!(doc[name("lease", "task")], json!(7), "the task is a JSON integer, which the precheck matches");
}

/// A heartbeat with no ttl given and none stored to extend the lease by is refused as someone else's.
#[test]
fn a_heartbeat_with_no_ttl_to_extend_by_is_refused() {
    let (store, _) = store("no-ttl");
    let lease = json!({
        name("lease", "task"): 8,
        name("lease", "project"): "alpha",
        name("lease", "holder"): "w1",
        name("lease", "expires_at"): "2099-01-01T00:00:00.000+00:00",
    });
    raw_row(&store, "lease", "8", &lease.to_string());
    let refused = store.heartbeat_lease(task(8), &holder("w1"), None).unwrap();
    assert_eq!((refused.state(), refused.exit_code()), (LockState::Foreign, 3));
    let beat = store.heartbeat_lease(task(8), &holder("w1"), Some(secs(60))).unwrap();
    assert_eq!(beat.state(), LockState::Heartbeat);
}

/// `elenca`: by the length of the key, then the key; a row that is not a JSON object lists as unreadable
/// and expired, under its key read as a number when it is digits.
#[test]
fn leases_list_by_number_and_an_unreadable_row_as_expired() {
    let (store, _) = store("listed");
    for number in [30, 7, 120] {
        store.acquire_lease(task(number), &project("alpha"), &holder("w1"), secs(600)).unwrap();
    }
    raw_row(&store, "lease", "12", "not an object");
    raw_row(&store, "lease", "x9", "[]");
    let all = store.leases(false).unwrap();
    assert_eq!(lease_tasks(&all), [json!(7), json!(12), json!(30), json!("x9"), json!(120)]);
    assert_eq!(lease_tasks(&store.leases(true).unwrap()), [json!(12), json!("x9")]);
    assert!(all.iter().filter(|entry| matches!(entry, LeaseEntry::Unreadable { .. })).all(LeaseEntry::is_expired));
}

/// `test_riavvio_la_lease_di_un_boot_precedente_e_scaduta`: a monotonic clock behind the stamp is a
/// restart, and the lease of the boot before is expired, whatever the wall clock says.
#[test]
fn a_lease_of_a_previous_boot_is_expired() {
    let (store, clock) = store("reboot");
    store.acquire_lease(task(4), &project("alpha"), &holder("a"), secs(3600)).unwrap();
    clock.set_monotonic(5.0);
    assert_eq!(lease_tasks(&store.leases(true).unwrap()), [json!(4)]);
    assert_eq!(store.acquire_lease(task(4), &project("alpha"), &holder("b"), secs(60)).unwrap().state(), LockState::Stolen);
}

#[test]
fn cursors_read_null_until_written_and_each_name_is_a_row_of_its_own() {
    let (store, _) = store("cursors");
    let events = CursorName::events();
    let detector = CursorName::new("detector").unwrap();
    assert_eq!(store.cursor(&project("alpha"), &events).unwrap(), Value::Null);
    store.write_cursor(&project("alpha"), &detector, json!({"n": 1})).unwrap();
    assert_eq!(store.cursor(&project("alpha"), &events).unwrap(), Value::Null, "events is not touched");
    store.write_cursor(&project("alpha"), &events, json!("t0")).unwrap();
    store.write_cursor(&project("alpha"), &detector, json!({"n": 2})).unwrap();
    store.write_cursor(&project("beta"), &events, json!("x")).unwrap();
    assert_eq!(store.cursor(&project("alpha"), &events).unwrap(), json!("t0"));
    assert_eq!(store.cursor(&project("alpha"), &detector).unwrap(), json!({"n": 2}));
    assert_eq!(store.cursor(&project("beta"), &events).unwrap(), json!("x"));
    assert_eq!(store.cursor(&project("beta"), &detector).unwrap(), Value::Null);
    // The events cursor keeps the key it always had; another name is `<project>:<name>`.
    let keys: Vec<String> = store.rows().unwrap().into_iter().map(|row| row.key).collect();
    assert_eq!(keys, ["alpha", "alpha:detector", "beta"]);
    let doc = stored(&store, "cursor", "alpha").unwrap();
    assert_eq!(doc, json!({name("cursor", "value"): "t0", name("cursor", "updated"): "2026-01-10T09:00:00.000+00:00"}));
    for bad in ["", "A", "a:b", "a b", "-a"] {
        assert!(CursorName::new(bad).is_err(), "{bad:?}");
    }
}

/// `test_prende_il_lock_e_registra_t`: a new step began to observe at the instant it is given, to the
/// second, with no comments, and names the session that opened it.
#[test]
fn opening_a_step_records_its_instant_and_no_comments() {
    let (store, clock) = store("open");
    let t = WallTime::from_unix_micros(T0_MICROS + 999_999).unwrap();
    clock.advance(2.0);
    store.acquire_lock(&project("alpha"), &holder("w1"), secs(1800)).unwrap();
    let StepOpening::Opened(step) = store.open_step(&project("alpha"), &holder("w1"), Some("s-1"), t).unwrap() else { panic!("not opened") };
    assert_eq!((step.t(), step.resumed()), (&json!(T0), false), "truncated to the second");
    let doc = stored(&store, "cursor", &step_key("alpha")).unwrap();
    let value = &doc[name("cursor", "value")];
    assert_eq!(value[name("step", "version")], json!(1));
    assert_eq!(value[name("step", "holder")], json!("w1"));
    assert_eq!(value[name("step", "session")], json!("s-1"));
    assert_eq!(value[name("step", "comments")], json!([]));
    assert_eq!(doc[name("cursor", "updated")], json!("2026-01-10T09:00:02.000+00:00"));
    assert_eq!(store.events_cursor(&project("alpha")).unwrap(), EventsCursor::Absent, "the events cursor is not touched");
}

/// `test_ripresa_dello_stesso_proprietario_tiene_t_e_commenti` and
/// `test_un_altro_proprietario_dopo_la_scadenza_riparte_da_capo`.
#[test]
fn the_holder_s_own_step_resumes_and_another_s_starts_anew() {
    let (store, clock) = store("resume");
    let step_key = step_key("alpha");
    let row = step_row(&[
        ("version", json!(1)),
        ("holder", json!("w1")),
        ("session", Value::Null),
        ("t", json!("2026-01-10T08:30:00Z")),
        ("comments", json!([43, 41])),
    ]);
    raw_row(&store, "cursor", &step_key, &row);
    store.acquire_lock(&project("alpha"), &holder("w1"), secs(1800)).unwrap();
    let StepOpening::Opened(step) = store.open_step(&project("alpha"), &holder("w1"), None, store.now()).unwrap() else { panic!() };
    assert_eq!((step.t(), step.resumed()), (&json!("2026-01-10T08:30:00Z"), true));
    let value = stored(&store, "cursor", &step_key).unwrap()[name("cursor", "value")].clone();
    assert_eq!(value[name("step", "comments")], json!([43, 41]), "the comments are kept as they are");
    // Resumed by another session: the step names the last one.
    store.open_step(&project("alpha"), &holder("w1"), Some("s-2"), store.now()).unwrap();
    assert_eq!(stored(&store, "cursor", &step_key).unwrap()[name("cursor", "value")][name("step", "session")], json!("s-2"));
    clock.advance(3600.0);
    assert_eq!(store.acquire_lock(&project("alpha"), &holder("w2"), secs(1800)).unwrap().state(), LockState::Stolen);
    let StepOpening::Opened(step) = store.open_step(&project("alpha"), &holder("w2"), None, store.now()).unwrap() else { panic!() };
    assert_eq!((step.t(), step.resumed()), (&json!("2026-01-10T10:00:00Z"), false));
    let value = stored(&store, "cursor", &step_key).unwrap()[name("cursor", "value")].clone();
    assert_eq!((value[name("step", "holder")].clone(), value[name("step", "comments")].clone()), (json!("w2"), json!([])));
}

/// A step stored with no session, opened again with none: the key stays missing, as the prototype, which
/// compares the stored session, missing or not, with none, leaves it.
#[test]
fn a_step_with_no_session_gets_no_session_key() {
    let (store, _) = store("no-session-key");
    let step_key = step_key("alpha");
    let fields = [("version", json!(1)), ("holder", json!("w1")), ("t", json!("2026-01-10T08:30:00Z"))];
    raw_row(&store, "cursor", &step_key, &step_row(&fields));
    store.acquire_lock(&project("alpha"), &holder("w1"), secs(1800)).unwrap();
    store.open_step(&project("alpha"), &holder("w1"), None, store.now()).unwrap();
    let value = stored(&store, "cursor", &step_key).unwrap()[name("cursor", "value")].clone();
    let expected: serde_json::Map<String, Value> = fields.iter().map(|(key, value)| (name("step", key), value.clone())).collect();
    assert_eq!(value, Value::Object(expected));
}

/// D3 of #417: the lock expires and passes to another between taking it and writing the step. The step
/// is refused, with who holds the lock now, and no cursor is written: neither a new one nor over the other
/// holder's step.
#[test]
fn a_lock_lost_before_the_step_is_written_leaves_every_cursor_as_it_was() {
    let (store, clock) = store("d3");
    store.acquire_lock(&project("alpha"), &holder("w1"), secs(60)).unwrap();
    clock.advance(61.0);
    store.acquire_lock(&project("alpha"), &holder("other"), secs(1800)).unwrap();
    assert_eq!(
        store.open_step(&project("alpha"), &holder("w1"), None, store.now()).unwrap(),
        StepOpening::LockLost { holder: Some(json!("other")) }
    );
    assert!(stored(&store, "cursor", &step_key("alpha")).is_none(), "no step written");
    // The other holder's step stays as it was, byte for byte.
    let StepOpening::Opened(_) = store.open_step(&project("alpha"), &holder("other"), None, store.now()).unwrap() else { panic!() };
    let before = store.rows().unwrap();
    assert!(matches!(store.open_step(&project("alpha"), &holder("w1"), None, store.now()).unwrap(), StepOpening::LockLost { .. }));
    assert_eq!(store.rows().unwrap(), before);
    // Released, the lock is nobody's.
    store.release_lock(&project("alpha"), &holder("other")).unwrap();
    assert_eq!(store.open_step(&project("alpha"), &holder("w1"), None, store.now()).unwrap(), StepOpening::LockLost { holder: None });
}

/// The cursor with `t` and the step's own comments, once each; the comments of a step before do not carry
/// over; and the reports of #406.
#[test]
fn closing_writes_the_events_cursor_and_empties_the_step() {
    let (store, clock) = store("close");
    let step_key = step_key("alpha");
    let row = step_row(&[
        ("version", json!(1)),
        ("holder", json!("w1")),
        ("session", Value::Null),
        ("t", json!("2026-01-10T08:30:00Z")),
        ("comments", json!([43, 41, 43])),
    ]);
    raw_row(&store, "cursor", &step_key, &row);
    clock.advance(5.0);
    let reports = BTreeMap::from([("2026-01-09-3-done.md".to_owned(), "2026-01-10T08:00:00.250000+00:00".to_owned())]);
    let StepClosing::Closed(cursor) = store.close_step(&project("alpha"), &holder("w1"), &reports).unwrap() else { panic!("not closed") };
    let expected = EventsCursor::Parts(EventsParts {
        version: Some(json!(1)),
        t: Some(json!("2026-01-10T08:30:00Z")),
        own: Some(OwnParts {
            comments: Some(json!([41, 43])),
            reports: Some(json!({"2026-01-09-3-done.md": "2026-01-10T08:00:00.250000+00:00"})),
        }),
    });
    assert_eq!(cursor, expected);
    assert_eq!(store.events_cursor(&project("alpha")).unwrap(), expected);
    let events = stored(&store, "cursor", "alpha").unwrap();
    assert_eq!(events[name("cursor", "updated")], json!("2026-01-10T09:00:05.000+00:00"));
    let step = stored(&store, "cursor", &step_key).unwrap();
    assert_eq!(step, json!({name("cursor", "value"): null, name("cursor", "updated"): "2026-01-10T09:00:05.000+00:00"}));
    // Closed once: a second close has no step, and writes nothing.
    let before = store.rows().unwrap();
    assert_eq!(store.close_step(&project("alpha"), &holder("w1"), &BTreeMap::new()).unwrap(), StepClosing::NoStep);
    assert_eq!(store.rows().unwrap(), before);
    // A new step starts with no comments, and with no reports the cursor has no field for them.
    store.acquire_lock(&project("alpha"), &holder("w1"), secs(1800)).unwrap();
    store.open_step(&project("alpha"), &holder("w1"), None, store.now()).unwrap();
    let StepClosing::Closed(cursor) = store.close_step(&project("alpha"), &holder("w1"), &BTreeMap::new()).unwrap() else { panic!() };
    let EventsCursor::Parts(parts) = cursor else { panic!() };
    assert_eq!(parts.own, Some(OwnParts { comments: Some(json!([])), reports: None }));
    assert_eq!(parts.t, Some(json!("2026-01-10T09:00:05Z")));
}

/// A lock taken by hand with no step, another holder's step, one with no instant, and projects that are
/// independent.
#[test]
fn a_close_with_no_step_of_the_holder_writes_nothing() {
    let (store, _) = store("no-step");
    assert_eq!(store.close_step(&project("alpha"), &holder("w1"), &BTreeMap::new()).unwrap(), StepClosing::NoStep);
    store.acquire_lock(&project("alpha"), &holder("w2"), secs(1800)).unwrap();
    store.open_step(&project("alpha"), &holder("w2"), None, store.now()).unwrap();
    raw_row(&store, "cursor", &step_key("beta"), &step_row(&[("holder", json!("w1")), ("t", json!(""))]));
    let before = store.rows().unwrap();
    for project_id in ["alpha", "beta", "gamma"] {
        assert_eq!(store.close_step(&project(project_id), &holder("w1"), &BTreeMap::new()).unwrap(), StepClosing::NoStep, "{project_id}");
    }
    assert_eq!(store.rows().unwrap(), before);
    assert!(matches!(store.close_step(&project("alpha"), &holder("w2"), &BTreeMap::new()).unwrap(), StepClosing::Closed(_)));
    assert_eq!(store.events_cursor(&project("beta")).unwrap(), EventsCursor::Absent, "projects are independent");
}

/// Comments a step records are numbers; anything else is refused before anything is written.
#[test]
fn a_step_whose_comments_are_not_numbers_is_refused() {
    let (store, _) = store("bad-comments");
    raw_row(&store, "cursor", &step_key("alpha"), &step_row(&[("holder", json!("w1")), ("t", json!("x")), ("comments", json!([1, "a"]))]));
    let before = store.rows().unwrap();
    assert!(matches!(store.close_step(&project("alpha"), &holder("w1"), &BTreeMap::new()), Err(StoreError::StepComments)));
    assert_eq!(store.rows().unwrap(), before);
}

/// The forms of the events cursor, as the precheck reads them.
#[test]
fn the_events_cursor_reads_in_each_form() {
    let (store, _) = store("events-forms");
    let events = CursorName::events();
    store.write_cursor(&project("alpha"), &events, json!("2026-01-09T18:00:00Z")).unwrap();
    assert_eq!(store.events_cursor(&project("alpha")).unwrap(), EventsCursor::Instant("2026-01-09T18:00:00Z".to_owned()));
    store.write_cursor(&project("alpha"), &events, json!({"t": "x"})).unwrap();
    assert_eq!(
        store.events_cursor(&project("alpha")).unwrap(),
        EventsCursor::Parts(EventsParts { version: None, t: Some(json!("x")), own: None })
    );
    store.write_cursor(&project("alpha"), &events, json!([1])).unwrap();
    assert_eq!(store.events_cursor(&project("alpha")).unwrap(), EventsCursor::Other(json!([1])));
}

#[test]
fn a_generated_holder_is_the_prefix_and_twelve_hex_digits() {
    let prefix = name("step", "generated_holder");
    let (a, b) = (generated_holder(), generated_holder());
    for id in [a.as_str(), b.as_str()] {
        let digits = id.strip_prefix(&prefix).unwrap_or_else(|| panic!("{id}"));
        assert!(digits.len() == 12 && digits.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()), "{id}");
    }
    assert_ne!(a, b);
}

#[test]
fn the_step_s_commit_and_its_reports_are_where_the_prototype_has_them() {
    let message = step_commit_message(&project("alpha"), &json!(T0));
    assert_eq!(message, format!("alpha: {} {T0}", name("step", "commit_word")));
    let folder = reports_folder(std::path::Path::new("root"));
    assert_eq!(folder, std::path::Path::new("root").join(".regia").join("report"));
}

/// The prototype's contention on a lease, with a connection per writer on the system's clock: eight
/// writers at once on a free lease, one wins; six on an expired one, one steals it.
#[test]
fn one_writer_wins_a_lease_and_one_thief_steals_an_expired_one() {
    let dir = folder("contention");
    let race = |task_number: u64, writers: usize, prefix: &'static str| {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(writers));
        let handles: Vec<_> = (0..writers)
            .map(|i| {
                let (barrier, dir) = (barrier.clone(), dir.clone());
                std::thread::spawn(move || {
                    let store = Store::at(dir).unwrap();
                    barrier.wait();
                    let outcome = store.acquire_lease(task(task_number), &project("alpha"), &holder(&format!("{prefix}{i}")), secs(60)).unwrap();
                    (outcome.state(), format!("{prefix}{i}"))
                })
            })
            .collect();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
    };
    let outcomes = race(9999, 8, "w");
    let winners: Vec<&String> = outcomes.iter().filter(|(state, _)| *state == LockState::Taken).map(|(_, who)| who).collect();
    assert_eq!(winners.len(), 1, "{outcomes:?}");
    assert!(outcomes.iter().all(|(state, _)| matches!(state, LockState::Taken | LockState::Busy)), "{outcomes:?}");
    let store = Store::at(&dir).unwrap();
    store.acquire_lease(task(42), &project("alpha"), &holder("gone"), Ttl::from_secs_f64(0.05).unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(120));
    let outcomes = race(42, 6, "thief");
    let thieves: Vec<&String> = outcomes.iter().filter(|(state, _)| *state == LockState::Stolen).map(|(_, who)| who).collect();
    assert_eq!(thieves.len(), 1, "{outcomes:?}");
    let listed = store.leases(false).unwrap();
    let held = listed.iter().find_map(|entry| match entry {
        LeaseEntry::Lease(view) if view.task() == Some(&json!(42)) => view.holder().map(str::to_owned),
        _ => None,
    });
    assert_eq!(held.as_deref(), Some(thieves[0].as_str()));
}

/// `test_cursori_concorrenti_non_perdono_aggiornamenti`: eight writers, each on its project's cursor, ten
/// writes each; every cursor ends with its last value.
#[test]
fn concurrent_cursors_lose_no_write() {
    let dir = folder("concurrent-cursors");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let (barrier, dir) = (barrier.clone(), dir.clone());
            std::thread::spawn(move || {
                let store = Store::at(dir).unwrap();
                barrier.wait();
                for n in 0..10 {
                    store.write_cursor(&project(&format!("p{i}")), &CursorName::events(), json!(n)).unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let store = Store::at(&dir).unwrap();
    let values: Vec<Value> = (0..8).map(|i| store.cursor(&project(&format!("p{i}")), &CursorName::events()).unwrap()).collect();
    assert_eq!(values, vec![json!(9); 8]);
}

/// After a step's end, another holder's new step is the one the cursor holds: the end emptied the step
/// before, never after.
#[test]
fn a_new_step_after_the_end_is_not_emptied() {
    let (store, _) = store("after-the-end");
    store.acquire_lock(&project("alpha"), &holder("w1"), secs(1800)).unwrap();
    store.open_step(&project("alpha"), &holder("w1"), None, store.now()).unwrap();
    assert!(matches!(store.close_step(&project("alpha"), &holder("w1"), &BTreeMap::new()).unwrap(), StepClosing::Closed(_)));
    store.release_lock(&project("alpha"), &holder("w1")).unwrap();
    store.acquire_lock(&project("alpha"), &holder("other"), secs(1800)).unwrap();
    store.open_step(&project("alpha"), &holder("other"), None, store.now()).unwrap();
    let value = stored(&store, "cursor", &step_key("alpha")).unwrap()[name("cursor", "value")].clone();
    assert_eq!(value[name("step", "holder")], json!("other"));
}

/// The comments of the open step, as stored.
fn step_comments(store: &Store, project: &str) -> Value {
    stored(store, "cursor", &step_key(project)).unwrap()[name("cursor", "value")][name("step", "comments")].clone()
}

/// The first of the prototype's `TestCommentoProprio` (M4, #445): an id is recorded only while a step is open;
/// the end copies it into the events cursor, and a comment after the end is not the step's.
#[test]
fn an_own_comment_is_recorded_only_while_a_step_is_open() {
    let (store, _) = store("own-comment");
    let alpha = project("alpha");
    assert_eq!(store.record_own_comment(&alpha, 101, Some("s-1")).unwrap(), OwnComment::NoStep);
    assert_eq!(store.cursor(&alpha, &CursorName::step()).unwrap(), Value::Null, "nothing is written without a step");
    store.acquire_lock(&alpha, &holder("w1"), secs(1800)).unwrap();
    store.open_step(&alpha, &holder("w1"), Some("s-1"), store.now()).unwrap();
    assert_eq!(store.record_own_comment(&alpha, 102, Some("s-1")).unwrap(), OwnComment::Recorded);
    assert_eq!(step_comments(&store, "alpha"), json!([102]));
    assert_eq!(store.step_session(&alpha).unwrap().as_deref(), Some("s-1"));
    store.close_step(&alpha, &holder("w1"), &BTreeMap::new()).unwrap();
    store.release_lock(&alpha, &holder("w1")).unwrap();
    assert_eq!(store.record_own_comment(&alpha, 103, Some("s-1")).unwrap(), OwnComment::NoStep);
    assert_eq!(store.step_session(&alpha).unwrap(), None, "no step, no session");
    let EventsCursor::Parts(parts) = store.events_cursor(&alpha).unwrap() else { panic!("the step's end writes the cursor") };
    assert_eq!(parts.own.unwrap().comments, Some(json!([102])), "only the comment written during the step");
}

/// The fifth and sixth of `TestCommentoProprio`: another session's step is left alone, and with either
/// session unknown the id is recorded. An id already there is not written again.
#[test]
fn another_session_s_step_is_left_alone_unless_a_session_is_unknown() {
    let (store, _) = store("own-comment-sessions");
    let alpha = project("alpha");
    let fields = |session: Value| {
        step_row(&[("version", json!(1)), ("holder", json!("w1")), ("session", session), ("t", json!(T0)), ("comments", json!([]))])
    };
    raw_row(&store, "cursor", &step_key("alpha"), &fields(json!("orchestrator-session")));
    assert_eq!(store.record_own_comment(&alpha, 7, Some("a-worker")).unwrap(), OwnComment::AnotherSession);
    assert_eq!(step_comments(&store, "alpha"), json!([]));
    assert_eq!(store.record_own_comment(&alpha, 7, None).unwrap(), OwnComment::Recorded, "this session unknown");
    assert_eq!(store.record_own_comment(&alpha, 8, Some("")).unwrap(), OwnComment::Recorded, "an empty id is none");
    assert_eq!(step_comments(&store, "alpha"), json!([7, 8]));
    let generation = |store: &Store| store.rows().unwrap().into_iter().find(|row| row.key == step_key("alpha")).unwrap().generation;
    let before = generation(&store);
    assert_eq!(store.record_own_comment(&alpha, 7, Some("orchestrator-session")).unwrap(), OwnComment::Recorded);
    assert_eq!(generation(&store), before, "an id already there writes nothing");
    let (store, _) = store_with_step("own-comment-no-session", &fields(Value::Null));
    assert_eq!(store.record_own_comment(&alpha, 9, Some("a-worker")).unwrap(), OwnComment::Recorded, "the step's session unknown");
    assert_eq!(store.step_session(&alpha).unwrap(), None);
}

fn store_with_step(label: &str, row: &str) -> (Store, ManualClock) {
    let (store, clock) = store(label);
    raw_row(&store, "cursor", &step_key("alpha"), row);
    (store, clock)
}

/// The step's comments as the prototype reads them: missing or empty is none, a number written as a float
/// is the same id, and a value that is not a list is refused before anything is written.
#[test]
fn the_step_s_comments_read_as_the_prototype_reads_them() {
    let alpha = project("alpha");
    let row = |comments: Value| step_row(&[("holder", json!("w1")), ("t", json!(T0)), ("comments", comments)]);
    let (store, _) = store_with_step("own-comment-float", &row(json!([101.0])));
    assert_eq!(store.record_own_comment(&alpha, 101, None).unwrap(), OwnComment::Recorded);
    assert_eq!(step_comments(&store, "alpha"), json!([101.0]), "101.0 is 101");
    let (store, _) = store_with_step("own-comment-missing", &step_row(&[("holder", json!("w1")), ("t", json!(T0))]));
    store.record_own_comment(&alpha, 5, None).unwrap();
    assert_eq!(step_comments(&store, "alpha"), json!([5]));
    let (store, _) = store_with_step("own-comment-not-a-list", &row(json!("abc")));
    assert!(matches!(store.record_own_comment(&alpha, 5, None), Err(StoreError::StepComments)));
    assert_eq!(step_comments(&store, "alpha"), json!("abc"));
    let (store, _) = store_with_step("own-comment-not-an-object", &json!({name("cursor", "value"): [1, 2]}).to_string());
    assert_eq!(store.record_own_comment(&alpha, 5, None).unwrap(), OwnComment::NoStep);
    assert_eq!(store.step_session(&alpha).unwrap(), None);
}

/// The last of `TestCommentoProprio`: writers that record at once, each on its own connection as processes
/// are, lose no id.
#[test]
fn concurrent_comments_lose_no_id() {
    let (store, _) = store("own-comment-concurrent");
    let alpha = project("alpha");
    store.acquire_lock(&alpha, &holder("w1"), secs(1800)).unwrap();
    store.open_step(&alpha, &holder("w1"), None, store.now()).unwrap();
    let dir = store.folder().to_path_buf();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (2001..2009)
        .map(|id| {
            let (barrier, dir) = (barrier.clone(), dir.clone());
            std::thread::spawn(move || {
                let store = Store::at(dir).unwrap();
                barrier.wait();
                store.record_own_comment(&project("alpha"), id, None).unwrap()
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap(), OwnComment::Recorded);
    }
    let mut ids: Vec<u64> = step_comments(&store, "alpha").as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect();
    ids.sort_unstable();
    assert_eq!(ids, (2001..2009).collect::<Vec<u64>>());
}
