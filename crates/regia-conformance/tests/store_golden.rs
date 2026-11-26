//! The store against the prototype in CI, with no Python: what the prototype recorded once is replayed
//! here on the Rust store (#561, items V1 and V2 of #437 and V3 of #463).
//!
//! - `fixtures/store/golden_rows.json`: a sequence of lock operations on two projects and three holders,
//!   with a steal after the expiry and a renewal by the other holder, then the budget cache and the claim of
//!   its probe. The prototype ran it on a clock that the steps move; for each step the file holds the outcome
//!   and every row of the table afterwards, `doc` as JSON and `gen`. The Rust store replays the same steps on a
//!   manual clock with the same tag and must leave the same outcomes and the same rows.
//! - `fixtures/store/iso_vectors.json`: strings and what `_parse` of the prototype made of each, an instant in
//!   microseconds or a refusal. `WallTime::parse_iso` must give the same on every one: a `scade` that the
//!   prototype accepts and the Rust refuses would have the Rust steal a live lock.
//! - `fixtures/store/golden_families.json` (#730): the other rows the store writes, a sequence for each family
//!   on a fresh folder and a fresh clock. A task's lease: takes, a refusal, heartbeats with and without a new
//!   ttl and by another holder, expiries, takes of an expired lease by its holder and by another, releases,
//!   and the list with and without the expired only. The cursors: the events cursor and a named one, read
//!   absent with and without a default, written with an instant (the old form of the events cursor), a list,
//!   the parts a step's end writes and numbers of each kind, and a name the prototype refuses. The
//!   orchestrator's step, through the prototype's own commands that start, renew and finish a step and the
//!   store's half of recording a comment: the step's lock, the step with its session and its comments, the
//!   events cursor with its instant and what the step wrote itself. For each step the file holds the outcome
//!   in the store's words and every row of the table afterwards, compared as the lock's are.
//!
//! The three files carry the oracle (`oracle_commit`, `oracle_repository`, `python`), each its own commit, and
//! are recorded again with `oracle/store_record.py`; its head says how. What is compared, and what is not:
//!
//! - the keys of a `doc` as a JSON object, not as bytes: the Rust writes them sorted, the prototype in the
//!   order it inserted them (a difference declared since #437);
//! - numbers as the JSON value they are, an integer or a float as each side wrote it: `1000.0` is not `1000`;
//! - the `token` of a probe's claim is random on both sides: it must be 32 hexadecimal digits, and is masked;
//! - the prototype's prose (`motivo`) is not compared: the Rust API returns codes, not sentences;
//! - a cursor read with a default: the store's API has none, as no caller of the prototype gives one but
//!   `None`. On an absent cursor the store answers `null` and the prototype the default it was given, which
//!   the replay puts in the place of the `null`;
//! - a step is the store's operations in the order both command lines run them: the instant read before the
//!   lock, the lock taken, the step opened, the events cursor read; a renewal is the lock's; at the end the
//!   lock renewed for a minute, the step closed, the lock released. The checks and the dossier of a step's end
//!   are the tracker's and git's, and are left out on both sides (`--no-push`);
//! - the reports a step's end processed: their dates are the command's, read from the files' times. The
//!   recording dates the files it creates, the prototype reads them back, and the replay gives the store the
//!   same dates: what is compared is where and how they land in the events cursor;
//! - the events cursor a step reads or writes is compared in the store's own form (`EventsCursor`): absent,
//!   the instant alone, its parts, or a value of another kind; its row is compared whole with the rows;
//! - recording a step's comment answers nothing in the prototype: its rows are compared, the store's answer
//!   is not.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use regia_conformance::{fixtures_root, json_diff};
use regia_store::{
    CursorName, EventsCursor, Holder, InvalidArgument, LeaseEntry, LockOutcome, LockState, LockStatus, LockView, ManualClock, ProjectId,
    ProviderUsage, Reading, StepClosing, StepOpening, Store, StoreError, TaskNumber, Ttl, UsageStatus, WallTime,
};
use serde_json::{Value, json};

const MASK: &str = "<32 hex digits>";

