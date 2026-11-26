//! The scheduled operations (#469, F27): the prototype's cases of `tools/tests/test_controller_seguito.py` §3
//! (`TestConfig`, `TestOperazioni`, `TestChatReale`) on the engine's tick, with the state's own database under
//! `target/`, an injected clock, the local time injected as UTC (so no case depends on the machine's zone: the
//! prototype's `epoca` builds local instants for the same reason), and the chat's surface faked as `ChatFinta`.
//!
//! - The start by hand of the dailies (`giornalieri avvia`, F274) is #480's: a case starts them by writing the cursor
//!   `avvia-giornalieri` as the prototype's `avvia_giornalieri` writes it, with its rule of one chat waiting.
//! - Also: D1 for the chat (#467/V8, `test_controller_lock_lanci.py`), the cursors written only by the lock's holder
//!   (#582), and the record of a step launched on Orca (#467/V11).
//!
//! The values are the prototype's words, so a word the format changes fails here. 2026-10-22 is a Thursday.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, NaiveDateTime};
use regia_agents::{Dry, LaunchError, LaunchRequest, LaunchSurface, ProbeConsent, Receipt, ServiceId, SurfaceKind, WakeOutcome, WakeUp};
use regia_controller::engine::native::{StoreLock, StoreSignals, new_record};
use regia_controller::engine::{
    Controller, ControllerLock, Folders, Host, LaunchedStep, Liveness, Options, Ports, Precheck, PrecheckRun, StateError, StepReceipt, Steps, TickError,
};
use regia_controller::schedule::{
    Action, ChatAgent, ChatSurface, Operations, ScheduleState, Source, StoreSchedule, chat_open, chat_surface_kind, last_launch, normalize, parse_days,
};
use regia_exec::{Effort, ModelId};
use regia_store::{Clock as _, CursorName, Holder, LockStatus, ManualClock, ProjectId, Store, Ttl, WallTime};
use serde_json::{Map, Value, json};

const HOLDER: &str = "controller-rust-test";
const THIEF: &str = "controller-ladro";

fn dailies() -> Value {
    json!({"operazioni": [{"nome": "giornalieri", "ora": "08:30", "giorni": "lun-ven", "azione": "chat", "prompt": "regia: giornalieri"}]})
}

fn evening() -> Value {
    json!({"operazioni": [{"nome": "sera", "ora": "18:00", "giorni": "tutti", "azione": "chat", "prompt": "regia: brief"}]})
}

/// The UTC instant of a day of October 2026, as the cases' local time.
fn instant(day: u32, hour: u32, minute: u32) -> String {
    format!("2026-10-{day:02}T{hour:02}:{minute:02}:00Z")
}

fn secs(day: u32, hour: u32, minute: u32) -> f64 {
    WallTime::parse_iso(&instant(day, hour, minute)).unwrap().unix_secs_f64()
}

/// The closing file's name of a chat started at that minute: `AAAA-MM-GG-HHMM.md`, in UTC.
fn closing_name(day: u32, hour: u32, minute: u32) -> String {
    format!("2026-10-{day:02}-{hour:02}{minute:02}.md")
}

fn utc_local(secs: f64) -> Option<NaiveDateTime> {
    let at = WallTime::from_unix_secs_f64(secs)?;
    DateTime::from_timestamp_micros(at.unix_micros()).map(|utc| utc.naive_utc())
}

fn reserved() -> ProjectId {
    ProjectId::new("controller").unwrap()
}

fn cursor_name(name: &str) -> CursorName {
    CursorName::new(name).unwrap()
}

fn ttl() -> Ttl {
    Ttl::from_secs(600).unwrap()
}

// ---------- the fakes ----------

/// The precheck of `NESSUN_EVENTO`: idle, nothing to launch.
struct Idle;

impl Precheck for Idle {
    fn run(&self, _projects: &[String], _timeout_s: f64) -> PrecheckRun {
        let line = json!({"v": 1, "outcome": "idle", "exit": 3, "motivo": "nessun-evento", "cursore": [], "payload": {"progetti": []}, "t": "2026-10-22T00:00:00Z"});
        PrecheckRun::from_output(Some(3), &line.to_string())
    }
}

struct NoFolders;

impl Folders for NoFolders {
    fn folders(&self) -> Result<std::collections::BTreeMap<String, Option<String>>, String> {
        Ok(std::collections::BTreeMap::new())
    }
}

struct Nobody;

impl Liveness for Nobody {
    fn alive(&self, _pid: u32) -> bool {
        false
    }
}

/// The registry, as far as the operations read it: the projects with a session suspended or resuming, and no step.
#[derive(Default)]
struct Registry {
    suspended: Mutex<BTreeSet<String>>,
    launched: AtomicUsize,
}

impl Steps for Registry {
    fn launched(&self, step: &LaunchedStep<'_>, _at: f64) -> Result<String, StateError> {
        self.launched.fetch_add(1, Ordering::SeqCst);
        Ok(step.project.to_owned())
    }

    fn finished(&self, _key: &str, _at: f64) -> Result<(), StateError> {
        Ok(())
    }

    fn suspended_projects(&self) -> BTreeSet<String> {
        self.suspended.lock().unwrap().clone()
    }
}

type Hook = Box<dyn FnMut() + Send>;

/// The controller's lock in the state, renewed as its holder; it counts the renewals.
struct CountingLock {
    inner: StoreLock,
    renewals: AtomicUsize,
}

impl ControllerLock for CountingLock {
    fn renew(&self) -> Result<(), StateError> {
        self.renewals.fetch_add(1, Ordering::SeqCst);
        self.inner.renew()
    }
}

