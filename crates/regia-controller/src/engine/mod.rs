//! The controller's tick (F25 of the dossier's plan, ADR 0001 §7): the prototype's tick of its `Controller` and what
//! it calls, with the clock, the precheck, the projects' folders, the liveness of a process, the launch
//! surface, the controller's lock, the postponements and the steps registry injected as ports, so the
//! prototype's cases run on fixed answers and a `Dry` surface (the shadow, #472).
//!
//! A tick renews the controller's lock, runs the precheck, reads the budget it reported, wakes the token when
//! the budget is unknown and work waits (#190), drops the steps no longer alive, decides from the precheck's
//! line which projects to launch and launches at most `max_steps` of them, one per project. A project it does
//! not launch gets a postponement, so the precheck does not count it towards a stall (#133).
//!
//! **D1, launching after losing the lock (ADR 0001 §7).** Every launch point goes through one guard,
//! [`Controller::guard`], which renews the lock right before the effect and outside the launch's own error
//! handling: a lost lock stops the tick with [`TickError::LockLost`], never as a launch error. This slice has
//! two launch points, the step and the token's wake-up; the scheduled chat (#469), the chronicler (#493) and
//! the resume of a suspended session (#470) call the same guard before their own effect.
//!
//! What the tick does not do, by the plan: the lock's taking and release, the log file, the state file
//! on disk, the wait between ticks and the restart, which are its host's, [`crate::resident`] (#468); the
//! scheduled operations (#469); the resume of suspended sessions (#470). The tick returns its line, and
//! [`Controller::status`] the state document, for the host that writes them.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use regia_agents::{LaunchRequest, LaunchSurface, Receipt, ServiceId, SurfaceKind, WakeAttempt, WakeUp};
use regia_exec::{Effort, ModelId};
use regia_store::{Clock, WallTime};
use serde_json::{Map, Value, json};

use crate::precheck::Json;
use crate::precheck::v1::format as precheck_words;

pub mod native;
mod prompt;
pub(crate) mod words;

pub use prompt::prompt;
use words::format as words;

/// The controller's lock is no longer this controller's: it stops, and its host exits with 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockLost(pub String);

impl fmt::Display for LockLost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LockLost {}

/// Why a write to the state did not happen: the lock is lost, or the write failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    LockLost(LockLost),
    Failed(String),
}

/// Why a tick stopped: the lock is lost (the controller stops), or the tick failed (the host records it and
/// goes on with the next one, as the prototype's failed tick).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickError {
    LockLost(LockLost),
    Failed(String),
}

impl fmt::Display for TickError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TickError::LockLost(lost) => write!(f, "{lost}"),
            TickError::Failed(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for TickError {}

impl From<LockLost> for TickError {
    fn from(lost: LockLost) -> TickError {
        TickError::LockLost(lost)
    }
}

impl From<StateError> for TickError {
    fn from(error: StateError) -> TickError {
        match error {
            StateError::LockLost(lost) => TickError::LockLost(lost),
            StateError::Failed(error) => TickError::Failed(error),
        }
    }
}

/// A run of the precheck, as the prototype's `esegui_precheck` gives it: the exit code (none when the run did
/// not end), the outcome line with its keys in order, and the error of a run that did not end.
#[derive(Debug, Clone, PartialEq)]
pub struct PrecheckRun {
    pub exit: Option<i32>,
    pub line: Option<Json>,
    pub error: Option<String>,
}

impl PrecheckRun {
    /// A run that ended with `exit` and printed `stdout`: its line is the last line that starts with `{` and
    /// reads as JSON.
    #[must_use]
    pub fn from_output(exit: Option<i32>, stdout: &str) -> PrecheckRun {
        let line = stdout.lines().rev().filter(|line| line.starts_with('{')).find_map(|line| serde_json::from_str::<Json>(line).ok());
        PrecheckRun { exit, line, error: None }
    }

    /// A run that did not end: a deadline, a program that did not start.
    #[must_use]
    pub fn failed(error: impl Into<String>) -> PrecheckRun {
        PrecheckRun { exit: None, line: None, error: Some(error.into()) }
    }
}

/// The precheck, once a tick: on `projects` only, or every active one when empty.
pub trait Precheck: Send + Sync {
    fn run(&self, projects: &[String], timeout_s: f64) -> PrecheckRun;
}

/// The folder of each project of the registry, as the tracker names it.
pub trait Folders: Send + Sync {
    /// # Errors
    ///
    /// When the registry cannot be read.
    fn folders(&self) -> Result<std::collections::BTreeMap<String, Option<String>>, String>;
}

/// Whether a process lives. A host that lives on also adopts the processes its launches started, and reaps
/// them once they end (#460/V5): on Unix an ended child stays a zombie until its parent waits on it.
pub trait Liveness: Send + Sync {
    fn alive(&self, pid: u32) -> bool;

    fn adopt(&self, _pid: u32) {}

    fn reap(&self) {}
}

/// The controller's lock, renewed: an error when it is no longer this controller's.
pub trait ControllerLock: Send + Sync {
    /// # Errors
    ///
    /// The lock is lost, or the state cannot be written.
    fn renew(&self) -> Result<(), StateError>;
}

/// The postponement of a project, the cursor the precheck reads.
pub trait Signals: Send + Sync {
    /// # Errors
    ///
    /// The lock is lost, or the cursor cannot be written.
    fn postpone(&self, project: &str, value: Value) -> Result<(), StateError>;
}

/// A step just launched, for the registry.
#[derive(Debug, Clone, Copy)]
pub struct LaunchedStep<'a> {
    pub project: &'a str,
    /// The folder as the registry names it, not as the launch resolved it.
    pub folder: &'a str,
    pub prompt: &'a str,
    pub receipt: &'a StepReceipt,
}

