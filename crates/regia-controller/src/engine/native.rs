//! The ports of a tick on this machine: the precheck in process, the registry's folders from the tracker,
//! the liveness of a process and the reaping of the sessions this host started, the controller's lock, the
//! postponements and the steps registry in the state database. The prototype's `esegui_precheck`,
//! folders of its registry, `processo_vivo`, `rinnova_lock`, the cursor of `_segna_rinvio` and `sospese.Registro`.
//!
//! Every write of the state here is the lock holder's: one transaction checks that the controller's lock is
//! still this controller's, then writes (#507 of the dossier, #582, ADR 0001 §7). A controller that stood still
//! past its lock's expiry, and that another one replaced, writes neither a postponement nor a record.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{BuildHasher, Hasher, RandomState};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::sync::atomic::{AtomicU64, Ordering};

use regia_core::{Registry, Tracker, TrackerBinding, TrackerConfigV1, project_entry};
use regia_github::GhTracker;
use regia_identity::{config_dir, prototype_env_var, state_dir};
use regia_store::{CursorName, Fenced, Holder, ProjectId, Store, Ttl, WallTime};
use serde_json::{Map, Value, json};

use super::words::format as words;
use super::{AliveStep, ControllerLock, Folders, LaunchedStep, Liveness, LockLost, Precheck, PrecheckRun, Signals, StateError, Steps, iso};
use crate::precheck::{Fault, Verdict, evaluate, live_sources_in};

/// How long the controller's lock lasts from a renewal: the prototype's `LOCK_TTL_S`, the TTL both languages
/// share (ADR 0001 §7). Longer than a precheck and a launch; a tick renews it at each of its launch points.
pub const LOCK_TTL_S: u64 = 600;

/// The precheck of this crate, in this process: the same reads and line as the `precheck` command. The
/// tick's deadline is checked between the reads of the tracker; a read that hangs waits for the limit of
/// `regia-exec`, and `timeout_s` has no process to stop (#465/V5, a divergence the report of #467 names).
#[derive(Debug, Clone, Default)]
pub struct NativePrecheck {
    /// The clock of the reads, when a run fixes it.
    pub fixed: Option<WallTime>,
    /// The state folder, when a run names one in place of the environment's: a shadow run's copy.
    pub folder: Option<PathBuf>,
}

impl Precheck for NativePrecheck {
    fn run(&self, projects: &[String], _timeout_s: f64) -> PrecheckRun {
        let options = crate::precheck::Options { projects: projects.to_vec(), ..crate::precheck::Options::default() };
        let now = self.fixed.unwrap_or_else(WallTime::now);
        let verdict = match live_sources_in(self.folder.as_deref(), options.deadline_s, self.fixed) {
            Ok(sources) => evaluate(&sources, &options, now),
            Err(detail) => Verdict::of_fault(&Fault::internal(detail)),
        };
        let (exit, line) = verdict.line(now);
        PrecheckRun::from_output(Some(i32::from(exit)), &line)
    }
}

/// The folder of each project, from the registry of the Objectives in the tracker, as `projects` reads it.
#[derive(Debug, Clone, Default)]
pub struct TrackerFolders {
    /// The state folder whose `tracker.json` is read, when a run names one: a shadow run's copy.
    pub folder: Option<PathBuf>,
}

impl Folders for TrackerFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        let dossier = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
        let state = self.folder.clone().or_else(state_dir);
        let config = TrackerConfigV1::load(state.as_deref(), dossier.as_deref(), config_dir().as_deref()).map_err(|error| error.to_string())?;
        let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
        let tracker = GhTracker::new(&config);
        let mut folders = BTreeMap::new();
        for mut issue in tracker.open_issues().map_err(|error| error.to_string())? {
            let body = std::mem::take(&mut issue.body);
            issue.status_option_id = None;
            let task = binding.resolve(issue).map_err(|error| error.to_string())?;
            if !task.objective {
                continue;
            }
            let entry = project_entry(&task, &Registry::parse(&body), &config.owner).map_err(|error| error.to_string())?;
            let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
            if let Some(id) = text("id") {
                folders.insert(id, text("folder"));
            }
        }
        Ok(folders)
    }
}