/// What `ChatFinta` does at a call: the chat opens, Orca is not there before any effect, or the launch fails after it.
#[derive(Clone)]
enum Chat {
    Opens,
    NotAvailable(&'static str),
    FailsAfter(&'static str),
}

/// The prototype's `ChatFinta`: the calls, in order, each with the next outcome and the last one for the rest; `during`
/// runs inside a call, as a user who starts the dailies while Orca opens the chat.
struct FakeChat {
    outcomes: Mutex<Vec<Chat>>,
    calls: Mutex<Vec<LaunchRequest>>,
    during: Mutex<Option<Hook>>,
}

impl FakeChat {
    fn new(outcomes: Vec<Chat>) -> Arc<FakeChat> {
        Arc::new(FakeChat { outcomes: Mutex::new(outcomes), calls: Mutex::new(Vec::new()), during: Mutex::new(None) })
    }

    fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl LaunchSurface for FakeChat {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Orca
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        let n = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(request.clone());
            calls.len()
        };
        if let Some(during) = self.during.lock().unwrap().as_mut() {
            during();
        }
        let outcome = {
            let mut outcomes = self.outcomes.lock().unwrap();
            if outcomes.len() > 1 { outcomes.remove(0) } else { outcomes.first().cloned().unwrap_or(Chat::Opens) }
        };
        match outcome {
            Chat::Opens => {
                let mut receipt = Dry::new().launch(request)?;
                receipt.surface = SurfaceKind::Orca;
                receipt.session = Some(format!("chat-{n}"));
                receipt.log = None;
                Ok(receipt)
            }
            Chat::NotAvailable(reason) => Err(LaunchError::before(reason)),
            Chat::FailsAfter(reason) => Err(LaunchError::AfterEffect(reason.to_owned())),
        }
    }

    fn wake(&self) -> WakeOutcome {
        WakeOutcome::fake()
    }
}

/// The cursors of the operations, with a failure or a hook in place of the state's own behaviour.
struct Cursors {
    inner: StoreSchedule,
    failing_write: Option<&'static str>,
    after_start_read: Mutex<Option<Hook>>,
}

impl ScheduleState for Cursors {
    fn operations(&self) -> Result<Map<String, Value>, StateError> {
        self.inner.operations()
    }

    fn started_by_hand(&self) -> Result<Map<String, Value>, StateError> {
        let read = self.inner.started_by_hand();
        if let Some(hook) = self.after_start_read.lock().unwrap().as_mut() {
            hook();
        }
        read
    }

    fn write(&self, operations: &Map<String, Value>) -> Result<(), StateError> {
        match self.failing_write {
            Some(error) => Err(StateError::Failed(error.to_owned())),
            None => self.inner.write(operations),
        }
    }
}

// ---------- a case ----------

struct Case {
    folder: PathBuf,
    closings: PathBuf,
    clock: ManualClock,
    store: Store,
    holder: Holder,
    chat: Arc<FakeChat>,
    registry: Arc<Registry>,
    lock: Arc<CountingLock>,
    cursors: Arc<Cursors>,
    controller: Controller,
}

/// How a case starts: its configuration, the chat's surface and the cursors' failure.
struct Setup {
    config: Source,
    chat: Option<Vec<Chat>>,
    failing_write: Option<&'static str>,
}

impl Setup {
    fn of(config: Value) -> Setup {
        Setup { config: Source::Document(config), chat: Some(vec![Chat::Opens]), failing_write: None }
    }
}

impl Case {
    /// The prototype's `ctl()`: the controller with the lock taken, the configuration given, the chat faked, the closing
    /// files in a folder of the case, never the dossier's.
    fn new(label: &str, setup: Setup) -> Case {
        let folder = regia_conformance::prepare_state_dir(&format!("schedule-{label}"));
        let clock = ManualClock::fixed(WallTime::parse_iso(&instant(21, 23, 0)).unwrap());
        let store = Store::at(&folder).unwrap().with_clock(clock.clone());
        let holder = Holder::new(HOLDER).unwrap();
        assert!(store.acquire_lock(&reserved(), &holder, ttl()).unwrap().is_ok());
        let closings = folder.join("giornalieri");
        let chat = FakeChat::new(setup.chat.clone().unwrap_or_else(|| vec![Chat::Opens]));
        let mut case = Case {
            folder: folder.clone(),
            closings,
            clock: clock.clone(),
            store: store.clone(),
            holder: holder.clone(),
            chat,
            registry: Arc::new(Registry::default()),
            lock: Arc::new(CountingLock { inner: StoreLock::new(store.clone(), holder.clone()), renewals: AtomicUsize::new(0) }),
            cursors: Arc::new(Cursors { inner: StoreSchedule::new(store.clone(), holder.clone()), failing_write: setup.failing_write, after_start_read: Mutex::new(None) }),
            controller: Controller::new(Options::default(), empty_ports(&store, &clock, &holder), WakeUp::new(ProbeConsent::Refused)),
        };
        case.controller = case.build(setup);
        case
    }

    fn build(&mut self, setup: Setup) -> Controller {
        let ports = Ports {
            clock: Arc::new(self.clock.clone()),
            precheck: Arc::new(Idle),
            folders: Arc::new(NoFolders),
            liveness: Arc::new(Nobody),
            surface: Arc::new(Dry::new()),
            lock: self.lock.clone(),
            signals: Arc::new(StoreSignals::new(self.store.clone(), self.holder.clone())),
            steps: self.registry.clone(),
            ids: Arc::new(|| "id-fixed".to_owned()),
        };
        let chat = match setup.chat {
            Some(_) => ChatSurface::Open(self.chat.clone()),
            None => ChatSurface::None,
        };
        let agent = ChatAgent { service: ServiceId::Claude, model: Some(ModelId::parse("opus").unwrap()), effort: Some(Effort::High) };
        let operations = Operations::new(setup.config, self.cursors.clone(), chat)
            .with_local(utc_local)
            .with_home(Some(self.folder.clone()))
            .with_closings(self.closings.clone())
            .with_agent(agent);
        Controller::new(Options::default(), ports, WakeUp::new(ProbeConsent::Refused)).with_before_precheck(Box::new(operations))
    }

    /// The prototype's `alle`: the clock moved forward to that instant, and a tick.
    fn at(&mut self, day: u32, hour: u32, minute: u32) -> Value {
        self.move_to(day, hour, minute);
        self.controller.tick(false).expect("a tick")
    }

    fn move_to(&self, day: u32, hour: u32, minute: u32) {
        let step = secs(day, hour, minute) - self.clock.now().unix_secs_f64();
        assert!(step >= 0.0, "the clock only moves forward");
        self.clock.advance(step);
    }

    /// The prototype's `rilascia_lock` and a new `ctl()` on the same state: a restart, with a new chat.
    fn restart(&mut self, setup: Setup) {
        assert!(self.store.release_lock(&reserved(), &self.holder).unwrap().is_ok());
        assert!(self.store.acquire_lock(&reserved(), &self.holder, ttl()).unwrap().is_ok());
        self.chat = FakeChat::new(setup.chat.clone().unwrap_or_else(|| vec![Chat::Opens]));
        self.controller = self.build(setup);
    }

    /// The state document after `record`'s tick, as the host writes it to `controller-stato.json`.
    fn status(&self, record: &Value) -> Value {
        let host = Host { pid: 1, holder: HOLDER.to_owned(), started: secs(21, 23, 0), interval_s: 900.0 };
        self.controller.status(record, &host)
    }

    fn operations_cursor(&self) -> Value {
        self.store.cursor(&reserved(), &cursor_name("operazioni")).unwrap()
    }

