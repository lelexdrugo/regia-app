//! The prototype's cases of the tick (#467), on the Rust engine with injected ports: `tools/tests/test_controller.py`
//! (the precheck, the launches, the budget and D1 of #417), `test_controller_lock_lanci.py` (D1 with two candidates)
//! and `test_controller_seguito.py` (the postponements and the budget across a restart). The values are the
//! prototype's words, so a word the format changes fails here. The ports are fakes, except the lock of the D1
//! cases, which is the state's own on an injected clock, so a thief takes it as another controller would.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use regia_agents::{Dry, LaunchError, LaunchRequest, LaunchSurface, ProbeConsent, Receipt, SurfaceKind, WakeOutcome, WakeUp};
use regia_controller::engine::native::{Processes, StoreLock, process_alive};
use regia_controller::engine::{
    Controller, ControllerLock, Folders, LaunchedStep, Liveness, LockLost, Options, Ports, Precheck, PrecheckRun, Signals, StateError, Steps,
    TickError, UnknownBudget,
};
use regia_exec::{Effort, ModelId};
use regia_store::{Holder, ManualClock, ProjectId, Store, Ttl, WallTime};
use serde_json::{Value, json};

const T0: &str = "2026-10-21T10:00:00Z";

/// What a fake runs at a point of the tick: a thief, in the cases of D1.
type Hook = Box<dyn FnMut(&str) + Send>;
type ReadHook = Box<dyn FnMut() + Send>;

// ---------- the precheck's answers ----------

fn entry(id: &str, outcome: &str, reason: &str) -> Value {
    json!({"id": id, "esito": outcome, "motivo": reason, "eventi": 1, "pronti": 1})
}

fn ready(id: &str) -> Value {
    entry(id, "new-activity", "pronti")
}

/// A run of the precheck that printed this line, as `test_controller.esito` builds it.
fn line(outcome: &str, reason: &str, projects: Value, exit: Option<i32>, budget: Option<Value>) -> PrecheckRun {
    let code = exit.unwrap_or(match outcome {
        "new-activity" | "terminal-state" => 0,
        "idle" => 3,
        _ => 1,
    });
    let mut payload = json!({"progetti": projects});
    if let Some(budget) = budget {
        payload["budget"] = budget;
    }
    let text = json!({"v": 1, "outcome": outcome, "exit": code, "motivo": reason, "cursore": [], "payload": payload, "t": T0}).to_string();
    PrecheckRun::from_output(Some(code), &text)
}

fn work(projects: &[&str]) -> PrecheckRun {
    line("new-activity", "pronti", Value::Array(projects.iter().map(|id| ready(id)).collect()), None, None)
}

fn nothing() -> PrecheckRun {
    line("idle", "nessun-evento", json!([]), None, None)
}

fn idle_budget(budget: Value) -> PrecheckRun {
    line("idle", "budget", json!([entry("a", "idle", "budget")]), None, Some(budget))
}

// ---------- the fakes ----------

#[derive(Default)]
struct FakePrecheck {
    answers: Mutex<Vec<PrecheckRun>>,
    calls: Mutex<Vec<(Vec<String>, f64)>>,
}

impl Precheck for FakePrecheck {
    fn run(&self, projects: &[String], timeout_s: f64) -> PrecheckRun {
        self.calls.lock().unwrap().push((projects.to_vec(), timeout_s));
        let mut answers = self.answers.lock().unwrap();
        if answers.len() > 1 { answers.remove(0) } else { answers[0].clone() }
    }
}

/// The registry's folders; `hook` runs at each read, the last thing before a launch's prompt.
#[derive(Default)]
struct FakeFolders {
    map: BTreeMap<String, Option<String>>,
    hook: Mutex<Option<ReadHook>>,
}

impl Folders for FakeFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        if let Some(hook) = self.hook.lock().unwrap().as_mut() {
            hook();
        }
        Ok(self.map.clone())
    }
}

/// A surface like the prototype's `LancioFinto`: a receipt with `pid`, a failure for some projects, and a hook
/// after a launch.
struct FakeSurface {
    pid: Option<u32>,
    failing: BTreeSet<String>,
    calls: Mutex<Vec<LaunchRequest>>,
    after: Mutex<Option<Hook>>,
    wakes: AtomicUsize,
}