/// Whether the process `pid` exists and runs. On Unix a child of this process that already ended (a zombie)
/// is not alive, and asking reaps it; a process of another user is alive. On Windows the process is opened
/// for its exit code only, and one that cannot be opened for want of rights exists.
#[cfg(unix)]
#[must_use]
pub fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    if pid <= 0 {
        return false;
    }
    let mut status: libc::c_int = 0;
    // SAFETY: waitpid takes plain values and writes only to `status`, which outlives the call.
    if unsafe { libc::waitpid(pid, &raw mut status, libc::WNOHANG) } == pid {
        return false;
    }
    // SAFETY: signal 0 sends nothing: it only asks whether the process exists.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
#[must_use]
pub fn process_alive(pid: u32) -> bool {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    if pid == 0 {
        return false;
    }
    // SAFETY: OpenProcess takes plain values; a handle it returns is owned from here.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if raw.is_null() {
        return std::io::Error::last_os_error().raw_os_error() == i32::try_from(ERROR_ACCESS_DENIED).ok();
    }
    // SAFETY: a non-null handle OpenProcess just returned, owned by no one else.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut code: u32 = 0;
    // SAFETY: the handle is open, and `code` outlives the call.
    let read = unsafe { GetExitCodeProcess(handle.as_raw_handle(), &raw mut code) } != 0;
    read && i32::try_from(code).ok() == Some(STILL_ACTIVE)
}

/// The processes of this machine, and the sessions this host started: each is reaped once it ended, at the
/// tick after, whether its step is still in the tick's memory or past its limit (#460/V5). Without this a
/// controller that lives on keeps on Unix a zombie for every launch until it exits.
#[derive(Debug, Default)]
pub struct Processes {
    started: Mutex<BTreeSet<u32>>,
}

impl Processes {
    #[must_use]
    pub fn new() -> Processes {
        Processes::default()
    }

    /// The sessions started and not yet reaped.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.started.lock().map_or(0, |started| started.len())
    }
}

impl Liveness for Processes {
    fn alive(&self, pid: u32) -> bool {
        process_alive(pid)
    }

    fn adopt(&self, pid: u32) {
        if let Ok(mut started) = self.started.lock() {
            started.insert(pid);
        }
    }

    fn reap(&self) {
        if let Ok(mut started) = self.started.lock() {
            started.retain(|pid| process_alive(*pid));
        }
    }
}

fn reserved() -> Result<ProjectId, StateError> {
    ProjectId::new(words().reserved_project.clone()).map_err(|error| StateError::Failed(error.to_string()))
}

/// The lock passed to someone else, or to nobody: `what` was neither read nor written.
fn lost(what: &str, holder: Option<&Value>) -> StateError {
    let now = match holder {
        Some(Value::String(holder)) => format!("{holder} holds it"),
        Some(other) => format!("{other} holds it"),
        None => "nobody holds it".to_owned(),
    };
    StateError::LockLost(LockLost(format!("the controller's lock is no longer ours ({now}): {what} is left alone")))
}

/// What became of an operation as the lock's holder.
fn fenced<T>(outcome: Result<Fenced<T>, regia_store::StoreError>, what: &str) -> Result<T, StateError> {
    match outcome {
        Ok(Fenced::Done(value)) => Ok(value),
        Ok(Fenced::LockLost { holder }) => Err(lost(what, holder.as_ref())),
        Err(error) => Err(StateError::Failed(error.to_string())),
    }
}

/// The controller's lock in the state database: renewed by its holder, for [`LOCK_TTL_S`].
#[derive(Debug, Clone)]
pub struct StoreLock {
    store: Store,
    holder: Holder,
}

impl StoreLock {
    #[must_use]
    pub fn new(store: Store, holder: Holder) -> StoreLock {
        StoreLock { store, holder }
    }
}