    /// The prototype's `chiudi`: the closing file of the dailies' chat started at that minute.
    fn close(&self, day: u32, hour: u32, minute: u32) {
        fs::create_dir_all(&self.closings).unwrap();
        fs::write(self.closings.join(closing_name(day, hour, minute)), "# Giornalieri\n").unwrap();
    }

    /// The prototype's `avvia_giornalieri` at the clock's instant, as far as the tick reads it: the rule of one chat
    /// waiting (`force` passes over it), the chat on the case's chat, and the cursor `avvia-giornalieri` written before
    /// and after it. Its outcome's word.
    fn start_by_hand(&self, force: bool) -> &'static str {
        start_by_hand(&self.store, &self.clock, &self.chat, &self.closings, force)
    }
}

fn empty_ports(store: &Store, clock: &ManualClock, holder: &Holder) -> Ports {
    Ports {
        clock: Arc::new(clock.clone()),
        precheck: Arc::new(Idle),
        folders: Arc::new(NoFolders),
        liveness: Arc::new(Nobody),
        surface: Arc::new(Dry::new()),
        lock: Arc::new(StoreLock::new(store.clone(), holder.clone())),
        signals: Arc::new(StoreSignals::new(store.clone(), holder.clone())),
        steps: Arc::new(Registry::default()),
        ids: Arc::new(|| "id-fixed".to_owned()),
    }
}

fn start_by_hand(store: &Store, clock: &ManualClock, chat: &FakeChat, closings: &Path, force: bool) -> &'static str {
    let started = cursor_name("avvia-giornalieri");
    let before = store.cursor(&reserved(), &started).unwrap();
    let before_map = before.as_object().cloned().unwrap_or_default();
    let operations = store.cursor(&reserved(), &cursor_name("operazioni")).unwrap();
    let entry = operations["ops"]["giornalieri"].as_object().cloned().unwrap_or_default();
    if !force && last_launch(&entry, &before_map).is_some_and(|last| chat_open(closings, &last)) {
        return "precedente-aperta";
    }
    let now = clock.now();
    let t = now.to_iso_seconds();
    let today = &t[..10];
    store.write_cursor(&reserved(), &started, json!({"v": 1, "giorno": today, "t": t, "esito": "in-corso", "origine": "avvia"})).unwrap();
    let request = LaunchRequest::new(ServiceId::Claude, "giornalieri", closings.parent().unwrap(), "regia: giornalieri").unwrap();
    match chat.launch(&request) {
        Err(LaunchError::BeforeEffect(_)) => {
            let back = if before.is_null() { json!({"v": 1}) } else { before };
            store.write_cursor(&reserved(), &started, back).unwrap();
            "non-disponibile"
        }
        Err(LaunchError::AfterEffect(_)) => "errore-lancio",
        Ok(receipt) => {
            let done = json!({"v": 1, "giorno": today, "t": t, "esito": "lanciata", "origine": "avvia", "sessione": receipt.session});
            store.write_cursor(&reserved(), &started, done).unwrap();
            "lanciata"
        }
    }
}

/// The operations of a tick's line, as the prototype's `voci`: name and action.
fn voices(record: &Value) -> Vec<(String, String)> {
    record["operazioni"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|voice| (voice["nome"].as_str().unwrap().to_owned(), voice["azione"].as_str().unwrap().to_owned()))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(name, action)| ((*name).to_owned(), (*action).to_owned())).collect()
}

// ---------- TestConfig ----------

/// `test_giorni`.
#[test]
fn days() {
    let set = |days: &[u32]| days.iter().copied().collect::<BTreeSet<u32>>();
    assert_eq!(parse_days("lun-ven").unwrap(), set(&[0, 1, 2, 3, 4]));
    assert_eq!(parse_days("lun,mer,ven").unwrap(), set(&[0, 2, 4]));
    assert_eq!(parse_days("sab-dom").unwrap(), set(&[5, 6]));
    assert_eq!(parse_days("ven-lun").unwrap(), set(&[4, 5, 6, 0]));
    assert_eq!(parse_days("tutti").unwrap(), set(&[0, 1, 2, 3, 4, 5, 6]));
    assert_eq!(parse_days("Gio").unwrap(), set(&[3]));
    for wrong in ["lunedi", "lun-xyz", "1-5"] {
        assert!(parse_days(wrong).is_err(), "{wrong}");
    }
}

/// `test_il_config_del_repo_e_valido`, on a synthetic document of the same shape as the dossier's: the dossier's own
/// names real projects, and this repository is public.
#[test]
fn a_configuration_of_the_dossier_s_shape_is_valid() {
    let document = json!({"_nota": "synthetic", "operazioni": [
        {"nome": "giornalieri", "ora": "09:00", "giorni": "lun-ven", "azione": "chat", "prompt": "regia: giornalieri", "attiva": true},
        {"nome": "cronista", "ora": "19:00", "giorni": "tutti", "azione": "cronista"},
        {"nome": "cerca-idee", "ora": "07:00", "giorni": "lun", "azione": "cerca-idee", "attiva": false, "progetti": ["alpha", "beta"], "idee": 3},
    ]});
    let (operations, errors) = normalize(&document);
    assert_eq!(errors, Vec::<String>::new());
    let shape: Vec<(&str, (u32, u32), Action, BTreeSet<u32>)> = operations.iter().map(|op| (op.name.as_str(), op.hour, op.action, op.days.clone())).collect();
    assert_eq!(
        shape,
        [
            ("giornalieri", (9, 0), Action::Chat, (0..5).collect()),
            ("cronista", (19, 0), Action::Chronicler, (0..7).collect()),
            ("cerca-idee", (7, 0), Action::IdeaSearch, BTreeSet::from([0])),
        ]
    );
    assert_eq!((operations[2].active, operations[2].projects.as_deref(), operations[2].ideas), (false, Some(&["alpha".to_owned(), "beta".to_owned()][..]), Some(3)));
}

