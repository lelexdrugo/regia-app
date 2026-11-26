//! The host of the tick (#468, F26), on the state's own database at an injected clock, with the precheck, the
//! folders, the surface, the liveness and the wait faked:
//!
//! - the prototype's cases of `tools/tests/test_controller.py` that #467 left to this slice: one controller and
//!   exit 3, the lock renewed and released, D2 of #417, the wake-up after a suspension, a failed tick, the log, its
//!   rotation and the state file;
//! - the four of `test_controller_lock_lanci.py` on the registry saved without the lock (#507), three on the Rust;
//! - D2 of `0001-review-codex.md` (round 2): a successor built while the first controller is active does not delete
//!   its records; and the save record by record with its version check, which only the Rust has (ADR 0001 §7);
//! - the postponements written under the lock (#467/V4), and the restart: the count of the unknown budget, the
//!   wake-up's cap and the steps still launched.
//!
//! The values are the prototype's words, so a word the format changes fails here.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use regia_agents::{Dry, LaunchError, LaunchRequest, LaunchSurface, ProbeConsent, Receipt, SurfaceKind, WakeOutcome};
use regia_controller::engine::native::{StoreSignals, StoreSteps};
use regia_controller::engine::{BeforePrecheck, Folders, LaunchedStep, Liveness, Options, Precheck, PrecheckRun, Signals, StateError, StepReceipt, Steps, TickError};
use regia_controller::resident::{BadConfig, Config, Console, Exit, HostPorts, Notice, Resident, Sleep, folders_or_none, record_alive};
use regia_exec::{Effort, ModelId};
use regia_store::{CursorName, Holder, LockStatus, ManualClock, ProjectId, Store, Ttl, WallTime};
use serde_json::{Map, Value, json};

const T0: &str = "2026-10-21T10:00:00Z";
const PID: u32 = 4321;

type Hook = Box<dyn FnMut(usize) + Send>;

fn t0() -> f64 {
    WallTime::parse_iso(T0).unwrap().unix_secs_f64()
}

fn reserved() -> ProjectId {
    ProjectId::new("controller").unwrap()
}

fn registry_name() -> CursorName {
    CursorName::new("passi").unwrap()
}

fn ttl() -> Ttl {
    Ttl::from_secs(600).unwrap()
}

fn here() -> String {
    std::env::current_dir().unwrap().display().to_string()
}

// ---------- the precheck's answers, as `test_controller.esito` builds them ----------

fn entry(id: &str, outcome: &str, reason: &str) -> Value {
    json!({"id": id, "esito": outcome, "motivo": reason, "eventi": 1, "pronti": 1})
}

fn line(outcome: &str, reason: &str, projects: Value, budget: Option<Value>) -> PrecheckRun {
    let code = match outcome {
        "new-activity" | "terminal-state" => 0,
        "idle" => 3,
        _ => 1,
    };
    let mut payload = json!({"progetti": projects});
    if let Some(budget) = budget {
        payload["budget"] = budget;
    }
    let text = json!({"v": 1, "outcome": outcome, "exit": code, "motivo": reason, "cursore": [], "payload": payload, "t": T0}).to_string();
    PrecheckRun::from_output(Some(code), &text)
}

fn work(projects: &[&str]) -> PrecheckRun {
    line("new-activity", "pronti", Value::Array(projects.iter().map(|id| entry(id, "new-activity", "pronti")).collect()), None)
}

fn nothing() -> PrecheckRun {
    line("idle", "nessun-evento", json!([]), None)
}

fn unknown_budget() -> PrecheckRun {
    line("idle", "budget", json!([entry("a", "idle", "budget")]), Some(json!({"claude_5h_pct": null, "motivo": "sonda: troppe richieste (HTTP 429)"})))
}

// ---------- the fakes ----------

struct FakePrecheck {
    answers: Mutex<Vec<PrecheckRun>>,
    calls: AtomicUsize,
}

impl Precheck for FakePrecheck {
    fn run(&self, _projects: &[String], _timeout_s: f64) -> PrecheckRun {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut answers = self.answers.lock().unwrap();
        if answers.len() > 1 { answers.remove(0) } else { answers[0].clone() }
    }
}

struct FakeFolders;

impl Folders for FakeFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        Ok(["a", "b", "c"].iter().map(|id| ((*id).to_owned(), Some(here()))).collect())
    }
}

/// A surface like the prototype's `LancioFinto`: a receipt with a pid and a session, nothing started; its wake-up is
/// a fake one, or with `real` one that went well.
struct FakeSurface {
    pid: Option<u32>,
    launched: Mutex<Vec<String>>,
    wakes: AtomicUsize,
    real: AtomicBool,
}

impl LaunchSurface for FakeSurface {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Cli
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        let project = request.label.as_str().to_owned();
        self.launched.lock().unwrap().push(project.clone());
        let mut receipt = Dry::new().launch(request)?;
        receipt.surface = SurfaceKind::Cli;
        receipt.pid = self.pid;
        receipt.session = Some(format!("sess-{project}"));
        Ok(receipt)
    }

    fn wake(&self) -> WakeOutcome {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        if self.real.load(Ordering::SeqCst) { WakeOutcome { ok: true, exit: Some(0), ..WakeOutcome::default() } } else { WakeOutcome::fake() }
    }
}

