//! The scheduled operations (F133, F148-F153, F256 of the dossier's inventory; #133 and #209 of the dossier): the
//! prototype's operations of a tick (F152, F153), as a hook before the precheck of every tick.
//!
//! - **Once a day, at the first tick after the hour**, on the days the configuration names, by the machine's local
//!   time. A tick that comes late the same day (after a suspension) still runs it; the days missed before today
//!   are recorded as skipped, once, at most `lookback_days` back, and never run. The first time there is no past.
//! - **What is done is in the state, before the effect.** The cursor `operazioni` of the reserved project says
//!   which day each operation ran and how; it is written with the outcome in progress before the chat opens, so a
//!   launch that falls halfway is not repeated. Every read and write is the lock holder's (#582): one transaction
//!   checks the lock, then reads or writes.
//! - **D1** (ADR 0001 §7): the chat is a launch point, so the tick's guard renews the lock right before the entry
//!   is marked in progress and the chat opens, outside the chat's own error handling, and again after it.
//! - **A chat waits for its owner** on the surface the format names, never on one that falls back to a session
//!   with nobody in front. Without that surface it stays to open: the state file lists it for the brief, and the
//!   next ticks of the day try again. A failure after the effect is not repeated: the chat may exist.
//! - **One chat of the dailies waiting** (#209): while the last one (scheduled, or started by hand with
//!   `dailies start`, F274) has no closing file, today's is skipped, and a start by hand of today counts as
//!   today's.
//! - **With a session suspended or resuming** nothing opens and the day is not marked: a later tick of the same
//!   day runs it.
//!
//! The chronicler's operation is checked as the prototype checks it, and left to #493: this hook neither runs it
//! nor touches its entry. A failure here does not stop the tick (it is recorded in the tick's line); a lost lock
//! does.

mod config;
mod words;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Datelike, Days, NaiveDate, NaiveDateTime, Timelike};
use regia_agents::{LaunchError, LaunchRequest, LaunchSurface, ServiceId, SurfaceKind};
use regia_exec::{Effort, ModelId};
use regia_store::{CursorName, Fenced, Holder, ProjectId, Store, WallTime};
use serde_json::{Map, Value, json};

pub use config::{Action, Operation, load, normalize, parse_days};

use crate::engine::words::format as engine_words;
use crate::engine::{BeforePrecheck, LockLost, StateError, StepReceipt, TickContext, TickError, iso};
use words::format as words;

/// Where the configuration comes from: a file, read at every tick; none, so no operation runs; or a document
/// already read.
#[derive(Debug, Clone)]
pub enum Source {
    File(PathBuf),
    None,
    Document(Value),
}

impl Source {
    /// The configuration of the regia's home `home`, when there is one: the prototype's file there.
    #[must_use]
    pub fn of_home(home: Option<&Path>) -> Source {
        home.map_or(Source::None, |home| Source::File(home.join(&words().config)))
    }

    fn operations(&self) -> (Vec<Operation>, Vec<String>) {
        match self {
            Source::File(path) => load(path),
            Source::None => (Vec::new(), Vec::new()),
            Source::Document(document) => normalize(document),
        }
    }
}

/// The scheduled operations of a tick's line that did something, as name and action: the ones already done today are
/// left out, as the prototype's summary of a tick leaves them out.
#[must_use]
pub fn line_operations(record: &Value) -> Vec<(String, String)> {
    let v = &words().voices;
    record
        .get(&v.operations)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|voice| Some((voice.get(&v.name)?.as_str()?.to_owned(), voice.get(&v.action)?.as_str()?.to_owned())))
        .filter(|(_, action)| *action != v.actions.already_done)
        .collect()
}

/// The surface a scheduled chat opens on, by the format: the one where its owner sees it.
#[must_use]
pub fn chat_surface_kind() -> Option<SurfaceKind> {
    SurfaceKind::parse(&words().chat_surface)
}

/// Where a scheduled chat opens.
#[derive(Clone)]
pub enum ChatSurface {
    /// No chat opens, as with the prototype's `cli` launcher: it stays to open, with the format's reason.
    None,
    /// On this surface: the one of [`chat_surface_kind`], or `Dry`, which starts nothing.
    Open(Arc<dyn LaunchSurface + Send + Sync>),
}

