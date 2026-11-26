//! The resume of the sessions a usage limit stopped (#470, F28): the prototype's cases of
//! `tools/tests/test_sospese_controller.py` on the engine's tick, with the state's own database under `target/`, an
//! injected clock that does not move inside a tick (the prototype's `Orologio`), synthetic transcripts with the shape of
//! real ones (never real ones), and the precheck, the launch, the resume and the app's index faked:
//!
//! - `GiroConSospese`, `ProgettoOccupato`, `OrcaMaiViaCli`, `OptInDelTitolare`, `Riavvio`, `SenzaSospese`: 28 cases;
//! - `test_con_una_sospesa_non_lancia_nulla_e_non_segna_il_giorno` of `test_controller_seguito.py`, with the scheduled
//!   operations after the resume (#469/V14);
//! - Acceptance 1 of #470: a session stopped by the limit is resumed once, and only after the hour of its reset.
//!
//! The values are the prototype's words, so a word the format changes fails here. The prototype's `rilascia --tutte` is
//! a command without a name in the facade table yet (✋C): its case writes the release with the library.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, NaiveDateTime};
use regia_agents::{ChatHost, Dry, Env, Known, LaunchError, LaunchRequest, LaunchSurface, ProbeConsent, Receipt, SurfaceKind, WakeOutcome, WakeUp, claude_slug};
use regia_controller::engine::native::{StoreLock, StoreSignals, StoreSteps, new_record};
use regia_controller::engine::{Controller, Folders, Host, LaunchedStep, Liveness, Options, Ports, Precheck, PrecheckRun, StepReceipt, Steps};
use regia_controller::resident::Summary;
use regia_controller::resume::{OptIn, Resume, ResumeReceipt, Resumer, Settings, read_opt_in, release, releases};
use regia_controller::schedule::{ChatAgent, ChatSurface, Operations, Source, StoreSchedule};
use regia_store::{Clock as _, CursorName, Holder, LockStatus, ManualClock, ProjectId, Store, Ttl, WallTime};
use serde_json::{Map, Value, json};

const T0: f64 = 1_792_000_000.0;
/// The limit was written an hour ago; its hour is half an hour ahead, or ten minutes past.
const LIMIT_T: f64 = T0 - 3600.0;
const WAITING: f64 = T0 + 1800.0;
const PAST: f64 = T0 - 600.0;
const SID: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const CWD: &str = "C:\\Users\\prova\\progetto";
const HOLDER: &str = "controller-resume-test";

fn reserved() -> ProjectId {
    ProjectId::new("controller").unwrap()
}

fn registry_name() -> CursorName {
    CursorName::new("passi").unwrap()
}

fn ttl() -> Ttl {
    Ttl::from_secs(600).unwrap()
}

/// The prototype's `_iso`: UTC, to the second.
fn iso(at: f64) -> String {
    regia_agents::suspension::iso(at).unwrap()
}

// ---------- the transcripts, as `test_sospese.py` builds them ----------

fn stamp(at: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    let utc = DateTime::from_timestamp_micros((at * 1e6) as i64).unwrap();
    utc.format("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

fn user(at: f64, text: &str, sid: &str, entrypoint: &str) -> Value {
    json!({"type": "user", "timestamp": stamp(at), "sessionId": sid, "cwd": CWD, "entrypoint": entrypoint,
           "message": {"role": "user", "content": [{"type": "text", "text": text}]}})
}

fn answer(at: f64, text: &str, sid: &str, entrypoint: &str) -> Value {
    json!({"type": "assistant", "timestamp": stamp(at), "sessionId": sid, "cwd": CWD, "entrypoint": entrypoint,
           "message": {"role": "assistant", "model": "claude-test", "content": [{"type": "text", "text": text}]}})
}

/// The limit as Claude Code writes it: a synthetic message, a 429, `quotaLimits`.
fn limit(at: f64, resets: f64, sid: &str, entrypoint: &str) -> Value {
    json!({"type": "assistant", "timestamp": stamp(at), "sessionId": sid, "cwd": CWD, "entrypoint": entrypoint,
           "message": {"role": "assistant", "model": "<synthetic>", "stop_reason": "stop_sequence",
                       "content": [{"type": "text", "text": "You've hit your session limit \u{b7} resets 11pm (Europe/Rome)"}]},
           "apiError": "usage_limit_reached", "error": "rate_limit", "isApiErrorMessage": true, "apiErrorStatus": 429,
           "quotaLimits": {"status": "rejected", "resetsAt": resets, "rateLimitType": "five_hour", "isUsingOverage": false},
           "apiErrorParams": {"rate_limit_info": {"status": "rejected", "resetsAt": resets}}})
}

/// The entries Claude Code writes around the messages, which do not change the session's state.
fn around(at: f64) -> Vec<Value> {
    ["queue-operation", "last-prompt", "mode", "cost-state", "attachment"].iter().map(|kind| json!({"type": kind, "timestamp": stamp(at)})).collect()
}

fn notice(at: f64, sid: &str, entrypoint: &str) -> Value {
    user(at, "<task-notification>\n<task-id>abc</task-id>\n</task-notification>", sid, entrypoint)
}

fn write(path: &Path, entries: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text: String = entries.iter().map(|entry| format!("{entry}\n")).collect();
    fs::write(path, text).unwrap();
}

fn append(path: &Path, entries: &[Value]) {
    let mut all: Vec<Value> = fs::read_to_string(path).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    all.extend(entries.iter().cloned());
    write(path, &all);
}

fn touch(path: &Path, at: f64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs_f64(at)).unwrap();
}

// ---------- the fakes ----------

/// The prototype's `PrecheckFinto`: its answers in order, the last for the rest; it counts its calls.
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

/// The prototype's `LAVORO`: work for `projects`.
fn work(projects: &[&str]) -> PrecheckRun {
    let entries: Vec<Value> = projects.iter().map(|id| json!({"id": id, "esito": "new-activity", "motivo": "pronti", "eventi": 1, "pronti": 1})).collect();
    let line = json!({"v": 1, "outcome": "new-activity", "exit": 0, "motivo": "pronti", "cursore": [], "payload": {"progetti": entries}, "t": "2026-10-21T10:00:00Z"});
    PrecheckRun::from_output(Some(0), &line.to_string())
}

struct FakeFolders(String);

impl Folders for FakeFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        Ok(["a", "b"].iter().map(|id| ((*id).to_owned(), Some(self.0.clone()))).collect())
    }
}