type Expiry = Arc<dyn Fn() -> Option<String> + Send + Sync>;

#[derive(Default)]
struct FakeLiveness {
    alive: Mutex<Vec<u32>>,
}

impl Liveness for FakeLiveness {
    fn alive(&self, pid: u32) -> bool {
        self.alive.lock().unwrap().contains(&pid)
    }
}

/// The prototype's `Orologio.dormi`: a sleep moves the clock, by more at the calls of `jumps` (a suspension); `hook`
/// runs after the clock moved, with the call's number from 1.
struct FakeSleep {
    clock: ManualClock,
    slept: Mutex<Vec<f64>>,
    jumps: Mutex<BTreeMap<usize, f64>>,
    hook: Mutex<Option<Hook>>,
}

impl Sleep for FakeSleep {
    fn sleep(&self, secs: f64) {
        let n = {
            let mut slept = self.slept.lock().unwrap();
            slept.push(secs);
            slept.len()
        };
        let jump = self.jumps.lock().unwrap().get(&n).copied().unwrap_or(0.0);
        self.clock.advance(secs + jump);
        if let Some(hook) = self.hook.lock().unwrap().as_mut() {
            hook(n);
        }
    }
}

/// The notices, in order; `on_tick` runs at each tick's notice.
#[derive(Default)]
struct Recorder {
    notices: Mutex<Vec<Notice>>,
    on_tick: Mutex<Option<Box<dyn FnMut() + Send>>>,
}

impl Console for Recorder {
    fn notice(&self, notice: &Notice) {
        self.notices.lock().unwrap().push(notice.clone());
        if matches!(notice, Notice::Ticked(_))
            && let Some(hook) = self.on_tick.lock().unwrap().as_mut()
        {
            hook();
        }
    }
}

/// A hook before the precheck that fails its first tick, as the prototype's precheck that raises once.
struct FailsOnce(bool);

impl BeforePrecheck for FailsOnce {
    fn before(&mut self, _record: &mut Map<String, Value>, _tick: &regia_controller::engine::TickContext<'_>) -> Result<bool, TickError> {
        if std::mem::replace(&mut self.0, false) {
            return Err(TickError::Failed("RuntimeError: boom".to_owned()));
        }
        Ok(false)
    }
}

// ---------- a run ----------

struct Run {
    folder: PathBuf,
    clock: ManualClock,
    store: Store,
    precheck: Arc<FakePrecheck>,
    surface: Arc<FakeSurface>,
    liveness: Arc<FakeLiveness>,
    sleep: Arc<FakeSleep>,
    console: Arc<Recorder>,
    stop: Arc<AtomicBool>,
    expiry: Mutex<Option<Expiry>>,
}

impl Run {
    /// A fresh state folder under `target/`: the store opens nothing until its first operation.
    fn new(label: &str, answers: Vec<PrecheckRun>) -> Run {
        let folder = regia_conformance::prepare_state_dir(&format!("resident-{label}"));
        let clock = ManualClock::fixed(WallTime::parse_iso(T0).unwrap());
        let store = Store::at(&folder).unwrap().with_clock(clock.clone());
        Run {
            folder,
            store,
            precheck: Arc::new(FakePrecheck { answers: Mutex::new(answers), calls: AtomicUsize::new(0) }),
            surface: Arc::new(FakeSurface { pid: Some(4242), launched: Mutex::new(Vec::new()), wakes: AtomicUsize::new(0), real: AtomicBool::new(false) }),
            liveness: Arc::new(FakeLiveness::default()),
            sleep: Arc::new(FakeSleep { clock: clock.clone(), slept: Mutex::new(Vec::new()), jumps: Mutex::new(BTreeMap::new()), hook: Mutex::new(None) }),
            console: Arc::new(Recorder::default()),
            stop: Arc::new(AtomicBool::new(false)),
            expiry: Mutex::new(None),
            clock,
        }
    }

    fn config(&self, rounds: u64, interval_s: f64) -> Config {
        let mut config = Config::new(&self.folder);
        config.rounds = rounds;
        config.interval_s = interval_s;
        config.pid = PID;
        config.options = Options { model: Some(ModelId::parse("opus").unwrap()), effort: Some(Effort::High), ..Options::default() };
        config
    }

    fn resident(&self, config: Config) -> Resident {
        let ports = HostPorts {
            store: self.store.clone(),
            clock: Arc::new(self.clock.clone()),
            sleep: self.sleep.clone(),
            precheck: self.precheck.clone(),
            folders: Arc::new(FakeFolders),
            liveness: self.liveness.clone(),
            surface: self.surface.clone(),
            ids: Arc::new(|| "id-fixed".to_owned()),
            consent: ProbeConsent::Granted,
            token_expiry: self.expiry.lock().unwrap().clone(),
            console: self.console.clone(),
            stop: self.stop.clone(),
        };
        Resident::new(config, ports)
    }