impl FakeSurface {
    fn new(pid: Option<u32>) -> FakeSurface {
        FakeSurface { pid, failing: BTreeSet::new(), calls: Mutex::new(Vec::new()), after: Mutex::new(None), wakes: AtomicUsize::new(0) }
    }

    fn projects(&self) -> Vec<String> {
        self.calls.lock().unwrap().iter().map(|request| request.label.as_str().to_owned()).collect()
    }
}

impl LaunchSurface for FakeSurface {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Cli
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        self.calls.lock().unwrap().push(request.clone());
        let project = request.label.as_str().to_owned();
        if self.failing.contains(&project) {
            return Err(LaunchError::AfterEffect("claude not found".to_owned()));
        }
        let mut receipt = Dry::new().launch(request)?;
        receipt.surface = SurfaceKind::Cli;
        receipt.pid = self.pid;
        receipt.session = Some(format!("sess-{project}"));
        if let Some(after) = self.after.lock().unwrap().as_mut() {
            after(&project);
        }
        Ok(receipt)
    }

    fn wake(&self) -> WakeOutcome {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        WakeOutcome::fake()
    }
}

/// A lock always ours, which counts its renewals; or lost from a renewal on.
#[derive(Default)]
struct FakeLock {
    renewals: AtomicUsize,
    lost_from: Mutex<Option<usize>>,
}

impl ControllerLock for FakeLock {
    fn renew(&self) -> Result<(), StateError> {
        let n = self.renewals.fetch_add(1, Ordering::SeqCst) + 1;
        match *self.lost_from.lock().unwrap() {
            Some(from) if n >= from => Err(StateError::LockLost(LockLost("another controller is active".to_owned()))),
            _ => Ok(()),
        }
    }
}

#[derive(Default)]
struct FakeSignals {
    written: Mutex<Vec<(String, Value)>>,
    failure: Mutex<Option<StateError>>,
}

impl Signals for FakeSignals {
    fn postpone(&self, project: &str, value: Value) -> Result<(), StateError> {
        if let Some(failure) = self.failure.lock().unwrap().clone() {
            return Err(failure);
        }
        self.written.lock().unwrap().push((project.to_owned(), value));
        Ok(())
    }
}

#[derive(Default)]
struct FakeSteps {
    launched: Mutex<Vec<(String, f64)>>,
    finished: Mutex<Vec<String>>,
    suspended: Mutex<BTreeSet<String>>,
    after: Mutex<Option<Hook>>,
}

impl Steps for FakeSteps {
    fn launched(&self, step: &LaunchedStep<'_>, at: f64) -> Result<String, StateError> {
        self.launched.lock().unwrap().push((step.project.to_owned(), at));
        if let Some(after) = self.after.lock().unwrap().as_mut() {
            after(step.project);
        }
        Ok(format!("{}/{}", step.project, step.receipt.session.clone().unwrap_or_default()))
    }

    fn finished(&self, key: &str, _at: f64) -> Result<(), StateError> {
        self.finished.lock().unwrap().push(key.to_owned());
        Ok(())
    }

    fn suspended_projects(&self) -> BTreeSet<String> {
        self.suspended.lock().unwrap().clone()
    }
}

#[derive(Default)]
struct FakeLiveness {
    alive: Mutex<BTreeSet<u32>>,
}

impl Liveness for FakeLiveness {
    fn alive(&self, pid: u32) -> bool {
        self.alive.lock().unwrap().contains(&pid)
    }
}

struct Harness {
    clock: ManualClock,
    precheck: Arc<FakePrecheck>,
    folders: Arc<FakeFolders>,
    surface: Arc<FakeSurface>,
    lock: Arc<dyn ControllerLock>,
    fake_lock: Arc<FakeLock>,
    signals: Arc<FakeSignals>,
    steps: Arc<FakeSteps>,
    liveness: Arc<FakeLiveness>,
}