/// `test_le_voci_invalide_si_scartano_e_le_altre_restano`, and the idea search's levers as `_leve_cerca_idee` checks them.
#[test]
fn the_entries_that_do_not_hold_are_left_out_and_the_others_stay() {
    let ok = json!({"nome": "ok", "ora": "09:00", "azione": "chat", "prompt": "p"});
    let with = |key: &str, value: Value| {
        let mut entry = ok.clone();
        entry[key] = value;
        entry
    };
    let entries = vec![
        ok.clone(),
        with("nome", json!("Maiuscola")),
        json!({"nome": "ora", "ora": "25:00", "azione": "chat", "prompt": "p"}),
        json!({"nome": "az", "ora": "09:00", "azione": "sessione", "prompt": "p"}),
        json!({"nome": "gg", "ora": "09:00", "azione": "chat", "prompt": "p", "giorni": "boh"}),
        json!({"nome": "vuoto", "ora": "09:00", "azione": "chat", "prompt": " "}),
        ok.clone(),
        json!("non una mappa"),
    ];
    let (operations, errors) = normalize(&json!({"operazioni": entries}));
    assert_eq!(operations.iter().map(|op| op.name.as_str()).collect::<Vec<_>>(), ["ok"]);
    assert_eq!(errors.len(), 7, "{errors:?}");
    assert!(normalize(&json!({"operazioni": "no"})).0.is_empty());
    assert!(normalize(&json!([])).0.is_empty());
    let search = |levers: Value| {
        let mut entry = json!({"nome": "ci", "ora": "07:00", "azione": "cerca-idee"});
        entry.as_object_mut().unwrap().extend(levers.as_object().unwrap().clone());
        normalize(&json!({"operazioni": [entry]}))
    };
    assert!(search(json!({})).1.is_empty(), "the levers' defaults hold");
    for wrong in [json!({"progetti": []}), json!({"progetti": ["Alpha"]}), json!({"progetti": ["a", "a"]}), json!({"progetti": "a"}), json!({"idee": 0}), json!({"idee": 6}), json!({"idee": true}), json!({"idee": 3.0})] {
        assert_eq!(search(wrong.clone()).1.len(), 1, "{wrong}");
    }
}

/// `test_attiva_false_si_legge`.
#[test]
fn active_false_reads() {
    let (operations, _) = normalize(&json!({"operazioni": [{"nome": "x", "ora": "09:00", "azione": "chat", "prompt": "p", "attiva": false}]}));
    assert!(!operations[0].active);
}

// ---------- TestOperazioni: once a day ----------

/// `test_una_volta_al_giorno_al_primo_giro_dopo_l_ora`.
#[test]
fn once_a_day_at_the_first_tick_after_the_hour() {
    let mut case = Case::new("once-a-day", Setup::of(dailies()));
    assert_eq!(voices(&case.at(22, 8, 0)), pairs(&[]), "before the hour: nothing");
    assert_eq!(voices(&case.at(22, 8, 31)), pairs(&[("giornalieri", "lanciata")]));
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "gia-eseguita-oggi")]));
    assert_eq!(voices(&case.at(22, 18, 0)), pairs(&[("giornalieri", "gia-eseguita-oggi")]));
    assert_eq!(case.chat.calls(), 1);
    case.close(22, 9, 5);
    assert_eq!(voices(&case.at(23, 8, 45)), pairs(&[("giornalieri", "lanciata")]), "yesterday's chat is closed, and the day after is a Friday");
    assert_eq!(case.chat.calls(), 2);
}

/// `test_la_chiamata_porta_prompt_cartella_e_modello_ma_niente_passo`.
#[test]
fn the_call_carries_prompt_folder_and_model_but_no_step() {
    let mut case = Case::new("the-call", Setup::of(dailies()));
    case.at(22, 9, 0);
    let call = case.chat.calls.lock().unwrap()[0].clone();
    assert_eq!((call.label.as_str(), call.prompt.as_str(), call.service), ("giornalieri", "regia: giornalieri", ServiceId::Claude));
    assert_eq!((call.model.as_ref().map(ModelId::as_str), call.effort), (Some("opus"), Some(Effort::High)));
    assert!(call.folder.is_dir());
    assert_eq!(case.registry.launched.load(Ordering::SeqCst), 0, "a chat is no step, and it is not in the registry");
}

/// `test_nei_giorni_non_previsti_non_parte`.
#[test]
fn on_the_days_not_scheduled_it_does_not_start() {
    let mut case = Case::new("days-off", Setup::of(dailies()));
    assert_eq!(voices(&case.at(24, 10, 0)), pairs(&[]), "Saturday");
    assert_eq!(voices(&case.at(25, 10, 0)), pairs(&[]), "Sunday");
    assert_eq!(case.chat.calls(), 0);
}

/// `test_lo_stato_sopravvive_a_un_riavvio`.
#[test]
fn the_state_survives_a_restart() {
    let mut case = Case::new("restart", Setup::of(dailies()));
    case.at(22, 9, 0);
    case.restart(Setup::of(dailies()));
    assert_eq!(voices(&case.at(22, 9, 30)), pairs(&[("giornalieri", "gia-eseguita-oggi")]));
    assert_eq!(case.chat.calls(), 0);
}

// ---------- after a suspension, and the days skipped ----------

/// `test_dopo_un_risveglio_nello_stesso_giorno_la_recupera`.
#[test]
fn after_a_wake_up_the_same_day_it_catches_up() {
    let mut case = Case::new("same-day", Setup::of(dailies()));
    case.at(22, 7, 0);
    assert_eq!(voices(&case.at(22, 15, 0)), pairs(&[("giornalieri", "lanciata")]), "the machine slept: the first tick after 08:30 is at 15");
}

/// `test_se_il_giorno_e_cambiato_la_salta_e_lo_registra`.
#[test]
fn when_the_day_changed_it_is_skipped_and_recorded() {
    let mut case = Case::new("day-changed", Setup::of(dailies()));
    case.at(22, 7, 0);
    let record = case.at(23, 10, 0);
    assert_eq!(voices(&record), pairs(&[("giornalieri", "saltata"), ("giornalieri", "lanciata")]));
    assert_eq!(record["operazioni"][0]["giorni"], json!(["2026-10-22"]));
    assert_eq!(case.chat.calls(), 1, "only Friday's");
    assert_eq!(case.status(&record)["operazioni"]["giornalieri"]["saltate"], json!(["2026-10-22"]));
}

/// Acceptance 1: a day skipped is recorded once, and never run. The tick after, a restart and the next day do not count
/// it again; the cursor keeps it once.
#[test]
fn a_skipped_day_is_recorded_once_only() {
    let mut case = Case::new("skipped-once", Setup::of(dailies()));
    case.at(22, 7, 0);
    assert_eq!(voices(&case.at(23, 10, 0)), pairs(&[("giornalieri", "saltata"), ("giornalieri", "lanciata")]));
    assert_eq!(voices(&case.at(23, 10, 15)), pairs(&[("giornalieri", "gia-eseguita-oggi")]));
    assert_eq!(case.chat.calls(), 1, "Friday's: none for the day skipped");
    case.restart(Setup::of(dailies()));
    assert_eq!(voices(&case.at(23, 11, 0)), pairs(&[("giornalieri", "gia-eseguita-oggi")]), "a restart does not count it again");
    case.close(23, 10, 0);
    let monday = case.at(26, 9, 0);
    assert_eq!(voices(&monday), pairs(&[("giornalieri", "lanciata")]), "Saturday and Sunday are not scheduled, Friday was done");
    assert_eq!(case.operations_cursor()["ops"]["giornalieri"]["saltate"], json!(["2026-10-22"]));
    assert_eq!(case.chat.calls(), 1, "after the restart, Monday's only");
}