/// The steps registry (#468): the steps launched, by key.
pub trait Steps: Send + Sync {
    /// Records a step just launched, at `at`; its key.
    ///
    /// # Errors
    ///
    /// The lock is lost, or the registry cannot be written.
    fn launched(&self, step: &LaunchedStep<'_>, at: f64) -> Result<String, StateError>;

    /// The step of `key` is no longer alive: a record still launched is finished, a suspended one stays.
    ///
    /// # Errors
    ///
    /// The lock is lost, or the registry cannot be written.
    fn finished(&self, key: &str, at: f64) -> Result<(), StateError>;

    /// The projects with a session suspended or resuming: their work is half done.
    fn suspended_projects(&self) -> BTreeSet<String>;
}

/// What runs before the precheck (the resume of suspended sessions, #470): true when the tick ends there,
/// with its decision written in the record.
pub trait BeforePrecheck: Send {
    /// # Errors
    ///
    /// As a tick's.
    fn before(&mut self, record: &mut Map<String, Value>, now: f64) -> Result<bool, TickError>;
}

/// A launch's receipt as the prototype's launcher gives it, whatever the surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReceipt {
    /// The prototype's word for the surface: a `Dry` launch is its fake launcher's.
    pub adapter: String,
    pub agent: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub session: Option<String>,
    pub transcript: Option<String>,
    pub pid: Option<u32>,
    pub log: Option<String>,
    /// Whether something started: false on `Dry`.
    pub started: bool,
    /// The surface's own id of the session, where it has one.
    pub surface_session: Option<String>,
    pub marker: Option<String>,
}

impl StepReceipt {
    /// The receipt of `regia-agents` as the prototype's: the adapter is the surface's id, `Dry`'s the fake
    /// launcher's; what only one surface fills is read by the receipt's keys, as data, so the tick names none.
    #[must_use]
    pub fn of(receipt: &Receipt) -> StepReceipt {
        let dry = receipt.surface == SurfaceKind::Dry;
        let adapter = if dry { words().adapters.fake.clone() } else { receipt.surface.id().to_owned() };
        let keys = &words().receipt;
        let value = serde_json::to_value(receipt).unwrap_or_default();
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        StepReceipt {
            adapter,
            agent: receipt.service.id().to_owned(),
            model: receipt.model.clone(),
            effort: receipt.effort.map(str::to_owned),
            session: receipt.session.clone(),
            transcript: receipt.transcript.clone(),
            pid: receipt.pid,
            log: receipt.log.clone(),
            started: !dry,
            surface_session: text(&keys.surface_session),
            marker: text(&keys.marker),
        }
    }
}

/// What a tick runs with: the prototype's options of a tick, and the agent of a step, by the `replanning` role.
#[derive(Debug, Clone)]
pub struct Options {
    /// Only these projects; every active one when empty.
    pub projects: Vec<String>,
    pub max_steps: usize,
    /// Past this a step no longer counts as alive.
    pub step_limit_s: f64,
    pub precheck_timeout_s: f64,
    pub no_wake: bool,
    pub service: ServiceId,
    pub model: Option<ModelId>,
    pub effort: Option<Effort>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            projects: Vec::new(),
            max_steps: 1,
            step_limit_s: words().step_limit_s,
            precheck_timeout_s: 120.0,
            no_wake: false,
            service: ServiceId::Claude,
            model: None,
            effort: None,
        }
    }
}