impl Harness {
    fn new(answers: Vec<PrecheckRun>, pid: Option<u32>) -> Harness {
        let folder = std::env::current_dir().unwrap().display().to_string();
        let map = ["a", "b", "c"].iter().map(|id| ((*id).to_owned(), Some(folder.clone()))).collect();
        let fake_lock = Arc::new(FakeLock::default());
        Harness {
            clock: ManualClock::fixed(WallTime::parse_iso(T0).unwrap()),
            precheck: Arc::new(FakePrecheck { answers: Mutex::new(answers), calls: Mutex::new(Vec::new()) }),
            folders: Arc::new(FakeFolders { map, hook: Mutex::new(None) }),
            surface: Arc::new(FakeSurface::new(pid)),
            lock: fake_lock.clone(),
            fake_lock,
            signals: Arc::new(FakeSignals::default()),
            steps: Arc::new(FakeSteps::default()),
            liveness: Arc::new(FakeLiveness::default()),
        }
    }

    fn options() -> Options {
        Options { model: Some(ModelId::parse("opus").unwrap()), effort: Some(Effort::High), ..Options::default() }
    }

    fn controller(&self, options: Options) -> Controller {
        let ports = Ports {
            clock: Arc::new(self.clock.clone()),
            precheck: self.precheck.clone(),
            folders: self.folders.clone(),
            liveness: self.liveness.clone(),
            surface: self.surface.clone(),
            lock: self.lock.clone(),
            signals: self.signals.clone(),
            steps: self.steps.clone(),
            ids: Arc::new(|| "id-fixed".to_owned()),
        };
        Controller::new(options, ports, WakeUp::new(ProbeConsent::Granted))
    }

    fn advance(&self, secs: f64) {
        self.clock.advance(secs);
    }
}

fn actions(record: &Value) -> Vec<String> {
    record["progetti"].as_array().unwrap().iter().map(|row| row["azione"].as_str().unwrap().to_owned()).collect()
}

// ---------- TestPrecheck ----------

/// Every run that is not new activity launches nothing, with the prototype's decision.
#[test]
fn a_precheck_without_new_activity_launches_nothing() {
    let mut truncated = work(&["a"]);
    if let Some(regia_controller::precheck::Json::Object(fields)) = truncated.line.as_mut() {
        for (key, value) in fields.iter_mut() {
            if key == "payload" {
                *value = serde_json::from_str("{\"troncato\":true,\"progetti\":4}").unwrap();
            }
        }
    }
    let cases: Vec<(PrecheckRun, &str)> = vec![
        (line("idle", "attesa-del-titolare", json!([{"id": "a", "esito": "idle"}]), None, None), "attesa-idle"),
        (line("error", "tracker", Value::Null, None, None), "attesa-errore"),
        (PrecheckRun::from_output(Some(1), ""), "attesa-errore"),
        (PrecheckRun::failed("timeout after 120 s"), "attesa-precheck-guasto"),
        (PrecheckRun::from_output(Some(2), ""), "attesa-exit-inatteso"),
        (PrecheckRun::from_output(Some(0), ""), "attesa-incoerente"),
        (PrecheckRun { exit: Some(0), ..line("idle", "nessun-evento", json!([ready("a")]), Some(3), None) }, "attesa-incoerente"),
        (PrecheckRun { exit: Some(0), ..line("new-activity", "pronti", json!([ready("a")]), Some(3), None) }, "attesa-incoerente"),
        (truncated, "attesa-payload-troncato"),
        (line("new-activity", "pronti", json!([entry("a", "idle", "budget")]), None, None), "attesa-incoerente"),
    ];
    for (run, decision) in cases {
        let h = Harness::new(vec![run.clone()], Some(4242));
        let record = h.controller(Harness::options()).tick(false).unwrap();
        assert_eq!(record["decisione"], json!(decision), "{run:?}");
        assert!(h.surface.calls.lock().unwrap().is_empty(), "{decision}: nothing launched");
        if decision == "attesa-precheck-guasto" {
            assert!(record["precheck"]["errore"].as_str().unwrap().contains("timeout"));
        }
        if decision == "attesa-idle" {
            assert_eq!(record["precheck"]["motivo"], "attesa-del-titolare");
        }
    }
}

#[test]
fn the_precheck_gets_the_projects_and_the_timeout() {
    let h = Harness::new(vec![nothing()], None);
    let options = Options { projects: vec!["a".to_owned(), "b".to_owned()], ..Harness::options() };
    h.controller(options).tick(false).unwrap();
    assert_eq!(*h.precheck.calls.lock().unwrap(), vec![(vec!["a".to_owned(), "b".to_owned()], 120.0)]);
}