/// `test_un_weekend_di_assenza_salta_solo_i_giorni_previsti`.
#[test]
fn a_weekend_away_skips_only_the_scheduled_days() {
    let mut case = Case::new("weekend", Setup::of(dailies()));
    case.at(22, 8, 40);
    case.close(22, 8, 45);
    let record = case.at(26, 9, 0);
    assert_eq!(record["operazioni"][0]["giorni"], json!(["2026-10-23"]), "Friday: Saturday and Sunday were not scheduled");
    assert_eq!(voices(&record), pairs(&[("giornalieri", "saltata"), ("giornalieri", "lanciata")]));
    assert_eq!(voices(&case.at(26, 9, 15)), pairs(&[("giornalieri", "gia-eseguita-oggi")]), "skipped once only");
}

/// `test_alla_prima_volta_non_c_e_un_passato_da_recuperare`.
#[test]
fn the_first_time_there_is_no_past_to_catch_up() {
    let mut case = Case::new("first-time", Setup::of(dailies()));
    assert_eq!(voices(&case.at(22, 15, 0)), pairs(&[("giornalieri", "lanciata")]));
}

// ---------- suspended sessions ----------

/// `test_con_una_sospesa_non_lancia_nulla_e_non_segna_il_giorno`. The prototype's decision `attesa-sospesa` is its resume's,
/// #470's: here the tick goes on to the precheck.
#[test]
fn with_a_session_suspended_nothing_opens_and_the_day_is_not_marked() {
    let mut case = Case::new("suspended", Setup::of(dailies()));
    case.registry.suspended.lock().unwrap().insert("a".to_owned());
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "rinviata-sospesa")]));
    assert_eq!(case.chat.calls(), 0);
    case.registry.suspended.lock().unwrap().clear();
    assert_eq!(voices(&case.at(22, 14, 0)), pairs(&[("giornalieri", "lanciata")]), "resumed: the same day it starts");
}

/// `test_con_una_ripresa_in_corso_non_lancia`: a session resuming keeps its project as busy as a suspended one.
#[test]
fn with_a_resume_under_way_nothing_opens() {
    let mut case = Case::new("resuming", Setup::of(dailies()));
    case.registry.suspended.lock().unwrap().insert("a".to_owned());
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "rinviata-sospesa")]));
    assert_eq!(case.chat.calls(), 0);
}

// ---------- one chat waiting (#209) ----------

/// `test_con_la_precedente_aperta_non_lancia_e_registra_l_esito`.
#[test]
fn with_the_previous_chat_open_nothing_opens_and_the_outcome_is_recorded() {
    let mut case = Case::new("previous-open", Setup::of(dailies()));
    assert_eq!(voices(&case.at(22, 8, 40)), pairs(&[("giornalieri", "lanciata")]));
    let record = case.at(23, 9, 0);
    assert_eq!(voices(&record), pairs(&[("giornalieri", "saltata-precedente-aperta")]), "no closing file: yesterday's chat is open");
    assert_eq!(case.chat.calls(), 1);
    let entry = case.status(&record)["operazioni"]["giornalieri"].clone();
    assert_eq!((entry["giorno"].clone(), entry["esito"].clone()), (json!("2026-10-23"), json!("saltata-precedente-aperta")));
    assert_eq!(entry["aperta_dal"], entry["ultima_lanciata"]["t"], "the reference to the open chat is not lost");
    assert_eq!(voices(&case.at(23, 9, 15)), pairs(&[("giornalieri", "gia-eseguita-oggi")]), "the day is spent");
    case.close(22, 9, 0);
    assert_eq!(voices(&case.at(23, 14, 0)), pairs(&[("giornalieri", "gia-eseguita-oggi")]), "closed later: today it does not start again");
    assert_eq!(case.chat.calls(), 1);
    assert_eq!(voices(&case.at(26, 9, 0)), pairs(&[("giornalieri", "lanciata")]), "Monday: closed, it starts");
    assert_eq!(case.chat.calls(), 2);
}

/// `test_con_la_precedente_chiusa_lancia_una_volta`.
#[test]
fn with_the_previous_chat_closed_it_opens_once() {
    let mut case = Case::new("previous-closed", Setup::of(dailies()));
    case.at(22, 8, 40);
    case.close(22, 8, 41);
    assert_eq!(voices(&case.at(23, 9, 0)), pairs(&[("giornalieri", "lanciata")]));
    assert_eq!(voices(&case.at(23, 9, 15)), pairs(&[("giornalieri", "gia-eseguita-oggi")]));
    assert_eq!(case.chat.calls(), 2);
}

/// `test_un_file_di_chiusura_di_prima_del_lancio_non_chiude_la_chat`.
#[test]
fn a_closing_file_from_before_the_launch_does_not_close_the_chat() {
    let mut case = Case::new("closing-before", Setup::of(dailies()));
    case.close(21, 20, 0);
    case.at(22, 8, 40);
    assert_eq!(voices(&case.at(23, 9, 0)), pairs(&[("giornalieri", "saltata-precedente-aperta")]));
    case.close(22, 8, 40);
    assert_eq!(voices(&case.at(26, 9, 0)).last().cloned(), Some(("giornalieri".to_owned(), "lanciata".to_owned())), "the minute of the launch: that chat");
}

/// `test_la_regola_vale_solo_per_i_giornalieri`.
#[test]
fn the_rule_holds_for_the_dailies_only() {
    let mut case = Case::new("only-dailies", Setup::of(evening()));
    case.at(22, 18, 5);
    assert_eq!(voices(&case.at(23, 18, 5)), pairs(&[("sera", "lanciata")]), "no closing file, and it starts all the same");
    assert_eq!(case.chat.calls(), 2);
}

/// `test_dopo_un_avvia_l_operazione_a_orario_di_quel_giorno_non_riparte`.
#[test]
fn after_a_start_by_hand_the_schedule_of_that_day_does_not_start() {
    let mut case = Case::new("after-start", Setup::of(dailies()));
    case.move_to(22, 7, 30);
    assert_eq!(case.start_by_hand(false), "lanciata", "by hand, before the hour");
    assert_eq!(voices(&case.at(22, 8, 45)), pairs(&[("giornalieri", "gia-avviata-oggi")]));
    assert_eq!(voices(&case.at(22, 18, 0)), pairs(&[("giornalieri", "gia-eseguita-oggi")]));
    assert_eq!(case.chat.calls(), 1, "one launch only: the start's");
    case.close(22, 7, 30);
    assert_eq!(voices(&case.at(23, 8, 45)), pairs(&[("giornalieri", "lanciata")]), "the next day the schedule is back");
    assert_eq!(case.chat.calls(), 2);
}