impl Options {
    /// The agent of a step as the policy assigns it.
    #[must_use]
    pub fn assigned(mut self, assignment: &regia_agents::Assignment) -> Options {
        self.service = assignment.service;
        self.model = assignment.model_id();
        self.effort = Some(assignment.effort_level());
        self
    }
}

/// The ports a tick reads and writes through.
#[derive(Clone)]
pub struct Ports {
    pub clock: Arc<dyn Clock>,
    pub precheck: Arc<dyn Precheck>,
    pub folders: Arc<dyn Folders>,
    pub liveness: Arc<dyn Liveness>,
    pub surface: Arc<dyn LaunchSurface + Send + Sync>,
    pub lock: Arc<dyn ControllerLock>,
    pub signals: Arc<dyn Signals>,
    pub steps: Arc<dyn Steps>,
    /// The id of a postponement: random, so the precheck never applies one twice.
    pub ids: Arc<dyn Fn() -> String + Send + Sync>,
}

/// A step this controller launched, while it counts as alive.
#[derive(Debug, Clone, PartialEq)]
pub struct AliveStep {
    pub project: String,
    pub pid: Option<u32>,
    /// When it was launched, in seconds since the epoch.
    pub t: f64,
    pub session: Option<String>,
    /// Its key in the registry.
    pub key: String,
}

/// The ticks in a row with the budget unknown, since when (the first one's instant) and why, as the
/// prototype counts them across restarts (#133).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnknownBudget {
    pub rounds: i64,
    pub since: Option<String>,
    pub reason: Option<String>,
}

/// What the state document names of its host (#468): its process, the lock's holder, when it started and
/// the wait between two ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct Host {
    pub pid: u32,
    pub holder: String,
    pub started: f64,
    pub interval_s: f64,
}

/// The engine: one tick at a time, with what it remembers between them.
pub struct Controller {
    options: Options,
    ports: Ports,
    wake_up: WakeUp,
    before_precheck: Option<Box<dyn BeforePrecheck>>,
    round: i64,
    alive: Vec<AliveStep>,
    unknown_budget: UnknownBudget,
}

/// An instant of the clock as the prototype writes it: UTC, to the second, the second truncated.
pub(crate) fn iso(secs: f64) -> String {
    WallTime::from_unix_secs_f64(secs).map(WallTime::to_iso_seconds).unwrap_or_default()
}

fn utc(secs: f64) -> DateTime<Utc> {
    WallTime::from_unix_secs_f64(secs).and_then(|at| DateTime::from_timestamp_micros(at.unix_micros())).unwrap_or_default()
}

/// What the prototype keeps of an error's text: its first characters.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Python's truth of a value of the line.
fn truthy(value: Option<&Json>) -> bool {
    match value {
        None | Some(Json::Null) => false,
        Some(Json::Bool(value)) => *value,
        Some(Json::Int(value)) => *value != 0,
        Some(Json::Float(value)) => *value != 0.0,
        Some(Json::Str(text)) => !text.is_empty(),
        Some(Json::List(items)) => !items.is_empty(),
        Some(Json::Object(fields)) => !fields.is_empty(),
    }
}

/// The outcomes of a project the tick launches, terminal states first: they are rare, and nobody else sees them.
fn launchable() -> [&'static str; 2] {
    let outcomes = &precheck_words().outcomes;
    [outcomes.terminal_state.word.as_str(), outcomes.new_activity.word.as_str()]
}

fn push(record: &mut Map<String, Value>, key: &str, value: Value) {
    if let Some(Value::Array(items)) = record.get_mut(key) {
        items.push(value);
    } else {
        record.insert(key.to_owned(), Value::Array(vec![value]));
    }
}

/// A wake-up as the prototype records it: a fake one says only that, a real one how it went.
fn wake_value(attempt: &WakeAttempt) -> Value {
    let w = &words().wake;
    let outcome = &attempt.outcome;
    let mut value = Map::new();
    value.insert(w.t.clone(), json!(attempt.at.format("%Y-%m-%dT%H:%M:%SZ").to_string()));
    value.insert(w.ok.clone(), json!(outcome.ok));
    if outcome.fake {
        value.insert(w.fake.clone(), json!(true));
        value.insert(w.error.clone(), json!(outcome.error));
        return Value::Object(value);
    }
    value.insert(w.exit.clone(), json!(outcome.exit));
    value.insert(w.duration.clone(), json!(outcome.duration_s));
    value.insert(w.cost.clone(), json!(outcome.cost_usd));
    value.insert(w.error.clone(), json!(outcome.error));
    value.insert(w.expiry_before.clone(), json!(outcome.token_expiry_before));
    value.insert(w.expiry_after.clone(), json!(outcome.token_expiry_after));
    value.insert(w.renewed.clone(), json!(outcome.token_renewed));
    Value::Object(value)
}

