//! The ports of a tick on this machine: the precheck in process, the registry's folders from the tracker,
//! the liveness of a process and the reaping of the sessions this host started, the controller's lock, the
//! postponements and the steps registry in the state database. The prototype's `esegui_precheck`,
//! folders of its registry, `processo_vivo`, `rinnova_lock`, the cursor of `_segna_rinvio` and `sospese.Registro`.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use regia_core::{Registry, Tracker, TrackerBinding, TrackerConfigV1, project_entry};
use regia_github::GhTracker;
use regia_identity::{config_dir, prototype_env_var, state_dir};
use regia_store::{CursorName, Holder, ProjectId, Store, Ttl, WallTime};
use serde_json::{Map, Value, json};

use super::words::format as words;
use super::{ControllerLock, Folders, LaunchedStep, Liveness, LockLost, Precheck, PrecheckRun, Signals, StateError, Steps, iso};
use crate::precheck::{Fault, Verdict, evaluate, live_sources};

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
}

impl Precheck for NativePrecheck {
    fn run(&self, projects: &[String], _timeout_s: f64) -> PrecheckRun {
        let options = crate::precheck::Options { projects: projects.to_vec(), ..crate::precheck::Options::default() };
        let now = self.fixed.unwrap_or_else(WallTime::now);
        let verdict = match live_sources(options.deadline_s, self.fixed) {
            Ok(sources) => evaluate(&sources, &options, now),
            Err(detail) => Verdict::of_fault(&Fault::internal(detail)),
        };
        let (exit, line) = verdict.line(now);
        PrecheckRun::from_output(Some(i32::from(exit)), &line)
    }
}

/// The folder of each project, from the registry of the Objectives in the tracker, as `projects` reads it.
#[derive(Debug, Clone, Copy, Default)]
pub struct TrackerFolders;

impl Folders for TrackerFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        let dossier = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(std::path::PathBuf::from);
        let config = TrackerConfigV1::load(state_dir().as_deref(), dossier.as_deref(), config_dir().as_deref()).map_err(|error| error.to_string())?;
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

/// The postponements in the state database: one cursor with a name per project. The owner of the lock is not
/// checked in the write's transaction yet: #468 does, with the registry's (#582).
#[derive(Debug, Clone)]
pub struct StoreSignals {
    store: Store,
}

impl StoreSignals {
    #[must_use]
    pub fn new(store: Store) -> StoreSignals {
        StoreSignals { store }
    }
}

impl Signals for StoreSignals {
    fn postpone(&self, project: &str, value: Value) -> Result<(), StateError> {
        let failed = |error: String| StateError::Failed(error);
        let project = ProjectId::new(project).map_err(|error| failed(error.to_string()))?;
        let name = CursorName::new(words().postpone.cursor.clone()).map_err(|error| failed(error.to_string()))?;
        self.store.write_cursor(&project, &name, value).map_err(|error| failed(error.to_string()))
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
/// project, every record under its key. It is read once, after the controller took its lock, and written
/// whole at each change, keeping the open records and the finished ones of the last week.
///
/// The writes neither check the lock's owner nor keep the records another controller added since the read:
/// D2 (ADR 0001 §7) and #507 are #468's, which saves record by record with a version check.
#[derive(Debug)]
pub struct StoreSteps {
    store: Store,
    records: Mutex<Map<String, Value>>,
}

impl StoreSteps {
    /// The registry as the state holds it now.
    ///
    /// # Errors
    ///
    /// The state cannot be read.
    pub fn load(store: Store) -> Result<StoreSteps, String> {
        let r = &words().registry;
        let name = CursorName::new(r.cursor.clone()).map_err(|error| error.to_string())?;
        let project = ProjectId::new(words().reserved_project.clone()).map_err(|error| error.to_string())?;
        let doc = store.cursor(&project, &name).map_err(|error| error.to_string())?;
        let records = doc.get(&r.steps).and_then(Value::as_object).cloned().unwrap_or_default();
        Ok(StoreSteps { store, records: Mutex::new(records) })
    }

    /// The records, by key.
    #[must_use]
    pub fn records(&self) -> Map<String, Value> {
        self.records.lock().map(|records| records.clone()).unwrap_or_default()
    }

    fn save(&self, records: &mut Map<String, Value>, at: f64) -> Result<(), StateError> {
        let r = &words().registry;
        let states = &r.states;
        let open = [&states.launched, &states.suspended, &states.resuming, &states.to_resume];
        records.retain(|_, record| {
            let state = record.get(&r.record.state).and_then(Value::as_str);
            let updated = record.get(&r.record.updated_t).and_then(Value::as_f64).filter(|updated| *updated != 0.0).unwrap_or(at);
            state.is_some_and(|state| open.iter().any(|open| open.as_str() == state)) || at - updated < r.keep_finished_s
        });
        let failed = |error: String| StateError::Failed(error);
        let name = CursorName::new(r.cursor.clone()).map_err(|error| failed(error.to_string()))?;
        let doc = json!({r.v.as_str(): 1, r.steps.as_str(): Value::Object(records.clone())});
        self.store.write_cursor(&reserved()?, &name, doc).map_err(|error| failed(error.to_string()))
    }
}

/// The record of a step just launched, as the prototype's `sospese.nuovo_record` writes it, and its key, as
/// its key of a step: the session to resume, else the session, else the second of the launch.
#[must_use]
pub fn new_record(step: &LaunchedStep<'_>, at: f64) -> (String, Value) {
    let (w, r) = (&words().registry.record, step.receipt);
    let orca = r.adapter == words().adapters.orca;
    let resume = if orca && (r.transcript.is_none() || r.session == r.orca_session) { None } else { r.session.clone() };
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
    record.insert(w.orca_session.clone(), json!(r.orca_session));
    record.insert(w.transcript.clone(), json!(r.transcript));
    record.insert(w.pid.clone(), json!(r.pid));
    record.insert(w.log.clone(), json!(r.log));
    record.insert(w.marker.clone(), json!(if orca { r.marker.clone() } else { None }));
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
        let mut records = self.records.lock().map_err(|_| StateError::Failed("the registry's memory is poisoned".to_owned()))?;
        records.insert(key.clone(), record);
        self.save(&mut records, at)?;
        Ok(key)
    }

    fn finished(&self, key: &str, at: f64) -> Result<(), StateError> {
        let r = &words().registry;
        let mut records = self.records.lock().map_err(|_| StateError::Failed("the registry's memory is poisoned".to_owned()))?;
        let Some(Value::Object(record)) = records.get_mut(key) else { return Ok(()) };
        if record.get(&r.record.state).and_then(Value::as_str) != Some(r.states.launched.as_str()) {
            return Ok(());
        }
        record.insert(r.record.state.clone(), json!(r.states.finished));
        record.insert(r.record.updated_t.clone(), json!(at));
        self.save(&mut records, at)
    }

    fn suspended_projects(&self) -> BTreeSet<String> {
        let r = &words().registry;
        let busy = [r.states.suspended.as_str(), r.states.resuming.as_str()];
        self.records()
            .values()
            .filter(|record| record.get(&r.record.state).and_then(Value::as_str).is_some_and(|state| busy.contains(&state)))
            .filter_map(|record| record.get(&r.record.project).and_then(Value::as_str).map(str::to_owned))
            .collect()
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
