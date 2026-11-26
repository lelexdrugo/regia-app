//! The resume of the sessions a usage limit stopped (F28 of the dossier's plan; F121, F151, F158, F159, F161–F163 and
//! F165–F170 of its inventory; #132 and ADR 0010 of the dossier): the prototype's rule of a tick before its precheck, as
//! the first hook of every tick.
//!
//! - **The registry** is the steps' (#468). Each record the tick knows is read again from its transcript, and takes a
//!   state: launched, suspended, resuming or finished. The workers a step on the app started, and the sessions the owner
//!   allowed (the opt-in, none by default), enter it as finished, and turn suspended only once a limit stopped them.
//! - **One resume a tick.** A suspended session whose hour has passed is resumed, the oldest first, and nothing else
//!   starts in that tick. With the hour still ahead nothing starts at all. A session the controller cannot resume safely
//!   goes to the file of the sessions to resume by hand, which the brief reads; while one is there nothing starts,
//!   unless the run says that it holds only its project. A suspended session holds its project past its lock.
//! - **Never one of the app's sessions from outside**: two writers on one transcript break its view. A session is the
//!   app's by its record, by its transcript's entrypoint, or by the app's own index; an app that does not answer is a
//!   "maybe", and nothing is resumed (fail-closed).
//! - **A release** the owner left (a file of the releases' folder, as `tools/sospese.py rilascia` writes it) closes a
//!   suspended record for its limit; a new limit of the same session opens it again.
//! - **D1** (ADR 0001 §7): the resume is a launch point, so the tick's guard renews the lock right before it, outside
//!   the resume's own error handling.
//!
//! Not ported: the message that wakes a session of the app by itself (#191, F285–F290 of the inventory, still to be
//! decided). Such a session goes to the file of the sessions to resume by hand, with the reason the guard gives, as the
//! prototype's controller does when it has no terminal to send from.

mod words;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use regia_agents::suspension::{self, Analysis, LocalOffset, Reading, Source, State, Zone};
use regia_agents::{
    AgentService, CHAT_ENTRYPOINT, ChatHost, Cli, Env, ResumeRequest, ServiceId, SurfaceKind, chat_wake_command, coordinated_workers, session_by_marker,
    worker_wake_command,
};
use regia_exec::{Effort, ModelId};
use serde_json::{Map, Value, json};

use crate::engine::words::format as engine_words;
use crate::engine::{BeforePrecheck, TickContext, TickError, iso};
use crate::resident::{folders_or_none, record_alive};
use words::format as words;

/// What a resume started, as the record keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResumeReceipt {
    pub pid: Option<u32>,
    pub log: Option<String>,
    /// The session the agent goes on with: a new one after a fork.
    pub session: Option<String>,
    pub transcript: Option<String>,
}

/// How a suspended session is resumed: on the agents' Cli surface, on nothing in a run that starts nothing, or as a
/// test has it.
pub trait Resumer: Send + Sync {
    /// Resumes the session of `record`, a record of the steps registry.
    ///
    /// # Errors
    ///
    /// The resume did not start, or its process ended with an error: the text that says why.
    fn resume(&self, record: &Map<String, Value>) -> Result<ResumeReceipt, String>;
}

/// The resume on the agents' Cli surface (`claude -p --resume`, `codex exec resume`), from what the record says of its
/// session: its agent, project, folder, id, model and effort.
pub struct CliResumer {
    cli: Cli,
}

impl CliResumer {
    #[must_use]
    pub fn new(cli: Cli) -> CliResumer {
        CliResumer { cli }
    }
}

impl Resumer for CliResumer {
    fn resume(&self, record: &Map<String, Value>) -> Result<ResumeReceipt, String> {
        let w = &engine_words().registry.record;
        let agent = text(record, &w.agent).unwrap_or(ServiceId::Claude.id());
        let service = ServiceId::parse(agent).ok_or_else(|| format!("agent not supported: {agent}"))?;
        let session = text(record, &w.resume).or_else(|| text(record, &w.session)).ok_or_else(|| "no session to resume".to_owned())?;
        let model = text(record, &w.model).map(ModelId::parse).transpose().map_err(|error| error.to_string())?;
        let effort = text(record, &w.effort).map(Effort::parse).transpose().map_err(|error| error.to_string())?;
        let folder = text(record, &w.folder).unwrap_or_default();
        let request = ResumeRequest::new(service, text(record, &w.project).unwrap_or_default(), Path::new(folder), session)
            .map_err(|error| error.to_string())?
            .model(model)
            .effort(effort);
        let resumed = self.cli.resume(&request).map_err(|error| error.to_string())?;
        Ok(ResumeReceipt { pid: Some(resumed.pid), log: Some(resumed.log), session: Some(resumed.session), transcript: resumed.transcript })
    }
}

/// A run that starts nothing (the shadow, the fake launcher): the resume is recorded, and no process starts.
#[derive(Debug, Clone, Copy, Default)]
pub struct DryResumer;

impl Resumer for DryResumer {
    fn resume(&self, _record: &Map<String, Value>) -> Result<ResumeReceipt, String> {
        Ok(ResumeReceipt::default())
    }
}

/// How the resume runs, by the options of a run.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// The file of the opt-in; without it, the one beside the state database.
    pub opt_in: Option<PathBuf>,
    /// A session to resume by hand holds only its own project, not the whole tick.
    pub only_project: bool,
}