    fn log_path(&self) -> PathBuf {
        self.folder.join("controller.log")
    }

    fn log(&self) -> Vec<Value> {
        fs::read_to_string(self.log_path()).map_or_else(|_| Vec::new(), |text| text.lines().map(|line| serde_json::from_str(line).unwrap()).collect())
    }

    fn status(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.folder.join("controller-stato.json")).unwrap()).unwrap()
    }

    fn registry(&self) -> Value {
        self.store.cursor(&reserved(), &registry_name()).unwrap()
    }

    fn holder_of_the_lock(&self) -> Option<String> {
        match self.store.lock_status(&reserved()).unwrap() {
            LockStatus::Held(view) | LockStatus::Expired(view) => view.holder().map(str::to_owned),
            _ => None,
        }
    }

    fn notices(&self) -> Vec<Notice> {
        self.console.notices.lock().unwrap().clone()
    }

    fn take(&self, holder: &str) -> Holder {
        let holder = Holder::new(holder).unwrap();
        assert!(self.store.acquire_lock(&reserved(), &holder, ttl()).unwrap().is_ok(), "{holder} takes the lock");
        holder
    }
}

fn decisions(log: &[Value]) -> Vec<String> {
    log.iter().map(|line| line["decisione"].as_str().unwrap_or_default().to_owned()).collect()
}

/// A record of the registry as the prototype's `nuovo_record` writes it, then in `state`, updated at `updated_t`.
fn record(project: &str, session: &str, state: &str, started_t: f64, updated_t: f64, pid: Option<u32>) -> (String, Value) {
    let record = json!({
        "progetto": project, "cartella": here(), "origine": "controller", "adapter": "cli", "agente": "claude",
        "modello": null, "effort": null, "sessione": session, "sessione_resume": session, "sessione_orca": null,
        "transcript": null, "pid": pid, "log": null, "marcatore": null, "avviato_t": started_t, "avviato": "-",
        "stato": state, "aggiornato_t": updated_t, "riprese": [],
    });
    (format!("{project}/{session}"), record)
}

fn document(records: &[(String, Value)]) -> Value {
    let steps: Map<String, Value> = records.iter().cloned().collect();
    json!({"v": 1, "passi": steps})
}

fn receipt(session: &str) -> StepReceipt {
    StepReceipt {
        adapter: "cli".to_owned(),
        agent: "claude".to_owned(),
        model: None,
        effort: None,
        session: Some(session.to_owned()),
        transcript: None,
        pid: Some(77),
        log: None,
        started: true,
        surface_session: None,
        marker: None,
    }
}

fn launch(steps: &StoreSteps, project: &str, session: &str, at: f64) -> Result<String, StateError> {
    let folder = here();
    let receipt = receipt(session);
    steps.launched(&LaunchedStep { project, folder: &folder, prompt: "the prompt", receipt: &receipt }, at)
}

fn lost<T: std::fmt::Debug>(result: Result<T, StateError>) -> bool {
    matches!(result, Err(StateError::LockLost(_)))
}

// ---------- TestUnSoloController ----------

/// `test_secondo_controller_esce_3_senza_toccare_il_precheck`.
#[test]
fn a_second_controller_exits_3_without_touching_the_precheck() {
    let run = Run::new("second", vec![nothing()]);
    run.take("controller-altro");
    let exit = run.resident(run.config(1, 900.0)).run();
    assert_eq!((exit.clone(), exit.code()), (Exit::AnotherController { holder: Some("controller-altro".to_owned()) }, 3));
    assert_eq!(run.precheck.calls.load(Ordering::SeqCst), 0);
    assert!(run.log().is_empty());
    assert_eq!(run.holder_of_the_lock().as_deref(), Some("controller-altro"), "the other's lock is not touched");
}

/// `test_il_lock_si_rilascia_alla_fine`.
#[test]
fn the_lock_is_released_at_the_end() {
    let run = Run::new("release", vec![nothing()]);
    let mut resident = run.resident(run.config(1, 900.0));
    let holder = resident.holder().as_str().to_owned();
    assert!(holder.starts_with(&format!("controller-{PID}-")) && holder.len() == format!("controller-{PID}-").len() + 8, "{holder}");
    assert_eq!(resident.run(), Exit::Done);
    assert_eq!(run.store.lock_status(&reserved()).unwrap(), LockStatus::Free);
    run.take("controller-dopo");
}