impl Controller {
    #[must_use]
    pub fn new(options: Options, ports: Ports, wake_up: WakeUp) -> Controller {
        Controller { options, ports, wake_up, before_precheck: None, round: 0, alive: Vec::new(), unknown_budget: UnknownBudget::default() }
    }

    /// What runs before the precheck, and may end the tick there.
    #[must_use]
    pub fn with_before_precheck(mut self, hook: Box<dyn BeforePrecheck>) -> Controller {
        self.before_precheck = Some(hook);
        self
    }

    /// The count of an earlier controller, from its state document.
    #[must_use]
    pub fn with_unknown_budget(mut self, count: UnknownBudget) -> Controller {
        self.unknown_budget = if count.rounds > 0 { count } else { UnknownBudget::default() };
        self
    }

    /// A step an earlier controller launched, from the registry, so it is not launched again.
    pub fn restore(&mut self, step: AliveStep) {
        self.alive.retain(|alive| alive.project != step.project);
        self.alive.push(step);
    }

    #[must_use]
    pub fn options(&self) -> &Options {
        &self.options
    }

    #[must_use]
    pub fn round(&self) -> i64 {
        self.round
    }

    #[must_use]
    pub fn unknown_budget(&self) -> &UnknownBudget {
        &self.unknown_budget
    }

    /// The count is the alarm's: no step starts until the budget is known again.
    #[must_use]
    pub fn budget_alarm(&self) -> bool {
        self.unknown_budget.rounds >= words().budget.alarm_rounds
    }

    #[must_use]
    pub fn last_wake(&self) -> Option<&WakeAttempt> {
        self.wake_up.last()
    }

    /// The projects with a step alive, sorted.
    #[must_use]
    pub fn alive_projects(&self) -> Vec<String> {
        let mut projects: Vec<String> = self.alive.iter().map(|step| step.project.clone()).collect();
        projects.sort();
        projects
    }

    #[must_use]
    pub fn alive_steps(&self) -> &[AliveStep] {
        &self.alive
    }

    fn now(&self) -> f64 {
        self.ports.clock.now().unix_secs_f64()
    }

    /// D1's guard: the lock renewed right before an effect, outside the effect's own error handling, at every
    /// launch point. A lost lock is the tick's end.
    ///
    /// # Errors
    ///
    /// The lock is lost, or the state cannot be written.
    pub fn guard(&self) -> Result<(), TickError> {
        self.ports.lock.renew().map_err(TickError::from)
    }

    /// One tick, `wakeup` when it follows a suspension of the machine. The line of `controller.log`.
    ///
    /// # Errors
    ///
    /// The lock is lost, or the registry could not be written.
    pub fn tick(&mut self, wakeup: bool) -> Result<Value, TickError> {
        self.round += 1;
        let r = &words().record;
        let t0 = self.now();
        let mut record = Map::new();
        record.insert(r.v.clone(), json!(words().version));
        record.insert(r.t.clone(), json!(iso(t0)));
        record.insert(r.round.clone(), json!(self.round));
        record.insert(r.wakeup.clone(), json!(wakeup));
        record.insert(r.precheck.clone(), Value::Null);
        record.insert(r.decision.clone(), Value::Null);
        record.insert(r.projects.clone(), json!([]));
        record.insert(r.launched.clone(), json!([]));
        record.insert(r.budget.clone(), Value::Null);
        self.guard()?;
        // The resume of suspended sessions (#470) and the scheduled operations (#469) go here, in this order.
        let mut hook = self.before_precheck.take();
        let ended = match hook.as_mut() {
            Some(hook) => hook.before(&mut record, t0),
            None => Ok(false),
        };
        self.before_precheck = hook;
        if ended? {
            self.guard()?;
            // No precheck in this tick, so no tick of the series of a stall: the signal says so to every project.
            let decision = record.get(&r.decision).cloned().unwrap_or(Value::Null);
            self.postpone(&words().reserved_project, decision, &mut record)?;
            record.insert(r.alive.clone(), json!(self.alive_projects()));
            return Ok(Value::Object(record));
        }
        let run = self.ports.precheck.run(&self.options.projects, self.options.precheck_timeout_s);
        self.guard()?;
        let line = run.line.as_ref().filter(|line| line.is_object());
        record.insert(r.precheck.clone(), precheck_record(&run, line));
        self.budget(&mut record, line);
        self.wake(&mut record, t0)?;
        self.reap(&mut record, t0)?;
        let candidates = self.evaluate(&mut record, &run, line);
        if let (false, Some(line)) = (candidates.is_empty(), line) {
            self.launch(&mut record, &candidates, line, t0)?;
            let launched = record.get(&r.launched).and_then(Value::as_array).is_some_and(|items| !items.is_empty());
            let d = &words().decisions;
            record.insert(r.decision.clone(), json!(if launched { &d.launch } else { &d.no_launch }));
        }
        record.insert(r.alive.clone(), json!(self.alive_projects()));
        Ok(Value::Object(record))
    }