/// The sessions and the folders the owner allowed to resume besides the regia's steps. None by default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptIn {
    pub sessions: Vec<String>,
    pub folders: Vec<String>,
}

/// The prototype's `leggi_optin`: a file that is missing, empty or not an object allows nothing.
#[must_use]
pub fn read_opt_in(path: &Path) -> OptIn {
    let o = &words().opt_in;
    let Some(Value::Object(document)) = fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) else {
        return OptIn::default();
    };
    let strings = |key: &str| -> Vec<String> {
        document.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
    };
    OptIn { sessions: strings(&o.sessions), folders: strings(&o.folders) }
}

/// A release's file name: the session's characters outside `[A-Za-z0-9_-]` as `_`, at most 100, `x` when none.
fn release_name(session: &str) -> String {
    let name: String = session.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).take(100).collect();
    if name.is_empty() { "x".to_owned() } else { name }
}

/// The prototype's `rilascia`: the owner closed a suspended session without resuming it. One file per session, so it
/// never races the controller, which applies it at the next tick and removes it. `session` is the id of the session
/// (the agent's or the app's), the key of its record, or the format's word for every one. The file's path.
///
/// # Errors
///
/// The folder or the file cannot be written.
pub fn release(state: &Path, session: &str, at: f64) -> std::io::Result<PathBuf> {
    let (f, r) = (&words().files, &words().release);
    let folder = state.join(&f.releases);
    fs::create_dir_all(&folder)?;
    let path = folder.join(format!("{}.json", release_name(session)));
    let temporary = path.with_file_name(format!("{}.json.tmp", release_name(session)));
    let mut document = Map::new();
    document.insert(r.session.clone(), json!(session));
    document.insert(r.t.clone(), json!(iso(at)));
    fs::write(&temporary, Value::Object(document).to_string())?;
    fs::rename(&temporary, &path)?;
    Ok(path)
}

/// The prototype's `leggi_rilasci`: the releases waiting, each with its file, by name; a file that does not read, or
/// names no session, is skipped.
#[must_use]
pub fn releases(state: &Path) -> Vec<(PathBuf, Map<String, Value>)> {
    let (f, r) = (&words().files, &words().release);
    let mut files: Vec<PathBuf> = fs::read_dir(state.join(&f.releases))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let document = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())?;
            let document = document.as_object()?.clone();
            document.get(&r.session).and_then(Value::as_str)?;
            Some((path, document))
        })
        .collect()
}

/// A text field of a record; an empty one counts as missing, as Python's truth has it.
fn text<'a>(record: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str).filter(|text| !text.is_empty())
}

fn number(record: &Map<String, Value>, key: &str) -> Option<f64> {
    record.get(key).and_then(Value::as_f64)
}

fn field(record: &Map<String, Value>, key: &str) -> Value {
    record.get(key).cloned().unwrap_or(Value::Null)
}

/// Python's truth of a field.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
    }
}

fn cut(text: &str) -> String {
    text.chars().take(words().error_chars).collect()
}

fn iso_of(at: Option<f64>) -> Value {
    at.map_or(Value::Null, |at| json!(iso(at)))
}

/// The prototype's key of a step: the project, then the session to resume, else the session, else the launch's second.
fn step_key(record: &Map<String, Value>) -> String {
    let w = &engine_words().registry.record;
    let project = text(record, &w.project).unwrap_or_default();
    #[allow(clippy::cast_possible_truncation)]
    let id = text(record, &w.resume).or_else(|| text(record, &w.session)).map_or_else(|| format!("t{}", number(record, &w.started_t).unwrap_or(0.0).trunc() as i64), str::to_owned);
    format!("{project}/{id}")
}

/// A record's agent as the prototype reads it with Claude for a missing one: a value that is not text is no agent.
fn agent_or_claude(record: &Map<String, Value>) -> Option<&str> {
    match record.get(&engine_words().registry.record.agent) {
        None => Some(ServiceId::Claude.id()),
        Some(Value::String(agent)) => Some(agent.as_str()),
        Some(_) => None,
    }
}

/// The service whose transcript a record has: Codex by its word, Claude otherwise.
fn service_of(record: &Map<String, Value>) -> ServiceId {
    match text(record, &engine_words().registry.record.agent) {
        Some(agent) if agent == ServiceId::Codex.id() => ServiceId::Codex,
        _ => ServiceId::Claude,
    }
}

/// Two folders are the same as Python's `Path` compares them: by their parts, without case on Windows.
fn same_folder(a: &str, b: &str) -> bool {
    let parts = |path: &str| -> Vec<String> {
        Path::new(path)
            .components()
            .map(|part| {
                let part = part.as_os_str().to_string_lossy().into_owned();
                if cfg!(windows) { part.to_lowercase() } else { part }
            })
            .collect()
    };
    parts(a) == parts(b)
}

/// The project of a working folder: the one whose folder it is, else its last part in lowercase.
fn project_of(folders: &std::collections::BTreeMap<String, Option<String>>, cwd: &str) -> String {
    folders
        .iter()
        .find(|(_, folder)| folder.as_deref().is_some_and(|folder| !folder.is_empty() && same_folder(folder, cwd)))
        .map(|(project, _)| project.clone())
        .unwrap_or_else(|| Path::new(cwd).file_name().map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default())
}

