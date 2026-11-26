//! The maintenance as a scheduled operation (#471): on the engine's tick, with the state's own database under
//! `target/`, an injected clock and the local time injected as UTC, as `controller_schedule.rs` runs the chats. The
//! log of declarations and the work folder are fresh folders of the case; the closed tasks are given.
//!
//! - Once a day at the first tick after the hour; the first run is a dry run, which changes nothing; the next day it
//!   compacts the log and removes the work folder of a task closed 31 days ago, and keeps the one closed 29 days ago.
//! - An error stays in the operation's entry and in the state file, never among the chats to open, and the run after
//!   it is a dry run again.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, NaiveDateTime};
use regia_agents::{Dry, ProbeConsent, WakeUp};
use regia_controller::engine::native::{StoreLock, StoreSignals};
use regia_controller::engine::{Controller, Folders, Host, LaunchedStep, Liveness, Options, Ports, Precheck, PrecheckRun, StateError, Steps};
use regia_controller::schedule::{ChatSurface, ClosedTasks, Maintenance, Operations, Source, StoreSchedule};
use regia_store::{Clock as _, Holder, ManualClock, ProjectId, Store, Ttl, WallTime, work_folder};
use serde_json::{Value, json};

const HOLDER: &str = "controller-rust-test";

fn config() -> Value {
    json!({"operazioni": [{"nome": "manutenzione", "ora": "03:00", "giorni": "tutti", "azione": "manutenzione"}]})
}

fn instant(day: u32, hour: u32) -> WallTime {
    WallTime::parse_iso(&format!("2026-11-{day:02}T{hour:02}:00:00Z")).unwrap()
}

fn utc_local(secs: f64) -> Option<NaiveDateTime> {
    let at = WallTime::from_unix_secs_f64(secs)?;
    DateTime::from_timestamp_micros(at.unix_micros()).map(|utc| utc.naive_utc())
}

struct Idle;

impl Precheck for Idle {
    fn run(&self, _projects: &[String], _timeout_s: f64) -> PrecheckRun {
        let line = json!({"v": 1, "outcome": "idle", "exit": 3, "motivo": "nessun-evento", "cursore": [], "payload": {"progetti": []}, "t": "2026-11-10T00:00:00Z"});
        PrecheckRun::from_output(Some(3), &line.to_string())
    }
}

struct NoFolders;

impl Folders for NoFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        Ok(BTreeMap::new())
    }
}

struct Nobody;

impl Liveness for Nobody {
    fn alive(&self, _pid: u32) -> bool {
        false
    }
}

struct NoSteps;

impl Steps for NoSteps {
    fn launched(&self, step: &LaunchedStep<'_>, _at: f64) -> Result<String, StateError> {
        Ok(step.project.to_owned())
    }

    fn finished(&self, _key: &str, _at: f64) -> Result<(), StateError> {
        Ok(())
    }

    fn suspended_projects(&self) -> BTreeSet<String> {
        BTreeSet::new()
    }
}

struct Case {
    clock: ManualClock,
    store: Store,
    controller: Controller,
    log: PathBuf,
    work: PathBuf,
}