fn fixture(name: &str) -> Value {
    let path = fixtures_root().join("store").join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_else(|| panic!("no string {key} in {value}"))
}

/// The oracle block every recorded file carries.
fn check_oracle(file: &Value) {
    let commit = text(file, "oracle_commit");
    assert!(commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)), "oracle_commit {commit}");
    assert_eq!(text(file, "oracle_repository"), "lelexdrugo/regia");
    assert!(text(file, "python").starts_with("3.12."), "the vectors are Python 3.12's: {}", text(file, "python"));
}

#[test]
fn parse_iso_answers_as_the_prototype_does_on_every_vector() {
    let file = fixture("iso_vectors.json");
    check_oracle(&file);
    let vectors = file["vectors"].as_array().expect("vectors");
    assert!(vectors.len() >= 130, "{} vectors", vectors.len());
    let (mut accepted, mut refused, mut differ) = (0, 0, Vec::new());
    for vector in vectors {
        let input = text(vector, "text");
        let expected = vector["micros"].as_i64();
        assert!(vector.get("raises").is_none(), "the prototype crashed on {input:?}: not a vector");
        if expected.is_some() { accepted += 1 } else { refused += 1 }
        let got = WallTime::parse_iso(input).map(WallTime::unix_micros);
        if got != expected {
            differ.push(format!("{input:?}: prototype {expected:?}, Rust {got:?}"));
        }
    }
    assert!(accepted >= 40 && refused >= 40, "the vectors must cover both answers: {accepted} accepted, {refused} refused");
    assert!(differ.is_empty(), "{} of {} vectors differ:\n{}", differ.len(), vectors.len(), differ.join("\n"));
}

fn folder(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-golden").join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    dir
}

fn state_name(state: LockState) -> &'static str {
    match state {
        LockState::Taken => "taken",
        LockState::Renewed => "renewed",
        LockState::Stolen => "stolen",
        LockState::Released => "released",
        LockState::Free => "free",
        LockState::Busy => "busy",
        LockState::Foreign => "foreign",
        LockState::Absent => "absent",
        LockState::Heartbeat => "heartbeat",
    }
}

/// The outcome of a lock operation as the fixture records it: no prose, the holder and the expiry of the
/// lock the answer shows, if it shows one.
fn lock_outcome(outcome: &LockOutcome) -> Value {
    let view = outcome.lock();
    json!({
        "ok": outcome.is_ok(),
        "state": state_name(outcome.state()),
        "holder": view.and_then(|view| view.holder()),
        "expired": view.map(|view| view.is_expired()),
        "expires_in_s": view.map(|view| view.expires_in_s()),
    })
}

fn status_outcome(status: &LockStatus) -> Value {
    let name = match status {
        LockStatus::Free => "free",
        LockStatus::Held(_) => "held",
        LockStatus::Expired(_) => "expired",
        LockStatus::Unreadable => "unreadable",
        LockStatus::Contended => "contended",
    };
    let view = status.lock();
    json!({
        "busy": status.is_busy(),
        "status": name,
        "holder": view.and_then(|view| view.holder()),
        "expired": view.map(|view| view.is_expired()),
        "expires_in_s": view.map(|view| view.expires_in_s()),
    })
}

fn ttl(value: &Value) -> Ttl {
    match value.as_u64() {
        Some(secs) => Ttl::from_secs(secs),
        None => Ttl::from_secs_f64(value.as_f64().expect("a ttl")),
    }
    .expect("a valid ttl")
}

/// Every row of the table as the fixture records them, the claim's token masked.
fn rows_of(store: &Store) -> Value {
    let rows = store.rows().expect("rows").into_iter().map(|row| {
        let mut doc: Value = serde_json::from_str(&row.document).unwrap_or(Value::Null);
        mask_token(&row.kind, &mut doc);
        json!({"kind": row.kind, "key": row.key, "gen": row.generation, "doc": doc})
    });
    Value::Array(rows.collect())
}

/// The fixture's rows, with the same mask: the random token is the one thing two runs do not share.
fn masked(rows: &Value) -> Value {
    let mut rows = rows.clone();
    for row in rows.as_array_mut().expect("rows") {
        let kind = row["kind"].as_str().expect("kind").to_owned();
        mask_token(&kind, &mut row["doc"]);
    }
    rows
}