/// The prototype's `LancioFinto`: a receipt with a pid and a session, nothing started.
struct FakeSurface {
    pid: Option<u32>,
    launched: Mutex<Vec<String>>,
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
        receipt.request = None;
        Ok(receipt)
    }

    fn wake(&self) -> WakeOutcome {
        WakeOutcome::fake()
    }
}

/// The processes alive: those of the set, or every one.
#[derive(Default)]
struct FakeLiveness {
    alive: Mutex<BTreeSet<u32>>,
    all: AtomicBool,
}

impl Liveness for FakeLiveness {
    fn alive(&self, pid: u32) -> bool {
        self.all.load(Ordering::SeqCst) || self.alive.lock().unwrap().contains(&pid)
    }
}

/// The prototype's `RiprendiFinto`: the sessions it was asked to resume; a receipt with pid 7000 + n, or the error.
#[derive(Default)]
struct FakeResumer {
    calls: Mutex<Vec<String>>,
    error: Mutex<Option<String>>,
    /// What the agent writes in the transcript once resumed, as a real one would.
    answers: Mutex<Option<PathBuf>>,
}

impl FakeResumer {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Resumer for FakeResumer {
    fn resume(&self, record: &Map<String, Value>) -> Result<ResumeReceipt, String> {
        let session = ["sessione_resume", "sessione"].iter().find_map(|key| record.get(*key).and_then(Value::as_str).filter(|text| !text.is_empty())).unwrap_or_default().to_owned();
        let n = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(session.clone());
            calls.len()
        };
        if let Some(error) = self.error.lock().unwrap().clone() {
            return Err(error);
        }
        if let Some(path) = self.answers.lock().unwrap().as_ref() {
            append(path, &[user(T0 + 1.0, "Continue where you left off", &session, "sdk-cli"), answer(T0 + 2.0, "riprendo", &session, "sdk-cli")]);
        }
        let pid = u32::try_from(7000 + n).unwrap();
        Ok(ResumeReceipt { pid: Some(pid), log: Some(format!("ripresa-{n}.log")), session: Some(session), transcript: None })
    }
}

/// The prototype's `cerca_orca`: what the app's index answers, `{}` by default (it answers, and knows no chat).
struct FakeHost(Mutex<Option<Known>>);

impl ChatHost for FakeHost {
    fn known(&self, _folder: &str) -> Option<Known> {
        self.0.lock().unwrap().clone()
    }
}

// ---------- a case ----------

/// How a step is registered, as the prototype's `passo` writes it.
struct StepOf {
    adapter: &'static str,
    origin: &'static str,
    pid: Option<u32>,
    state: &'static str,
    surface_session: Option<&'static str>,
    started: f64,
}

impl Default for StepOf {
    fn default() -> StepOf {
        StepOf { adapter: "cli", origin: "controller", pid: None, state: "lanciato", surface_session: None, started: T0 - 7200.0 }
    }
}

struct Case {
    folder: PathBuf,
    work: PathBuf,
    claude: PathBuf,
    clock: ManualClock,
    store: Store,
    holder: Holder,
    precheck: Arc<FakePrecheck>,
    surface: Arc<FakeSurface>,
    liveness: Arc<FakeLiveness>,
    resumer: Arc<FakeResumer>,
    host: Arc<FakeHost>,
    steps: Option<Arc<StoreSteps>>,
    controller: Option<Controller>,
}

impl Case {
    fn new(label: &str) -> Case {
        let folder = regia_conformance::prepare_state_dir(&format!("resume-{label}"));
        let work = folder.join("work");
        fs::create_dir_all(&work).unwrap();
        let clock = ManualClock::fixed(WallTime::from_unix_secs_f64(T0).unwrap());
        let store = Store::at(&folder).unwrap().with_clock(clock.clone());
        Case {
            claude: folder.join("claude"),
            work,
            clock,
            store,
            holder: Holder::new(HOLDER).unwrap(),
            precheck: Arc::new(FakePrecheck { answers: Mutex::new(vec![work_of_two()]), calls: AtomicUsize::new(0) }),
            surface: Arc::new(FakeSurface { pid: Some(4242), launched: Mutex::new(Vec::new()) }),
            liveness: Arc::new(FakeLiveness::default()),
            resumer: Arc::new(FakeResumer::default()),
            host: Arc::new(FakeHost(Mutex::new(Some(Known::new())))),
            steps: None,
            controller: None,
            folder,
        }
    }

    fn work_text(&self) -> String {
        self.work.display().to_string()
    }

    fn env(&self) -> Env {
        Env::from_pairs([("CLAUDE_CONFIG_DIR", self.claude.as_os_str())])
    }

    fn transcript_path(&self, name: &str) -> PathBuf {
        self.claude.join("projects").join(claude_slug(&self.work)).join(format!("{name}.jsonl"))
    }

    fn transcript(&self, name: &str, entries: &[Value]) -> PathBuf {
        let path = self.transcript_path(name);
        write(&path, entries);
        path
    }

    /// The registry as stored, by key.
    fn registry(&self) -> Map<String, Value> {
        let doc = self.store.cursor(&reserved(), &registry_name()).unwrap();
        doc.get("passi").and_then(Value::as_object).cloned().unwrap_or_default()
    }

    fn put(&self, key: String, record: Value) {
        let mut records = self.registry();
        records.insert(key, record);
        self.store.write_cursor(&reserved(), &registry_name(), json!({"v": 1, "passi": records})).unwrap();
    }