/// `test_un_avvia_durante_il_lancio_dell_operazione_non_ne_apre_una_seconda`: Orca takes minutes, and meanwhile the user
/// starts the dailies by hand.
#[test]
fn a_start_during_the_schedule_s_launch_opens_no_second_chat() {
    let mut case = Case::new("start-during", Setup::of(dailies()));
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let (store, clock, closings, seen) = (case.store.clone(), case.clock.clone(), case.closings.clone(), outcomes.clone());
    let other = FakeChat::new(vec![Chat::Opens]);
    *case.chat.during.lock().unwrap() = Some(Box::new(move || {
        seen.lock().unwrap().push(start_by_hand(&store, &clock, &other, &closings, false));
    }));
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "lanciata")]));
    assert_eq!(*outcomes.lock().unwrap(), ["precedente-aperta"]);
    assert_eq!(case.chat.calls(), 1);
}

/// `test_un_errore_dopo_l_effetto_conta_come_chat_aperta_il_giorno_dopo`.
#[test]
fn an_error_after_the_effect_counts_as_an_open_chat_the_day_after() {
    let mut case = Case::new("error-open", Setup { chat: Some(vec![Chat::FailsAfter("orca agent launch returned no session")]), ..Setup::of(dailies()) });
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "errore-lancio")]));
    assert_eq!(voices(&case.at(23, 9, 0)), pairs(&[("giornalieri", "saltata-precedente-aperta")]));
    assert_eq!(case.chat.calls(), 1);
}

/// `test_senza_orca_il_tentativo_non_lascia_una_chat_aperta`.
#[test]
fn without_orca_the_attempt_leaves_no_open_chat() {
    let mut case = Case::new("no-orca-no-chat", Setup { chat: Some(vec![Chat::NotAvailable("no"), Chat::Opens]), ..Setup::of(dailies()) });
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "da-aprire")]));
    let entry = case.operations_cursor()["ops"]["giornalieri"].as_object().cloned().unwrap();
    assert_eq!(last_launch(&entry, &Map::new()), None, "no last chat: Orca was not there");
    assert_eq!(voices(&case.at(22, 9, 15)), pairs(&[("giornalieri", "lanciata")]));
}

/// `test_un_avvia_forzato_dopo_il_giorno_saltato_toglie_l_avviso`.
#[test]
fn a_forced_start_after_the_day_skipped_clears_the_warning() {
    let mut case = Case::new("forced-start", Setup::of(dailies()));
    case.at(22, 8, 40);
    assert_eq!(voices(&case.at(23, 9, 0)), pairs(&[("giornalieri", "saltata-precedente-aperta")]));
    case.move_to(23, 9, 5);
    assert_eq!(case.start_by_hand(true), "lanciata");
    let record = case.at(23, 9, 15);
    assert_eq!(voices(&record), pairs(&[("giornalieri", "gia-avviata-oggi")]));
    assert_eq!(case.status(&record)["operazioni"]["giornalieri"]["esito"], "avviata", "the brief no longer says it did not open today");
}

/// `test_un_avvia_chiude_il_da_aprire_di_oggi`.
#[test]
fn a_start_by_hand_closes_today_s_chat_to_open() {
    let mut case = Case::new("start-closes", Setup { chat: Some(vec![Chat::NotAvailable("no"), Chat::Opens]), ..Setup::of(dailies()) });
    let record = case.at(22, 9, 0);
    assert_eq!(case.status(&record)["da_aprire"].as_array().map(Vec::len), Some(1));
    case.move_to(22, 9, 5);
    assert_eq!(case.start_by_hand(false), "lanciata");
    let record = case.at(22, 9, 15);
    assert_eq!(voices(&record), pairs(&[("giornalieri", "gia-avviata-oggi")]));
    assert_eq!(case.status(&record)["da_aprire"], json!([]));
    assert_eq!(case.chat.calls(), 2);
}

/// `test_un_giorno_avviato_a_mano_non_conta_come_saltato`.
#[test]
fn a_day_started_by_hand_does_not_count_as_skipped() {
    let mut case = Case::new("by-hand-not-skipped", Setup::of(dailies()));
    case.move_to(22, 9, 0);
    case.start_by_hand(false);
    case.close(22, 9, 1);
    assert_eq!(voices(&case.at(23, 9, 0)), pairs(&[("giornalieri", "lanciata")]), "Thursday is not skipped: the controller was off");
}

/// `test_chat_aperta_confronta_il_bollo_al_minuto`.
#[test]
fn an_open_chat_compares_the_stamp_to_the_minute() {
    let folder = regia_conformance::prepare_state_dir("schedule-stamp").join("gg");
    let t = instant(22, 9, 0);
    assert!(chat_open(&folder, &t), "no folder: no closing file");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("non-un-bollo.md"), "x").unwrap();
    fs::write(folder.join(closing_name(22, 8, 59)), "x").unwrap();
    assert!(chat_open(&folder, "2026-10-22T09:00:30Z"), "a minute before does not close it");
    fs::write(folder.join(closing_name(22, 9, 0)), "x").unwrap();
    assert!(!chat_open(&folder, "2026-10-22T09:00:30Z"), "the launch's minute does");
    assert!(!chat_open(&folder, "boh"), "an instant that does not read blocks nothing for ever");
}

// ---------- without Orca ----------

/// `test_senza_orca_resta_da_aprire_si_riprova_e_poi_si_chiude`.
#[test]
fn without_orca_it_stays_to_open_is_tried_again_and_then_closes() {
    let reason = "Orca is not reachable";
    let mut case = Case::new("no-orca", Setup { chat: Some(vec![Chat::NotAvailable(reason), Chat::NotAvailable(reason), Chat::Opens]), ..Setup::of(dailies()) });
    let record = case.at(22, 9, 0);
    assert_eq!(voices(&record), pairs(&[("giornalieri", "da-aprire")]));
    let to_open = case.status(&record)["da_aprire"].clone();
    let shape: Vec<(Value, Value, Value)> = to_open.as_array().unwrap().iter().map(|entry| (entry["nome"].clone(), entry["giorno"].clone(), entry["prompt"].clone())).collect();
    assert_eq!(shape, [(json!("giornalieri"), json!("2026-10-22"), json!("regia: giornalieri"))]);
    assert!(to_open[0]["motivo"].as_str().unwrap().contains(reason));
    assert_eq!(voices(&case.at(22, 9, 15)), pairs(&[("giornalieri", "da-aprire")]), "tried again");
    assert_eq!(case.chat.calls(), 2);
    let record = case.at(22, 9, 30);
    assert_eq!(voices(&record), pairs(&[("giornalieri", "lanciata")]), "Orca is back");
    assert_eq!(case.status(&record)["da_aprire"], json!([]));
    assert_eq!(voices(&case.at(22, 10, 0)), pairs(&[("giornalieri", "gia-eseguita-oggi")]));
}