/// `test_il_lock_si_rinnova_a_ogni_giro_e_a_ogni_fetta`: two renewals a tick (its start, after the precheck) and one a
/// slice of the wait, 100 s in slices of 30. Each renewal rewrites the lock's row, so its generation counts them.
#[test]
fn the_lock_is_renewed_at_every_tick_and_every_slice() {
    let run = Run::new("renewals", vec![nothing()]);
    let generations = Arc::new(Mutex::new(Vec::new()));
    let (store, seen) = (run.store.clone(), generations.clone());
    *run.console.on_tick.lock().unwrap() = Some(Box::new(move || {
        let row = store.rows().unwrap().into_iter().find(|row| row.kind == "lock" && row.key == "controller").expect("the lock's row");
        seen.lock().unwrap().push(row.generation);
    }));
    assert_eq!(run.resident(run.config(2, 100.0)).run(), Exit::Done);
    assert_eq!(*generations.lock().unwrap(), [1 + 2, 1 + 2 + 4 + 2], "taken once, then 2 x 2 renewals and 4 slices");
    assert_eq!(*run.sleep.slept.lock().unwrap(), [30.0, 30.0, 30.0, 10.0]);
}

/// The lock stolen while the resident waits (the host's side of `test_lock_preso_da_un_altro_durante_il_giro_esce_3`;
/// the tick's side is #467's): the slice's renewal ends the run with 3, and the thief's lock is left alone.
#[test]
fn a_lock_stolen_during_the_wait_exits_3_and_the_thief_keeps_it() {
    let run = Run::new("stolen-wait", vec![nothing()]);
    run.sleep.jumps.lock().unwrap().insert(1, 601.0);
    let store = run.store.clone();
    *run.sleep.hook.lock().unwrap() = Some(Box::new(move |n| {
        if n == 1 {
            let thief = Holder::new("controller-ladro").unwrap();
            assert!(store.acquire_lock(&reserved(), &thief, ttl()).unwrap().is_ok(), "the lock expired while the machine stood still");
        }
    }));
    let exit = run.resident(run.config(3, 100.0)).run();
    assert!(matches!(exit, Exit::LockLost(_)) && exit.code() == 3, "{exit:?}");
    assert_eq!(run.precheck.calls.load(Ordering::SeqCst), 1);
    assert_eq!(run.holder_of_the_lock().as_deref(), Some("controller-ladro"));
}

/// A stop asked, as by Ctrl+C, ends the wait with 0 and releases the lock.
#[test]
fn a_stop_ends_the_run_with_0_and_releases_the_lock() {
    let run = Run::new("stop", vec![nothing()]);
    let stop = run.stop.clone();
    *run.sleep.hook.lock().unwrap() = Some(Box::new(move |_| stop.store(true, Ordering::SeqCst)));
    assert_eq!(run.resident(run.config(0, 100.0)).run(), Exit::Stopped);
    assert_eq!(run.store.lock_status(&reserved()).unwrap(), LockStatus::Free);
}

/// The prototype's check of its options, before the lock is asked for: exit 1, and no database opened.
#[test]
fn an_option_that_is_not_positive_is_refused_before_the_lock() {
    let run = Run::new("bad-option", vec![nothing()]);
    let exit = run.resident(run.config(1, 0.0)).run();
    assert_eq!((exit.clone(), exit.code()), (Exit::BadConfig(BadConfig::Interval), 1));
    assert!(!run.store.database().exists());
}

// ---------- D2: TestRegistroCaricatoDopoIlLock, and the counterexample of 0001-review-codex.md ----------

/// `test_il_costruttore_non_legge_il_registro`: building a resident opens no database at all.
#[test]
fn building_a_resident_reads_nothing() {
    let run = Run::new("builds-nothing", vec![nothing()]);
    let _resident = run.resident(run.config(1, 900.0));
    assert!(!run.store.database().exists(), "nothing read, nothing written");
}

/// `test_record_scritto_dal_vecchio_fra_costruzione_e_lock_resta_nel_registro`: the old controller registers its
/// last step and lets the lock go between the resident's building and its lock.
#[test]
fn a_record_the_old_controller_wrote_between_building_and_lock_stays() {
    let run = Run::new("d2-417", vec![work(&["a"])]);
    let mut resident = run.resident(run.config(1, 900.0));
    let old = record("b", "sess-vecchia", "finito", t0() - 60.0, t0() - 30.0, Some(1));
    run.store.write_cursor(&reserved(), &registry_name(), document(std::slice::from_ref(&old))).unwrap();
    assert_eq!(resident.run(), Exit::Done);
    assert_eq!(run.log()[0]["lanciati"][0]["progetto"], "a", "the new step saves the registry...");
    let saved = run.registry()["passi"].as_object().unwrap().clone();
    assert_eq!(saved.get(&old.0), Some(&old.1), "...and the old controller's record is there");
    assert_eq!(saved.len(), 2);
}

