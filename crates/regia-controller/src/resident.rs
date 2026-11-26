//! The resident controller (F26 of the dossier's plan, ADR 0001 §7): the host of the engine's tick, which runs
//! the ticks one after the other as the prototype's resident controller does (F069), on this machine's state.
//!
//! - **One controller across the languages** (F137–F139). It takes the lock of the reserved project before it
//!   reads anything else of the state, the same row, fields and TTL the prototype's controller takes; the tick
//!   renews it at each of its launch points, the wait at each of its slices; it is released at every exit. A lock
//!   another controller holds, Rust or Python, stops it with exit 3, and is left as it is.
//! - **D2 is not inherited** (F255, F156, F157). Building a resident reads and writes nothing: the steps registry is
//!   read once the lock is taken, in a transaction that checks the lock, and saved record by record with a
//!   version check ([`StoreSteps`]).
//! - **A restart launches nothing twice** (F135, F136). The count of the ticks with Claude's budget unknown and the
//!   last wake-up of the token come back from the state file, so the alarm and the wake-up's cap hold across a
//!   restart; the steps still launched come back from the registry, so the window between a launch and its step's
//!   start does not launch the project again.
//! - **What it writes** (F171–F173): the lock; the log, one ASCII JSON line a tick, moved aside past 1 MB into three
//!   copies; and the state file, replaced whole, which the Python brief reads for its alarm. The tick writes the
//!   postponements and the registry.
//! - **The wait** (F174, F175): in slices, the lock renewed after each; far more time than the interval since the
//!   last tick is a suspension of the machine, and a tick runs at once.
//!
//! It prints nothing and names no surface: what its user reads comes as typed [`Notice`]s and an [`Exit`], which
//! the command renders in the user's language. The command line, the scheduled operations and the resume of the
//! sessions the usage limit stopped are #469's and #470's; the hook before the precheck is where they go.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::DateTime;
use regia_agents::{LaunchError, LaunchRequest, LaunchSurface, ProbeConsent, Receipt, SurfaceKind, WakeAttempt, WakeOutcome, WakeUp};
use regia_store::{Clock, Holder, LockView, ProjectId, Store, Ttl, WallTime};
use serde_json::{Map, Value, json};

use crate::engine::native::{LOCK_TTL_S, StoreLock, StoreSignals, StoreSteps, random_id};
use crate::engine::words::format as words;
use crate::engine::{self, BeforePrecheck, Controller, Folders, Host, Liveness, Options, Precheck, StateError, TickError, UnknownBudget, iso};
use crate::precheck::Json;

/// How a resident runs: the prototype's options of a run (F005), by meaning; their command line is #469's.
#[derive(Debug, Clone)]
pub struct Config {
    /// The tick's own.
    pub options: Options,
    /// Seconds from the end of a tick to the next one.
    pub interval_s: f64,
    /// The ticks it runs before it exits, 0 for ever: 1 for a single tick.
    pub rounds: u64,
    /// The state folder, where the log and the state file go, beside the database.
    pub folder: PathBuf,
    /// Its process, as the state file and the lock's holder name it.
    pub pid: u32,
}

impl Config {
    /// The prototype's defaults: the tick's, 900 seconds between two ticks, for ever, this process.
    #[must_use]
    pub fn new(folder: impl Into<PathBuf>) -> Config {
        Config { options: Options::default(), interval_s: words().resident.interval_s, rounds: 0, folder: folder.into(), pid: std::process::id() }
    }

    /// The prototype's check of its options: the interval, the steps of a tick, the step's limit and the
    /// precheck's timeout are positive.
    ///
    /// # Errors
    ///
    /// The first option that is not.
    pub fn check(&self) -> Result<(), BadConfig> {
        let positive = |value: f64| value.is_finite() && value > 0.0;
        if !positive(self.interval_s) {
            return Err(BadConfig::Interval);
        }
        if self.options.max_steps < 1 {
            return Err(BadConfig::MaxSteps);
        }
        if !positive(self.options.step_limit_s) {
            return Err(BadConfig::StepLimit);
        }
        if !positive(self.options.precheck_timeout_s) {
            return Err(BadConfig::PrecheckTimeout);
        }
        Ok(())
    }
}

/// An option of [`Config`] that is not positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadConfig {
    Interval,
    MaxSteps,
    StepLimit,
    PrecheckTimeout,
}