/// `test_il_da_aprire_scade_con_il_giorno`.
#[test]
fn the_chat_to_open_expires_with_the_day() {
    let mut case = Case::new("to-open-expires", Setup { chat: Some(vec![Chat::NotAvailable("no")]), ..Setup::of(dailies()) });
    let record = case.at(22, 9, 0);
    assert_eq!(case.status(&record)["da_aprire"].as_array().map(Vec::len), Some(1));
    let record = case.at(24, 9, 0);
    assert_eq!(case.status(&record)["da_aprire"], json!([]), "Saturday: Thursday's occurrence is no longer to open");
}

/// `test_un_errore_dopo_l_effetto_non_si_ripete`.
#[test]
fn an_error_after_the_effect_is_not_repeated() {
    let mut case = Case::new("error-once", Setup { chat: Some(vec![Chat::FailsAfter("orca agent launch returned no session")]), ..Setup::of(dailies()) });
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "errore-lancio")]));
    let record = case.at(22, 9, 15);
    assert_eq!(voices(&record), pairs(&[("giornalieri", "gia-eseguita-oggi")]));
    assert_eq!(case.chat.calls(), 1);
    assert_eq!(case.status(&record)["da_aprire"][0]["esito"], "errore", "the owner checks whether the chat exists");
}

/// `test_con_l_adapter_cli_non_apre_mai_una_sessione_senza_nessuno_davanti`: without a surface for the chat it stays to
/// open, with the reason that says why.
#[test]
fn with_the_cli_launcher_no_session_opens_with_nobody_in_front() {
    let mut case = Case::new("cli-launcher", Setup { chat: None, ..Setup::of(dailies()) });
    let record = case.at(22, 9, 0);
    assert_eq!(voices(&record), pairs(&[("giornalieri", "da-aprire")]));
    assert!(record["operazioni"][0]["motivo"].as_str().unwrap().contains("cli"), "{record}");
    assert_eq!(case.chat.calls(), 0);
    assert_eq!(case.registry.launched.load(Ordering::SeqCst), 0);
}

/// `test_lancio_finto_non_apre_nulla_e_lo_dice`: on `Dry` the chat is the fake launcher's, and starts nothing.
#[test]
fn the_fake_launcher_opens_nothing_and_says_so() {
    let folder = regia_conformance::prepare_state_dir("schedule-dry");
    let clock = ManualClock::fixed(WallTime::parse_iso(&instant(22, 9, 0)).unwrap());
    let store = Store::at(&folder).unwrap().with_clock(clock.clone());
    let holder = Holder::new(HOLDER).unwrap();
    assert!(store.acquire_lock(&reserved(), &holder, ttl()).unwrap().is_ok());
    let operations = Operations::new(Source::Document(dailies()), Arc::new(StoreSchedule::new(store.clone(), holder.clone())), ChatSurface::Open(Arc::new(Dry::new())))
        .with_local(utc_local)
        .with_home(Some(folder.clone()))
        .with_closings(folder.join("giornalieri"));
    let mut controller = Controller::new(Options::default(), empty_ports(&store, &clock, &holder), WakeUp::new(ProbeConsent::Refused)).with_before_precheck(Box::new(operations));
    let record = controller.tick(false).unwrap();
    assert_eq!(voices(&record), pairs(&[("giornalieri", "lanciata")]));
    assert_eq!(record["operazioni"][0]["adapter"], "finto");
}

/// `test_se_non_si_puo_scrivere_lo_stato_non_si_lancia`.
#[test]
fn when_the_state_cannot_be_written_nothing_opens() {
    let mut case = Case::new("no-write", Setup { failing_write: Some("busy"), ..Setup::of(dailies()) });
    let record = case.at(22, 9, 0);
    assert_eq!(case.chat.calls(), 0);
    assert_eq!(record["operazioni_errore"], "busy");
    assert_eq!(record["decisione"], "attesa-idle", "the tick goes on");
}

/// `test_il_lock_si_rinnova_dopo_un_lancio`: at the tick's start and after its precheck, and the chat's two, right
/// before it (#507) and after it.
#[test]
fn the_lock_is_renewed_after_a_launch() {
    let mut case = Case::new("renewals", Setup::of(dailies()));
    case.at(22, 9, 0);
    assert_eq!(case.chat.calls(), 1);
    assert_eq!(case.lock.renewals.load(Ordering::SeqCst), 2 + 1 + 1);
}

// ---------- the configuration ----------

/// `test_il_lanciatore_di_chat_dipende_dall_adapter`, `TestChatReale.test_apre_con_l_adapter_orca_e_mai_con_auto`: a
/// chat opens on the format's surface, Orca, never on `auto`, whose fallback is a session with nobody in front. The
/// command's choice by launcher is its own test; a refusal of Orca before any effect is a chat to open, any other
/// failure an error (the two cases above).
#[test]
fn a_chat_opens_on_orca_and_never_on_auto() {
    assert_eq!(chat_surface_kind(), Some(SurfaceKind::Orca));
}

/// `test_voce_non_attiva_o_config_assente_o_rotta`.
#[test]
fn an_entry_not_active_or_a_configuration_missing_or_broken() {
    let mut off = dailies();
    off["operazioni"][0]["attiva"] = json!(false);
    let mut case = Case::new("off", Setup::of(off));
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[]));
    let missing = case.folder.join("non-c-e.json");
    case.restart(Setup { config: Source::File(missing), ..Setup::of(dailies()) });
    assert_eq!(voices(&case.at(22, 9, 5)), pairs(&[]));
    let broken = case.folder.join("rotto.json");
    fs::write(&broken, "{non json").unwrap();
    case.restart(Setup { config: Source::File(broken), ..Setup::of(dailies()) });
    let record = case.at(22, 9, 10);
    assert_eq!(voices(&record), pairs(&[]));
    assert!(record["operazioni_config"][0].as_str().unwrap().contains("rotto.json"), "{record}");
    assert_eq!(record["decisione"], "attesa-idle");
}