// ---------- TestLanci ----------

#[test]
fn exit_0_launches_a_step_with_its_prompt_model_and_effort() {
    let h = Harness::new(vec![work(&["a"])], Some(4242));
    let record = h.controller(Harness::options()).tick(false).unwrap();
    assert_eq!(record["decisione"], "lancio");
    assert_eq!(record["lanciati"][0]["progetto"], "a");
    assert_eq!(record["lanciati"][0]["sessione"], "sess-a");
    assert_eq!(record["lanciati"][0]["pid"], 4242);
    let request = h.surface.calls.lock().unwrap()[0].clone();
    assert_eq!((request.model.unwrap().as_str(), request.effort.unwrap().as_str()), ("opus", "high"));
    for part in ["regia step start --project a", "ONE orchestrator step", "\"progetti\":[{", "\"id\":\"a\"", "§1.3"] {
        assert!(request.prompt.contains(part), "{part}");
    }
}

#[test]
fn model_and_effort_from_the_options() {
    let h = Harness::new(vec![work(&["a"])], Some(4242));
    let options = Options { model: Some(ModelId::parse("sonnet").unwrap()), effort: Some(Effort::Medium), ..Harness::options() };
    h.controller(options).tick(false).unwrap();
    let request = h.surface.calls.lock().unwrap()[0].clone();
    assert_eq!((request.model.unwrap().as_str(), request.effort.unwrap().as_str()), ("sonnet", "medium"));
}

#[test]
fn no_second_step_while_the_first_lives_another_once_it_ended() {
    let h = Harness::new(vec![work(&["a"])], Some(4242));
    h.liveness.alive.lock().unwrap().insert(4242);
    let mut c = h.controller(Harness::options());
    c.tick(false).unwrap();
    h.advance(900.0);
    let record = c.tick(false).unwrap();
    assert_eq!((actions(&record), record["decisione"].clone(), record["passi_vivi"].clone()), (vec!["saltato-passo-vivo".to_owned()], json!("nessun-lancio"), json!(["a"])));
    h.liveness.alive.lock().unwrap().clear();
    h.advance(900.0);
    let record = c.tick(false).unwrap();
    assert_eq!(record["passi_finiti"], json!([{"progetto": "a", "perche": "terminato"}]));
    assert_eq!(h.surface.projects(), ["a", "a"]);
    assert_eq!(*h.steps.finished.lock().unwrap(), ["a/sess-a"]);
}

#[test]
fn past_the_limit_a_step_counts_no_more_even_alive() {
    let h = Harness::new(vec![work(&["a"])], Some(4242));
    h.liveness.alive.lock().unwrap().insert(4242);
    let mut c = h.controller(Options { step_limit_s: 3600.0, ..Harness::options() });
    c.tick(false).unwrap();
    h.advance(3599.0);
    c.tick(false).unwrap();
    assert_eq!(h.surface.projects().len(), 1);
    h.advance(2.0);
    let record = c.tick(false).unwrap();
    assert_eq!(h.surface.projects().len(), 2);
    assert_eq!(record["passi_finiti"], json!([{"progetto": "a", "perche": "tetto"}]));
    assert_eq!(Options::default().step_limit_s, 3600.0);
}

#[test]
fn without_a_pid_the_window_of_grace_holds() {
    let h = Harness::new(vec![work(&["a"])], None);
    let mut c = h.controller(Harness::options());
    c.tick(false).unwrap();
    h.advance(299.0);
    c.tick(false).unwrap();
    assert_eq!(h.surface.projects().len(), 1);
    h.advance(2.0);
    let record = c.tick(false).unwrap();
    assert_eq!(h.surface.projects().len(), 2);
    assert_eq!(record["passi_finiti"], json!([{"progetto": "a", "perche": "senza pid: grazia finita"}]));
}

#[test]
fn one_step_a_tick_and_one_a_project() {
    let h = Harness::new(vec![work(&["a", "b"])], Some(4242));
    h.liveness.alive.lock().unwrap().insert(4242);
    let mut c = h.controller(Harness::options());
    assert_eq!(actions(&c.tick(false).unwrap()), ["lanciato", "rinviato-limite-giro"]);
    h.advance(900.0);
    assert_eq!(actions(&c.tick(false).unwrap()), ["saltato-passo-vivo", "lanciato"]);
    h.advance(900.0);
    c.tick(false).unwrap();
    assert_eq!(h.surface.projects(), ["a", "b"]);
}