    /// The prototype's `passo`: a step already in the registry, as the controller writes it at the launch, with its
    /// transcript.
    fn step(&self, project: &str, name: &str, entries: &[Value], of: &StepOf) -> String {
        let path = self.transcript(name, entries);
        let receipt = StepReceipt {
            adapter: of.adapter.to_owned(),
            agent: "claude".to_owned(),
            model: None,
            effort: None,
            session: Some(name.to_owned()),
            transcript: Some(path.display().to_string()),
            pid: of.pid,
            log: None,
            started: true,
            surface_session: of.surface_session.map(str::to_owned),
            marker: (of.adapter == "orca").then(|| "prompt".to_owned()),
        };
        let folder = self.work_text();
        let (key, mut record) = new_record(&LaunchedStep { project, folder: &folder, prompt: "prompt", receipt: &receipt }, of.started);
        record["origine"] = json!(of.origin);
        record["stato"] = json!(of.state);
        self.put(key.clone(), record);
        key
    }

    /// The prototype's `sospesa`: the limit followed by the entries around it and a subagent's notice; `claude -p`
    /// writes the entrypoint `sdk-cli`, the app's chats `sdk-ts`.
    fn suspended(&self, name: &str, project: &str, resets: f64, of: &StepOf) -> String {
        let entrypoint = if of.adapter == "orca" { "sdk-ts" } else { "sdk-cli" };
        let mut entries = vec![user(LIMIT_T - 60.0, "passo", name, entrypoint), answer(LIMIT_T - 30.0, "fatto", name, entrypoint), limit(LIMIT_T, resets, name, entrypoint)];
        entries.extend(around(LIMIT_T + 1.0));
        entries.push(notice(LIMIT_T + 200.0, name, entrypoint));
        self.step(project, name, &entries, of)
    }

    /// A step on the app, with the app's id of its chat.
    fn app() -> StepOf {
        StepOf { adapter: "orca", surface_session: Some("claude_o_1"), ..StepOf::default() }
    }

    /// The controller of a run: the registry read once the lock is taken, the steps still launched back in memory, the
    /// resume first and then the hooks given.
    fn start(&mut self, settings: Settings, opt_in: Option<OptIn>, more: Vec<Box<dyn regia_controller::engine::BeforePrecheck>>) {
        let _ = self.store.release_lock(&reserved(), &self.holder);
        assert!(self.store.acquire_lock(&reserved(), &self.holder, ttl()).unwrap().is_ok());
        let steps = Arc::new(StoreSteps::new(self.store.clone(), self.holder.clone()));
        steps.load().unwrap();
        let ports = Ports {
            clock: Arc::new(self.clock.clone()),
            precheck: self.precheck.clone(),
            folders: Arc::new(FakeFolders(self.work_text())),
            liveness: self.liveness.clone(),
            surface: self.surface.clone(),
            lock: Arc::new(StoreLock::new(self.store.clone(), self.holder.clone())),
            signals: Arc::new(StoreSignals::new(self.store.clone(), self.holder.clone())),
            steps: steps.clone(),
            ids: Arc::new(|| "id-fixed".to_owned()),
        };
        let mut resume = Resume::new(&self.folder, self.env(), self.resumer.clone(), self.host.clone()).with_settings(settings).with_pid(1);
        if let Some(opt_in) = opt_in {
            resume = resume.with_opt_in(opt_in);
        }
        let mut controller = Controller::new(Options::default(), ports, WakeUp::new(ProbeConsent::Refused)).with_before_precheck(Box::new(resume));
        for hook in more {
            controller = controller.with_before_precheck(hook);
        }
        for step in steps.launched_steps(T0) {
            controller.restore(step);
        }
        self.steps = Some(steps);
        self.controller = Some(controller);
    }

    /// The prototype's `ctl()` with the opt-in empty, then `prendi_lock`.
    fn ctl(&mut self) {
        self.start(Settings::default(), Some(OptIn::default()), Vec::new());
    }

    fn tick(&mut self) -> Value {
        self.controller.as_mut().expect("a controller").tick(false).expect("a tick")
    }

    fn advance(&self, secs: f64) {
        self.clock.advance(secs);
    }

    fn launched(&self) -> Vec<String> {
        self.surface.launched.lock().unwrap().clone()
    }

    fn prechecks(&self) -> usize {
        self.precheck.calls.load(Ordering::SeqCst)
    }

    /// `da-riprendere.json`, as the brief reads it.
    fn to_resume(&self) -> Option<Value> {
        fs::read(self.folder.join("da-riprendere.json")).ok().map(|bytes| serde_json::from_slice(&bytes).unwrap())
    }

    fn first_to_resume(&self) -> Value {
        self.to_resume().unwrap()["sessioni"][0].clone()
    }

    fn status(&self, record: &Value) -> Value {
        let host = Host { pid: 1, holder: HOLDER.to_owned(), started: T0, interval_s: 900.0 };
        self.controller.as_ref().unwrap().status(record, &host)
    }

    fn the_record(&self) -> Value {
        self.registry().values().next().cloned().unwrap()
    }
}

fn work_of_two() -> PrecheckRun {
    work(&["a", "b"])
}

fn actions(record: &Value) -> BTreeMap<String, String> {
    record["progetti"].as_array().unwrap().iter().map(|row| (row["progetto"].as_str().unwrap().to_owned(), row["azione"].as_str().unwrap().to_owned())).collect()
}

fn decision(record: &Value) -> &str {
    record["decisione"].as_str().unwrap_or_default()
}

// ---------- GiroConSospese ----------