impl ControllerLock for StoreLock {
    fn renew(&self) -> Result<(), StateError> {
        let ttl = Ttl::from_secs(LOCK_TTL_S).map_err(|error| StateError::Failed(error.to_string()))?;
        match self.store.renew_lock(&reserved()?, &self.holder, ttl) {
            Ok(outcome) if outcome.is_ok() => Ok(()),
            Ok(outcome) => Err(StateError::LockLost(LockLost(format!(
                "the controller's lock is no longer ours ({:?}): another controller is active",
                outcome.state()
            )))),
            Err(error) => Err(StateError::Failed(error.to_string())),
        }
    }
}

/// The postponements in the state database: one cursor with a name per project, written in a transaction that
/// first checks the controller's lock is still `holder`'s (#582, the residue #467/V4). The prototype writes it
/// without that check; a postponement written after the lock passed would hide a project's stall from the
/// controller that holds it now.
#[derive(Debug, Clone)]
pub struct StoreSignals {
    store: Store,
    holder: Holder,
}

impl StoreSignals {
    /// The postponements of the controller whose lock `holder` holds.
    #[must_use]
    pub fn new(store: Store, holder: Holder) -> StoreSignals {
        StoreSignals { store, holder }
    }
}

impl Signals for StoreSignals {
    fn postpone(&self, project: &str, value: Value) -> Result<(), StateError> {
        let failed = |error: String| StateError::Failed(error);
        let project = ProjectId::new(project).map_err(|error| failed(error.to_string()))?;
        let name = CursorName::new(words().postpone.cursor.clone()).map_err(|error| failed(error.to_string()))?;
        let lock = reserved()?;
        let written = self.store.as_holder(&lock, &self.holder).write_cursor(&project, &name, |_| (Some(value), ()));
        fenced(written, "the postponement")
    }
}

/// A random id of eight hexadecimal digits, as the prototype's `uuid4().hex[:8]`.
#[must_use]
pub fn random_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos()));
    format!("{:08x}", hasher.finish() & 0xffff_ffff)
}

/// The steps registry in the state database, in the prototype's document: the cursor `passi` of the reserved
/// project, `{"v": 1, "passi": {key: record}}`, which both controllers read and write. D2 of ADR 0001 §7 is not
/// inherited (the counterexample of round 2 of `0001-review-codex.md`):
///
/// - **read after the lock.** Building it reads nothing; [`StoreSteps::load`] reads in a transaction that first
///   checks the controller's lock is `holder`'s, so the registry cannot be read before the lock is taken;
/// - **saved record by record, with a version check.** In one transaction that first checks the lock (#507), each
///   record this controller changed since its last load or save goes over the stored one only where the stored
///   record is still the version the change started from; where another writer changed it, the stored one stays,
///   and so does every record this controller did not change, also one added after the load. Then the
///   prototype's rule of what is kept: the open records, and the finished ones of the last week.
///
/// After a save the memory is the document as written.
#[derive(Debug)]
pub struct StoreSteps {
    store: Store,
    holder: Holder,
    memory: Mutex<Memory>,
}

/// What a controller knows of the registry.
#[derive(Debug, Default)]
struct Memory {
    /// The records, by key.
    records: Map<String, Value>,
    /// Each record as the state held it at the last load or save: the version a change is checked against.
    bases: Map<String, Value>,
    /// The keys of the records changed since.
    changed: BTreeSet<String>,
}

fn registry_cursor() -> Result<CursorName, StateError> {
    CursorName::new(words().registry.cursor.clone()).map_err(|error| StateError::Failed(error.to_string()))
}

/// The records of a stored registry; none when it is missing or does not read as one.
fn stored_records(doc: &Value) -> Map<String, Value> {
    doc.get(&words().registry.steps).and_then(Value::as_object).cloned().unwrap_or_default()
}

/// The prototype's rule of what a save keeps: the open records, and the finished ones updated within a week.
fn keep_recent(records: &mut Map<String, Value>, at: f64) {
    let r = &words().registry;
    let states = &r.states;
    let open = [&states.launched, &states.suspended, &states.resuming, &states.to_resume];
    records.retain(|_, record| {
        let state = record.get(&r.record.state).and_then(Value::as_str);
        let updated = record.get(&r.record.updated_t).and_then(Value::as_f64).filter(|updated| *updated != 0.0).unwrap_or(at);
        state.is_some_and(|state| open.iter().any(|open| open.as_str() == state)) || at - updated < r.keep_finished_s
    });
}