/// Acceptance 1, D2's counterexample (round 2 of `0001-review-codex.md`): A holds the lock; B is built and stands
/// still before taking it; A launches a step, saves its record and lets the lock go; B takes the lock and its first
/// tick saves. The prototype before #417 deleted A's record, with no theft of the lock at all. Here the record stays
/// as A wrote it, and B takes A's step, still alive, among its own, so it does not launch it again.
#[test]
fn a_successor_built_while_the_first_is_active_does_not_delete_its_records() {
    let run = Run::new("d2-codex", vec![work(&["a"])]);
    let first = run.take("controller-primo");
    let first_steps = StoreSteps::new(run.store.clone(), first.clone());
    first_steps.load().unwrap();
    let mut successor = run.resident(run.config(1, 900.0));
    let key = launch(&first_steps, "b", "sess-primo", t0()).unwrap();
    run.liveness.alive.lock().unwrap().push(77);
    let written = run.registry()["passi"][&key].clone();
    assert!(run.store.release_lock(&reserved(), &first).unwrap().is_ok());

    assert_eq!(successor.run(), Exit::Done);
    let saved = run.registry()["passi"].as_object().unwrap().clone();
    assert_eq!(saved.get(&key), Some(&written), "the first controller's record survives the successor's save");
    assert!(saved.keys().any(|key| key.starts_with("a/")), "the successor's own step is saved too: {saved:?}");
    assert_eq!(saved.len(), 2);
    assert_eq!(run.log()[0]["passi_vivi"], json!(["a", "b"]), "A's step is alive in B's memory");
}

/// The save record by record (Rust only, ADR 0001 §7): a record another writer added after the load (a prototype
/// before #507, which writes without the lock's check) survives a save that did not load it.
#[test]
fn a_record_added_after_the_load_survives_the_save() {
    let run = Run::new("d2-added", vec![nothing()]);
    let holder = run.take("controller-rust");
    let steps = StoreSteps::new(run.store.clone(), holder);
    assert_eq!(steps.load().unwrap(), 0);
    let theirs = record("c", "sess-loro", "lanciato", t0(), t0(), Some(9));
    run.store.write_cursor(&reserved(), &registry_name(), document(std::slice::from_ref(&theirs))).unwrap();
    let ours = launch(&steps, "a", "sess-nostra", t0() + 1.0).unwrap();
    let saved = run.registry()["passi"].as_object().unwrap().clone();
    assert_eq!(saved.get(&theirs.0), Some(&theirs.1), "a record this controller did not load is not deleted");
    assert!(saved.contains_key(&ours));
    assert_eq!(steps.records().len(), 2, "after the save the memory is the document as written");
}

/// The version check: a change to a record that another writer changed since the load leaves the stored record as it
/// is; a record nobody else touched takes the change.
#[test]
fn a_change_to_a_record_another_writer_changed_keeps_the_stored_one() {
    let run = Run::new("d2-version", vec![nothing()]);
    let holder = run.take("controller-rust");
    let mine = record("a", "sess-a", "lanciato", t0(), t0(), Some(1));
    let shared = record("b", "sess-b", "lanciato", t0(), t0(), Some(2));
    run.store.write_cursor(&reserved(), &registry_name(), document(&[mine.clone(), shared.clone()])).unwrap();
    let steps = StoreSteps::new(run.store.clone(), holder);
    assert_eq!(steps.load().unwrap(), 2);
    let mut suspended = shared.1.clone();
    suspended["stato"] = json!("sospesa");
    suspended["aggiornato_t"] = json!(t0() + 5.0);
    run.store.write_cursor(&reserved(), &registry_name(), document(&[mine.clone(), (shared.0.clone(), suspended.clone())])).unwrap();

    steps.finished(&shared.0, t0() + 10.0).unwrap();
    steps.finished(&mine.0, t0() + 10.0).unwrap();
    let saved = run.registry()["passi"].clone();
    assert_eq!(saved[&shared.0], suspended, "the stored version wins over a change made on an older one");
    assert_eq!(saved[&mine.0]["stato"], "finito");
    assert_eq!(steps.records()[&shared.0]["stato"], "sospesa", "the memory takes the stored version");
}

/// The registry cannot be read before the lock is taken: the load is the lock holder's.
#[test]
fn the_registry_is_not_read_without_the_lock() {
    let run = Run::new("d2-load", vec![nothing()]);
    run.store.write_cursor(&reserved(), &registry_name(), document(&[record("b", "s", "lanciato", t0(), t0(), Some(1))])).unwrap();
    let steps = StoreSteps::new(run.store.clone(), Holder::new("controller-rust").unwrap());
    assert!(lost(steps.load()));
    assert!(steps.records().is_empty());
}

// ---------- TestSalvataggioRegistroSenzaLock (#507) ----------

/// `test_salvato_da_un_controller_senza_lock_il_documento_resta_quello_del_nuovo`.
#[test]
fn saved_by_a_controller_without_the_lock_the_document_stays_the_new_one_s() {
    let run = Run::new("507-stolen", vec![nothing()]);
    let holder = run.take("controller-vecchio");
    let steps = StoreSteps::new(run.store.clone(), holder);
    steps.load().unwrap();
    launch(&steps, "b", "sess-vecchia", t0()).unwrap();
    run.clock.advance(601.0);
    run.take("controller-ladro");
    let new = document(&[record("b", "sess-nuova", "finito", t0(), t0(), Some(1))]);
    run.store.write_cursor(&reserved(), &registry_name(), new.clone()).unwrap();
    assert!(lost(launch(&steps, "b", "altra", t0() + 610.0)));
    assert_eq!(run.registry(), new, "nothing written: the new controller's record is still there");
}