/// The wait between two looks at the clock.
pub trait Sleep: Send + Sync {
    fn sleep(&self, secs: f64);
}

/// The system's sleep. A stop asked during a slice is seen at the slice's end.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemSleep;

impl Sleep for SystemSleep {
    fn sleep(&self, secs: f64) {
        if secs.is_finite() && secs > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(secs));
        }
    }
}

/// How often a [`StopSleep`] looks at its flag.
const STOP_LOOK: Duration = Duration::from_millis(100);

/// The system's sleep, cut short by a stop: it looks at the flag every tenth of a second, so a stop asked during
/// the wait (Ctrl+C, #468/V3) ends it at once instead of at its slice's end.
#[derive(Debug, Clone)]
pub struct StopSleep {
    stop: Arc<AtomicBool>,
}

impl StopSleep {
    #[must_use]
    pub fn new(stop: Arc<AtomicBool>) -> StopSleep {
        StopSleep { stop }
    }
}

impl Sleep for StopSleep {
    fn sleep(&self, secs: f64) {
        if !(secs.is_finite() && secs > 0.0) {
            return;
        }
        let span = Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX);
        let start = std::time::Instant::now();
        while !self.stop.load(Ordering::SeqCst) {
            let left = span.saturating_sub(start.elapsed());
            if left.is_zero() {
                return;
            }
            std::thread::sleep(left.min(STOP_LOOK));
        }
    }
}

/// What a resident tells its user, as data: the command renders it in the user's language (#469).
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// A tick ended: its line, as the log has it.
    Ticked(Value),
    /// At this tick the ticks in a row with Claude's budget unknown reached the alarm: nothing is launched until
    /// the probe answers again. How many, since when, and the probe's reason.
    BudgetAlarm { rounds: i64, since: Option<String>, reason: Option<String> },
    /// A tick failed, and the next one runs as usual.
    TickFailed { round: i64, error: String },
    /// Far more time passed than the interval: the machine was suspended, and a tick runs at once.
    Wakeup,
    /// The log could not be written: the resident goes on.
    LogNotWritten(String),
    /// The state file could not be written: the resident goes on.
    StatusNotWritten(String),
}

/// Where a resident's notices go.
pub trait Console: Send + Sync {
    fn notice(&self, notice: &Notice);
}

/// Why a resident stopped, and its exit code: 0 done, 1 an error, 3 another controller is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    /// The ticks asked for are done.
    Done,
    /// A stop was asked, as by Ctrl+C.
    Stopped,
    /// Another controller held the lock, and holds it still: `holder` names it, when its row says.
    AnotherController { holder: Option<String> },
    /// The lock passed to another controller while this one ran.
    LockLost(String),
    /// An option the resident refuses.
    BadConfig(BadConfig),
    /// The state could not be used.
    Failed(String),
}

impl Exit {
    #[must_use]
    pub fn code(&self) -> u8 {
        match self {
            Exit::Done | Exit::Stopped => 0,
            Exit::AnotherController { .. } | Exit::LockLost(_) => 3,
            Exit::BadConfig(_) | Exit::Failed(_) => 1,
        }
    }
}

/// What a resident runs on: the state database and the clock of its expiries, the wait, the tick's ports, the
/// consent to the usage probes (for the token's wake-up), where its notices go, and the flag that asks it to stop.
#[derive(Clone)]
pub struct HostPorts {
    pub store: Store,
    pub clock: Arc<dyn Clock>,
    pub sleep: Arc<dyn Sleep>,
    pub precheck: Arc<dyn Precheck>,
    pub folders: Arc<dyn Folders>,
    pub liveness: Arc<dyn Liveness>,
    pub surface: Arc<dyn LaunchSurface + Send + Sync>,
    /// The id of a postponement.
    pub ids: Arc<dyn Fn() -> String + Send + Sync>,
    pub consent: ProbeConsent,
    /// The reader of Claude's token expiry, the probes' own (`UsageProbes::claude_token_expiry`): the token's
    /// wake-up reads it before and after (#462/V3). None reads nothing.
    pub token_expiry: Option<Arc<dyn Fn() -> Option<String> + Send + Sync>>,
    pub console: Arc<dyn Console>,
    pub stop: Arc<AtomicBool>,
}