/// Why a chat did not open.
enum ChatError {
    /// Nothing happened: it stays to open, and a later tick of the day tries again.
    NotAvailable(String),
    /// Something may have happened, or the request did not hold: not repeated.
    Failed(String),
}

/// Why the operations of a tick stopped.
enum Stop {
    /// The tick stops: the lock is no longer this controller's.
    LockLost(LockLost),
    /// The operations stop, and the tick goes on: the error is in its line.
    Failed(String),
}

impl From<StateError> for Stop {
    fn from(error: StateError) -> Stop {
        match error {
            StateError::LockLost(lost) => Stop::LockLost(lost),
            StateError::Failed(error) => Stop::Failed(error),
        }
    }
}

impl From<TickError> for Stop {
    fn from(error: TickError) -> Stop {
        match error {
            TickError::LockLost(lost) => Stop::LockLost(lost),
            TickError::Failed(error) => Stop::Failed(error),
        }
    }
}

/// The two cursors of the operations, as the holder of the controller's lock reads and writes them.
pub trait ScheduleState: Send + Sync {
    /// The entries of the operations, by name: the objects of the cursor `operazioni`.
    ///
    /// # Errors
    ///
    /// The lock is lost, or the state cannot be read.
    fn operations(&self) -> Result<Map<String, Value>, StateError>;

    /// The last start of the dailies by hand, as `dailies start` (F274) writes it; empty without one.
    ///
    /// # Errors
    ///
    /// The lock is lost, or the state cannot be read.
    fn started_by_hand(&self) -> Result<Map<String, Value>, StateError>;

    /// Writes the entries, over the stored ones of the same names; the others stay as they are stored.
    ///
    /// # Errors
    ///
    /// The lock is lost, or the state cannot be written.
    fn write(&self, operations: &Map<String, Value>) -> Result<(), StateError>;
}

/// The cursors in the state database: each read and write is one transaction that first checks the controller's
/// lock is still `holder`'s (#582).
#[derive(Debug, Clone)]
pub struct StoreSchedule {
    store: Store,
    holder: Holder,
}

impl StoreSchedule {
    #[must_use]
    pub fn new(store: Store, holder: Holder) -> StoreSchedule {
        StoreSchedule { store, holder }
    }

    fn names(name: &str) -> Result<(ProjectId, CursorName), StateError> {
        let failed = |error: String| StateError::Failed(error);
        let lock = ProjectId::new(engine_words().reserved_project.clone()).map_err(|error| failed(error.to_string()))?;
        let name = CursorName::new(name).map_err(|error| failed(error.to_string()))?;
        Ok((lock, name))
    }

    fn read(&self, name: &str) -> Result<Value, StateError> {
        let (lock, name) = Self::names(name)?;
        fenced(self.store.as_holder(&lock, &self.holder).cursor(&lock, &name))
    }
}

/// What became of an operation as the lock's holder.
fn fenced<T>(outcome: Result<Fenced<T>, regia_store::StoreError>) -> Result<T, StateError> {
    match outcome {
        Ok(Fenced::Done(value)) => Ok(value),
        Ok(Fenced::LockLost { holder }) => {
            let now = holder.map_or_else(|| "nobody holds it".to_owned(), |holder| format!("{} holds it", holder.as_str().unwrap_or("?")));
            Err(StateError::LockLost(LockLost(format!("the controller's lock is no longer ours ({now}): the operations are left alone"))))
        }
        Err(error) => Err(StateError::Failed(error.to_string())),
    }
}

/// The objects of a cursor's map.
fn objects(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .map(|entries| entries.iter().filter(|(_, entry)| entry.is_object()).map(|(name, entry)| (name.clone(), entry.clone())).collect())
        .unwrap_or_default()
}

impl ScheduleState for StoreSchedule {
    fn operations(&self) -> Result<Map<String, Value>, StateError> {
        let c = &words().cursor;
        Ok(objects(self.read(&c.operations)?.get(&c.ops)))
    }