impl StoreSteps {
    /// The registry of the controller whose lock `holder` holds, not read yet: [`StoreSteps::load`] reads it, once
    /// the lock is taken.
    #[must_use]
    pub fn new(store: Store, holder: Holder) -> StoreSteps {
        StoreSteps { store, holder, memory: Mutex::new(Memory::default()) }
    }

    /// Reads the registry, in a transaction that first checks the lock is still `holder`'s: the number of records.
    ///
    /// # Errors
    ///
    /// The lock is not `holder`'s, or the state cannot be read.
    pub fn load(&self) -> Result<usize, StateError> {
        let (lock, name) = (reserved()?, registry_cursor()?);
        let doc = fenced(self.store.as_holder(&lock, &self.holder).cursor(&lock, &name), "the steps registry")?;
        let records = stored_records(&doc);
        let mut memory = self.memory()?;
        *memory = Memory { bases: records.clone(), records, changed: BTreeSet::new() };
        Ok(memory.records.len())
    }

    /// The records, by key.
    #[must_use]
    pub fn records(&self) -> Map<String, Value> {
        self.memory.lock().map(|memory| memory.records.clone()).unwrap_or_default()
    }

    /// The steps a controller launched that are still launched, for the tick's memory after a restart, so the
    /// window between a launch and the step's start does not launch the project again (F136, Choice 1 of the
    /// dossier's #102): each with its pid, its session and its key, at its launch, or at `started` without one.
    /// By their launch, so a project launched twice is the later launch.
    #[must_use]
    pub fn launched_steps(&self, started: f64) -> Vec<AliveStep> {
        let r = &words().registry;
        let mut steps: Vec<(f64, AliveStep)> = self
            .records()
            .into_iter()
            .filter_map(|(key, record)| {
                let text = |name: &str| record.get(name).and_then(Value::as_str);
                if text(&r.record.origin) != Some(r.origins.controller.as_str()) || text(&r.record.state) != Some(r.states.launched.as_str()) {
                    return None;
                }
                let project = text(&r.record.project)?.to_owned();
                let launched = record.get(&r.record.started_t).and_then(Value::as_f64).filter(|t| *t != 0.0);
                let pid = record.get(&r.record.pid).and_then(Value::as_u64).and_then(|pid| u32::try_from(pid).ok());
                let session = text(&r.record.session).map(str::to_owned);
                Some((launched.unwrap_or(0.0), AliveStep { project, pid, t: launched.unwrap_or(started), session, key }))
            })
            .collect();
        steps.sort_by(|a, b| a.0.total_cmp(&b.0));
        steps.into_iter().map(|(_, step)| step).collect()
    }

    fn memory(&self) -> Result<MutexGuard<'_, Memory>, StateError> {
        self.memory.lock().map_err(|_| StateError::Failed("the registry's memory is poisoned".to_owned()))
    }

    fn save(&self, memory: &mut Memory, at: f64) -> Result<(), StateError> {
        let (lock, name) = (reserved()?, registry_cursor()?);
        let r = &words().registry;
        let known: &Memory = memory;
        let written = self.store.as_holder(&lock, &self.holder).write_cursor(&lock, &name, |stored| {
            let mut merged = stored_records(&stored);
            for key in &known.changed {
                // The version check: the stored record is still the one this change started from.
                if merged.get(key) == known.bases.get(key) {
                    match known.records.get(key) {
                        Some(record) => merged.insert(key.clone(), record.clone()),
                        None => merged.remove(key),
                    };
                }
            }
            keep_recent(&mut merged, at);
            let doc = json!({r.v.as_str(): 1, r.steps.as_str(): Value::Object(merged.clone())});
            (Some(doc), merged)
        });
        let merged = fenced(written, "the steps registry")?;
        memory.records = merged.clone();
        memory.bases = merged;
        memory.changed.clear();
        Ok(())
    }
}