fn mask_token(kind: &str, doc: &mut Value) {
    if kind != "sonda" {
        return;
    }
    let token = doc["token"].as_str().unwrap_or_else(|| panic!("a claim without a token: {doc}"));
    assert!(token.len() == 32 && token.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)), "token {token}");
    doc["token"] = Value::from(MASK);
}

fn usage_status(status: UsageStatus) -> &'static str {
    match status {
        UsageStatus::Ok => "ok",
        UsageStatus::Unknown => "unknown",
        UsageStatus::ProbeOff => "probe_off",
    }
}

fn budget_outcome(reading: &Reading, probe_called: bool) -> Value {
    let cache = reading.cache.as_ref().expect("a budget reading carries its cache");
    let claude = reading.provider("claude").expect("claude");
    let served = claude.served.as_ref().expect("served");
    json!({
        "probe_called": probe_called,
        "from_cache": cache.from_cache,
        "generated": reading.generated,
        "claude_status": usage_status(claude.status),
        "claude_used_pct": claude.window("5h").and_then(|window| window.used_pct()),
        "claude_age_s": served.age_s,
        "claude_update_failed": served.update_failed.is_some(),
        "attempt_age_s": cache.attempt_age_s,
    })
}

#[test]
fn the_store_replays_the_prototype_s_sequence_with_the_same_outcomes_and_rows() {
    let file = fixture("golden_rows.json");
    check_oracle(&file);
    let clock_spec = &file["clock"];
    let start = WallTime::parse_iso(text(clock_spec, "start")).expect("start");
    let clock = ManualClock::new(start, clock_spec["monotonic_start"].as_f64().expect("monotonic"), text(clock_spec, "tag"));
    let store = Store::at(folder("sequence")).expect("store").with_clock(clock.clone());
    let steps = file["steps"].as_array().expect("steps");
    let seen_states: std::collections::BTreeSet<&str> = steps.iter().filter_map(|step| step["outcome"]["state"].as_str()).collect();
    for state in ["taken", "renewed", "stolen", "released", "free", "busy", "foreign", "absent"] {
        assert!(seen_states.contains(state), "the sequence must reach {state}: {seen_states:?}");
    }
    for (index, step) in steps.iter().enumerate() {
        let op = text(step, "op");
        let at = format!("step {index} ({op}) of the recorded sequence");
        if op == "advance" {
            clock.advance(step["seconds"].as_f64().expect("seconds"));
            continue;
        }
        let (outcome, during) = if op == "budget" {
            let name = text(step, "reading");
            let mut providers = Vec::new();
            for (provider, stored) in file["readings"][name].as_object().expect("a reading") {
                providers.push((provider.clone(), ProviderUsage::from_v1(provider, stored).expect("a reading the cache can hold")));
            }
            let called = std::cell::Cell::new(false);
            let during = std::cell::RefCell::new(Value::Null);
            let reading = store
                .budget(step["min_interval_s"].as_u64().expect("interval"), step["force"].as_bool().expect("force"), || {
                    called.set(true);
                    *during.borrow_mut() = rows_of(&store);
                    Ok::<_, std::io::Error>(Reading { generated: None, providers, cache: None })
                })
                .unwrap_or_else(|error| panic!("{at}: {error}"));
            (budget_outcome(&reading, called.get()), Some(during.into_inner()))
        } else {
            let project = ProjectId::new(text(step, "project")).expect("project");
            let who = || Holder::new(text(step, "holder")).expect("holder");
            let outcome = match op {
                "status" => status_outcome(&store.lock_status(&project).unwrap_or_else(|error| panic!("{at}: {error}"))),
                "acquire" => lock_outcome(&store.acquire_lock(&project, &who(), ttl(&step["ttl"])).expect("acquire")),
                "renew" => lock_outcome(&store.renew_lock(&project, &who(), ttl(&step["ttl"])).expect("renew")),
                "release" => lock_outcome(&store.release_lock(&project, &who()).expect("release")),
                other => panic!("{at}: unknown operation {other}"),
            };
            (outcome, None)
        };
        assert_eq!(outcome, step["outcome"], "{at}: outcome");
        if let Some(during) = during {
            let recorded = step.get("rows_during_probe").filter(|rows| !rows.is_null()).map_or(Value::Null, masked);
            assert_eq!(during, recorded, "{at}: the rows while the probe runs, the claim among them");
        }
        assert_eq!(rows_of(&store), masked(&step["rows"]), "{at}: rows after");
    }
    // The claim of a probe was a row while it ran, and none is left.
    let during_any = steps.iter().filter(|step| text(step, "op") == "budget").any(|step| {
        step["rows_during_probe"].as_array().is_some_and(|rows| rows.iter().any(|row| row["kind"] == "sonda"))
    });
    assert!(during_any, "the fixture must show the claim of a probe");
}