    fn started_by_hand(&self) -> Result<Map<String, Value>, StateError> {
        Ok(self.read(&words().cursor.started)?.as_object().cloned().unwrap_or_default())
    }

    fn write(&self, operations: &Map<String, Value>) -> Result<(), StateError> {
        let c = &words().cursor;
        let (lock, name) = Self::names(&c.operations)?;
        let written = self.store.as_holder(&lock, &self.holder).write_cursor(&lock, &name, |stored| {
            let mut merged = stored.get(&c.ops).and_then(Value::as_object).cloned().unwrap_or_default();
            merged.extend(operations.iter().map(|(name, entry)| (name.clone(), entry.clone())));
            let mut document = Map::new();
            document.insert(c.v.clone(), json!(words().version));
            document.insert(c.ops.clone(), Value::Object(merged));
            (Some(Value::Object(document)), ())
        });
        fenced(written)
    }
}

/// An instant in the machine's local time, as the prototype counts the day: `datetime.fromtimestamp(t)`.
#[must_use]
pub fn system_local(secs: f64) -> Option<NaiveDateTime> {
    let at = WallTime::from_unix_secs_f64(secs)?;
    let utc = chrono::DateTime::from_timestamp_micros(at.unix_micros())?;
    Some(utc.with_timezone(&chrono::Local).naive_local())
}

/// The agent of a chat when its operation names none: the step's, by the `replanning` role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAgent {
    pub service: ServiceId,
    pub model: Option<ModelId>,
    pub effort: Option<Effort>,
}

impl Default for ChatAgent {
    fn default() -> ChatAgent {
        ChatAgent { service: ServiceId::Claude, model: None, effort: None }
    }
}

type Local = Arc<dyn Fn(f64) -> Option<NaiveDateTime> + Send + Sync>;

/// The hook of the scheduled operations: [`Operations::new`], then the tick runs it before its precheck.
pub struct Operations {
    source: Source,
    state: Arc<dyn ScheduleState>,
    chat: ChatSurface,
    local: Local,
    home: Option<PathBuf>,
    closings: Option<PathBuf>,
    agent: ChatAgent,
    to_open: Vec<Value>,
    status: Map<String, Value>,
}

/// The UTC minute of an instant, as the closing files of the dailies are named: `AAAA-MM-GG-HHMM`.
#[must_use]
pub fn stamp_utc(text: &str) -> Option<String> {
    let at = WallTime::parse_iso(text)?;
    let utc = chrono::DateTime::from_timestamp_micros(at.unix_micros())?;
    Some(utc.format("%Y-%m-%d-%H%M").to_string())
}

/// A closing file's name: `AAAA-MM-GG-HHMM` and the extension.
fn closing_stamp(name: &str) -> Option<&str> {
    let stamp = name.strip_suffix(&words().closing.extension)?;
    let shape = stamp.len() == 15
        && stamp.bytes().enumerate().all(|(at, byte)| if [4, 7, 10].contains(&at) { byte == b'-' } else { byte.is_ascii_digit() });
    shape.then_some(stamp)
}

/// Whether the chat started at `launched` (an ISO instant) is still open: no closing file in `folder` names its
/// UTC minute or a later one. A missing folder has no closing file; an instant or a folder that does not read
/// counts as closed, so the dailies are never blocked for ever.
#[must_use]
pub fn chat_open(folder: &Path, launched: &str) -> bool {
    let Some(stamp) = stamp_utc(launched) else { return false };
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
        Err(_) => return false,
    };
    !entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        closing_stamp(&name).is_some_and(|closing| closing >= stamp.as_str())
    })
}

/// The instant of the last chat of the dailies, from the operation's entry or from the start by hand: the later
/// of the two that read.
#[must_use]
pub fn last_launch(entry: &Map<String, Value>, started: &Map<String, Value>) -> Option<String> {
    let e = &words().entry;
    let scheduled = entry.get(&e.last_launched).and_then(|last| last.get(&e.t));
    [scheduled, started.get(&e.t)]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|t| WallTime::parse_iso(t).map(|at| (at.unix_secs_f64(), t)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t.to_owned())
}