    /// The budget the precheck reported (#102 point 7). With work waiting, an unknown share of Claude's
    /// window means the probe does not answer and the cache is older than an hour: the regia stands still, and
    /// the count of such ticks raises the alarm. Without a candidate the precheck asked for no budget, and the
    /// count stays as it is.
    fn budget(&mut self, record: &mut Map<String, Value>, line: Option<&Json>) {
        let b = &words().budget;
        let payload_key = &precheck_words().line.keys.payload;
        let Some(budget) = line.and_then(|line| line.get(payload_key)).filter(|payload| payload.is_object()).and_then(|payload| payload.get(&precheck_words().payload.budget)) else {
            return;
        };
        let Some(pct) = budget.get(&b.pct).filter(|_| budget.is_object()) else { return };
        let value = if *pct == Json::Null {
            let count = &mut self.unknown_budget;
            if count.rounds == 0 {
                count.since = record.get(&words().record.t).and_then(Value::as_str).map(str::to_owned);
            }
            count.rounds += 1;
            if let Some(reason) = budget.get(&b.reason).and_then(Json::as_str) {
                count.reason = Some(reason.to_owned());
            }
            let mut value = Map::new();
            value.insert(b.unknown.clone(), json!(true));
            value.insert(b.rounds.clone(), json!(count.rounds));
            value.insert(b.since.clone(), json!(count.since));
            value.insert(b.alarm.clone(), json!(count.rounds >= b.alarm_rounds));
            if let Some(reason) = &count.reason {
                value.insert(b.reason.clone(), json!(reason));
            }
            value
        } else {
            self.unknown_budget = UnknownBudget::default();
            let mut value = Map::new();
            value.insert(b.unknown.clone(), json!(false));
            value.insert(b.pct.clone(), pct.to_value());
            if truthy(budget.get(&b.stale)) {
                value.insert(b.stale.clone(), json!(true));
            }
            if budget.get(&b.open) == Some(&Json::Bool(false)) {
                value.insert(b.open.clone(), json!(false));
            }
            value
        };
        record.insert(words().record.budget.clone(), Value::Object(value));
    }

    /// The token's wake-up (#190): with the budget unknown and work waiting, one minimal session, at most once
    /// an hour from the last attempt, also a failed one. It starts a session, so the lock is checked before it;
    /// it waits for its end, so the lock is renewed after it.
    fn wake(&mut self, record: &mut Map<String, Value>, t0: f64) -> Result<(), TickError> {
        let b = &words().budget;
        let unknown = record.get(&words().record.budget).and_then(|budget| budget.get(&b.unknown)) == Some(&Value::Bool(true));
        let now = utc(t0);
        if self.options.no_wake || !unknown || !self.wake_up.due(true, now) {
            return Ok(());
        }
        self.guard()?;
        let surface = Arc::clone(&self.ports.surface);
        if let Some(attempt) = self.wake_up.round(true, now, || surface.wake()) {
            record.insert(words().record.wake.clone(), wake_value(attempt));
        }
        self.guard()
    }

    /// The prototype's `_attivo`: alive while its process lives, up to the limit; without a pid, for a window of
    /// grace between the launch and the step's start.
    fn is_alive(&self, project: &str, now: f64) -> (bool, &'static str) {
        let notes = &words().finished.notes;
        let Some(step) = self.alive.iter().find(|step| step.project == project) else { return (false, &notes.not_launched) };
        let age = now - step.t;
        if age >= self.options.step_limit_s {
            return (false, &notes.limit);
        }
        match step.pid {
            None if age < words().grace_without_pid_s => (true, &notes.grace),
            None => (false, &notes.grace_over),
            Some(pid) if self.ports.liveness.alive(pid) => (true, &notes.alive),
            Some(_) => (false, &notes.ended),
        }
    }