/// The tick's surface, whose token wake-up reads the token's expiry before and after it, so the state says whether
/// the wake-up renewed the token (#462/V3: `WakeUp::round` in the tick, around `WakeOutcome::with_expiry`).
struct ExpiryAround {
    surface: Arc<dyn LaunchSurface + Send + Sync>,
    expiry: Arc<dyn Fn() -> Option<String> + Send + Sync>,
}

impl LaunchSurface for ExpiryAround {
    fn kind(&self) -> SurfaceKind {
        self.surface.kind()
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        self.surface.launch(request)
    }

    fn wake(&self) -> WakeOutcome {
        WakeOutcome::with_expiry(|| (self.expiry)(), || self.surface.wake())
    }
}

/// What the controller before this one left in its state file (F135): the count of the ticks with the budget
/// unknown, and its last wake-up of the token.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Earlier {
    pub unknown: UnknownBudget,
    pub wake: Option<WakeAttempt>,
}

impl Earlier {
    /// From the state file in `folder`: nothing from a file that is missing or does not read, as the prototype's.
    #[must_use]
    pub fn read(folder: &Path) -> Earlier {
        let status = fs::read(folder.join(&words().resident.status)).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        match status {
            Some(Value::Object(status)) => Earlier::of(&status),
            _ => Earlier::default(),
        }
    }

    /// From a state document: a count of whole ticks above 0, and a wake-up whose instant reads.
    #[must_use]
    pub fn of(status: &Map<String, Value>) -> Earlier {
        let (s, b, w) = (&words().status, &words().budget, &words().wake);
        let mut earlier = Earlier::default();
        if let Some(Value::Object(count)) = status.get(&s.unknown_budget)
            && let Some(rounds) = count.get(&b.rounds).and_then(Value::as_i64).filter(|rounds| *rounds > 0)
        {
            let text = |key: &str| count.get(key).and_then(Value::as_str).map(str::to_owned);
            earlier.unknown = UnknownBudget { rounds, since: text(&b.since), reason: text(&b.reason) };
        }
        if let Some(Value::Object(wake)) = status.get(&s.wake)
            && let Some(at) = wake.get(&w.t).and_then(Value::as_str).and_then(WallTime::parse_iso).and_then(|at| DateTime::from_timestamp_micros(at.unix_micros()))
        {
            let text = |key: &str| wake.get(key).and_then(Value::as_str).map(str::to_owned);
            let flag = |key: &str| wake.get(key).and_then(Value::as_bool).unwrap_or(false);
            let outcome = WakeOutcome {
                ok: flag(&w.ok),
                exit: wake.get(&w.exit).and_then(Value::as_i64).and_then(|exit| i32::try_from(exit).ok()),
                duration_s: wake.get(&w.duration).and_then(Value::as_f64),
                cost_usd: wake.get(&w.cost).and_then(Value::as_f64),
                error: text(&w.error),
                fake: flag(&w.fake),
                token_expiry_before: text(&w.expiry_before),
                token_expiry_after: text(&w.expiry_after),
                token_renewed: flag(&w.renewed),
            };
            earlier.wake = Some(WakeAttempt { at, outcome });
        }
        earlier
    }
}

/// F160: a record of the registry is alive while its process lives; one without a pid is not.
#[must_use]
pub fn record_alive(record: &Value, liveness: &dyn Liveness) -> bool {
    let pid = record.get(&words().registry.record.pid).and_then(Value::as_u64).and_then(|pid| u32::try_from(pid).ok());
    pid.filter(|pid| *pid != 0).is_some_and(|pid| liveness.alive(pid))
}

/// F164: the projects' folders, none when the registry cannot be read.
#[must_use]
pub fn folders_or_none(folders: &dyn Folders) -> BTreeMap<String, Option<String>> {
    folders.folders().unwrap_or_default()
}

/// How a wait ended.
enum Waited {
    /// The interval passed: the next tick.
    Next,
    /// A suspension of the machine: a tick at once.
    Wakeup,
    Exit(Exit),
}

/// The resident controller: [`Resident::run`].
pub struct Resident {
    config: Config,
    ports: HostPorts,
    before_precheck: Vec<Box<dyn BeforePrecheck>>,
    holder: Holder,
    started: f64,
}

fn reserved() -> Result<ProjectId, String> {
    ProjectId::new(words().reserved_project.clone()).map_err(|error| error.to_string())
}