/// `test_sospesa_scaduta_una_ripresa_nessun_lancio_e_il_precheck_non_parte`.
#[test]
fn a_session_past_its_hour_is_resumed_nothing_launches_and_the_precheck_does_not_run() {
    let mut case = Case::new("past-resumed");
    case.suspended("s-a", "a", PAST, &StepOf { pid: Some(4242), ..StepOf::default() });
    case.ctl();
    let record = case.tick();
    assert_eq!(decision(&record), "ripresa");
    assert_eq!(case.resumer.calls(), ["s-a"]);
    assert!(case.launched().is_empty());
    assert_eq!(case.prechecks(), 0, "no precheck: the series of a stall does not grow");
    assert_eq!(record["lanciati"], json!([]));
    let resume = &record["riprese"][0];
    assert_eq!(
        (&resume["sessione"], &resume["adapter"], &resume["attesa"], &resume["effettivo"], &resume["pid"]),
        (&json!("s-a"), &json!("cli"), &json!(iso(PAST)), &json!(iso(T0)), &json!(7001))
    );
    assert_eq!(Summary::of(&record).resumes, [("s-a".to_owned(), "cli".to_owned())], "the summary says `ripresa: s-a (cli)`");
    let stored = case.the_record();
    assert_eq!((&stored["stato"], &stored["pid"]), (&json!("in-ripresa"), &json!(7001)));
}

/// `test_sospesa_non_scaduta_non_lancia_nulla_e_lo_dice_con_l_ora_attesa`.
#[test]
fn a_session_before_its_hour_launches_nothing_and_says_until_when() {
    let mut case = Case::new("waiting");
    case.suspended("s-a", "a", WAITING, &StepOf::default());
    case.ctl();
    let record = case.tick();
    assert_eq!(decision(&record), "attesa-sospesa");
    assert_eq!(record["attesa"], json!({"fino_a": iso(WAITING), "sessioni": 1}));
    assert!(case.resumer.calls().is_empty() && case.launched().is_empty() && case.prechecks() == 0);
    assert_eq!(Summary::of(&record).suspended_until, Some(iso(WAITING)), "the summary says `sospese fino a …`");
}

/// `test_non_scaduta_e_scaduta_insieme_si_riprende_la_scaduta_e_non_si_lancia`.
#[test]
fn of_a_session_waiting_and_one_past_its_hour_the_past_one_is_resumed() {
    let mut case = Case::new("waiting-and-past");
    case.suspended("s-attesa", "a", WAITING, &StepOf::default());
    case.suspended("s-scaduta", "b", PAST, &StepOf::default());
    case.ctl();
    let record = case.tick();
    assert_eq!((decision(&record), case.resumer.calls(), case.launched()), ("ripresa", vec!["s-scaduta".to_owned()], Vec::new()));
}

/// `test_una_sola_ripresa_per_giro_la_seconda_al_giro_dopo`.
#[test]
fn one_resume_a_tick_the_second_at_the_next_one() {
    let mut case = Case::new("one-a-tick");
    case.suspended("s-1", "a", PAST - 100.0, &StepOf::default());
    case.suspended("s-2", "b", PAST, &StepOf::default());
    case.liveness.all.store(true, Ordering::SeqCst);
    case.ctl();
    assert_eq!(decision(&case.tick()), "ripresa");
    assert_eq!(case.resumer.calls(), ["s-1"], "the oldest first");
    assert_eq!(decision(&case.tick()), "ripresa");
    assert_eq!(case.resumer.calls(), ["s-1", "s-2"]);
    assert!(case.launched().is_empty());
    assert_eq!(case.prechecks(), 0);
}

/// `test_ripresa_riuscita_poi_il_precheck_riprende_a_lanciare`.
#[test]
fn once_the_resume_answered_the_precheck_launches_again() {
    let mut case = Case::new("resumed-then-launch");
    let key = case.suspended("s-a", "a", PAST, &StepOf { pid: Some(4242), ..StepOf::default() });
    case.ctl();
    case.tick();
    // The session answers: its transcript no longer ends with the limit; the resumed process ended.
    append(&case.transcript_path("s-a"), &[user(T0 - 500.0, "Continue where you left off", "s-a", "sdk-cli"), answer(T0 - 400.0, "riprendo", "s-a", "sdk-cli")]);
    case.advance(900.0);
    let record = case.tick();
    assert_eq!(decision(&record), "lancio");
    assert_eq!(case.launched(), ["a"]);
    assert_eq!(case.registry()[&key]["stato"], "finito");
}

/// `test_ripresa_che_non_parte_si_ritenta_poi_passa_a_da_riprendere`.
#[test]
fn a_resume_that_does_not_start_is_tried_again_then_left_to_the_owner() {
    let mut case = Case::new("resume-fails");
    case.suspended("s-a", "a", PAST, &StepOf::default());
    *case.resumer.error.lock().unwrap() = Some("claude non trovato".to_owned());
    case.ctl();
    let first = case.tick();
    assert_eq!((decision(&first), case.launched()), ("ripresa-errore", Vec::new()));
    assert!(first["ripresa_errore"]["errore"].as_str().unwrap().contains("claude non trovato"));
    assert_eq!(decision(&case.tick()), "ripresa-errore");
    let third = case.tick();
    assert_ne!(decision(&third), "ripresa-errore", "two attempts failed: the owner's");
    assert_eq!(case.resumer.calls().len(), 2);
    assert!(case.first_to_resume()["motivo"].as_str().unwrap().contains("fallita"));
}

/// `test_ripresa_conclusa_con_limite_invariato_conta_come_fallita`.
#[test]
fn a_resume_that_ended_with_the_same_limit_counts_as_failed() {
    let mut case = Case::new("same-limit");
    case.suspended("s-a", "a", PAST, &StepOf::default());
    case.ctl();
    case.tick();
    case.tick();
    case.tick();
    assert_eq!(case.resumer.calls(), ["s-a", "s-a"]);
    assert!(case.first_to_resume()["motivo"].as_str().unwrap().contains("fallita"));
}

// ---------- ProgettoOccupato ----------

/// `test_sospesa_che_il_controller_non_riprende_ferma_tutti_finche_c_e`: a chat of the app (not to resume from here)
/// past its hour; while one is stopped, nothing else starts.
#[test]
fn a_session_the_controller_does_not_resume_stops_everything_while_it_is_there() {
    let mut case = Case::new("app-stops-all");
    case.suspended("s-a", "a", PAST, &StepOf { started: T0 - 4.0 * 3600.0, ..Case::app() });
    case.ctl();
    let record = case.tick();
    assert_eq!(decision(&record), "attesa-da-riprendere");
    assert!(case.launched().is_empty() && case.prechecks() == 0 && case.resumer.calls().is_empty());
    assert_eq!(record["attesa_da_riprendere"][0]["sessione"], "s-a");
}