/// The record's `analisi`: what its transcript said, in the format's words.
fn analysis_value(analysis: &Analysis) -> Value {
    let (a, w) = (&words().analysis, &words().analysis.states);
    let state = match analysis.state {
        State::Suspended => &w.suspended,
        State::Resuming => &w.resuming,
        State::Active => &w.active,
        State::Empty => &w.empty,
        State::Unreadable => &w.unreadable,
    };
    let s = &a.sources;
    let source = analysis.source.map(|source| match source {
        Source::QuotaLimits => &s.quota_limits,
        Source::ApiErrorParams => &s.api_error_params,
        Source::Text => &s.text,
        Source::TaskError => &s.task_error,
    });
    let zone = analysis.zone.as_ref().map(|zone| match zone {
        Zone::Named(name) => name.clone(),
        Zone::Local => a.zones.local.clone(),
    });
    let mut value = Map::new();
    value.insert(a.state.clone(), json!(state));
    value.insert(a.resets_iso.clone(), iso_of(analysis.resets));
    value.insert(a.source.clone(), json!(source));
    value.insert(a.zone.clone(), json!(zone));
    value.insert(a.estimated.clone(), json!(analysis.estimated));
    value.insert(a.limit_iso.clone(), iso_of(analysis.limit_at));
    value.insert(a.entrypoint.clone(), json!(analysis.entrypoint));
    Value::Object(value)
}

/// The hook of the resume: [`Resume::new`], then the tick runs it before its precheck, first.
pub struct Resume {
    state: PathBuf,
    env: Env,
    settings: Settings,
    resumer: Arc<dyn Resumer>,
    host: Arc<dyn ChatHost>,
    offset: LocalOffset,
    pid: u32,
    opt_in: Option<OptIn>,
    suspended: Vec<Value>,
    to_resume: Vec<Value>,
}