// ---------- the families of #730: a task's lease, the cursors, the orchestrator's step ----------

const FAMILIES: &str = "golden_families.json";
/// The ttl a step's end renews the lock with before it closes the step: a minute, an integer.
const FINISH_TTL_S: u64 = 60;

fn families() -> Value {
    let file = fixture(FAMILIES);
    check_oracle(&file);
    file
}

fn steps<'a>(file: &'a Value, family: &str) -> &'a [Value] {
    file["families"][family].as_array().unwrap_or_else(|| panic!("no {family} family")).as_slice()
}

/// A lease of the list as the fixture records it: the fields of its document as the view shows them.
fn lease_entry(entry: &LeaseEntry) -> Value {
    match entry {
        LeaseEntry::Lease(view) => {
            json!({"unreadable": false, "expired": view.is_expired(), "expires_in_s": view.expires_in_s(), "fields": view.fields()})
        }
        LeaseEntry::Unreadable { task } => json!({"unreadable": true, "task": task, "expired": true, "expires_in_s": 0, "fields": null}),
    }
}

/// The events cursor in the store's own form.
fn events_form(cursor: &EventsCursor) -> Value {
    match cursor {
        EventsCursor::Absent => json!({"form": "absent"}),
        EventsCursor::Instant(instant) => json!({"form": "instant", "instant": instant}),
        EventsCursor::Parts(parts) => json!({
            "form": "parts",
            "version": parts.version,
            "t": parts.t,
            "own": parts.own.as_ref().map(|own| json!({"comments": own.comments, "reports": own.reports})),
        }),
        EventsCursor::Other(value) => json!({"form": "other", "value": value}),
    }
}

fn with_exit(mut outcome: Value, exit: u8) -> Value {
    outcome["exit"] = Value::from(exit);
    outcome
}

/// The cursor a step names, the events cursor when it names none.
fn cursor_name(step: &Value) -> Result<CursorName, InvalidArgument> {
    match step.get("name") {
        Some(_) => CursorName::new(text(step, "name")),
        None => Ok(CursorName::events()),
    }
}

/// A step's start: the instant before the lock, the lock, the step opened, the events cursor read.
fn step_start(store: &Store, project: &ProjectId, holder: &Holder, session: Option<&str>, ttl: Ttl) -> Result<Value, StoreError> {
    let t = store.now();
    let taken = store.acquire_lock(project, holder, ttl)?;
    if !taken.is_ok() {
        return Ok(with_exit(lock_outcome(&taken), taken.exit_code()));
    }
    let StepOpening::Opened(opened) = store.open_step(project, holder, session, t)? else {
        panic!("the lock was lost between taking it and opening the step, which the sequence never does")
    };
    let cursor = store.events_cursor(project)?;
    let lock = taken.lock();
    Ok(json!({
        "exit": 0,
        "ok": true,
        "state": state_name(taken.state()),
        "holder": lock.and_then(LockView::holder),
        "expires_in_s": lock.map(LockView::expires_in_s),
        "t": opened.t(),
        "resumed": opened.resumed(),
        "cursor": events_form(&cursor),
    }))
}