    /// The steps no longer alive leave the tick's memory, and their records are finished. Before that, every
    /// process this host started is reaped once it ended, also one past its step's limit.
    fn reap(&mut self, record: &mut Map<String, Value>, now: f64) -> Result<(), TickError> {
        self.ports.liveness.reap();
        let f = &words().finished;
        let projects: Vec<String> = self.alive.iter().map(|step| step.project.clone()).collect();
        for project in projects {
            let (alive, why) = self.is_alive(&project, now);
            if alive {
                continue;
            }
            let Some(at) = self.alive.iter().position(|step| step.project == project) else { continue };
            let step = self.alive.remove(at);
            self.ports.steps.finished(&step.key, now)?;
            let mut finished = Map::new();
            finished.insert(f.project.clone(), json!(project));
            finished.insert(f.why.clone(), json!(why));
            push(record, &words().record.finished, Value::Object(finished));
        }
        Ok(())
    }

    /// The decision on the precheck's run: the entries to launch, or none after writing the decision.
    fn evaluate(&self, record: &mut Map<String, Value>, run: &PrecheckRun, line: Option<&Json>) -> Vec<Json> {
        let d = &words().decisions;
        let r = &words().record;
        let keys = &precheck_words().line.keys;
        let mut error = None;
        let decision = match run.exit {
            None => &d.precheck_broken,
            Some(3) => &d.idle,
            Some(1) => &d.error,
            Some(code) if code != 0 => &d.unexpected_exit,
            Some(_) => match line {
                None => {
                    error = Some("exit 0 without a JSON line".to_owned());
                    &d.incoherent
                }
                Some(line) => {
                    let outcome = line.get(&keys.outcome);
                    let launches = outcome.and_then(Json::as_str).is_some_and(|outcome| launchable().contains(&outcome));
                    if line.get(&keys.exit).and_then(Json::as_i64) != Some(0) || !launches {
                        let text = |value: Option<&Json>| value.map_or_else(|| "None".to_owned(), |value| value.as_str().map_or_else(|| value.to_ascii(), str::to_owned));
                        error = Some(format!("exit 0 but JSON {}/exit {}", text(outcome), text(line.get(&keys.exit))));
                        &d.incoherent
                    } else {
                        let payload = line.get(&keys.payload).filter(|payload| payload.is_object());
                        match payload.and_then(|payload| payload.get(&precheck_words().payload.projects)).and_then(Json::as_list) {
                            None => {
                                error = Some("payload truncated or without the list of projects: no launch".to_owned());
                                &d.truncated
                            }
                            Some(projects) => {
                                let p = &precheck_words().project;
                                let order = launchable();
                                let mut entries: Vec<Json> = projects
                                    .iter()
                                    .filter(|entry| entry.is_object() && truthy(entry.get(&p.id)))
                                    .filter(|entry| entry.get(&p.outcome).and_then(Json::as_str).is_some_and(|outcome| order.contains(&outcome)))
                                    .cloned()
                                    .collect();
                                let rank = |entry: &Json| order.iter().position(|outcome| Some(*outcome) == entry.get(&p.outcome).and_then(Json::as_str));
                                entries.sort_by_key(rank);
                                if entries.is_empty() {
                                    record.insert(r.decision.clone(), json!(d.incoherent));
                                    set_error(record, "exit 0 but no project to launch in the payload");
                                }
                                return entries;
                            }
                        }
                    }
                }
            },
        };
        record.insert(r.decision.clone(), json!(decision));
        if let Some(error) = error {
            set_error(record, &error);
        }
        Vec::new()
    }