fn line(ts: &str, session: &str) -> String {
    format!(r#"{{"v":1,"ts":"{ts}","sessione":"{session}","agente":"claude","claude":"{session}","progetto":"alfa","comando":"dichiara"}}"#)
}

impl Case {
    fn new(label: &str, closed: ClosedTasks) -> Case {
        let folder = regia_conformance::prepare_state_dir(&format!("maintenance-{label}"));
        let clock = ManualClock::fixed(instant(10, 0));
        let store = Store::at(&folder).unwrap().with_clock(clock.clone());
        let holder = Holder::new(HOLDER).unwrap();
        let reserved = ProjectId::new("controller").unwrap();
        assert!(store.acquire_lock(&reserved, &holder, Ttl::from_secs(600).unwrap()).unwrap().is_ok());
        let log = folder.join("log");
        fs::create_dir_all(&log).unwrap();
        // Two quiet files of October, last written on the 3rd: the compaction's `now` is the tick's.
        for (name, session) in [("s1.10.jsonl", "s1"), ("s2.11.jsonl", "s2")] {
            let path = log.join(name);
            fs::write(&path, format!("{}\n", line("2026-10-01T08:00:00Z", session))).unwrap();
            let written = SystemTime::UNIX_EPOCH + Duration::from_micros(u64::try_from(instant(3, 0).unix_micros()).unwrap());
            fs::File::options().write(true).open(&path).unwrap().set_modified(written).unwrap();
        }
        let work = work_folder(&folder.join("cache"));
        for task in [29, 31] {
            fs::create_dir_all(work.join("alfa").join(task.to_string())).unwrap();
            fs::write(work.join("alfa").join(task.to_string()).join("draft.md"), "x").unwrap();
        }
        let ports = Ports {
            clock: Arc::new(clock.clone()),
            precheck: Arc::new(Idle),
            folders: Arc::new(NoFolders),
            liveness: Arc::new(Nobody),
            surface: Arc::new(Dry::new()),
            lock: Arc::new(StoreLock::new(store.clone(), holder.clone())),
            signals: Arc::new(StoreSignals::new(store.clone(), holder.clone())),
            steps: Arc::new(NoSteps),
            ids: Arc::new(|| "id-fixed".to_owned()),
        };
        let maintenance = Maintenance::new(log.clone(), Some(work.clone()), closed);
        let operations = Operations::new(Source::Document(config()), Arc::new(StoreSchedule::new(store.clone(), holder)), ChatSurface::None)
            .with_local(utc_local)
            .with_maintenance(maintenance);
        let controller = Controller::new(Options::default(), ports, WakeUp::new(ProbeConsent::Refused)).with_before_precheck(Box::new(operations));
        Case { clock, store, controller, log, work }
    }

    fn at(&mut self, day: u32, hour: u32) -> Value {
        let step = instant(day, hour).unix_secs_f64() - self.clock.now().unix_secs_f64();
        self.clock.advance(step);
        self.controller.tick(false).expect("a tick")
    }

    fn status(&self, record: &Value) -> Value {
        let host = Host { pid: 1, holder: HOLDER.to_owned(), started: instant(10, 0).unix_secs_f64(), interval_s: 900.0 };
        self.controller.status(record, &host)
    }
}

fn voice(record: &Value) -> (String, String) {
    let voice = &record["operazioni"][0];
    (voice["nome"].as_str().unwrap_or_default().to_owned(), voice["azione"].as_str().unwrap_or_default().to_owned())
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    out.sort();
    out
}

fn closed_days_before(day: WallTime, days: &[(u64, i64)]) -> ClosedTasks {
    let closed: BTreeMap<u64, WallTime> =
        days.iter().map(|(task, ago)| (*task, WallTime::from_unix_micros(day.unix_micros() - ago * 86_400_000_000).unwrap())).collect();
    Arc::new(move || Ok(closed.clone()))
}

/// Once a day after its hour; the first run dry; the second compacts and cleans, 31 days out, 29 days in.
#[test]
fn the_maintenance_runs_once_a_day_and_its_first_run_changes_nothing() {
    let mut case = Case::new("once", closed_days_before(instant(12, 3), &[(29, 29), (31, 31)]));
    assert!(case.at(11, 2)["operazioni"].is_null(), "before the hour: nothing");
    let first = case.at(11, 3);
    assert_eq!(voice(&first), ("manutenzione".to_owned(), "a-secco".to_owned()), "{first}");
    assert_eq!(first["operazioni"][0]["dichiarazioni"]["merged"], json!(2), "{first}");
    assert_eq!(names(&case.log), ["s1.10.jsonl", "s2.11.jsonl"], "a dry run changes nothing");
    assert_eq!(voice(&case.at(11, 9)), ("manutenzione".to_owned(), "gia-eseguita-oggi".to_owned()));

    let second = case.at(12, 3);
    assert_eq!(voice(&second), ("manutenzione".to_owned(), "eseguita".to_owned()), "{second}");
    assert_eq!(names(&case.log), ["compattate-2026-10.jsonl"]);
    assert_eq!(second["operazioni"][0]["passaggio"]["removed"], json!(["alfa/31"]), "{second}");
    assert!(case.work.join("alfa").join("29").exists() && !case.work.join("alfa").join("31").exists());
    let entry = case.store.cursor(&ProjectId::new("controller").unwrap(), &regia_store::CursorName::new("operazioni").unwrap()).unwrap();
    assert_eq!(entry["ops"]["manutenzione"]["esito"], json!("eseguita"), "{entry}");
    assert_eq!(entry["ops"]["manutenzione"]["giorno"], json!("2026-11-12"), "{entry}");
}

/// An error: in the line, in the entry and the state file with its reason, never a chat to open; the next run is dry.
#[test]
fn an_error_stays_in_the_entry_and_the_next_run_is_dry() {
    let broken: ClosedTasks = Arc::new(|| Err("the tracker does not answer".to_owned()));
    let mut case = Case::new("error", broken);
    let first = case.at(11, 3);
    assert_eq!(voice(&first), ("manutenzione".to_owned(), "errore-manutenzione".to_owned()), "{first}");
    assert!(first["operazioni"][0]["motivo"].as_str().unwrap().contains("the tracker does not answer"), "{first}");
    let status = case.status(&first);
    assert_eq!(status["operazioni"]["manutenzione"]["esito"], json!("errore"), "{status}");
    assert!(status["operazioni"]["manutenzione"]["motivo"].as_str().is_some(), "{status}");
    assert_eq!(status["da_aprire"], json!([]), "{status}");
    assert!(case.work.join("alfa").join("31").exists(), "nothing removed");
    let next = case.at(12, 3);
    assert_eq!(voice(&next), ("manutenzione".to_owned(), "errore-manutenzione".to_owned()), "the tracker still fails: {next}");
    assert_eq!(next["operazioni"][0]["dichiarazioni"]["merged"], json!(2));
    assert_eq!(names(&case.log), ["s1.10.jsonl", "s2.11.jsonl"], "after an error the run is dry");
}