/// `test_il_config_si_legge_da_file`.
#[test]
fn the_configuration_is_read_from_a_file() {
    let folder = regia_conformance::prepare_state_dir("schedule-file-config-source");
    let file = folder.join("ops.json");
    fs::write(&file, dailies().to_string()).unwrap();
    let mut case = Case::new("file-config", Setup { config: Source::File(file), ..Setup::of(dailies()) });
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "lanciata")]));
}

/// `test_due_operazioni_indipendenti`.
#[test]
fn two_independent_operations() {
    let mut two = dailies();
    two["operazioni"].as_array_mut().unwrap().push(evening()["operazioni"][0].clone());
    let mut case = Case::new("two", Setup::of(two));
    assert_eq!(voices(&case.at(22, 9, 0)), pairs(&[("giornalieri", "lanciata")]));
    let record = case.at(24, 18, 5);
    let launched: Vec<(String, String)> = voices(&record).into_iter().filter(|(_, action)| action == "lanciata").collect();
    assert_eq!(launched, pairs(&[("sera", "lanciata")]), "Saturday: only the second");
    assert_eq!(record["operazioni"][0]["giorni"], json!(["2026-10-23"]), "Friday: both skipped");
    let names: Vec<String> = case.chat.calls.lock().unwrap().iter().map(|call| call.label.as_str().to_owned()).collect();
    assert_eq!(names, ["giornalieri", "sera"]);
}

// ---------- D1 and #582 ----------

/// `TestChatSenzaLock.test_lock_rubato_prima_della_chat_nessuna_chat_e_nessun_segno_nel_cursore` (#467/V8): the lock is
/// taken by another controller after the last read before the chat. The tick ends with the lock lost, not with a
/// launch error it would record and forget; no chat, the thief keeps the lock, and the cursor holds nothing.
#[test]
fn a_lock_stolen_before_the_chat_opens_no_chat_and_marks_nothing() {
    let mut case = Case::new("d1-chat", Setup::of(evening()));
    let (store, holder) = (case.store.clone(), case.holder.clone());
    *case.cursors.after_start_read.lock().unwrap() = Some(Box::new(move || {
        assert!(store.release_lock(&reserved(), &holder).unwrap().is_ok());
        assert!(store.acquire_lock(&reserved(), &Holder::new(THIEF).unwrap(), ttl()).unwrap().is_ok());
    }));
    case.move_to(22, 18, 5);
    let outcome = case.controller.tick(false);
    assert!(matches!(outcome, Err(TickError::LockLost(_))), "{outcome:?}");
    assert_eq!(case.chat.calls(), 0);
    match case.store.lock_status(&reserved()).unwrap() {
        LockStatus::Held(view) => assert_eq!(view.holder(), Some(THIEF)),
        other => panic!("{other:?}"),
    }
    assert_eq!(case.operations_cursor(), Value::Null);
}

/// #582: the cursors of the operations are read and written by the lock's holder only. After the lock passed, a write
/// changes nothing, and a read is refused too.
#[test]
fn the_cursors_are_the_lock_holder_s_only() {
    let case = Case::new("582", Setup::of(dailies()));
    let state = StoreSchedule::new(case.store.clone(), case.holder.clone());
    let entry = |outcome: &str| Map::from_iter([("giornalieri".to_owned(), json!({"giorno": "2026-10-22", "esito": outcome}))]);
    state.write(&entry("lanciata")).unwrap();
    assert_eq!(case.operations_cursor()["ops"]["giornalieri"]["esito"], "lanciata");
    case.clock.advance(601.0);
    assert!(case.store.acquire_lock(&reserved(), &Holder::new(THIEF).unwrap(), ttl()).unwrap().is_ok());
    assert!(matches!(state.write(&entry("in-corso")), Err(StateError::LockLost(_))));
    assert!(matches!(state.operations(), Err(StateError::LockLost(_))));
    assert_eq!(case.operations_cursor()["ops"]["giornalieri"]["esito"], "lanciata", "nothing written after the lock passed");
    assert_eq!(case.operations_cursor()["v"], 1);
}

// ---------- #467/V11: the record of a step launched on Orca ----------

/// The prototype's `nuovo_record` with an Orca receipt: the agent's session is resumed only when its transcript was
/// found and it is not Orca's own id, and the record keeps the marker the session is found again by. On `cli` there is
/// no marker, and the session is the one to resume.
#[test]
fn a_step_launched_on_orca_keeps_its_marker_and_resumes_only_a_found_session() {
    let receipt = |adapter: &str, session: &str, orca: Option<&str>, transcript: Option<&str>| StepReceipt {
        adapter: adapter.to_owned(),
        agent: "claude".to_owned(),
        model: Some("opus".to_owned()),
        effort: Some("high".to_owned()),
        session: Some(session.to_owned()),
        transcript: transcript.map(str::to_owned),
        pid: None,
        log: None,
        started: true,
        surface_session: orca.map(str::to_owned),
        marker: Some("do the step".to_owned()),
    };
    let record = |receipt: &StepReceipt| new_record(&LaunchedStep { project: "alpha", folder: "/projects/alpha", prompt: "do the step", receipt }, secs(22, 9, 0));
    let (key, found) = record(&receipt("orca", "claude-uuid", Some("orca-id"), Some("/sessions/claude-uuid.jsonl")));
    assert_eq!(key, "alpha/claude-uuid");
    assert_eq!((found["sessione_resume"].clone(), found["sessione_orca"].clone(), found["marcatore"].clone()), (json!("claude-uuid"), json!("orca-id"), json!("do the step")));
    let (key, not_found) = record(&receipt("orca", "orca-id", Some("orca-id"), None));
    assert_eq!(key, "alpha/orca-id", "the key is the session, as the prototype's `chiave_passo`");
    assert_eq!((not_found["sessione_resume"].clone(), not_found["marcatore"].clone()), (Value::Null, json!("do the step")));
    let (_, same) = record(&receipt("orca", "orca-id", Some("orca-id"), Some("/sessions/x.jsonl")));
    assert_eq!(same["sessione_resume"], Value::Null, "the session is Orca's own id: nothing to resume");
    let (_, cli) = record(&receipt("cli", "cli-uuid", None, Some("/sessions/cli-uuid.jsonl")));
    assert_eq!((cli["sessione_resume"].clone(), cli["marcatore"].clone(), cli["adapter"].clone()), (json!("cli-uuid"), Value::Null, json!("cli")));
    assert_eq!((found["stato"].clone(), found["origine"].clone(), found["avviato"].clone()), (json!("lanciato"), json!("controller"), json!("2026-10-22T09:00:00Z")));
}