fn state_exit(error: StateError) -> Exit {
    match error {
        StateError::LockLost(lost) => Exit::LockLost(lost.0),
        StateError::Failed(error) => Exit::Failed(error),
    }
}

fn tick_exit(error: TickError) -> Exit {
    match error {
        TickError::LockLost(lost) => Exit::LockLost(lost.0),
        TickError::Failed(error) => Exit::Failed(error),
    }
}

/// A value as the prototype writes its log and its state file: compact, ASCII, with Python's floats.
fn ascii(value: &Value) -> String {
    serde_json::from_value::<Json>(value.clone()).map_or_else(|_| value.to_string(), |json| json.to_ascii())
}

/// F172: one line at the log's end, after a log past its size moved to the first copy and the older copies one
/// up, as many as the format keeps.
fn append_line(folder: &Path, line: &Value) -> io::Result<()> {
    let r = &words().resident;
    let path = folder.join(&r.log);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > r.log_max_bytes) {
        let copy = |n: u32| folder.join(format!("{}.{n}", r.log));
        for n in (1..r.log_copies).rev() {
            if copy(n).exists() {
                fs::rename(copy(n), copy(n + 1))?;
            }
        }
        fs::rename(&path, copy(1))?;
    }
    let mut file = fs::OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{}", ascii(line))
}

/// F173: `name` replaced whole through a temporary file of this process beside it, so a reader finds the old file
/// or the new one, and two controllers never write the same temporary file.
fn replace_file(folder: &Path, name: &str, pid: u32, bytes: &[u8]) -> io::Result<()> {
    let temporary = folder.join(format!("{name}.{pid}.tmp"));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, folder.join(name))
}

impl Resident {
    /// A resident and the holder it takes the lock as, `<reserved project>-<pid>-<8 hex digits>` as the
    /// prototype's. It reads and writes nothing (F255): the state is the lock holder's, and [`Resident::run`] takes
    /// the lock.
    #[must_use]
    pub fn new(config: Config, ports: HostPorts) -> Resident {
        let holder = Holder::new(format!("{}-{}-{}", words().reserved_project, config.pid, random_id())).expect("a holder with letters");
        let started = ports.clock.now().unix_secs_f64();
        Resident { config, ports, before_precheck: Vec::new(), holder, started }
    }

    /// What runs in each tick before the precheck, and may end the tick there, after the hooks given before it:
    /// the resume of the suspended sessions (#470), then the scheduled operations (#469).
    #[must_use]
    pub fn with_before_precheck(mut self, hook: Box<dyn BeforePrecheck>) -> Resident {
        self.before_precheck.push(hook);
        self
    }