fn text_of(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

fn cut(text: &str) -> String {
    text.chars().take(words().reason_chars).collect()
}

/// A voice of the tick's line: the operation's name, the action, then its own keys.
fn voice(name: &str, action: &str, extra: impl IntoIterator<Item = (String, Value)>) -> Map<String, Value> {
    let v = &words().voices;
    let mut voice = Map::new();
    voice.insert(v.name.clone(), json!(name));
    voice.insert(v.action.clone(), json!(action));
    voice.extend(extra);
    voice
}

impl Operations {
    /// The operations of `source`, with their cursors in `state`, the chats on `chat`: the machine's local time, the
    /// regia's home as the default folder of a chat (`REGIA_HOME`), the closing files in each chat's folder.
    #[must_use]
    pub fn new(source: Source, state: Arc<dyn ScheduleState>, chat: ChatSurface) -> Operations {
        Operations {
            source,
            state,
            chat,
            local: Arc::new(system_local),
            home: None,
            closings: None,
            agent: ChatAgent::default(),
            to_open: Vec::new(),
            status: Map::new(),
        }
    }

    /// The local time of an instant, in place of the machine's.
    #[must_use]
    pub fn with_local(mut self, local: impl Fn(f64) -> Option<NaiveDateTime> + Send + Sync + 'static) -> Operations {
        self.local = Arc::new(local);
        self
    }

    /// The folder of a chat whose operation names none: the regia's home.
    #[must_use]
    pub fn with_home(mut self, home: Option<PathBuf>) -> Operations {
        self.home = home;
        self
    }

    /// The closing files of the dailies in `folder`, in place of the chat's folder.
    #[must_use]
    pub fn with_closings(mut self, folder: PathBuf) -> Operations {
        self.closings = Some(folder);
        self
    }

    #[must_use]
    pub fn with_agent(mut self, agent: ChatAgent) -> Operations {
        self.agent = agent;
        self
    }

    /// The chats of today to open by hand, after the last tick, as the state file lists them.
    #[must_use]
    pub fn to_open(&self) -> &[Value] {
        &self.to_open
    }

    /// The active operations' entries after the last tick, as the state file has them.
    #[must_use]
    pub fn operations_status(&self) -> &Map<String, Value> {
        &self.status
    }

    fn folder_of(&self, operation: &Operation) -> Option<String> {
        operation.folder.clone().or_else(|| self.home.as_ref().map(|home| home.display().to_string()))
    }

    /// The chat of `operation` in `folder`, on the chat's surface.
    fn open_chat(&self, operation: &Operation, folder: Option<&str>) -> Result<StepReceipt, ChatError> {
        let surface = match &self.chat {
            ChatSurface::None => return Err(ChatError::NotAvailable(words().reasons.no_chat.clone())),
            ChatSurface::Open(surface) => surface,
        };
        let folder = folder.ok_or_else(|| ChatError::Failed("no folder: the operation names none, and there is no regia home".to_owned()))?;
        let failed = |error: String| ChatError::Failed(error);
        let model = match &operation.model {
            Some(model) => Some(ModelId::parse(model).map_err(|error| failed(error.to_string()))?),
            None => self.agent.model.clone(),
        };
        let effort = match &operation.effort {
            Some(effort) => Some(Effort::parse(effort).map_err(|error| failed(error.to_string()))?),
            None => self.agent.effort,
        };
        let prompt = operation.prompt.as_deref().unwrap_or_default();
        let request = LaunchRequest::new(self.agent.service, &operation.name, Path::new(folder), prompt)
            .map_err(|error| failed(error.to_string()))?
            .model(model)
            .effort(effort);
        match surface.launch(&request) {
            Ok(receipt) => Ok(StepReceipt::of(&receipt)),
            Err(LaunchError::BeforeEffect(error)) => Err(ChatError::NotAvailable(error.to_string())),
            Err(LaunchError::AfterEffect(error)) => Err(ChatError::Failed(error)),
        }
    }

    /// The prototype's operations of a tick (F153).
    #[allow(clippy::too_many_lines)]
    fn run(&mut self, record: &mut Map<String, Value>, tick: &TickContext<'_>) -> Result<(), Stop> {
        let (w, e, o, v) = (words(), &words().entry, &words().outcomes, &words().voices);
        let actions = &v.actions;
        let (operations, errors) = self.source.operations();
        if !errors.is_empty() {
            record.insert(v.config_errors.clone(), json!(errors));
        }
        // The chronicler's operation is #493's.
        let active: Vec<Operation> = operations.into_iter().filter(|operation| operation.active && operation.action == Action::Chat).collect();
        if active.is_empty() {
            self.to_open.clear();
            self.status.clear();
            return Ok(());
        }
        let now = tick.now();
        let local = (self.local)(now).ok_or_else(|| Stop::Failed(format!("no local time for the instant {now}")))?;
        let today = local.date();
        let day_text = |day: NaiveDate| day.format("%Y-%m-%d").to_string();
        let today_text = day_text(today);
        let mut state = self.state.operations()?;
        let started = self.state.started_by_hand()?;
        let blocked = !tick.suspended_projects().is_empty();
        let mut voices = Vec::new();
        let mut changed = false;
        for operation in &active {
            let name = operation.name.as_str();
            let dailies = name == w.dailies;
            let mut entry = match state.get(name) {
                Some(Value::Object(entry)) => entry.clone(),
                _ => {
                    changed = true;
                    Map::from_iter([(e.counted.clone(), json!(today_text))])
                }
            };
            // The days missed since the last day counted, at most the lookback: those of the schedule that were not
            // done, by the schedule or by hand, are skipped.
            let counted = text_of(entry.get(&e.counted)).and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()).unwrap_or(today);
            let lookback = Days::new(u64::try_from(w.lookback_days).unwrap_or(0));
            let first = counted.max(today.checked_sub_days(lookback).unwrap_or(today));
            let done = [text_of(entry.get(&e.day)).map(str::to_owned), if dailies { text_of(started.get(&e.day)).map(str::to_owned) } else { None }];
            let missed: Vec<String> = first
                .iter_days()
                .take_while(|day| *day < today)
                .filter(|day| operation.days.contains(&day.weekday().num_days_from_monday()))
                .map(day_text)
                .filter(|day| !done.iter().flatten().any(|done| done == day))
                .collect();
            if !missed.is_empty() {
                let mut skipped = entry.get(&e.skipped).and_then(Value::as_array).cloned().unwrap_or_default();
                skipped.extend(missed.iter().map(|day| json!(day)));
                let keep = skipped.len().saturating_sub(w.skipped_kept);
                entry.insert(e.skipped.clone(), Value::Array(skipped.split_off(keep)));
                voices.push(voice(name, &actions.skipped, [(v.days.clone(), json!(missed))]));
                changed = true;
            }
            if counted < today {
                entry.insert(e.counted.clone(), json!(today_text));
                changed = true;
            }
            let due = operation.days.contains(&today.weekday().num_days_from_monday()) && (local.hour(), local.minute()) >= operation.hour;
            if !due {
                state.insert(name.to_owned(), Value::Object(entry));
                continue;
            }
            let is_today = |entry: &Map<String, Value>| text_of(entry.get(&e.day)) == Some(today_text.as_str());
            let outcome = |entry: &Map<String, Value>| text_of(entry.get(&e.outcome)).map(str::to_owned);
            if dailies && text_of(started.get(&e.day)) == Some(today_text.as_str()) {
                let ran = outcome(&entry).is_some_and(|outcome| outcome == o.launched || outcome == o.started);
                if !(is_today(&entry) && ran) {
                    // Started by hand today: the scheduled one does not start again, and today's to open or error closes.
                    let at = started.get(&e.t).cloned().unwrap_or(Value::Null);
                    entry.insert(e.day.clone(), json!(today_text));
                    entry.insert(e.outcome.clone(), json!(o.started));
                    entry.insert(e.t.clone(), at.clone());
                    entry.remove(&e.reason);
                    voices.push(voice(name, &actions.already_started, [(v.t.clone(), at)]));
                    state.insert(name.to_owned(), Value::Object(entry));
                    self.state.write(&state)?;
                    changed = false;
                    continue;
                }
            }
            if is_today(&entry) && outcome(&entry).as_deref() != Some(o.to_open.as_str()) {
                voices.push(voice(name, &actions.already_done, [(v.outcome.clone(), entry.get(&e.outcome).cloned().unwrap_or(Value::Null))]));
                state.insert(name.to_owned(), Value::Object(entry));
                continue;
            }
            if blocked {
                voices.push(voice(name, &actions.postponed_suspended, []));
                state.insert(name.to_owned(), Value::Object(entry));
                continue;
            }
            let folder = self.folder_of(operation);
            let closings = self.closings.clone().or_else(|| folder.as_ref().map(|folder| Path::new(folder).join(&w.closing.folder)));
            let last = if dailies { last_launch(&entry, &started) } else { None };
            if let (Some(last), Some(closings)) = (&last, &closings)
                && chat_open(closings, last)
            {
                // One chat waiting (#209): the last one has no closing file. The day is spent: closed later, the new
                // one does not start by itself; `dailies start` starts it.
                let reason = w.reasons.previous_open.replace("{t}", last).replace("{folder}", &w.closing.folder);
                entry.insert(e.day.clone(), json!(today_text));
                entry.insert(e.outcome.clone(), json!(o.previous_open));
                entry.insert(e.t.clone(), json!(iso(now)));
                entry.insert(e.open_since.clone(), json!(last));
                entry.insert(e.reason.clone(), json!(cut(&reason)));
                entry.remove(&e.attempts);
                voices.push(voice(name, &actions.previous_open, [(v.open_since.clone(), json!(last))]));
                state.insert(name.to_owned(), Value::Object(entry));
                self.state.write(&state)?;
                changed = false;
                continue;
            }
            // D1, before the entry in progress and the effect, outside the chat's error handling: a controller that lost its
            // lock does not spend the day for the new one.
            tick.guard()?;
            let attempts = if is_today(&entry) { entry.get(&e.attempts).and_then(Value::as_i64).unwrap_or(0) + 1 } else { 1 };
            entry.insert(e.day.clone(), json!(today_text));
            entry.insert(e.outcome.clone(), json!(o.in_progress));
            entry.insert(e.t.clone(), json!(iso(now)));
            entry.insert(e.attempts.clone(), json!(attempts));
            let previous = entry.get(&e.last_launched).cloned();
            if dailies {
                // The launch under way counts as an open chat too: a start by hand meanwhile opens no second one, and
                // an error after the effect does not open it again tomorrow.
                entry.insert(e.last_launched.clone(), json!({e.t.as_str(): iso(now), e.origin.as_str(): e.scheduled}));
            }
            state.insert(name.to_owned(), Value::Object(entry.clone()));
            // Before the chat: what cannot be written does not open.
            self.state.write(&state)?;
            changed = false;
            let chat = self.open_chat(operation, folder.as_deref());
            let action = match chat {
                Err(ChatError::NotAvailable(reason)) => {
                    let reason = cut(&reason);
                    entry.insert(e.outcome.clone(), json!(o.to_open));
                    entry.insert(e.reason.clone(), json!(reason));
                    if dailies {
                        // Nothing happened: the attempt leaves no open chat.
                        entry.remove(&e.last_launched);
                        if let Some(previous) = previous.filter(|previous| previous.as_object().is_some_and(|fields| !fields.is_empty())) {
                            entry.insert(e.last_launched.clone(), previous);
                        }
                    }
                    voice(name, &actions.to_open, [(v.reason.clone(), json!(reason))])
                }
                Err(ChatError::Failed(reason)) => {
                    let reason = cut(&reason);
                    entry.insert(e.outcome.clone(), json!(o.error));
                    entry.insert(e.reason.clone(), json!(reason));
                    voice(name, &actions.launch_error, [(v.reason.clone(), json!(reason))])
                }
                Ok(receipt) => {
                    entry.insert(e.outcome.clone(), json!(o.launched));
                    entry.remove(&e.reason);
                    if dailies && let Some(Value::Object(last)) = entry.get_mut(&e.last_launched) {
                        last.insert(e.session.clone(), json!(receipt.session));
                    }
                    voice(
                        name,
                        &actions.launched,
                        [(v.adapter.clone(), json!(receipt.adapter)), (v.session.clone(), json!(receipt.session)), (v.log.clone(), json!(receipt.log))],
                    )
                }
            };
            voices.push(action);
            state.insert(name.to_owned(), Value::Object(entry));
            self.state.write(&state)?;
            // A chat can take minutes to open.
            tick.guard()?;
        }
        if changed {
            self.state.write(&state)?;
        }
        if !voices.is_empty() {
            record.insert(v.operations.clone(), Value::Array(voices.into_iter().map(Value::Object).collect()));
        }
        self.remember(&active, &state, &today_text);
        Ok(())
    }

    /// What the state file says of the active operations after the tick: today's chats to open by hand, and each
    /// one's entry.
    fn remember(&mut self, active: &[Operation], state: &Map<String, Value>, today: &str) {
        let (e, o, t) = (&words().entry, &words().outcomes, &words().to_open);
        let entry = |name: &str| state.get(name).and_then(Value::as_object).cloned().unwrap_or_default();
        let field = |entry: &Map<String, Value>, key: &str| entry.get(key).cloned().unwrap_or(Value::Null);
        self.to_open = active
            .iter()
            .filter_map(|operation| {
                let current = entry(&operation.name);
                let open = text_of(current.get(&e.day)) == Some(today)
                    && text_of(current.get(&e.outcome)).is_some_and(|outcome| outcome == o.to_open || outcome == o.error);
                open.then(|| {
                    json!({
                        t.name.as_str(): operation.name,
                        t.day.as_str(): field(&current, &e.day),
                        t.outcome.as_str(): field(&current, &e.outcome),
                        t.reason.as_str(): field(&current, &e.reason),
                        t.prompt.as_str(): operation.prompt,
                        t.folder.as_str(): self.folder_of(operation),
                        t.attempts.as_str(): field(&current, &e.attempts),
                    })
                })
            })
            .collect();
        self.status = active
            .iter()
            .map(|operation| {
                let current = entry(&operation.name);
                let keys = [&e.day, &e.outcome, &e.skipped, &e.open_since, &e.last_launched];
                let fields: Map<String, Value> = keys.iter().map(|key| ((*key).clone(), field(&current, key))).collect();
                (operation.name.clone(), Value::Object(fields))
            })
            .collect();
    }
}