    /// One step per candidate, at most `max_steps` in the tick: a project whose step is alive is skipped, one
    /// with a suspended session too, and the rest wait for the next tick with a postponement.
    fn launch(&mut self, record: &mut Map<String, Value>, entries: &[Json], line: &Json, t0: f64) -> Result<(), TickError> {
        let keys = &precheck_words().line.keys;
        let p = &precheck_words().project;
        let (row_words, actions, r) = (&words().row, &words().actions, &words().record);
        let payload = line.get(&keys.payload).cloned().unwrap_or(Json::Null);
        let precheck_t = line.get(&keys.t).and_then(Json::as_str);
        let mut folders: Option<std::collections::BTreeMap<String, Option<String>>> = None;
        let mut launched = 0;
        for entry in entries {
            let project = entry.get(&p.id).map_or_else(String::new, |id| id.as_str().map_or_else(|| id.to_ascii(), str::to_owned));
            let mut row = Map::new();
            row.insert(row_words.project.clone(), json!(project));
            row.insert(row_words.outcome.clone(), entry.get(&p.outcome).map_or(Value::Null, Json::to_value));
            row.insert(row_words.reason.clone(), entry.get(&p.reason).map_or(Value::Null, Json::to_value));
            let action = if self.is_alive(&project, t0).0 {
                &actions.skipped_alive
            } else if self.ports.steps.suspended_projects().contains(&project) {
                // Work half done: no second step, even with the project's lock expired.
                self.postpone(&project, json!(actions.skipped_suspended), record)?;
                &actions.skipped_suspended
            } else if launched >= self.options.max_steps {
                self.postpone(&project, json!(actions.postponed), record)?;
                &actions.postponed
            } else {
                match self.prepare(&project, entry, &payload, precheck_t, &mut folders) {
                    Err(error) => {
                        row.insert(row_words.error.clone(), json!(cut(&error, 300)));
                        &actions.launch_error
                    }
                    Ok((folder, text, request)) => {
                        // D1: right before the effect, outside the launch's error handling.
                        self.guard()?;
                        match self.ports.surface.launch(&request) {
                            Err(error) => {
                                row.insert(row_words.error.clone(), json!(cut(&error.to_string(), 300)));
                                &actions.launch_error
                            }
                            Ok(receipt) => {
                                let receipt = StepReceipt::of(&receipt);
                                let at = self.now();
                                if let Some(pid) = receipt.pid {
                                    self.ports.liveness.adopt(pid);
                                }
                                let step = LaunchedStep { project: &project, folder: &folder, prompt: &text, receipt: &receipt };
                                let key = self.ports.steps.launched(&step, at)?;
                                self.alive.retain(|alive| alive.project != project);
                                self.alive.push(AliveStep { project: project.clone(), pid: receipt.pid, t: at, session: receipt.session.clone(), key });
                                push(record, &r.launched, launched_value(&receipt, &project));
                                launched += 1;
                                &actions.launched
                            }
                        }
                    }
                }
            };
            // The row's action, then its error, as the prototype's row has them.
            let error = row.remove(&row_words.error);
            row.insert(row_words.action.clone(), json!(action));
            if let Some(error) = error {
                row.insert(row_words.error.clone(), error);
            }
            push(record, &r.projects, Value::Object(row));
        }
        Ok(())
    }

    /// The folder, the prompt and the request of a launch, before any effect: a project without a folder on
    /// this machine is a launch error. The registry is read once a tick, again after a failed read.
    fn prepare(
        &self,
        project: &str,
        entry: &Json,
        payload: &Json,
        precheck_t: Option<&str>,
        folders: &mut Option<std::collections::BTreeMap<String, Option<String>>>,
    ) -> Result<(String, String, LaunchRequest), String> {
        if folders.is_none() {
            *folders = Some(self.ports.folders.folders()?);
        }
        let folder = folders.as_ref().and_then(|folders| folders.get(project).cloned().flatten());
        let folder = match folder {
            Some(folder) if !folder.is_empty() && Path::new(&folder).is_dir() => folder,
            other => return Err(format!("the project's folder was not found: {other:?}")),
        };
        let text = prompt(project, &folder, entry, payload, precheck_t);
        let request = LaunchRequest::new(self.options.service, project, Path::new(&folder), &text)
            .map_err(|error| error.to_string())?
            .model(self.options.model.clone())
            .effort(self.options.effort);
        Ok((folder, text, request))
    }

    /// The postponement of `project` (#133, §6.3 of the design note), or of the reserved project for a tick
    /// without a precheck: the instant, an id and the reason, which the precheck applies once. A failed write
    /// does not stop the tick, and is recorded; a lost lock does.
    fn postpone(&self, project: &str, reason: Value, record: &mut Map<String, Value>) -> Result<(), TickError> {
        let w = &words().postpone;
        let mut value = Map::new();
        value.insert(w.v.clone(), json!(1));
        value.insert(w.t.clone(), json!(iso(self.now())));
        value.insert(w.id.clone(), json!((self.ports.ids)()));
        value.insert(w.reason.clone(), reason);
        value.insert(w.round.clone(), json!(self.round));
        match self.ports.signals.postpone(project, Value::Object(value)) {
            Ok(()) => Ok(()),
            Err(StateError::LockLost(lost)) => Err(TickError::LockLost(lost)),
            Err(StateError::Failed(error)) => {
                push(record, &words().record.postpone_errors, json!(cut(&format!("{project}: {error}"), 200)));
                Ok(())
            }
        }
    }

