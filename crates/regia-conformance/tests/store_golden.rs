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
//!
//! Both files carry the oracle (`oracle_commit`, `oracle_repository`, `python`) and are recorded again with
//! `oracle/store_record.py`; its head says how. What is compared, and what is not:
//!
//! - the keys of a `doc` as a JSON object, not as bytes: the Rust writes them sorted, the prototype in the
//!   order it inserted them (a difference declared since #437);
//! - numbers as the JSON value they are, an integer or a float as each side wrote it: `1000.0` is not `1000`;
//! - the `token` of a probe's claim is random on both sides: it must be 32 hexadecimal digits, and is masked;
//! - the prototype's prose (`motivo`) is not compared: the Rust API returns codes, not sentences.

use std::fs;
use std::path::PathBuf;

use regia_conformance::fixtures_root;
use regia_store::{
    Holder, LockOutcome, LockState, LockStatus, ManualClock, ProjectId, ProviderUsage, Reading, Store, Ttl, UsageStatus, WallTime,
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