/// The record of a step just launched, as the prototype's `sospese.nuovo_record` writes it, and its key, as
/// its key of a step: the session to resume, else the session, else the second of the launch.
#[must_use]
pub fn new_record(step: &LaunchedStep<'_>, at: f64) -> (String, Value) {
    let (w, r) = (&words().registry.record, step.receipt);
    let own = r.adapter == words().adapters.own_sessions;
    let resume = if own && (r.transcript.is_none() || r.session == r.surface_session) { None } else { r.session.clone() };
    let mut record = Map::new();
    record.insert(w.project.clone(), json!(step.project));
    record.insert(w.folder.clone(), json!(step.folder));
    record.insert(w.origin.clone(), json!(words().registry.origins.controller));
    record.insert(w.adapter.clone(), json!(r.adapter));
    record.insert(w.agent.clone(), json!(r.agent));
    record.insert(w.model.clone(), json!(r.model));
    record.insert(w.effort.clone(), json!(r.effort));
    record.insert(w.session.clone(), json!(r.session));
    record.insert(w.resume.clone(), json!(resume));
    record.insert(w.surface_session.clone(), json!(r.surface_session));
    record.insert(w.transcript.clone(), json!(r.transcript));
    record.insert(w.pid.clone(), json!(r.pid));
    record.insert(w.log.clone(), json!(r.log));
    record.insert(w.marker.clone(), json!(if own { r.marker.clone() } else { None }));
    record.insert(w.started_t.clone(), json!(at));
    record.insert(w.started.clone(), json!(iso(at)));
    record.insert(w.state.clone(), json!(words().registry.states.launched));
    record.insert(w.updated_t.clone(), json!(at));
    record.insert(w.resumes.clone(), json!([]));
    let given = |id: &Option<String>| id.clone().filter(|id| !id.is_empty());
    #[allow(clippy::cast_possible_truncation)]
    let id = given(&resume).or_else(|| given(&r.session)).unwrap_or_else(|| format!("t{}", at.trunc() as i64));
    (format!("{}/{id}", step.project), Value::Object(record))
}

impl Steps for StoreSteps {
    fn launched(&self, step: &LaunchedStep<'_>, at: f64) -> Result<String, StateError> {
        let (key, record) = new_record(step, at);
        let mut memory = self.memory()?;
        memory.records.insert(key.clone(), record);
        memory.changed.insert(key.clone());
        self.save(&mut memory, at)?;
        Ok(key)
    }

    fn finished(&self, key: &str, at: f64) -> Result<(), StateError> {
        let r = &words().registry;
        let mut memory = self.memory()?;
        let Some(Value::Object(record)) = memory.records.get_mut(key) else { return Ok(()) };
        if record.get(&r.record.state).and_then(Value::as_str) != Some(r.states.launched.as_str()) {
            return Ok(());
        }
        record.insert(r.record.state.clone(), json!(r.states.finished));
        record.insert(r.record.updated_t.clone(), json!(at));
        memory.changed.insert(key.to_owned());
        self.save(&mut memory, at)
    }

    fn suspended_projects(&self) -> BTreeSet<String> {
        let r = &words().registry;
        let busy = [r.states.suspended.as_str(), r.states.resuming.as_str()];
        StoreSteps::records(self)
            .values()
            .filter(|record| record.get(&r.record.state).and_then(Value::as_str).is_some_and(|state| busy.contains(&state)))
            .filter_map(|record| record.get(&r.record.project).and_then(Value::as_str).map(str::to_owned))
            .collect()
    }

    fn records(&self) -> Map<String, Value> {
        StoreSteps::records(self)
    }

    /// Each record over the one of its key, then the save record by record with its version check: a record another
    /// writer changed meanwhile stays as stored. With none, the save still applies what is kept.
    fn save_records(&self, records: Map<String, Value>, at: f64) -> Result<(), StateError> {
        let mut memory = self.memory()?;
        for (key, record) in records {
            memory.records.insert(key.clone(), record);
            memory.changed.insert(key);
        }
        self.save(&mut memory, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_random_id_has_eight_hexadecimal_digits_and_repeats_not() {
        let (a, b) = (random_id(), random_id());
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn no_pid_is_alive_that_is_not_a_process() {
        assert!(!process_alive(0));
        assert!(process_alive(std::process::id()), "this process lives");
    }
}