#[test]
fn the_steps_of_a_tick_are_an_option() {
    let h = Harness::new(vec![work(&["a", "b", "c"])], Some(4242));
    let record = h.controller(Options { max_steps: 2, ..Harness::options() }).tick(false).unwrap();
    let launched: Vec<&str> = record["lanciati"].as_array().unwrap().iter().map(|x| x["progetto"].as_str().unwrap()).collect();
    assert_eq!(launched, ["a", "b"]);
}

#[test]
fn a_terminal_state_goes_first_with_the_reporting_prompt() {
    let h = Harness::new(vec![line("terminal-state", "stallo", json!([ready("a"), entry("b", "terminal-state", "stallo")]), None, None)], Some(4242));
    h.controller(Harness::options()).tick(false).unwrap();
    let request = h.surface.calls.lock().unwrap()[0].clone();
    assert_eq!(request.label.as_str(), "b");
    assert!(request.prompt.contains("REPORTS") && request.prompt.contains("does NOT start with `regia step start`"));
    assert!(!request.prompt.contains("regia step start --project b"));
}

#[test]
fn a_failed_launch_is_recorded_retried_at_each_tick_and_leaves_its_place() {
    let mut h = Harness::new(vec![work(&["a", "b"])], Some(4242));
    Arc::get_mut(&mut h.surface).unwrap().failing.insert("a".to_owned());
    h.liveness.alive.lock().unwrap().insert(4242);
    let mut c = h.controller(Harness::options());
    let record = c.tick(false).unwrap();
    assert_eq!(actions(&record), ["errore-lancio", "lanciato"]);
    assert!(record["progetti"][0]["errore"].as_str().unwrap().contains("claude not found"));
    assert_eq!(record["decisione"], "lancio");
    h.advance(10.0);
    c.tick(false).unwrap();
    assert_eq!(h.surface.projects(), ["a", "b", "a"], "nothing remembered of a: tried again, b is alive");
}

#[test]
fn a_project_without_its_folder_is_a_launch_error() {
    let mut h = Harness::new(vec![work(&["a"])], Some(4242));
    Arc::get_mut(&mut h.folders).unwrap().map.insert("a".to_owned(), Some("no-such-folder".to_owned()));
    let record = h.controller(Harness::options()).tick(false).unwrap();
    assert_eq!((actions(&record), record["decisione"].clone()), (vec!["errore-lancio".to_owned()], json!("nessun-lancio")));
    assert!(h.surface.calls.lock().unwrap().is_empty());
}

#[test]
fn a_dry_launch_starts_nothing_and_keeps_the_request() {
    let dry = Dry::new();
    let request = LaunchRequest::new(regia_agents::ServiceId::Claude, "a", &std::env::current_dir().unwrap(), "the prompt").unwrap();
    let receipt = dry.launch(&request).unwrap();
    let step = regia_controller::engine::StepReceipt::of(&receipt);
    assert_eq!((step.adapter.as_str(), step.started, step.pid, step.log), ("finto", false, None, None));
    assert_eq!(dry.requests()[0]["prompt"], "the prompt");
}

// ---------- D1 (#417, ADR 0001 §7) ----------

/// A controller on the state's own lock, with a thief: another controller that takes the lock once it expired.
struct OnTheStore {
    harness: Harness,
    store: Store,
}

impl OnTheStore {
    fn new(answers: Vec<PrecheckRun>) -> OnTheStore {
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        let run = RUNS.fetch_add(1, Ordering::SeqCst);
        let state = regia_conformance::prepare_state_dir(&format!("controller-d1-{}-{run}", std::process::id()));
        let mut harness = Harness::new(answers, None);
        let store = Store::at(&state).unwrap().with_clock(harness.clock.clone());
        let holder = Holder::new("controller-rust").unwrap();
        assert!(store.acquire_lock(&reserved(), &holder, Ttl::from_secs(600).unwrap()).unwrap().is_ok());
        harness.lock = Arc::new(StoreLock::new(store.clone(), holder));
        OnTheStore { harness, store }
    }