/// `test_senza_nessun_lock_il_salvataggio_non_scrive`.
#[test]
fn with_no_lock_at_all_the_save_writes_nothing() {
    let run = Run::new("507-free", vec![nothing()]);
    let holder = run.take("controller-rust");
    let steps = StoreSteps::new(run.store.clone(), holder.clone());
    steps.load().unwrap();
    assert!(run.store.release_lock(&reserved(), &holder).unwrap().is_ok());
    assert!(lost(launch(&steps, "a", "sess-x", t0())));
    assert_eq!(run.registry(), Value::Null);
}

/// `test_lock_scaduto_ma_ancora_nostro_il_salvataggio_riesce`: the owner counts, not the expiry.
#[test]
fn a_lock_expired_but_still_ours_saves() {
    let run = Run::new("507-expired", vec![nothing()]);
    let holder = run.take("controller-rust");
    let steps = StoreSteps::new(run.store.clone(), holder);
    steps.load().unwrap();
    run.clock.advance(3600.0);
    assert!(matches!(run.store.lock_status(&reserved()).unwrap(), LockStatus::Expired(_)));
    let key = launch(&steps, "a", "sess-lunga", t0() + 3600.0).unwrap();
    assert_eq!(run.registry()["passi"].as_object().unwrap().keys().cloned().collect::<Vec<_>>(), [key]);
    assert_eq!(run.holder_of_the_lock().as_deref(), Some("controller-rust"));
}

// `test_un_registro_senza_proprietario_salva_come_prima` has no Rust side: the Rust registry is always a lock
// holder's, and the prototype keeps the owner-less one for its tools and tests only.

/// #467/V4: a postponement is written under the lock too.
#[test]
fn a_postponement_after_the_lock_passed_writes_nothing() {
    let run = Run::new("signals", vec![nothing()]);
    let holder = run.take("controller-rust");
    let signals = StoreSignals::new(run.store.clone(), holder);
    let name = CursorName::new("rinvio").unwrap();
    let project = ProjectId::new("a").unwrap();
    signals.postpone("a", json!({"v": 1, "id": "first"})).unwrap();
    assert_eq!(run.store.cursor(&project, &name).unwrap()["id"], "first");
    run.clock.advance(601.0);
    run.take("controller-ladro");
    let result = signals.postpone("a", json!({"v": 1, "id": "second"}));
    assert!(matches!(result, Err(StateError::LockLost(_))), "{result:?}");
    assert_eq!(run.store.cursor(&project, &name).unwrap()["id"], "first");
}

// ---------- TestRisveglio ----------

/// `test_dopo_una_sospensione_giro_subito`: the machine sleeps during the third slice.
#[test]
fn after_a_suspension_a_tick_at_once() {
    let run = Run::new("wakeup", vec![nothing()]);
    run.sleep.jumps.lock().unwrap().insert(3, 5.0 * 3600.0);
    assert_eq!(run.resident(run.config(2, 900.0)).run(), Exit::Done);
    let log = run.log();
    assert_eq!(decisions(&log), ["attesa-idle", "risveglio", "attesa-idle"]);
    assert_eq!((log[0]["risveglio"].clone(), log[2]["risveglio"].clone()), (json!(false), json!(true)));
    assert_eq!(log[1]["nota"], "tempo trascorso molto oltre l'intervallo: sospensione; giro subito");
    assert_eq!(*run.sleep.slept.lock().unwrap(), [30.0, 30.0, 30.0], "no other wait after the wake-up: a tick at once");
    assert!(run.notices().contains(&Notice::Wakeup));
}

/// `test_un_ritardo_normale_non_e_un_risveglio`.
#[test]
fn a_normal_delay_is_no_suspension() {
    let run = Run::new("delay", vec![nothing()]);
    run.sleep.jumps.lock().unwrap().insert(3, 90.0);
    assert_eq!(run.resident(run.config(2, 100.0)).run(), Exit::Done);
    let flags: Vec<Value> = run.log().iter().map(|line| line["risveglio"].clone()).collect();
    assert_eq!(flags, [json!(false), json!(false)]);
}

/// `test_un_giro_guasto_non_ferma_il_controller`.
#[test]
fn a_failed_tick_does_not_stop_the_resident() {
    let run = Run::new("failed-tick", vec![nothing()]);
    let mut resident = run.resident(run.config(2, 10.0)).with_before_precheck(Box::new(FailsOnce(true)));
    assert_eq!(resident.run(), Exit::Done);
    let log = run.log();
    assert_eq!(decisions(&log), ["errore-giro", "attesa-idle"]);
    assert_eq!((log[0]["giro"].clone(), log[0]["errore"].clone()), (json!(1), json!("RuntimeError: boom")));
    assert!(run.notices().contains(&Notice::TickFailed { round: 1, error: "RuntimeError: boom".to_owned() }));
}

// ---------- TestLog ----------