/// A step's end: the lock renewed for a minute, which checks it is the caller's, the step closed, the lock
/// released. A step that was never opened is an error, with the lock left renewed.
fn step_finish(store: &Store, project: &ProjectId, holder: &Holder, reports: &BTreeMap<String, String>) -> Result<Value, StoreError> {
    let renewed = store.renew_lock(project, holder, Ttl::from_secs(FINISH_TTL_S).expect("a ttl"))?;
    if !renewed.is_ok() {
        return Ok(with_exit(lock_outcome(&renewed), renewed.exit_code()));
    }
    Ok(match store.close_step(project, holder, reports)? {
        StepClosing::Closed(cursor) => {
            let released = store.release_lock(project, holder)?;
            json!({"exit": 0, "ok": true, "holder": holder.as_str(), "cursor": events_form(&cursor), "lock": state_name(released.state())})
        }
        StepClosing::NoStep => json!({"exit": 1, "error": "no-step"}),
    })
}

/// One step of a family on the store, and its outcome as the fixture records it.
fn family_op(store: &Store, step: &Value, step_ttl: Ttl) -> Result<Value, StoreError> {
    let project = || ProjectId::new(text(step, "project")).expect("project");
    let holder = || Holder::new(text(step, "holder")).expect("holder");
    let task = || TaskNumber::new(step["task"].as_u64().expect("task"));
    let given_ttl = || step.get("ttl").map_or(step_ttl, ttl);
    let session = step.get("session").and_then(Value::as_str);
    Ok(match text(step, "op") {
        "acquire" => lock_outcome(&store.acquire_lock(&project(), &holder(), ttl(&step["ttl"]))?),
        "lease-take" => lock_outcome(&store.acquire_lease(task(), &project(), &holder(), ttl(&step["ttl"]))?),
        "lease-beat" => lock_outcome(&store.heartbeat_lease(task(), &holder(), (!step["ttl"].is_null()).then(|| ttl(&step["ttl"])))?),
        "lease-release" => lock_outcome(&store.release_lease(task(), &holder())?),
        "lease-list" => Value::Array(store.leases(step["expired"].as_bool().expect("expired"))?.iter().map(lease_entry).collect()),
        "cursor-read" => {
            let value = store.cursor(&project(), &cursor_name(step).expect("a name the prototype reads"))?;
            match step.get("default") {
                Some(default) if value.is_null() => json!({"value": default}),
                _ => json!({"value": value}),
            }
        }
        "cursor-write" => match cursor_name(step) {
            Ok(name) => {
                store.write_cursor(&project(), &name, step["value"].clone())?;
                Value::Null
            }
            Err(_) => json!({"error": "invalid-name"}),
        },
        "step-comment" => {
            store.record_own_comment(&project(), step["comment"].as_u64().expect("comment"), session)?;
            Value::Null
        }
        "step-start" => step_start(store, &project(), &holder(), session, given_ttl())?,
        "step-renew" => {
            let renewed = store.renew_lock(&project(), &holder(), given_ttl())?;
            with_exit(lock_outcome(&renewed), renewed.exit_code())
        }
        "step-finish" => {
            let reports = step.get("reports").and_then(Value::as_object).map_or_else(BTreeMap::new, |reports| {
                reports.iter().filter_map(|(name, date)| date.as_str().map(|date| (name.clone(), date.to_owned()))).collect()
            });
            step_finish(store, &project(), &holder(), &reports)?
        }
        other => panic!("unknown operation {other}"),
    })
}

fn described(rust: &Value, prototype: &Value) -> String {
    let lines = json_diff(rust, prototype);
    // The type of a number is a difference the reading of values may not name.
    if lines.is_empty() { format!("rust {rust}, prototype {prototype}") } else { lines.join("; ") }
}