    /// The process stands still past the lock's expiry, and another controller takes it.
    fn steal(store: &Store, clock: &ManualClock) {
        clock.advance(601.0);
        let thief = Holder::new("controller-thief").unwrap();
        assert!(store.acquire_lock(&reserved(), &thief, Ttl::from_secs(600).unwrap()).unwrap().is_ok(), "the thief takes the expired lock");
    }

    fn thief_holds(&self) -> bool {
        match self.store.lock_status(&reserved()).unwrap() {
            regia_store::LockStatus::Held(view) => view.holder() == Some("controller-thief"),
            _ => false,
        }
    }
}

fn reserved() -> ProjectId {
    ProjectId::new("controller").unwrap()
}

fn lost(result: Result<Value, TickError>) -> bool {
    matches!(result, Err(TickError::LockLost(_)))
}

/// `TestLockRicontrollatoPrimaDelLancio`: the lock stolen between the last renewal and the launch (here while the
/// registry's folders are read, after the renewal that follows the precheck): no launch, nothing recorded.
#[test]
fn a_lock_stolen_between_the_renewal_and_the_launch_launches_nothing() {
    let on = OnTheStore::new(vec![work(&["a"])]);
    let (store, clock) = (on.store.clone(), on.harness.clock.clone());
    *on.harness.folders.hook.lock().unwrap() = Some(Box::new(move || OnTheStore::steal(&store, &clock)));
    let mut c = on.harness.controller(Harness::options());
    assert!(lost(c.tick(false)), "a LockLost, not a launch error the tick would record and forget");
    assert!(on.harness.surface.calls.lock().unwrap().is_empty());
    assert!(on.harness.steps.launched.lock().unwrap().is_empty());
    assert!(c.alive_projects().is_empty());
    assert!(on.thief_holds());
}

/// `TestDueCandidati`, #417/V3: the lock renewed before each launch, not before the first only.
#[test]
fn with_two_candidates_a_lock_stolen_during_the_first_launch_stops_the_second() {
    for after_the_registry in [false, true] {
        let on = OnTheStore::new(vec![work(&["a", "b"])]);
        let (store, clock) = (on.store.clone(), on.harness.clock.clone());
        let steal: Hook = Box::new(move |project| {
            if project == "a" {
                OnTheStore::steal(&store, &clock);
            }
        });
        if after_the_registry {
            *on.harness.steps.after.lock().unwrap() = Some(steal);
        } else {
            *on.harness.surface.after.lock().unwrap() = Some(steal);
        }
        let mut c = on.harness.controller(Options { max_steps: 2, ..Harness::options() });
        assert!(lost(c.tick(false)));
        assert_eq!(on.harness.surface.projects(), ["a"], "after the registry: {after_the_registry}");
        assert!(on.thief_holds());
        if after_the_registry {
            assert_eq!(c.alive_projects(), ["a"]);
        }
    }
}

/// The token's wake-up is a launch point too: a lock lost right before it wakes nothing.
#[test]
fn a_lock_lost_before_the_wake_up_wakes_nothing() {
    let h = Harness::new(vec![idle_budget(json!({"claude_5h_pct": null}))], None);
    // The renewals of a tick: its start, after the precheck, then the wake-up's.
    *h.fake_lock.lost_from.lock().unwrap() = Some(3);
    assert!(lost(h.controller(Harness::options()).tick(false)));
    assert_eq!(h.surface.wakes.load(Ordering::SeqCst), 0);
    let h = Harness::new(vec![idle_budget(json!({"claude_5h_pct": null}))], None);
    let record = h.controller(Harness::options()).tick(false).unwrap();
    assert_eq!((h.surface.wakes.load(Ordering::SeqCst), record["sveglia"]["finta"].clone()), (1, json!(true)));
    assert_eq!(h.fake_lock.renewals.load(Ordering::SeqCst), 4, "the wake-up renews before and after");
}

/// `test_lock_preso_da_un_altro_durante_il_giro_esce_3`: lost before the tick, no precheck.
#[test]
fn a_lock_lost_before_the_tick_runs_no_precheck() {
    let h = Harness::new(vec![nothing()], None);
    *h.fake_lock.lost_from.lock().unwrap() = Some(1);
    assert!(lost(h.controller(Harness::options()).tick(false)));
    assert!(h.precheck.calls.lock().unwrap().is_empty());
}