/// `test_una_riga_json_per_giro`.
#[test]
fn one_json_line_a_tick_in_ascii() {
    let run = Run::new("log", vec![work(&["a"]), line("idle", "occupato", json!([entry("a", "new-activity", "pronti")]), None)]);
    assert_eq!(run.resident(run.config(2, 10.0)).run(), Exit::Done);
    let log = run.log();
    assert_eq!(log.len(), 2);
    let first = &log[0];
    assert_eq!((first["v"].clone(), first["giro"].clone(), first["risveglio"].clone(), first["decisione"].clone()), (json!(1), json!(1), json!(false), json!("lancio")));
    assert_eq!((first["precheck"]["exit"].clone(), first["precheck"]["outcome"].clone(), first["precheck"]["motivo"].clone()), (json!(0), json!("new-activity"), json!("pronti")));
    assert_eq!((first["lanciati"][0]["progetto"].clone(), first["lanciati"][0]["pid"].clone()), (json!("a"), json!(4242)));
    assert!(first["t"].as_str().unwrap().ends_with('Z'));
    assert_eq!((log[1]["decisione"].clone(), log[1]["precheck"]["exit"].clone()), (json!("attesa-idle"), json!(3)));
    assert!(fs::read(run.log_path()).unwrap().is_ascii(), "ASCII, as the precheck's JSON");
}

/// `test_il_log_sta_nel_runtime_non_in_temp`.
#[test]
fn the_log_is_in_the_state_folder_never_in_the_temporary_one() {
    let run = Run::new("log-place", vec![nothing()]);
    assert_eq!(run.resident(run.config(1, 900.0)).run(), Exit::Done);
    assert!(run.log_path().is_file());
    assert!(!regia_store::is_within(&run.folder, &regia_store::temporary_folder()), "{}", run.folder.display());
}

/// `test_rotazione_per_dimensione`.
#[test]
fn the_log_rotates_by_size() {
    let run = Run::new("rotation", vec![nothing()]);
    let big = |c: char| c.to_string().repeat(1_000_001);
    fs::write(run.log_path(), big('x')).unwrap();
    assert_eq!(run.resident(run.config(1, 900.0)).run(), Exit::Done);
    assert_eq!(fs::read_to_string(run.folder.join("controller.log.1")).unwrap(), big('x'));
    assert_eq!(run.log().len(), 1);
    fs::write(run.log_path(), big('y')).unwrap();
    assert_eq!(run.resident(run.config(1, 900.0)).run(), Exit::Done);
    let first = |n: u32| fs::read_to_string(run.folder.join(format!("controller.log.{n}"))).unwrap().chars().next();
    assert_eq!((first(2), first(1)), (Some('x'), Some('y')));
}

/// `test_file_di_stato`, and its writing: replaced whole, no temporary file left.
#[test]
fn the_state_file() {
    let run = Run::new("status", vec![nothing()]);
    let mut resident = run.resident(run.config(1, 900.0));
    let holder = resident.holder().as_str().to_owned();
    assert_eq!(resident.run(), Exit::Done);
    let status = run.status();
    assert_eq!((status["giro"].clone(), status["decisione"].clone(), status["pid"].clone()), (json!(1), json!("attesa-idle"), json!(PID)));
    assert_eq!(status["budget_sconosciuto"], Value::Null);
    assert_eq!((status["proprietario"].clone(), status["intervallo_s"].clone(), status["ultimo_giro"].clone()), (json!(holder), json!(900.0), json!(T0)));
    assert!(fs::read(run.folder.join("controller-stato.json")).unwrap().is_ascii());
    assert!(!run.folder.join(format!("controller-stato.json.{PID}.tmp")).exists());
}

// ---------- the restart ----------

/// `TestBudget`'s alarm on the host: the notice at every tick from the fourth with the budget unknown, and the count
/// in the state file.
#[test]
fn the_alarm_of_the_unknown_budget_is_a_notice_from_the_fourth_tick() {
    let run = Run::new("alarm", vec![unknown_budget()]);
    let mut config = run.config(5, 10.0);
    config.options.no_wake = true;
    assert_eq!(run.resident(config).run(), Exit::Done);
    let alarms: Vec<(i64, Option<String>)> = run
        .notices()
        .into_iter()
        .filter_map(|notice| match notice {
            Notice::BudgetAlarm { rounds, reason, since } => {
                assert_eq!(since.as_deref(), Some(T0));
                Some((rounds, reason))
            }
            _ => None,
        })
        .collect();
    let reason = Some("sonda: troppe richieste (HTTP 429)".to_owned());
    assert_eq!(alarms, [(4, reason.clone()), (5, reason)]);
    assert_eq!(run.status()["budget_sconosciuto"]["giri"], 5);
}

fn write_status(folder: &Path, status: &Value) {
    fs::write(folder.join("controller-stato.json"), serde_json::to_string(status).unwrap()).unwrap();
}