/// Replays the sequence of `family` on a fresh store and a fresh clock, and says every way the store's outcomes
/// and rows differ from the prototype's, one line a difference, each naming its step.
fn replay_family(file: &Value, family: &str, label: &str) -> Vec<String> {
    let clock_spec = &file["clock"];
    let start = WallTime::parse_iso(text(clock_spec, "start")).expect("start");
    let clock = ManualClock::new(start, clock_spec["monotonic_start"].as_f64().expect("monotonic"), text(clock_spec, "tag"));
    let store = Store::at(folder(&format!("{family}-{label}"))).expect("store").with_clock(clock.clone());
    let step_ttl = Ttl::from_secs(file["step_ttl_s"].as_u64().expect("the step's default ttl, an integer")).expect("a ttl");
    let mut differences = Vec::new();
    for (index, step) in steps(file, family).iter().enumerate() {
        let op = text(step, "op");
        if op == "advance" {
            clock.advance(step["seconds"].as_f64().expect("seconds"));
            continue;
        }
        let at = format!("step {index} ({op}) of the {family} sequence");
        let outcome = family_op(&store, step, step_ttl).unwrap_or_else(|error| panic!("{at}: {error}"));
        if outcome != step["outcome"] {
            differences.push(format!("{at}: outcome: {}", described(&outcome, &step["outcome"])));
        }
        let (rows, recorded) = (rows_of(&store), masked(&step["rows"]));
        if rows != recorded {
            differences.push(format!("{at}: rows: {}", described(&rows, &recorded)));
        }
    }
    differences
}

/// The values a field of the outcomes takes in a family, for what the sequence must reach.
fn outcome_values(file: &Value, family: &str, field: &str) -> BTreeSet<String> {
    let outcomes = steps(file, family).iter().filter_map(|step| step.get("outcome"));
    outcomes.filter_map(|outcome| outcome.get(field)).map(|value| value.as_str().map_or_else(|| value.to_string(), str::to_owned)).collect()
}

fn assert_replayed(file: &Value, family: &str) {
    let differences = replay_family(file, family, "replay");
    assert!(differences.is_empty(), "{} differences in the {family} sequence:\n{}", differences.len(), differences.join("\n"));
}

#[test]
fn the_store_replays_the_prototype_s_leases_with_the_same_outcomes_and_rows() {
    let file = families();
    let states = outcome_values(&file, "lease", "state");
    for state in ["taken", "renewed", "stolen", "heartbeat", "released", "free", "busy", "foreign", "absent"] {
        assert!(states.contains(state), "the lease sequence must reach {state}: {states:?}");
    }
    let lists: Vec<&Value> = steps(&file, "lease").iter().filter(|step| step["op"] == "lease-list").collect();
    for expired in [true, false] {
        assert!(
            lists.iter().any(|step| step["expired"] == expired && step["outcome"].as_array().is_some_and(|leases| leases.len() >= 2)),
            "the lease sequence must list two leases or more with expired only {expired}"
        );
    }
    assert_replayed(&file, "lease");
}

#[test]
fn the_store_replays_the_prototype_s_cursors_with_the_same_outcomes_and_rows() {
    let file = families();
    let cursor = steps(&file, "cursor");
    assert!(cursor.iter().any(|step| step.get("default").is_some()), "a read with a default");
    assert!(cursor.iter().any(|step| step["outcome"]["error"] == "invalid-name"), "a name the prototype refuses");
    let keys: Vec<&str> = cursor.iter().flat_map(|step| step["rows"].as_array().into_iter().flatten()).filter_map(|row| row["key"].as_str()).collect();
    assert!(keys.iter().any(|key| key.contains(':')) && keys.iter().any(|key| !key.contains(':')), "a named cursor and an events cursor: {keys:?}");
    assert_replayed(&file, "cursor");
}

#[test]
fn the_store_replays_the_prototype_s_steps_with_the_same_outcomes_and_rows() {
    let file = families();
    assert_eq!(outcome_values(&file, "step", "exit"), BTreeSet::from(["0", "1", "3"].map(str::to_owned)), "the step's three exits");
    assert!(outcome_values(&file, "step", "resumed").contains("true"), "a step resumed");
    let forms: BTreeSet<&str> = steps(&file, "step").iter().filter_map(|step| step["outcome"]["cursor"]["form"].as_str()).collect();
    assert_eq!(forms, BTreeSet::from(["absent", "instant", "parts"]), "the events cursor in each of its forms");
    let reports = steps(&file, "step").iter().filter_map(|step| step["outcome"]["cursor"]["own"]["reports"].as_object());
    assert!(reports.into_iter().any(|reports| !reports.is_empty()), "a step's end with a report it processed");
    assert_replayed(&file, "step");
}