    /// The state document after the tick of `record`, as the prototype's state file of the controller: the host
    /// writes it (#468). The sessions to resume and the scheduled operations are empty until #469 and #470.
    #[must_use]
    pub fn status(&self, record: &Value, host: &Host) -> Value {
        let (s, r, b) = (&words().status, &words().record, &words().budget);
        let field = |key: &str| record.get(key).cloned().unwrap_or(Value::Null);
        let count = &self.unknown_budget;
        let unknown = if count.rounds > 0 {
            json!({b.rounds.as_str(): count.rounds, b.since.as_str(): count.since, b.alarm.as_str(): self.budget_alarm(), b.reason.as_str(): count.reason})
        } else {
            Value::Null
        };
        let mut status = Map::new();
        status.insert(s.v.clone(), json!(words().version));
        status.insert(s.pid.clone(), json!(host.pid));
        status.insert(s.holder.clone(), json!(host.holder));
        status.insert(s.started.clone(), json!(iso(host.started)));
        status.insert(s.interval.clone(), json!(host.interval_s));
        status.insert(s.last_round.clone(), field(&r.t));
        status.insert(s.round.clone(), field(&r.round));
        status.insert(s.precheck.clone(), field(&r.precheck));
        status.insert(s.decision.clone(), field(&r.decision));
        status.insert(s.budget.clone(), field(&r.budget));
        status.insert(s.unknown_budget.clone(), unknown);
        status.insert(s.alive.clone(), json!(self.alive_projects()));
        status.insert(s.suspended.clone(), json!([]));
        status.insert(s.to_resume.clone(), json!(0));
        status.insert(s.to_open.clone(), json!([]));
        status.insert(s.operations.clone(), json!({}));
        status.insert(s.wake.clone(), self.wake_up.last().map_or(Value::Null, wake_value));
        Value::Object(status)
    }
}

fn set_error(record: &mut Map<String, Value>, error: &str) {
    let r = &words().record;
    if let Some(Value::Object(precheck)) = record.get_mut(&r.precheck) {
        precheck.insert(r.precheck_keys.error.clone(), json!(error));
    }
}

/// What the tick keeps of the precheck's run.
fn precheck_record(run: &PrecheckRun, line: Option<&Json>) -> Value {
    let k = &words().record.precheck_keys;
    let keys = &precheck_words().line.keys;
    let field = |key: &str| line.and_then(|line| line.get(key)).map_or(Value::Null, Json::to_value);
    let mut value = Map::new();
    value.insert(k.exit.clone(), json!(run.exit));
    value.insert(k.outcome.clone(), field(&keys.outcome));
    value.insert(k.reason.clone(), field(&keys.reason));
    value.insert(k.t.clone(), field(&keys.t));
    if let Some(error) = &run.error {
        value.insert(k.error.clone(), json!(error));
    }
    Value::Object(value)
}

/// A launch as the line of the tick lists it.
fn launched_value(receipt: &StepReceipt, project: &str) -> Value {
    let l = &words().launched;
    let mut value = Map::new();
    value.insert(l.adapter.clone(), json!(receipt.adapter));
    value.insert(l.agent.clone(), json!(receipt.agent));
    value.insert(l.model.clone(), json!(receipt.model));
    value.insert(l.effort.clone(), json!(receipt.effort));
    value.insert(l.session.clone(), json!(receipt.session));
    value.insert(l.transcript.clone(), json!(receipt.transcript));
    value.insert(l.pid.clone(), json!(receipt.pid));
    value.insert(l.log.clone(), json!(receipt.log));
    value.insert(l.started.clone(), json!(receipt.started));
    value.insert(l.project.clone(), json!(project));
    Value::Object(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_parses_and_its_numbers_are_the_prototype_s() {
        let format = words();
        assert_eq!((format.step_limit_s, format.grace_without_pid_s, format.budget.alarm_rounds), (3600.0, 300.0, 4));
        assert_eq!(format.registry.keep_finished_s, 7.0 * 86400.0);
        assert_eq!(Options::default().max_steps, 1);
    }

    #[test]
    fn a_run_s_line_is_its_last_json_line_with_its_keys_in_order() {
        let run = PrecheckRun::from_output(Some(3), "precheck: x=1\n{\"b\":1,\"a\":[2.0,\"\\u00e8\"]}\n{not json\n");
        let line = run.line.expect("a line");
        assert_eq!(line.to_ascii(), "{\"b\":1,\"a\":[2.0,\"\\u00e8\"]}");
        assert_eq!(PrecheckRun::from_output(Some(0), "{non json\n").line, None);
        assert_eq!(PrecheckRun::failed("timeout after 120 s").exit, None);
    }

    #[test]
    fn an_instant_is_written_to_the_second_truncated() {
        assert_eq!(iso(1_768_035_600.9), "2026-01-10T09:00:00Z");
        assert_eq!(cut("abcdef", 3), "abc");
        assert!(truthy(Some(&Json::Bool(true))) && !truthy(Some(&Json::Int(0))) && !truthy(None));
    }
}