/// F135: the count goes on from the state file of the controller before, so the alarm comes at the fourth tick across
/// a restart.
#[test]
fn a_restart_takes_up_the_count_of_the_unknown_budget() {
    let run = Run::new("restart-count", vec![unknown_budget()]);
    write_status(&run.folder, &json!({"v": 1, "budget_sconosciuto": {"giri": 3, "da": "2026-10-21T09:15:00Z", "allarme": false, "motivo": "429"}}));
    let mut config = run.config(1, 900.0);
    config.options.no_wake = true;
    assert_eq!(run.resident(config).run(), Exit::Done);
    let budget = run.log()[0]["budget"].clone();
    assert_eq!((budget["giri"].clone(), budget["allarme"].clone(), budget["da"].clone()), (json!(4), json!(true), json!("2026-10-21T09:15:00Z")));
    assert!(run.notices().iter().any(|notice| matches!(notice, Notice::BudgetAlarm { rounds: 4, .. })));
}

/// F135: the last wake-up of the controller before holds the next one back for the hour.
#[test]
fn a_restart_keeps_the_cap_of_the_wake_up() {
    let run = Run::new("restart-wake", vec![unknown_budget()]);
    let earlier = json!({"t": "2026-10-21T09:50:00Z", "ok": false, "finta": true, "errore": "fake wake-up: no session started"});
    write_status(&run.folder, &json!({"v": 1, "sveglia": earlier}));
    assert_eq!(run.resident(run.config(1, 900.0)).run(), Exit::Done);
    assert_eq!(run.surface.wakes.load(Ordering::SeqCst), 0, "ten minutes after the last one: none");
    assert_eq!(run.log()[0].get("sveglia"), None);
    assert_eq!(run.status()["sveglia"], earlier);
}

/// #462/V3: the token's wake-up reads Claude's expiry before and after it, and the line and the state say the token
/// was renewed.
#[test]
fn the_wake_up_reads_the_token_s_expiry_around_it() {
    let run = Run::new("wake-expiry", vec![unknown_budget()]);
    run.surface.real.store(true, Ordering::SeqCst);
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    let expiry: Expiry = Arc::new(move || {
        let before = counter.fetch_add(1, Ordering::SeqCst) == 0;
        Some(if before { "2026-10-21T08:00:00+00:00" } else { "2026-10-21T18:00:00+00:00" }.to_owned())
    });
    *run.expiry.lock().unwrap() = Some(expiry);
    assert_eq!(run.resident(run.config(1, 900.0)).run(), Exit::Done);
    let wake = run.log()[0]["sveglia"].clone();
    assert_eq!((wake["ok"].clone(), wake["token_rinnovato"].clone()), (json!(true), json!(true)), "{wake}");
    assert_eq!((wake["token_scadenza_prima"].clone(), wake["token_scadenza_dopo"].clone()), (json!("2026-10-21T08:00:00+00:00"), json!("2026-10-21T18:00:00+00:00")));
    assert_eq!((reads.load(Ordering::SeqCst), run.surface.wakes.load(Ordering::SeqCst)), (2, 1));
    assert_eq!(run.status()["sveglia"]["token_rinnovato"], true);
}

/// F136: a step an earlier controller launched, still launched and alive, is not launched again; a finished one, or
/// one not the controller's, is not taken back.
#[test]
fn a_restart_does_not_launch_again_a_step_still_launched() {
    let run = Run::new("restart-steps", vec![work(&["a", "b"])]);
    run.liveness.alive.lock().unwrap().push(4242);
    let alive = record("a", "sess-a", "lanciato", t0() - 60.0, t0() - 60.0, Some(4242));
    let finished = record("b", "sess-b", "finito", t0() - 600.0, t0() - 300.0, Some(4242));
    run.store.write_cursor(&reserved(), &registry_name(), document(&[alive, finished])).unwrap();
    let mut config = run.config(1, 900.0);
    config.options.max_steps = 2;
    assert_eq!(run.resident(config).run(), Exit::Done);
    let first = &run.log()[0];
    let actions: Vec<&str> = first["progetti"].as_array().unwrap().iter().map(|row| row["azione"].as_str().unwrap()).collect();
    assert_eq!(actions, ["saltato-passo-vivo", "lanciato"]);
    assert_eq!(*run.surface.launched.lock().unwrap(), ["b"]);
    assert_eq!(first["passi_vivi"], json!(["a", "b"]));
}

// ---------- F160, F164 ----------

#[test]
fn a_record_is_alive_while_its_pid_lives_and_the_folders_fall_back_to_none() {
    let liveness = FakeLiveness::default();
    liveness.alive.lock().unwrap().push(7);
    assert!(record_alive(&json!({"pid": 7}), &liveness));
    for dead in [json!({"pid": 8}), json!({"pid": 0}), json!({"pid": null}), json!({})] {
        assert!(!record_alive(&dead, &liveness), "{dead}");
    }
    struct Broken;
    impl Folders for Broken {
        fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
            Err("gh: not found".to_owned())
        }
    }
    assert!(folders_or_none(&Broken).is_empty());
    assert_eq!(folders_or_none(&FakeFolders).len(), 3);
}