/// `test_con_solo_progetto_il_progetto_resta_occupato_oltre_il_lock_e_gli_altri_ripartono`.
#[test]
fn with_only_its_project_held_the_project_stays_busy_past_its_lock_and_the_others_go_on() {
    let mut case = Case::new("only-project");
    case.suspended("s-a", "a", PAST, &StepOf { started: T0 - 4.0 * 3600.0, ..Case::app() });
    assert!(!matches!(case.store.lock_status(&ProjectId::new("a").unwrap()).unwrap(), LockStatus::Held(_)), "the project's own lock is gone");
    case.start(Settings { only_project: true, ..Settings::default() }, Some(OptIn::default()), Vec::new());
    let record = case.tick();
    assert_eq!(actions(&record), BTreeMap::from([("a".to_owned(), "saltato-sospesa".to_owned()), ("b".to_owned(), "lanciato".to_owned())]));
    assert_eq!(case.launched(), ["b"]);
    assert!(case.resumer.calls().is_empty());
}

/// `test_rilascio_del_titolare_sblocca_e_vale_solo_per_quel_limite`.
#[test]
fn the_owner_s_release_unblocks_and_holds_for_that_limit_only() {
    let mut case = Case::new("release");
    let key = case.suspended("s-a", "a", PAST, &Case::app());
    case.ctl();
    assert_eq!(decision(&case.tick()), "attesa-da-riprendere");
    // The owner closed the chat without the button.
    release(&case.folder, "s-a", T0).unwrap();
    let record = case.tick();
    assert_eq!(record["rilasci"], json!([{"progetto": "a", "sessione": "s-a"}]));
    assert_eq!(decision(&record), "lancio");
    assert_eq!(case.launched(), ["a"], "one step a tick, now also on `a`");
    assert!(releases(&case.folder).is_empty(), "the release is spent");
    assert_eq!(case.registry()[&key]["stato"], "finito");
    // The transcript still ends with the same limit: it does not open again.
    case.advance(900.0);
    assert_ne!(decision(&case.tick()), "attesa-da-riprendere");
    // A new limit of the same session opens it again.
    append(
        &case.transcript_path("s-a"),
        &[user(T0 + 100.0, "Continue where you left off", "s-a", "sdk-ts"), answer(T0 + 110.0, "fatto", "s-a", "sdk-ts"), limit(T0 + 200.0, T0 - 100.0, "s-a", "sdk-ts")],
    );
    case.advance(900.0);
    assert_eq!(decision(&case.tick()), "attesa-da-riprendere");
}

/// `test_rilascio_tutte_e_riga_di_comando`: the release of every session (the command line is ✋C: no name yet).
#[test]
fn a_release_of_every_session_closes_them_all() {
    let mut case = Case::new("release-all");
    case.suspended("s-1", "a", PAST, &StepOf { surface_session: Some("claude_o_1"), ..Case::app() });
    case.suspended("s-2", "b", PAST - 5.0, &StepOf { surface_session: Some("claude_o_2"), ..Case::app() });
    release(&case.folder, "*", T0).unwrap();
    assert_eq!(releases(&case.folder).iter().map(|(_, request)| request["sessione"].clone()).collect::<Vec<_>>(), [json!("*")]);
    case.ctl();
    let record = case.tick();
    assert_eq!(record["rilasci"].as_array().map(Vec::len), Some(2));
    assert_eq!(decision(&record), "lancio");
}

/// `test_rilascio_di_una_sessione_che_non_c_e_resta_un_giorno_poi_si_butta`.
#[test]
fn a_release_of_a_session_that_is_not_there_stays_a_day_then_goes() {
    let mut case = Case::new("release-unknown");
    let path = release(&case.folder, "mai-vista", T0).unwrap();
    case.ctl();
    case.tick();
    assert!(path.exists());
    case.advance(86_400.0 + 10.0);
    case.tick();
    assert!(!path.exists());
}

/// `test_passo_in_ripresa_tiene_occupato_il_progetto`: "Continue" written a minute ago, no answer yet.
#[test]
fn a_step_being_resumed_keeps_its_project_busy() {
    let mut case = Case::new("resuming-busy");
    case.step("a", "s-r", &[limit(LIMIT_T, PAST, "s-r", "sdk-cli"), user(T0 - 60.0, "Continue where you left off", "s-r", "sdk-cli")], &StepOf::default());
    case.ctl();
    let record = case.tick();
    assert_eq!(actions(&record), BTreeMap::from([("a".to_owned(), "saltato-sospesa".to_owned()), ("b".to_owned(), "lanciato".to_owned())]));
}

// ---------- OrcaMaiViaCli ----------

/// `test_sessione_orca_scaduta_non_si_riprende_via_cli_e_finisce_in_da_riprendere`.
#[test]
fn a_session_of_the_app_past_its_hour_is_never_resumed_from_here_and_is_to_resume_by_hand() {
    let mut case = Case::new("app-to-resume");
    case.suspended("s-a", "a", PAST, &Case::app());
    case.ctl();
    let record = case.tick();
    assert!(case.resumer.calls().is_empty());
    assert!(record.get("riprese").is_none());
    let document = case.to_resume().unwrap();
    assert_eq!(document["sessioni"].as_array().map(Vec::len), Some(1));
    let voice = &document["sessioni"][0];
    assert_eq!((&voice["progetto"], &voice["sessione"], &voice["adapter"], &voice["riprende"]), (&json!("a"), &json!("s-a"), &json!("orca"), &json!(iso(PAST))));
    assert!(voice["motivo"].as_str().unwrap().contains("Orca"));
    let how = voice["come"].as_str().unwrap();
    assert!(how.contains("pulsante Continue"));
    assert!(!how.replace("mai `claude --resume`", "").contains("claude --resume"), "it does not suggest the command line");
}