    #[must_use]
    pub fn holder(&self) -> &Holder {
        &self.holder
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Runs until the ticks asked for are done, a stop is asked or the lock is lost (F175). The lock is released at
    /// every exit once it was asked for, and another holder's is left alone (F139).
    pub fn run(&mut self) -> Exit {
        if let Err(bad) = self.config.check() {
            return Exit::BadConfig(bad);
        }
        let exit = self.hold();
        if let Ok(lock) = reserved() {
            // Releasing a lock that is someone else's, or nobody's, changes nothing.
            let _ = self.ports.store.release_lock(&lock, &self.holder);
        }
        exit
    }

    fn now(&self) -> f64 {
        self.ports.clock.now().unix_secs_f64()
    }

    fn notice(&self, notice: Notice) {
        self.ports.console.notice(&notice);
    }

    /// F137: the lock first, then the registry and what the state file says of the controller before, then the
    /// ticks.
    fn hold(&mut self) -> Exit {
        let lock = match reserved() {
            Ok(lock) => lock,
            Err(error) => return Exit::Failed(error),
        };
        let ttl = Ttl::from_secs(LOCK_TTL_S).expect("a ttl above 0");
        match self.ports.store.acquire_lock(&lock, &self.holder, ttl) {
            Ok(outcome) if outcome.is_ok() => {}
            Ok(outcome) => return Exit::AnotherController { holder: outcome.lock().and_then(LockView::holder).map(str::to_owned) },
            Err(error) => return Exit::Failed(error.to_string()),
        }
        let (store, holder) = (&self.ports.store, &self.holder);
        let steps = Arc::new(StoreSteps::new(store.clone(), holder.clone()));
        if let Err(error) = steps.load() {
            return state_exit(error);
        }
        let earlier = Earlier::read(&self.config.folder);
        let surface: Arc<dyn LaunchSurface + Send + Sync> = match &self.ports.token_expiry {
            Some(expiry) => Arc::new(ExpiryAround { surface: Arc::clone(&self.ports.surface), expiry: Arc::clone(expiry) }),
            None => Arc::clone(&self.ports.surface),
        };
        let ports = engine::Ports {
            clock: Arc::clone(&self.ports.clock),
            precheck: Arc::clone(&self.ports.precheck),
            folders: Arc::clone(&self.ports.folders),
            liveness: Arc::clone(&self.ports.liveness),
            surface,
            lock: Arc::new(StoreLock::new(store.clone(), holder.clone())),
            signals: Arc::new(StoreSignals::new(store.clone(), holder.clone())),
            steps: steps.clone(),
            ids: Arc::clone(&self.ports.ids),
        };
        let wake_up = WakeUp::new(self.ports.consent).after(earlier.wake);
        let mut controller = Controller::new(self.config.options.clone(), ports, wake_up).with_unknown_budget(earlier.unknown);
        for hook in std::mem::take(&mut self.before_precheck) {
            controller = controller.with_before_precheck(hook);
        }
        for step in steps.launched_steps(self.started) {
            controller.restore(step);
        }
        self.cycle(&mut controller)
    }

    /// F175: a tick, then the wait, until the ticks asked for are done. A tick that fails is logged and the next
    /// one runs; a lost lock ends the run.
    fn cycle(&self, controller: &mut Controller) -> Exit {
        let mut wakeup = false;
        loop {
            match controller.tick(wakeup) {
                Ok(record) => self.close(controller, &record),
                Err(TickError::LockLost(lost)) => return Exit::LockLost(lost.0),
                Err(TickError::Failed(error)) => self.failed(controller.round(), &error),
            }
            if self.config.rounds > 0 && u64::try_from(controller.round()).unwrap_or(0) >= self.config.rounds {
                return Exit::Done;
            }
            match self.wait(controller, self.now()) {
                Waited::Next => wakeup = false,
                Waited::Wakeup => {
                    self.woke(controller.round());
                    wakeup = true;
                }
                Waited::Exit(exit) => return exit,
            }
        }
    }

    /// F174: in slices until the interval passed since `end`, the lock renewed after each slice (F138). More than
    /// the format's factor of intervals since `end` is a suspension.
    fn wait(&self, controller: &Controller, end: f64) -> Waited {
        let r = &words().resident;
        let interval = self.config.interval_s;
        let next = end + interval;
        loop {
            if self.ports.stop.load(Ordering::SeqCst) {
                return Waited::Exit(Exit::Stopped);
            }
            let now = self.now();
            if now - end > r.wakeup_factor * interval {
                return Waited::Wakeup;
            }
            let left = next - now;
            if left <= 0.0 {
                return Waited::Next;
            }
            self.ports.sleep.sleep(left.min(r.slice_s));
            if let Err(error) = controller.guard() {
                return Waited::Exit(tick_exit(error));
            }
        }
    }

    /// F171: the tick's alarm, its line in the log, the state file, and the tick's notice.
    fn close(&self, controller: &Controller, record: &Value) {
        let alarm = &words().budget.alarm;
        if record.get(&words().record.budget).and_then(|budget| budget.get(alarm)) == Some(&Value::Bool(true)) {
            let count = controller.unknown_budget();
            self.notice(Notice::BudgetAlarm { rounds: count.rounds, since: count.since.clone(), reason: count.reason.clone() });
        }
        self.log(record);
        let host = Host { pid: self.config.pid, holder: self.holder.as_str().to_owned(), started: self.started, interval_s: self.config.interval_s };
        let status = ascii(&controller.status(record, &host));
        if let Err(error) = replace_file(&self.config.folder, &words().resident.status, self.config.pid, status.as_bytes()) {
            self.notice(Notice::StatusNotWritten(error.to_string()));
        }
        self.notice(Notice::Ticked(record.clone()));
    }

    /// A line of the log that is not a tick's: its version, instant and tick, then `decision` and its own key.
    fn line(&self, round: i64, decision: &str, key: &str, value: Value) -> Value {
        let r = &words().record;
        let mut line = Map::new();
        line.insert(r.v.clone(), json!(words().version));
        line.insert(r.t.clone(), json!(iso(self.now())));
        line.insert(r.round.clone(), json!(round));
        line.insert(r.decision.clone(), json!(decision));
        line.insert(key.to_owned(), value);
        Value::Object(line)
    }

    fn failed(&self, round: i64, error: &str) {
        let words = &words().resident.line;
        let cut: String = error.chars().take(300).collect();
        self.log(&self.line(round, &words.tick_failed, &words.error, json!(cut)));
        self.notice(Notice::TickFailed { round, error: error.to_owned() });
    }

    fn woke(&self, round: i64) {
        let words = &words().resident.line;
        self.log(&self.line(round, &words.wakeup, &words.note, json!(words.wakeup_note)));
        self.notice(Notice::Wakeup);
    }

    fn log(&self, line: &Value) {
        if let Err(error) = append_line(&self.config.folder, line) {
            self.notice(Notice::LogNotWritten(error.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_s_numbers_are_the_prototype_s() {
        let r = &words().resident;
        assert_eq!((r.log_max_bytes, r.log_copies, r.interval_s, r.slice_s, r.wakeup_factor), (1_000_000, 3, 900.0, 30.0, 2.0));
        assert_eq!(LOCK_TTL_S, 600);
    }

    #[test]
    fn the_exit_codes_are_the_prototype_s() {
        assert_eq!([Exit::Done.code(), Exit::Stopped.code()], [0, 0]);
        assert_eq!([Exit::AnotherController { holder: None }.code(), Exit::LockLost(String::new()).code()], [3, 3]);
        assert_eq!([Exit::BadConfig(BadConfig::Interval).code(), Exit::Failed(String::new()).code()], [1, 1]);
    }

    #[test]
    fn options_that_are_not_positive_are_refused() {
        let config = Config::new("state");
        assert_eq!(config.check(), Ok(()));
        let bad = |change: fn(&mut Config)| {
            let mut config = Config::new("state");
            change(&mut config);
            config.check()
        };
        assert_eq!(bad(|c| c.interval_s = 0.0), Err(BadConfig::Interval));
        assert_eq!(bad(|c| c.interval_s = f64::NAN), Err(BadConfig::Interval));
        assert_eq!(bad(|c| c.options.max_steps = 0), Err(BadConfig::MaxSteps));
        assert_eq!(bad(|c| c.options.step_limit_s = -3.0), Err(BadConfig::StepLimit));
        assert_eq!(bad(|c| c.options.precheck_timeout_s = 0.0), Err(BadConfig::PrecheckTimeout));
    }

    #[test]
    fn a_line_is_ascii_with_python_s_floats() {
        assert_eq!(ascii(&json!({"a": "è", "b": 82.0, "c": 1e-5})), "{\"a\":\"\\u00e8\",\"b\":82.0,\"c\":1e-05}");
    }

    /// The keys are the format's: the count of whole ticks above 0, and a wake-up whose instant reads.
    #[test]
    fn the_earlier_count_and_wake_up_read_as_the_prototype_reads_them() {
        let (s, b, w) = (&words().status, &words().budget, &words().wake);
        let status = json!({
            s.unknown_budget.as_str(): {b.rounds.as_str(): 3, b.since.as_str(): "2026-10-22T08:00:00Z", b.alarm.as_str(): false, b.reason.as_str(): "429"},
            s.wake.as_str(): {w.t.as_str(): "2026-10-22T09:00:00Z", w.ok.as_str(): false, w.fake.as_str(): true, w.error.as_str(): "fake"},
        });
        let earlier = Earlier::of(status.as_object().unwrap());
        assert_eq!(earlier.unknown, UnknownBudget { rounds: 3, since: Some("2026-10-22T08:00:00Z".to_owned()), reason: Some("429".to_owned()) });
        let wake = earlier.wake.unwrap();
        assert_eq!((wake.at.to_rfc3339(), wake.outcome.fake, wake.outcome.error.as_deref()), ("2026-10-22T09:00:00+00:00".to_owned(), true, Some("fake")));
        for count in [json!({b.rounds.as_str(): true}), json!({b.rounds.as_str(): 0}), json!({b.rounds.as_str(): 2.0}), json!("x")] {
            let earlier = Earlier::of(json!({s.unknown_budget.as_str(): count, s.wake.as_str(): {w.t.as_str(): "never"}}).as_object().unwrap());
            assert_eq!(earlier, Earlier::default(), "{count}");
        }
    }
}