impl BeforePrecheck for Operations {
    /// The prototype's hook of the operations (F152): a failure stays in the tick's line and the tick goes on; a lost lock ends it.
    /// The operations never end the tick.
    fn before(&mut self, record: &mut Map<String, Value>, tick: &TickContext<'_>) -> Result<bool, TickError> {
        match self.run(record, tick) {
            Ok(()) => {}
            Err(Stop::LockLost(lost)) => return Err(TickError::LockLost(lost)),
            Err(Stop::Failed(error)) => {
                record.insert(words().voices.error.clone(), json!(cut(&error)));
            }
        }
        Ok(false)
    }

    fn status(&self, status: &mut Map<String, Value>) {
        let s = &engine_words().status;
        status.insert(s.to_open.clone(), Value::Array(self.to_open.clone()));
        status.insert(s.operations.clone(), Value::Object(self.status.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closing_file_is_named_by_its_utc_minute() {
        assert_eq!(closing_stamp("2026-10-22-0905.md"), Some("2026-10-22-0905"));
        for name in ["2026-10-22-905.md", "2026-10-22T0905.md", "2026-10-22-0905.txt", "x2026-10-22-0905.md"] {
            assert_eq!(closing_stamp(name), None, "{name}");
        }
        assert_eq!(stamp_utc("2026-10-22T09:05:59Z").as_deref(), Some("2026-10-22-0905"));
        assert_eq!(stamp_utc("2026-10-22T11:05:00+02:00").as_deref(), Some("2026-10-22-0905"));
        assert_eq!(stamp_utc("never"), None);
    }
}