/// `test_transcript_di_chat_orca_anche_se_il_registro_dice_cli`.
#[test]
fn a_transcript_of_an_app_s_chat_is_never_resumed_even_if_the_registry_says_cli() {
    let mut case = Case::new("app-transcript");
    case.step("a", "s-x", &[answer(LIMIT_T - 30.0, "fatto", SID, "sdk-ts"), limit(LIMIT_T, PAST, "s-x", "sdk-ts")], &StepOf::default());
    case.ctl();
    case.tick();
    assert!(case.resumer.calls().is_empty());
    assert!(case.first_to_resume()["motivo"].as_str().unwrap().contains("sdk-ts"));
}

/// `test_orca_scaduta_ma_in_attesa_di_un_altra_sospesa_non_riprende_nulla`.
#[test]
fn a_session_of_the_app_past_its_hour_while_another_waits_resumes_nothing() {
    let mut case = Case::new("app-and-waiting");
    case.suspended("s-o", "a", PAST, &Case::app());
    case.suspended("s-c", "b", WAITING, &StepOf::default());
    case.ctl();
    let record = case.tick();
    assert_eq!((decision(&record), case.resumer.calls()), ("attesa-sospesa", Vec::new()));
    assert_eq!(case.first_to_resume()["sessione"], "s-o");
}

/// `test_worker_di_orca_dei_nostri_passi_sono_da_riprendere_con_il_comando_orca`: a step on the app, registered with its
/// id; a worker that session started is stopped.
#[test]
fn the_workers_of_our_steps_on_the_app_are_to_resume_by_hand_with_the_app_s_command() {
    let mut case = Case::new("app-workers");
    case.step(
        "a",
        "s-passo",
        &[answer(T0 - 5000.0, "fatto", SID, "sdk-ts"), user(T0 - 4000.0, "fine", SID, "sdk-ts")],
        &StepOf { adapter: "orca", surface_session: Some("claude_ORC_1"), ..StepOf::default() },
    );
    let preamble = "You are a dispatched worker.\nYour coordinator's Orca session ID is: orca_session_id:claude_ORC_1\n\
                    orca orchestration send --type heartbeat --task-id task_0123456789ab --dispatch-id ctx_0123456789ab";
    let worker = case.transcript("w-1", &[user(T0 - 3000.0, preamble, "w-1", "sdk-ts"), answer(LIMIT_T, "fatto", "w-1", "sdk-ts"), limit(LIMIT_T + 5.0, PAST, "w-1", "sdk-ts")]);
    touch(&worker, T0 - 3000.0);
    case.ctl();
    case.tick();
    let voice = case.first_to_resume();
    assert_eq!((&voice["origine"], &voice["sessione"], &voice["progetto"]), (&json!("worker-orca"), &json!("w-1"), &json!("a")));
    assert!(voice["comando_orca"].as_str().unwrap().contains("--to dispatch:ctx_0123456789ab"));
    assert!(case.resumer.calls().is_empty());
}

// ---------- OptInDelTitolare ----------

impl Case {
    /// A chat of the owner, outside the regia, stopped.
    fn owners_chat(&self, name: &str) -> PathBuf {
        self.transcript(name, &[answer(LIMIT_T - 30.0, "fatto", name, "cli"), limit(LIMIT_T, PAST, name, "cli")])
    }
}

/// `test_opt_in_vuoto_le_chat_del_titolare_sono_ignorate`.
#[test]
fn with_an_empty_opt_in_the_owner_s_chats_are_left_alone() {
    let mut case = Case::new("opt-in-empty");
    touch(&case.owners_chat("chat-1"), T0);
    case.ctl();
    let record = case.tick();
    assert_eq!(decision(&record), "lancio");
    assert_eq!(record["sospese"], json!([]));
    assert!(case.resumer.calls().is_empty());
    assert_eq!(case.to_resume().unwrap()["sessioni"], json!([]));
    assert!(case.registry().values().all(|record| !matches!(record["origine"].as_str(), Some("optin" | "worker-orca"))));
}

/// `test_opt_in_senza_file_nel_runtime_e_come_vuoto`.
#[test]
fn an_opt_in_without_its_file_in_the_state_folder_is_empty() {
    let mut case = Case::new("opt-in-no-file");
    case.owners_chat("chat-1");
    assert_eq!(read_opt_in(&case.folder.join("ripresa-optin.json")), OptIn::default());
    case.start(Settings::default(), None, Vec::new());
    assert_eq!(decision(&case.tick()), "lancio");
}

/// `test_opt_in_per_id_si_riprende_se_orca_non_la_conosce`.
#[test]
fn a_session_allowed_by_its_id_is_resumed_when_the_app_does_not_know_it() {
    let mut case = Case::new("opt-in-by-id");
    touch(&case.owners_chat("chat-ok"), T0);
    case.start(Settings::default(), Some(OptIn { sessions: vec!["chat-ok".to_owned()], folders: Vec::new() }), Vec::new());
    let record = case.tick();
    assert_eq!((decision(&record), case.resumer.calls()), ("ripresa", vec!["chat-ok".to_owned()]));
    assert_eq!(record["riprese"][0]["adapter"], "cli");
}

/// `test_opt_in_di_una_chat_di_orca_o_con_orca_muto_non_si_riprende`.
#[test]
fn an_allowed_session_the_app_shows_or_an_app_that_does_not_answer_resumes_nothing() {
    let mut case = Case::new("opt-in-app");
    touch(&case.owners_chat("chat-orca"), T0);
    *case.host.0.lock().unwrap() = Some(Known::from([("chat-orca".to_owned(), Some("claude_x_1".to_owned()))]));
    let opt_in = OptIn { sessions: vec!["chat-orca".to_owned()], folders: Vec::new() };
    case.start(Settings::default(), Some(opt_in.clone()), Vec::new());
    case.tick();
    assert!(case.resumer.calls().is_empty());
    assert!(case.first_to_resume()["motivo"].as_str().unwrap().contains("Orca"));
    // The app does not answer: fail-closed.
    *case.host.0.lock().unwrap() = None;
    case.start(Settings::default(), Some(opt_in), Vec::new());
    case.tick();
    assert!(case.resumer.calls().is_empty());
    assert!(case.first_to_resume()["motivo"].as_str().unwrap().contains("non risponde"));
}