// ---------- the budget (TestBudget, TestBudgetSeguito) ----------

#[test]
fn the_unknown_budget_is_counted_and_raises_the_alarm_at_the_fourth_tick() {
    let h = Harness::new(vec![idle_budget(json!({"claude_5h_pct": null, "motivo": "sonda: troppe richieste (HTTP 429)"}))], None);
    let mut c = h.controller(Options { no_wake: true, ..Harness::options() });
    let mut seen = Vec::new();
    for _ in 0..5 {
        let record = c.tick(false).unwrap();
        seen.push((record["budget"]["giri"].clone(), record["budget"]["allarme"].clone(), record["budget"]["da"].clone()));
        h.advance(10.0);
    }
    let rounds: Vec<Value> = seen.iter().map(|(rounds, _, _)| rounds.clone()).collect();
    let alarms: Vec<Value> = seen.iter().map(|(_, alarm, _)| alarm.clone()).collect();
    assert_eq!(rounds, [json!(1), json!(2), json!(3), json!(4), json!(5)]);
    assert_eq!(alarms, [json!(false), json!(false), json!(false), json!(true), json!(true)]);
    assert!(seen.iter().all(|(_, _, since)| *since == json!(T0)));
    assert!(c.budget_alarm());
    assert_eq!(c.unknown_budget().reason.as_deref(), Some("sonda: troppe richieste (HTTP 429)"));
}

#[test]
fn a_known_budget_resets_the_count_and_a_full_one_is_no_alarm() {
    let h = Harness::new(
        vec![idle_budget(json!({"claude_5h_pct": null})), idle_budget(json!({"claude_5h_pct": 82.0, "stale": true})), idle_budget(json!({"claude_5h_pct": null}))],
        None,
    );
    let mut c = h.controller(Harness::options());
    assert_eq!(c.tick(false).unwrap()["budget"]["giri"], 1);
    assert_eq!(c.tick(false).unwrap()["budget"], json!({"sconosciuto": false, "claude_5h_pct": 82.0, "stale": true}));
    assert_eq!(c.tick(false).unwrap()["budget"]["giri"], 1);
}

#[test]
fn without_a_candidate_the_count_stays() {
    let h = Harness::new(vec![idle_budget(json!({"claude_5h_pct": null})), nothing()], None);
    let mut c = h.controller(Harness::options());
    c.tick(false).unwrap();
    assert_eq!(c.tick(false).unwrap()["budget"], Value::Null);
    assert_eq!(c.unknown_budget().rounds, 1);
}

/// `test_il_contatore_sopravvive_a_un_riavvio_e_l_allarme_scatta_alla_soglia`: a new controller at each tick, with
/// the count of the one before, as the state document gives it to #468's restart.
#[test]
fn the_count_goes_on_across_a_restart() {
    let mut count = UnknownBudget::default();
    let mut alarms = Vec::new();
    for _ in 0..5 {
        let h = Harness::new(vec![idle_budget(json!({"claude_5h_pct": null}))], None);
        let mut c = h.controller(Harness::options()).with_unknown_budget(count.clone());
        alarms.push(c.tick(false).unwrap()["budget"]["allarme"].clone());
        count = c.unknown_budget().clone();
    }
    assert_eq!(alarms, [json!(false), json!(false), json!(false), json!(true), json!(true)]);
    assert_eq!(count.rounds, 5);
}

// ---------- the postponements (TestFalsoStallo) ----------

#[test]
fn a_postponed_project_gets_its_signal_with_instant_reason_and_id() {
    let h = Harness::new(vec![work(&["a", "b"])], Some(4242));
    let record = h.controller(Harness::options()).tick(false).unwrap();
    assert_eq!(actions(&record), ["lanciato", "rinviato-limite-giro"]);
    let written = h.signals.written.lock().unwrap().clone();
    assert_eq!(written, [("b".to_owned(), json!({"v": 1, "t": T0, "id": "id-fixed", "motivo": "rinviato-limite-giro", "giro": 1}))]);
}