impl Resume {
    /// The resume on the state folder `state`, with the agents' transcripts where `env` says, its sessions resumed by
    /// `resumer`, and the app asked through `host`.
    #[must_use]
    pub fn new(state: impl Into<PathBuf>, env: Env, resumer: Arc<dyn Resumer>, host: Arc<dyn ChatHost>) -> Resume {
        Resume {
            state: state.into(),
            env,
            settings: Settings::default(),
            resumer,
            host,
            offset: suspension::system_offset,
            pid: std::process::id(),
            opt_in: None,
            suspended: Vec::new(),
            to_resume: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_settings(mut self, settings: Settings) -> Resume {
        self.settings = settings;
        self
    }

    /// The machine's offset from UTC, in place of this one's: Codex's hour is local.
    #[must_use]
    pub fn with_offset(mut self, offset: LocalOffset) -> Resume {
        self.offset = offset;
        self
    }

    /// The process the temporary file of the sessions to resume by hand is named by.
    #[must_use]
    pub fn with_pid(mut self, pid: u32) -> Resume {
        self.pid = pid;
        self
    }

    /// An opt-in already read, in place of its file.
    #[must_use]
    pub fn with_opt_in(mut self, opt_in: OptIn) -> Resume {
        self.opt_in = Some(opt_in);
        self
    }

    /// The suspended sessions of the last tick, as the state file lists them.
    #[must_use]
    pub fn suspended(&self) -> &[Value] {
        &self.suspended
    }

    /// The sessions to resume by hand after the last tick, as their file lists them.
    #[must_use]
    pub fn to_resume(&self) -> &[Value] {
        &self.to_resume
    }

    /// F159, the prototype's `_optin`.
    fn opt_in(&self) -> OptIn {
        if let Some(opt_in) = &self.opt_in {
            return opt_in.clone();
        }
        read_opt_in(&self.settings.opt_in.clone().unwrap_or_else(|| self.state.join(&words().files.opt_in)))
    }

    /// The changed records over the registry's, saved; then the registry as saved.
    fn save(&self, records: &mut Map<String, Value>, changed: &BTreeSet<String>, tick: &TickContext<'_>, at: f64) -> Result<(), TickError> {
        let update: Map<String, Value> = changed.iter().filter_map(|key| records.get(key).map(|record| (key.clone(), record.clone()))).collect();
        tick.steps().save_records(update, at)?;
        *records = tick.steps().records();
        Ok(())
    }

    /// F163, the prototype's `_scopri`: the workers the app's steps of the registry started, and the sessions of the
    /// opt-in, as new records, finished.
    fn discover(&self, records: &Map<String, Value>, tick: &TickContext<'_>, t0: f64) -> Vec<(String, Value)> {
        let (e, r) = (engine_words(), words());
        let w = &e.registry.record;
        let mut found = Vec::new();
        for record in records.values().filter_map(Value::as_object) {
            if text(record, &w.origin) != Some(e.registry.origins.controller.as_str()) || text(record, &w.adapter) != Some(e.adapters.own_sessions.as_str()) {
                continue;
            }
            let (Some(coordinator), Some(folder)) = (text(record, &w.surface_session), text(record, &w.folder)) else { continue };
            if agent_or_claude(record) != Some(ServiceId::Claude.id()) {
                continue;
            }
            let skip: Vec<&str> = [text(record, &w.resume), text(record, &w.session)].into_iter().flatten().collect();
            for (session, path) in coordinated_workers(&self.env, folder, coordinator, &skip, number(record, &w.started_t)) {
                let at = suspension::modified(&path).unwrap_or(0.0);
                let mut worker = Map::new();
                worker.insert(w.project.clone(), field(record, &w.project));
                worker.insert(w.folder.clone(), field(record, &w.folder));
                worker.insert(w.origin.clone(), json!(r.origins.worker));
                worker.insert(w.adapter.clone(), json!(e.adapters.own_sessions));
                worker.insert(w.agent.clone(), json!(ServiceId::Claude.id()));
                worker.insert(w.model.clone(), Value::Null);
                worker.insert(w.effort.clone(), Value::Null);
                worker.insert(w.session.clone(), json!(session));
                worker.insert(w.resume.clone(), json!(session));
                worker.insert(w.surface_session.clone(), Value::Null);
                worker.insert(w.transcript.clone(), json!(path.display().to_string()));
                worker.insert(w.pid.clone(), Value::Null);
                worker.insert(w.log.clone(), Value::Null);
                worker.insert(w.marker.clone(), Value::Null);
                worker.insert(w.started_t.clone(), json!(at));
                worker.insert(w.started.clone(), json!(iso(at)));
                worker.insert(w.state.clone(), json!(e.registry.states.finished));
                worker.insert(w.updated_t.clone(), json!(t0));
                worker.insert(w.resumes.clone(), json!([]));
                worker.insert(r.record.parent.clone(), json!(step_key(record)));
                found.push((step_key(&worker), Value::Object(worker)));
            }
        }
        let opt_in = self.opt_in();
        if !opt_in.sessions.is_empty() || !opt_in.folders.is_empty() {
            let folders = folders_or_none(tick.folders());
            for opted in suspension::opted_in(&self.env, &opt_in.sessions, &opt_in.folders, r.opt_in_days, t0) {
                let mut session = Map::new();
                session.insert(w.project.clone(), json!(project_of(&folders, &opted.cwd)));
                session.insert(w.folder.clone(), json!(opted.cwd));
                session.insert(w.origin.clone(), json!(r.origins.opt_in));
                session.insert(w.adapter.clone(), json!(SurfaceKind::Cli.id()));
                session.insert(w.agent.clone(), json!(ServiceId::Claude.id()));
                session.insert(w.session.clone(), json!(opted.session));
                session.insert(w.resume.clone(), json!(opted.session));
                session.insert(w.surface_session.clone(), Value::Null);
                session.insert(w.transcript.clone(), json!(opted.transcript.display().to_string()));
                session.insert(w.pid.clone(), Value::Null);
                session.insert(w.log.clone(), Value::Null);
                session.insert(w.marker.clone(), Value::Null);
                session.insert(w.started_t.clone(), json!(opted.modified));
                session.insert(w.started.clone(), json!(iso(opted.modified)));
                session.insert(w.state.clone(), json!(e.registry.states.finished));
                session.insert(w.updated_t.clone(), json!(t0));
                session.insert(w.resumes.clone(), json!([]));
                found.push((step_key(&session), Value::Object(session)));
            }
        }
        found
    }

    /// F162, the prototype's `_transcript`: the record's transcript, or found again by its session, or, for a step on
    /// the app whose session was not found at its launch, by the marker its receipt gave (#462/V7).
    fn transcript_of(&self, record: &mut Map<String, Value>) -> Option<PathBuf> {
        let (e, w) = (engine_words(), &engine_words().registry.record);
        if let Some(path) = text(record, &w.transcript)
            && Path::new(path).exists()
        {
            return Some(PathBuf::from(path));
        }
        let service = service_of(record);
        let folder = PathBuf::from(text(record, &w.folder).unwrap_or_default());
        let found = if let Some(session) = text(record, &w.resume) {
            AgentService::known(service).transcript(&self.env, &folder, session)
        } else if text(record, &w.adapter) == Some(e.adapters.own_sessions.as_str())
            && let Some(marker) = text(record, &w.marker)
        {
            let after = number(record, &w.started_t).unwrap_or(0.0);
            session_by_marker(service, &self.env, &folder, after, marker).map(|(session, path)| {
                record.insert(w.resume.clone(), json!(session));
                path
            })
        } else {
            None
        };
        if let Some(path) = &found {
            record.insert(w.transcript.clone(), json!(path.display().to_string()));
        }
        found
    }

    /// One record of F161, the prototype's `_aggiorna_registro`: read again from its transcript, its state follows.
    fn read_again(&self, value: &mut Value, tick: &TickContext<'_>, t0: f64) {
        let (e, r) = (engine_words(), words());
        let (w, st) = (&e.registry.record, &e.registry.states);
        let alive = record_alive(value, tick.liveness());
        let Some(record) = value.as_object_mut() else { return };
        let before = text(record, &w.state).map(str::to_owned);
        let open = [&st.launched, &st.suspended, &st.resuming, &st.to_resume];
        let is_open = before.as_ref().is_some_and(|state| open.contains(&state));
        if !is_open && t0 - number(record, &w.updated_t).unwrap_or(0.0) > r.stale_s {
            return;
        }
        if before.as_deref() == Some(st.resuming.as_str()) && alive {
            // Its resume is under way, the process is ours.
            return;
        }
        let Some(path) = self.transcript_of(record) else { return };
        let analysis = suspension::analyze(&path, service_of(record), &Reading { now: t0, grace_s: suspension::RESUME_GRACE_S, offset: self.offset });
        record.insert(r.record.analysis.clone(), analysis_value(&analysis));
        record.insert(r.record.resets.clone(), json!(analysis.resets));
        record.insert(r.record.resets_iso.clone(), iso_of(analysis.resets));
        let last = record.get(&w.resumes).and_then(Value::as_array).and_then(|resumes| resumes.last()).and_then(Value::as_object).cloned().unwrap_or_default();
        let limit_iso = analysis.limit_at.and_then(suspension::iso);
        let resets_iso = analysis.resets.and_then(suspension::iso);
        let was = |state: &str| before.as_deref() == Some(state);
        let after = match analysis.state {
            // Someone wrote after the limit, with no answer yet.
            State::Resuming => Some(&st.resuming),
            // Released by the owner for this limit.
            State::Suspended if limit_iso.as_deref() == text(record, &r.record.released) => None,
            State::Suspended if !alive => {
                let same_limit = resets_iso.as_deref() == last.get(&r.attempt.limit).and_then(Value::as_str);
                if was(&st.resuming) && same_limit && !last.contains_key(&r.attempt.error) {
                    // Our resume ended, and the limit is still the same.
                    let failed = record.get(&r.record.failed).and_then(Value::as_i64).unwrap_or(0) + 1;
                    record.insert(r.record.failed.clone(), json!(failed));
                }
                Some(&st.suspended)
            }
            State::Active | State::Empty if was(&st.suspended) || was(&st.resuming) => Some(if alive { &st.launched } else { &st.finished }),
            _ => None,
        };
        if let Some(after) = after {
            record.insert(w.state.clone(), json!(after));
        }
        if text(record, &w.state).map(str::to_owned) != before {
            record.insert(w.updated_t.clone(), json!(t0));
        }
    }

    /// F161: the records of the registry and the new ones read again, then the registry saved.
    fn update(&self, records: &mut Map<String, Value>, tick: &TickContext<'_>, t0: f64) -> Result<(), TickError> {
        let mut changed = BTreeSet::new();
        for (key, record) in self.discover(records, tick, t0) {
            if !records.contains_key(&key) {
                records.insert(key.clone(), record);
                changed.insert(key);
            }
        }
        let keys: Vec<String> = records.keys().cloned().collect();
        for key in keys {
            let Some(value) = records.get_mut(&key) else { continue };
            let before = value.clone();
            self.read_again(value, tick, t0);
            if *value != before {
                changed.insert(key);
            }
        }
        self.save(records, &changed, tick, t0)
    }

    /// F166, the prototype's `_applica_rilasci`: each release closes the suspended records it names for their limit;
    /// its file goes once applied, or after a day without a match.
    fn apply_releases(&self, records: &mut Map<String, Value>, line: &mut Map<String, Value>, tick: &TickContext<'_>, t0: f64) -> Result<(), TickError> {
        let (e, r) = (engine_words(), words());
        let (w, st) = (&e.registry.record, &e.registry.states);
        for (path, request) in releases(&self.state) {
            let who = request.get(&r.release.session).and_then(Value::as_str).unwrap_or_default();
            let matching: BTreeSet<String> = records
                .iter()
                .filter(|(key, value)| {
                    let Some(record) = value.as_object() else { return false };
                    let busy = text(record, &w.state).is_some_and(|state| state == st.suspended || state == st.resuming);
                    let ids = [Some(key.as_str()), text(record, &w.session), text(record, &w.resume), text(record, &w.surface_session)];
                    busy && (who == r.release.all || ids.contains(&Some(who)))
                })
                .map(|(key, _)| key.clone())
                .collect();
            for key in &matching {
                let Some(Value::Object(record)) = records.get_mut(key) else { continue };
                record.insert(w.state.clone(), json!(st.finished));
                record.insert(w.updated_t.clone(), json!(t0));
                let limit = record.get(&r.record.analysis).and_then(|analysis| analysis.get(&r.analysis.limit_iso)).and_then(Value::as_str).filter(|limit| !limit.is_empty());
                record.insert(r.record.released.clone(), json!(limit.unwrap_or(&r.release.all)));
                let mut released = Map::new();
                released.insert(r.waiting.project.clone(), field(record, &w.project));
                released.insert(r.waiting.session.clone(), json!(text(record, &w.resume).or_else(|| text(record, &w.session))));
                push(line, &r.line.releases, Value::Object(released));
            }
            if !matching.is_empty() {
                self.save(records, &matching, tick, t0)?;
            }
            let asked = suspension::epoch(request.get(&r.release.t)).unwrap_or(t0);
            if !matching.is_empty() || t0 - asked > r.release_kept_s {
                let _ = fs::remove_file(&path);
            }
        }
        Ok(())
    }

    /// Whether the app may show this session: by its record, its transcript, or the app's index; None when the app does
    /// not answer. The prototype's check of the app's sessions (`sospese.py`).
    fn app_session(&self, record: &Map<String, Value>, analysis: Option<&Map<String, Value>>) -> Option<bool> {
        let (e, w) = (engine_words(), &engine_words().registry.record);
        if text(record, &w.adapter) == Some(e.adapters.own_sessions.as_str()) || truthy(record.get(&w.surface_session)) {
            return Some(true);
        }
        if analysis.and_then(|analysis| analysis.get(&words().analysis.entrypoint)).and_then(Value::as_str) == Some(CHAT_ENTRYPOINT) {
            return Some(true);
        }
        let session = text(record, &w.resume).or_else(|| text(record, &w.session));
        let known = self.host.known(text(record, &w.folder).unwrap_or_default())?;
        Some(session.and_then(|session| known.get(session)).is_some_and(|chat| chat.as_deref().is_some_and(|chat| !chat.is_empty())))
    }

    /// F167, the prototype's `_riprendibile` with its guard of the Cli surface: whether this suspended session may be
    /// resumed from here, and why. Never one of the app's.
    fn resumable(&self, record: &Map<String, Value>) -> (bool, String) {
        let (e, r) = (engine_words(), words());
        let (w, reasons) = (&e.registry.record, &r.reasons);
        let failed = record.get(&r.record.failed).and_then(Value::as_i64).unwrap_or(0);
        if failed >= r.max_failed {
            return (false, reasons.failed.replace("{n}", &failed.to_string()));
        }
        match agent_or_claude(record) {
            Some(agent) if ServiceId::parse(agent).is_some() => {}
            agent => {
                let shown = agent.map_or_else(|| record.get(&w.agent).map_or_else(|| "None".to_owned(), |value| if value.is_null() { "None".to_owned() } else { value.to_string() }), str::to_owned);
                return (false, reasons.agent.replace("{agent}", &shown));
            }
        }
        if text(record, &w.adapter) == Some(e.adapters.own_sessions.as_str()) || truthy(record.get(&w.surface_session)) {
            return (false, reasons.app_session.clone());
        }
        let analysis = record.get(&r.record.analysis).and_then(Value::as_object).filter(|analysis| !analysis.is_empty());
        if analysis.and_then(|analysis| analysis.get(&r.analysis.entrypoint)).and_then(Value::as_str) == Some(CHAT_ENTRYPOINT) {
            return (false, reasons.app_chat.clone());
        }
        if !truthy(record.get(&w.resume)) && !truthy(record.get(&w.session)) {
            return (false, reasons.no_session.clone());
        }
        if text(record, &w.adapter) == Some(SurfaceKind::Cli.id()) && text(record, &w.origin) == Some(e.registry.origins.controller.as_str()) {
            return (true, reasons.launched.clone());
        }
        match self.app_session(record, analysis) {
            None => (false, reasons.no_answer.clone()),
            Some(true) => (false, reasons.shown.clone()),
            Some(false) => (true, reasons.opted_in.clone()),
        }
    }

    /// The prototype's channel of a session of the app, whether a way to wake it exists: a worker with its dispatch, or a step's
    /// chat with an id of a chat. The message itself is not sent (#191).
    fn wake_command(record: &Map<String, Value>) -> Option<String> {
        let (e, r) = (engine_words(), words());
        let w = &e.registry.record;
        if text(record, &w.adapter) != Some(e.adapters.own_sessions.as_str()) && !truthy(record.get(&w.surface_session)) {
            return None;
        }
        if text(record, &w.origin) == Some(r.origins.worker.as_str()) {
            return text(record, &w.transcript).and_then(|transcript| worker_wake_command(Path::new(transcript)));
        }
        let agent = agent_or_claude(record).is_some_and(|agent| ServiceId::parse(agent).is_some());
        if text(record, &w.origin) == Some(e.registry.origins.controller.as_str()) && agent {
            return record.get(&w.surface_session).and_then(Value::as_str).and_then(chat_wake_command);
        }
        None
    }

    /// F168: a session the controller does not resume, as the brief lists it.
    fn voice(record: &Map<String, Value>, reason: &str) -> Value {
        let (e, r) = (engine_words(), words());
        let (w, t) = (&e.registry.record, &r.to_resume);
        let session = text(record, &w.resume).or_else(|| text(record, &w.session));
        let app = text(record, &w.adapter) == Some(e.adapters.own_sessions.as_str()) || truthy(record.get(&w.surface_session));
        let claude = text(record, &w.agent).unwrap_or(ServiceId::Claude.id()) == ServiceId::Claude.id();
        let how = if app {
            json!(r.reasons.how_app)
        } else {
            session.filter(|_| claude).map_or(Value::Null, |session| json!(AgentService::known(ServiceId::Claude).resume(session)))
        };
        let command = if text(record, &w.origin) == Some(r.origins.worker.as_str())
            && let Some(transcript) = text(record, &w.transcript)
        {
            worker_wake_command(Path::new(transcript))
        } else if text(record, &w.origin) == Some(r.origins.worker.as_str()) {
            None
        } else {
            Resume::wake_command(record)
        };
        let mut voice = Map::new();
        voice.insert(t.project.clone(), field(record, &w.project));
        voice.insert(t.folder.clone(), field(record, &w.folder));
        voice.insert(t.origin.clone(), field(record, &w.origin));
        voice.insert(t.adapter.clone(), field(record, &w.adapter));
        voice.insert(t.agent.clone(), field(record, &w.agent));
        voice.insert(t.session.clone(), json!(session));
        voice.insert(t.surface_session.clone(), field(record, &w.surface_session));
        voice.insert(t.transcript.clone(), field(record, &w.transcript));
        voice.insert(t.resets.clone(), field(record, &r.record.resets_iso));
        voice.insert(t.reason.clone(), json!(reason));
        voice.insert(t.how.clone(), how);
        voice.insert(t.command.clone(), json!(command));
        Value::Object(voice)
    }

    /// F169: the sessions to resume by hand, replaced whole through a temporary
    /// file of this process, as the brief and the dailies read it. A failed write is no failure of the tick.
    fn write_to_resume(&self, t0: f64) {
        let (f, t) = (&words().files, &words().to_resume);
        let mut document = Map::new();
        document.insert(t.v.clone(), json!(words().version));
        document.insert(t.updated.clone(), json!(iso(t0)));
        document.insert(t.sessions.clone(), Value::Array(self.to_resume.clone()));
        let mut bytes = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
        if serde::Serialize::serialize(&Value::Object(document), &mut serializer).is_err() {
            return;
        }
        let temporary = self.state.join(format!("{}.{}.tmp", f.to_resume, self.pid));
        if fs::write(&temporary, &bytes).and_then(|()| fs::rename(&temporary, self.state.join(&f.to_resume))).is_err() {
            let _ = fs::remove_file(&temporary);
        }
    }

    /// F170, the prototype's `_riprendi`: one resume, and its receipt in the record and in the line. A resume that does
    /// not start counts as an attempt: past the format's attempts the session is the owner's.
    fn resume_one(&self, records: &mut Map<String, Value>, key: &str, line: &mut Map<String, Value>, tick: &TickContext<'_>, t0: f64) -> Result<(), TickError> {
        let (e, r) = (engine_words(), words());
        let (w, a, v) = (&e.registry.record, &r.attempt, &r.resume);
        // D1: right before the effect, outside the resume's own error handling.
        tick.guard()?;
        let Some(Value::Object(record)) = records.get(key).cloned() else { return Ok(()) };
        let outcome = self.resumer.resume(&record);
        let Some(Value::Object(record)) = records.get_mut(key) else { return Ok(()) };
        let session = text(record, &w.resume).or_else(|| text(record, &w.session)).map(str::to_owned);
        let limit = field(record, &r.record.resets_iso);
        let effective = iso(tick.clock());
        let mut attempt = Map::new();
        attempt.insert(a.limit.clone(), limit.clone());
        attempt.insert(a.effective.clone(), json!(effective));
        match outcome {
            Err(error) => {
                let error = cut(&error);
                attempt.insert(a.error.clone(), json!(error));
                push(record, &w.resumes, Value::Object(attempt));
                let failed = record.get(&r.record.failed).and_then(Value::as_i64).unwrap_or(0) + 1;
                record.insert(r.record.failed.clone(), json!(failed));
                let mut failure = Map::new();
                failure.insert(v.session.clone(), json!(session));
                failure.insert(v.project.clone(), field(record, &w.project));
                failure.insert(v.error.clone(), json!(error));
                line.insert(r.line.resume_error.clone(), Value::Object(failure));
            }
            Ok(receipt) => {
                let mut resume = Map::new();
                resume.insert(v.session.clone(), json!(session));
                resume.insert(v.project.clone(), field(record, &w.project));
                resume.insert(v.adapter.clone(), field(record, &w.adapter));
                resume.insert(v.agent.clone(), field(record, &w.agent));
                resume.insert(v.expected.clone(), limit);
                resume.insert(v.effective.clone(), json!(effective));
                resume.insert(v.pid.clone(), json!(receipt.pid));
                resume.insert(v.log.clone(), json!(receipt.log));
                resume.insert(v.new_session.clone(), json!(receipt.session));
                attempt.insert(a.pid.clone(), json!(receipt.pid));
                attempt.insert(a.log.clone(), json!(receipt.log));
                attempt.insert(a.new_session.clone(), json!(receipt.session));
                push(record, &w.resumes, Value::Object(attempt));
                record.insert(w.state.clone(), json!(e.registry.states.resuming));
                record.insert(w.pid.clone(), json!(receipt.pid));
                record.insert(w.updated_t.clone(), json!(t0));
                record.insert(r.record.resumed_t.clone(), json!(t0));
                if let Some(log) = &receipt.log {
                    record.insert(w.log.clone(), json!(log));
                }
                if let Some(new) = receipt.session.as_ref().filter(|new| Some(new.as_str()) != session.as_deref()) {
                    // The resume opened another session: that one is followed.
                    record.insert(w.resume.clone(), json!(new));
                    record.insert(w.transcript.clone(), json!(receipt.transcript));
                }
                record.insert(r.record.analysis.clone(), Value::Null);
                if let Some(pid) = receipt.pid {
                    tick.liveness().adopt(pid);
                }
                push(line, &r.line.resumes, Value::Object(resume));
            }
        }
        self.save(records, &BTreeSet::from([key.to_owned()]), tick, t0)
    }

    /// F165, the prototype's `_sospese`: the rule of the tick. True when the tick ends here.
    fn run(&mut self, line: &mut Map<String, Value>, tick: &TickContext<'_>) -> Result<bool, TickError> {
        let (e, r) = (engine_words(), words());
        let (w, st) = (&e.registry.record, &e.registry.states);
        let t0 = tick.now();
        self.suspended.clear();
        self.to_resume.clear();
        let mut records = tick.steps().records();
        self.update(&mut records, tick, t0)?;
        self.apply_releases(&mut records, line, tick, t0)?;
        let mut stopped: Vec<(String, f64)> = records
            .iter()
            .filter_map(|(key, value)| value.as_object().map(|record| (key, record)))
            .filter(|(_, record)| text(record, &w.state) == Some(st.suspended.as_str()))
            .map(|(key, record)| (key.clone(), number(record, &r.record.resets).unwrap_or(0.0)))
            .collect();
        stopped.sort_by(|a, b| a.1.total_cmp(&b.1));
        let waits = |resets: f64| resets + r.margin_s > t0;
        let s = &r.suspended;
        for (key, resets) in &stopped {
            let Some(record) = records.get(key).and_then(Value::as_object) else { continue };
            let mut entry = Map::new();
            entry.insert(s.project.clone(), field(record, &w.project));
            entry.insert(s.session.clone(), json!(text(record, &w.resume).or_else(|| text(record, &w.session))));
            entry.insert(s.adapter.clone(), field(record, &w.adapter));
            entry.insert(s.origin.clone(), field(record, &w.origin));
            entry.insert(s.resets.clone(), field(record, &r.record.resets_iso));
            entry.insert(s.due.clone(), json!(!waits(*resets)));
            self.suspended.push(Value::Object(entry));
        }
        line.insert(r.line.suspended.clone(), Value::Array(self.suspended.clone()));
        let mut chosen: Option<String> = None;
        let mut with_a_way: Vec<(String, String)> = Vec::new();
        for (key, resets) in &stopped {
            if waits(*resets) {
                continue;
            }
            let Some(record) = records.get(key).and_then(Value::as_object) else { continue };
            let (ok, why) = self.resumable(record);
            if ok {
                if chosen.is_none() {
                    chosen = Some(key.clone());
                }
            } else if Resume::wake_command(record).is_some() {
                with_a_way.push((key.clone(), why));
            } else {
                self.to_resume.push(Resume::voice(record, &why));
            }
        }
        // The app's sessions with a way to wake them come after, the step before its workers, as the prototype lists them.
        let worker = |key: &String| records.get(key).and_then(Value::as_object).is_some_and(|record| text(record, &w.origin) == Some(r.origins.worker.as_str()));
        with_a_way.sort_by_key(|(key, _)| worker(key));
        for (key, why) in &with_a_way {
            if let Some(record) = records.get(key).and_then(Value::as_object) {
                self.to_resume.push(Resume::voice(record, why));
            }
        }
        self.write_to_resume(t0);
        let d = &r.decisions;
        let decision = &e.record.decision;
        if let Some(key) = chosen {
            self.resume_one(&mut records, &key, line, tick, t0)?;
            let failed = line.contains_key(&r.line.resume_error);
            line.insert(decision.clone(), json!(if failed { &d.resume_error } else { &d.resumed }));
            return Ok(true);
        }
        let waiting: Vec<&Map<String, Value>> =
            stopped.iter().filter(|(_, resets)| waits(*resets)).filter_map(|(key, _)| records.get(key).and_then(Value::as_object)).collect();
        if !waiting.is_empty() {
            let until = waiting.iter().filter_map(|record| text(record, &r.record.resets_iso)).min();
            line.insert(decision.clone(), json!(d.suspended));
            line.insert(r.line.wait.clone(), json!({r.wait.until.as_str(): until, r.wait.sessions.as_str(): waiting.len()}));
            return Ok(true);
        }
        if !self.to_resume.is_empty() && !self.settings.only_project {
            // A stopped session only its owner can unblock: while it is there, nothing else starts.
            let t = &r.to_resume;
            let listed: Vec<Value> = self
                .to_resume
                .iter()
                .map(|voice| {
                    json!({
                        r.waiting.project.as_str(): voice.get(&t.project).cloned().unwrap_or(Value::Null),
                        r.waiting.session.as_str(): voice.get(&t.session).cloned().unwrap_or(Value::Null),
                        r.waiting.reason.as_str(): voice.get(&t.reason).cloned().unwrap_or(Value::Null),
                    })
                })
                .collect();
            line.insert(decision.clone(), json!(d.to_resume));
            line.insert(r.line.waiting_to_resume.clone(), Value::Array(listed));
            return Ok(true);
        }
        Ok(false)
    }
}

/// The resumes of a tick's line, as the prototype's summary of a tick names them: the session, and the surface's word.
#[must_use]
pub fn line_resumes(record: &Value) -> Vec<(String, String)> {
    let (l, v) = (&words().line, &words().resume);
    record
        .get(&l.resumes)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|resume| {
            let text = |key: &str| resume.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
            (text(&v.session), text(&v.adapter))
        })
        .collect()
}

/// Until when a tick's line waits for a suspended session, as the summary of a tick names it.
#[must_use]
pub fn line_wait(record: &Value) -> Option<String> {
    record.get(&words().line.wait).and_then(|wait| wait.get(&words().wait.until)).and_then(Value::as_str).map(str::to_owned)
}

fn push(record: &mut Map<String, Value>, key: &str, value: Value) {
    if let Some(Value::Array(items)) = record.get_mut(key) {
        items.push(value);
    } else {
        record.insert(key.to_owned(), Value::Array(vec![value]));
    }
}

impl BeforePrecheck for Resume {
    fn before(&mut self, record: &mut Map<String, Value>, tick: &TickContext<'_>) -> Result<bool, TickError> {
        self.run(record, tick)
    }

    /// The suspended sessions of the tick, and how many are to resume by hand.
    fn status(&self, status: &mut Map<String, Value>) {
        let s = &engine_words().status;
        status.insert(s.suspended.clone(), Value::Array(self.suspended.clone()));
        status.insert(s.to_resume.clone(), json!(self.to_resume.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_parses_and_its_numbers_are_the_prototype_s() {
        let format = words();
        assert_eq!((format.margin_s, format.max_failed, format.stale_s, format.opt_in_days), (60.0, 2, 86_400.0, 14.0));
        assert_eq!(format.version, 1);
    }

    #[test]
    fn a_release_is_named_by_its_session_s_safe_characters() {
        assert_eq!(release_name("s-a"), "s-a");
        assert_eq!(release_name("*"), "_");
        assert_eq!(release_name("a/b c.d"), "a_b_c_d");
        assert_eq!(release_name(""), "x");
        assert_eq!(release_name(&"é".repeat(120)).len(), 100);
    }
}