// ---------- Riavvio ----------

/// `test_controller_riavviato_ritrova_i_passi_e_non_rilancia`.
#[test]
fn a_restarted_controller_finds_its_steps_and_launches_none_again() {
    let mut case = Case::new("restart-steps");
    *case.precheck.answers.lock().unwrap() = vec![work(&["a"])];
    case.liveness.all.store(true, Ordering::SeqCst);
    case.ctl();
    let first = case.tick();
    assert_eq!(first["lanciati"].as_array().unwrap().iter().map(|launch| launch["progetto"].clone()).collect::<Vec<_>>(), [json!("a")]);
    let stored = case.the_record();
    for key in ["progetto", "adapter", "agente", "modello", "effort", "sessione", "sessione_resume", "transcript", "pid", "avviato", "stato"] {
        assert!(stored.get(key).is_some(), "{key}");
    }
    assert_eq!(
        (&stored["progetto"], &stored["adapter"], &stored["agente"], &stored["sessione"], &stored["pid"], &stored["stato"]),
        (&json!("a"), &json!("cli"), &json!("claude"), &json!("sess-a"), &json!(4242), &json!("lanciato"))
    );
    // A new controller, the process restarted: the same memory from the state.
    case.advance(60.0);
    case.liveness.all.store(false, Ordering::SeqCst);
    case.liveness.alive.lock().unwrap().insert(4242);
    case.surface.launched.lock().unwrap().clear();
    case.ctl();
    assert_eq!(case.controller.as_ref().unwrap().alive_projects(), ["a"]);
    let second = case.tick();
    assert_eq!(second["progetti"][0]["azione"], "saltato-passo-vivo");
    assert!(case.launched().is_empty());
}

/// `test_controller_riavviato_ritrova_la_sospesa_e_la_riprende`.
#[test]
fn a_restarted_controller_finds_the_suspended_session_and_resumes_it() {
    let mut case = Case::new("restart-suspended");
    case.suspended("s-a", "a", PAST, &StepOf { pid: Some(4242), ..StepOf::default() });
    case.ctl();
    assert_eq!(case.controller.as_ref().unwrap().alive_projects(), ["a"], "the step still launched is back in memory");
    let record = case.tick();
    assert_eq!((decision(&record), case.resumer.calls()), ("ripresa", vec!["s-a".to_owned()]));
}

/// `test_passo_finito_prima_del_riavvio_e_sospeso_al_riavvio_viene_riaperto`.
#[test]
fn a_step_finished_before_the_restart_and_suspended_is_opened_again() {
    let mut case = Case::new("finished-suspended");
    case.suspended("s-a", "a", PAST, &StepOf { pid: Some(4242), state: "finito", ..StepOf::default() });
    case.ctl();
    let record = case.tick();
    assert_eq!((decision(&record), case.resumer.calls()), ("ripresa", vec!["s-a".to_owned()]));
}

/// `test_passo_orca_la_sessione_si_ritrova_dal_transcript_e_poi_e_sospesa`: registered without the session to resume
/// (the file was not there at the launch); the transcript starts with the marker the receipt gave (#462/V7).
#[test]
fn a_step_on_the_app_finds_its_session_by_the_receipt_s_marker_and_then_is_suspended() {
    let mut case = Case::new("app-marker");
    let prompt = "Sei un passo di orchestratore della regia sul progetto a (prova)";
    let marker: String = prompt.chars().take(60).collect();
    let receipt = StepReceipt {
        adapter: "orca".to_owned(),
        agent: "claude".to_owned(),
        model: None,
        effort: None,
        session: Some("claude_o_9".to_owned()),
        transcript: None,
        pid: None,
        log: None,
        started: true,
        surface_session: Some("claude_o_9".to_owned()),
        marker: Some(marker),
    };
    let folder = case.work_text();
    let (key, record) = new_record(&LaunchedStep { project: "a", folder: &folder, prompt, receipt: &receipt }, T0 - 3000.0);
    assert_eq!(record["sessione_resume"], Value::Null);
    case.put(key.clone(), record);
    let found = case.transcript(
        "uuid-trovato",
        &[user(T0 - 2900.0, prompt, "uuid-trovato", "sdk-ts"), answer(LIMIT_T, "fatto", "uuid-trovato", "sdk-ts"), limit(LIMIT_T + 1.0, PAST, "uuid-trovato", "sdk-ts")],
    );
    touch(&found, T0 - 2900.0);
    case.ctl();
    case.tick();
    let stored = &case.registry()[&key];
    assert_eq!((&stored["sessione_resume"], &stored["stato"]), (&json!("uuid-trovato"), &json!("sospesa")));
    assert_eq!(case.first_to_resume()["sessione"], "uuid-trovato");
    assert!(case.resumer.calls().is_empty());
}

// ---------- SenzaSospese ----------

/// `test_senza_sospese_il_giro_e_quello_di_prima`.
#[test]
fn without_suspended_sessions_the_tick_is_as_before() {
    let mut case = Case::new("none");
    case.ctl();
    let record = case.tick();
    assert_eq!((decision(&record), &record["sospese"]), ("lancio", &json!([])));
    assert_eq!(case.to_resume(), Some(json!({"v": 1, "aggiornato": iso(T0), "sessioni": []})));
}