#[test]
fn a_step_alive_or_a_failed_launch_writes_no_signal_a_suspended_one_does() {
    let mut h = Harness::new(vec![work(&["a", "b", "c"])], Some(4242));
    Arc::get_mut(&mut h.surface).unwrap().failing.insert("c".to_owned());
    h.liveness.alive.lock().unwrap().insert(4242);
    h.steps.suspended.lock().unwrap().insert("b".to_owned());
    let mut c = h.controller(Options { max_steps: 3, ..Harness::options() });
    c.tick(false).unwrap();
    h.advance(900.0);
    let record = c.tick(false).unwrap();
    assert_eq!(actions(&record), ["saltato-passo-vivo", "saltato-sospesa", "errore-lancio"]);
    let projects: Vec<String> = h.signals.written.lock().unwrap().iter().map(|(project, value)| format!("{project} {}", value["motivo"])).collect();
    assert_eq!(projects, ["b \"saltato-sospesa\"", "b \"saltato-sospesa\""]);
}

/// A failed write of the signal does not stop the tick, and is recorded; a lost lock does stop it (#582).
#[test]
fn a_failed_signal_is_recorded_a_lost_lock_stops_the_tick() {
    let h = Harness::new(vec![work(&["a", "b"])], Some(4242));
    *h.signals.failure.lock().unwrap() = Some(StateError::Failed("disk full".to_owned()));
    let record = h.controller(Harness::options()).tick(false).unwrap();
    assert_eq!(record["rinvio_errore"], json!(["b: disk full"]));
    let h = Harness::new(vec![work(&["a", "b"])], Some(4242));
    *h.signals.failure.lock().unwrap() = Some(StateError::LockLost(LockLost("not ours".to_owned())));
    assert!(lost(h.controller(Harness::options()).tick(false)));
}

/// `test_l_attesa_per_una_sospesa_non_fa_crescere_la_serie`: a tick that ends before the precheck writes the global
/// signal with its decision, and runs no precheck.
struct Waiting;

impl regia_controller::engine::BeforePrecheck for Waiting {
    fn before(&mut self, record: &mut serde_json::Map<String, Value>, _tick: &regia_controller::engine::TickContext<'_>) -> Result<bool, TickError> {
        record.insert("decisione".to_owned(), json!("attesa-sospesa"));
        Ok(true)
    }
}

#[test]
fn a_tick_that_ends_before_the_precheck_postpones_every_project() {
    let h = Harness::new(vec![work(&["a"])], None);
    let record = h.controller(Harness::options()).with_before_precheck(Box::new(Waiting)).tick(false).unwrap();
    assert_eq!(record["decisione"], "attesa-sospesa");
    assert!(h.precheck.calls.lock().unwrap().is_empty());
    let written = h.signals.written.lock().unwrap().clone();
    assert_eq!(written.len(), 1);
    assert_eq!((written[0].0.as_str(), written[0].1["motivo"].clone()), ("controller", json!("attesa-sospesa")));
}

// ---------- processo_vivo and the reaping (#460/V5) ----------

/// `test_processo_vivo_su_un_processo_vero`, and a session that ended is reaped at the next look, also when its step
/// is no longer in the tick's memory.
#[test]
fn a_process_is_alive_until_it_ends_and_an_ended_session_is_reaped() {
    let probe = regia_conformance::bin_dir().join(format!("probe{}", std::env::consts::EXE_SUFFIX));
    let state = regia_conformance::prepare_state_dir(&format!("controller-reap-{}", std::process::id()));
    let input = state.join("input.txt");
    std::fs::write(&input, "").unwrap();
    let launch = |mode: &str, name: &str| {
        regia_exec::harness::launch(&probe, &[], &state)
            .env("REGIA_PROBE_MODE", mode)
            .env("REGIA_PROBE_BEAT", state.join("beat.txt"))
            .detach(&input, &state.join(name))
            .expect("the probe starts")
    };
    let mut beating = launch("beat", "beat.log");
    assert!(process_alive(beating.pid()), "the check does not kill it");
    assert!(matches!(beating.try_wait(), Ok(None)));
    beating.kill().unwrap();
    assert!(!process_alive(beating.pid()));
    assert!(!process_alive(0));

    let processes = Processes::new();
    let echo = launch("", "echo.log");
    processes.adopt(echo.pid());
    drop(echo);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while processes.pending() > 0 && std::time::Instant::now() < deadline {
        processes.reap();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(processes.pending(), 0, "the ended session is reaped");
}