/// The families' fixture with one step of `family` changed by `change`, the first it changes, and that step.
fn altered(family: &str, change: impl Fn(&mut Value) -> bool) -> (Value, usize) {
    let mut file = families();
    let index = file["families"][family]
        .as_array_mut()
        .expect("a family")
        .iter_mut()
        .position(change)
        .unwrap_or_else(|| panic!("nothing in the {family} sequence to change"));
    (file, index)
}

/// Change 5 of #730: the replay of the changed fixture names the changed step, in its outcome or its rows.
fn assert_noticed(family: &str, label: &str, (file, index): (Value, usize), part: &str) {
    let differences = replay_family(&file, family, label);
    let prefix = format!("step {index} (");
    let noticed: Vec<&String> = differences.iter().filter(|line| line.starts_with(&prefix) && line.contains(&format!(": {part}: "))).collect();
    assert!(!noticed.is_empty(), "{label}: the change of step {index} went unnoticed: {differences:?}");
    println!("{family} / {label}: {}", noticed[0]);
}

/// Every row of a step, mutable.
fn rows_mut(step: &mut Value) -> impl Iterator<Item = &mut Value> {
    step["rows"].as_array_mut().into_iter().flatten()
}

#[test]
fn the_lease_replay_notices_a_changed_row_and_a_changed_outcome() {
    // An integer written as a float: the task of a lease's document.
    let float_task = altered("lease", |step| {
        rows_mut(step).find(|row| row["doc"]["task"].is_u64()).map(|row| row["doc"]["task"] = json!(row["doc"]["task"].as_f64())).is_some()
    });
    assert_noticed("lease", "control-task-as-float", float_task, "rows");
    // A list that says a second more of a lease's life.
    let later = altered("lease", |step| {
        let first = step["outcome"].as_array_mut().and_then(|leases| leases.first_mut());
        first.map(|lease| lease["expires_in_s"] = json!(lease["expires_in_s"].as_i64().expect("seconds") + 1)).is_some()
    });
    assert_noticed("lease", "control-list-expiry", later, "outcome");
}

#[test]
fn the_cursor_replay_notices_a_changed_row_and_a_changed_outcome() {
    // The key of the events cursor written as a named cursor's.
    let events = CursorName::events();
    let named_key = altered("cursor", |step| {
        rows_mut(step).find(|row| row["key"] == "alpha").map(|row| row["key"] = json!(format!("alpha:{events}"))).is_some()
    });
    assert_noticed("cursor", "control-events-key", named_key, "rows");
    // An integer of a cursor's value read back as a float.
    let float_value = altered("cursor", |step| {
        let value = &mut step["outcome"]["value"];
        (value.get("n").is_some_and(Value::is_u64)).then(|| value["n"] = json!(1.0)).is_some()
    });
    assert_noticed("cursor", "control-value-as-float", float_value, "outcome");
}

#[test]
fn the_step_replay_notices_a_changed_row_and_a_changed_outcome() {
    // The lock of a step started with the prototype's default ttl, an integer, written as a float.
    let default_ttl = families()["step_ttl_s"].clone();
    let float_ttl = altered("step", |step| {
        let row = rows_mut(step).find(|row| row["doc"]["ttl_s"] == default_ttl && row["doc"]["ttl_s"].is_u64());
        row.map(|row| row["doc"]["ttl_s"] = json!(row["doc"]["ttl_s"].as_f64())).is_some()
    });
    assert_noticed("step", "control-ttl-as-float", float_ttl, "rows");
    // The comments of a step's end out of order in the events cursor it wrote.
    let unsorted = altered("step", |step| {
        let comments = step["outcome"]["cursor"]["own"]["comments"].as_array_mut().filter(|comments| comments.len() >= 2);
        comments.map(|comments| comments.reverse()).is_some()
    });
    assert_noticed("step", "control-comments-order", unsorted, "outcome");
}