/// `test_no_ripresa_salta_tutto`: `--no-resume` leaves the hook out.
#[test]
fn without_the_resume_everything_is_skipped() {
    let case = Case::new("no-resume");
    case.suspended("s-a", "a", WAITING, &StepOf::default());
    let _ = case.store.release_lock(&reserved(), &case.holder);
    assert!(case.store.acquire_lock(&reserved(), &case.holder, ttl()).unwrap().is_ok());
    let steps = Arc::new(StoreSteps::new(case.store.clone(), case.holder.clone()));
    steps.load().unwrap();
    let ports = Ports {
        clock: Arc::new(case.clock.clone()),
        precheck: case.precheck.clone(),
        folders: Arc::new(FakeFolders(case.work_text())),
        liveness: case.liveness.clone(),
        surface: case.surface.clone(),
        lock: Arc::new(StoreLock::new(case.store.clone(), case.holder.clone())),
        signals: Arc::new(StoreSignals::new(case.store.clone(), case.holder.clone())),
        steps: steps.clone(),
        ids: Arc::new(|| "id-fixed".to_owned()),
    };
    let mut controller = Controller::new(Options::default(), ports, WakeUp::new(ProbeConsent::Refused));
    for step in steps.launched_steps(T0) {
        controller.restore(step);
    }
    let record = controller.tick(false).unwrap();
    assert_eq!(decision(&record), "lancio");
    assert!(record.get("sospese").is_none());
}

/// `test_lo_stato_del_controller_riporta_le_sospese`.
#[test]
fn the_state_file_lists_the_suspended_sessions() {
    let mut case = Case::new("status");
    case.suspended("s-a", "a", WAITING, &StepOf::default());
    case.ctl();
    let record = case.tick();
    let status = case.status(&record);
    assert_eq!(status["sospese"][0]["progetto"], "a");
    assert_eq!(status["decisione"], "attesa-sospesa");
    assert_eq!(status["da_riprendere"], 0);
}

// ---------- #469/V14: the scheduled operations after the resume ----------

struct FakeChat(Mutex<usize>);

impl LaunchSurface for FakeChat {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Orca
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        *self.0.lock().unwrap() += 1;
        let mut receipt = Dry::new().launch(request)?;
        receipt.session = Some("chat-1".to_owned());
        Ok(receipt)
    }

    fn wake(&self) -> WakeOutcome {
        WakeOutcome::fake()
    }
}

fn utc_local(secs: f64) -> Option<NaiveDateTime> {
    let at = WallTime::from_unix_secs_f64(secs)?;
    DateTime::from_timestamp_micros(at.unix_micros()).map(|utc| utc.naive_utc())
}

fn utc_secs(text: &str) -> f64 {
    WallTime::parse_iso(text).unwrap().unix_secs_f64()
}

/// `test_con_una_sospesa_non_lancia_nulla_e_non_segna_il_giorno` (`test_controller_seguito.py`): with a session
/// suspended the dailies' chat waits, the tick's decision is the resume's, and the day is not spent.
#[test]
fn with_a_suspended_session_the_dailies_wait_and_the_tick_waits_for_it() {
    let mut case = Case::new("schedule-suspended");
    case.clock.advance(utc_secs("2026-10-22T09:00:00Z") - T0);
    let now = case.clock.now().unix_secs_f64();
    case.put(
        "a/s1".to_owned(),
        json!({"progetto": "a", "stato": "sospesa", "origine": "controller", "adapter": "cli", "agente": "claude", "sessione": "s1",
               "riprende": now + 36_000.0, "riprende_iso": "2030-01-01T00:00:00Z"}),
    );
    let chat = Arc::new(FakeChat(Mutex::new(0)));
    let config = json!({"operazioni": [{"nome": "giornalieri", "ora": "08:30", "giorni": "lun-ven", "azione": "chat", "prompt": "regia: giornalieri"}]});
    let operations = Operations::new(Source::Document(config), Arc::new(StoreSchedule::new(case.store.clone(), case.holder.clone())), ChatSurface::Open(chat.clone()))
        .with_local(utc_local)
        .with_home(Some(case.work.clone()))
        .with_closings(case.folder.join("giornalieri"))
        .with_agent(ChatAgent::default());
    case.start(Settings::default(), Some(OptIn::default()), vec![Box::new(operations)]);
    let record = case.tick();
    assert_eq!(record["operazioni"], json!([{"nome": "giornalieri", "azione": "rinviata-sospesa"}]));
    assert_eq!(decision(&record), "attesa-sospesa", "the resume's decision (#469/V14)");
    assert_eq!(*chat.0.lock().unwrap(), 0);
    // The suspended session was resumed meanwhile: the same day the chat opens.
    let steps = case.steps.clone().unwrap();
    let mut finished = steps.records()["a/s1"].clone();
    finished["stato"] = json!("finito");
    steps.save_records(Map::from_iter([("a/s1".to_owned(), finished)]), now).unwrap();
    case.clock.advance(5.0 * 3600.0);
    let record = case.tick();
    assert_eq!(record["operazioni"][0]["azione"], "lanciata");
    assert_eq!(*chat.0.lock().unwrap(), 1);
}

// ---------- Acceptance 1 ----------

/// Acceptance 1 of #470: a synthetic transcript stopped by the limit is resumed once, and only after the hour of its
/// reset (with the prototype's minute of margin), on the injected clock; neither the next ticks nor a restart resume it
/// again.
#[test]
fn a_session_stopped_by_the_limit_is_resumed_once_after_its_reset() {
    let mut case = Case::new("acceptance-1");
    let resets = T0 + 300.0;
    case.suspended("s-a", "a", resets, &StepOf::default());
    *case.resumer.answers.lock().unwrap() = Some(case.transcript_path("s-a"));
    case.ctl();
    assert_eq!(decision(&case.tick()), "attesa-sospesa", "before the reset");
    case.advance(resets + 59.0 - T0);
    assert_eq!(decision(&case.tick()), "attesa-sospesa", "a minute of margin after the reset");
    assert!(case.resumer.calls().is_empty());
    case.advance(1.0);
    let record = case.tick();
    assert_eq!(decision(&record), "ripresa");
    assert_eq!(record["riprese"][0]["attesa"], json!(iso(resets)));
    for _ in 0..3 {
        case.advance(900.0);
        assert_ne!(decision(&case.tick()), "ripresa");
    }
    case.ctl();
    case.advance(900.0);
    assert_ne!(decision(&case.tick()), "ripresa", "a restart does not resume it again");
    assert_eq!(case.resumer.calls(), ["s-a"], "resumed once");
    assert_eq!(case.the_record()["stato"], "finito");
}
