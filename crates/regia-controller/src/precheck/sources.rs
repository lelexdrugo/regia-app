//! The precheck's sources on the machine: the user's `gh` for the tracker, the state database, the
//! reports on disk, the budget policy, the user's consent to the usage probes and their cache. The tick
//! has a deadline, checked before each read of the tracker.

use std::cell::Cell;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use regia_agents::{Env, Https, Platform, ProbeConsent, Settings, UsageProbes};
use regia_core::policy::{file_path, local_file_name, read_local};
use regia_core::{Policy, RawComment, Registry, Tracker, TrackerBinding, TrackerConfigV1, project_label_prefix};
use regia_github::GhTracker;
use regia_identity::{config_dir, env_var, prototype_env_var, state_dir};
use regia_store::{CursorName, LeaseEntry, LockStatus, ManualClock, ProjectId, ProviderUsage, Store, ViewField, WallTime, read_state_file, reports_folder};
use serde_json::Value;

use super::v1::format;
use super::{ExpiredLease, HeldLock, Issue, ObjectiveRegistry, RecentComment, Report, Sources};

/// The variable that names the folder of the user's settings in place of the system's, for the harness:
/// `REGIA_CONFIG_DIR`. The system's folder is not one a variable moves on every system, and the harness
/// must never read the machine's. Provisional, for the gate of names (#223).
pub const SETTINGS_DIR_SUFFIX: &str = "CONFIG_DIR";

/// How often the cache asks the probes, at most: the prototype's `BUDGET_MIN_INTERVALLO_S`.
const PROBE_INTERVAL_S: u64 = regia_store::MIN_INTERVAL_S;

/// How many of an issue's comments the precheck reads: its last ones, as the prototype's query asks.
const LAST_COMMENTS: usize = 100;

/// Below this many seconds before the tick's deadline the tracker is not read: the read fails instead.
const DEADLINE_MARGIN_S: f64 = 1.0;

pub struct LiveSources {
    /// Shared with the thread of a read that has a deadline of its own: the recent comments (#393).
    tracker: Arc<GhTracker>,
    binding: TrackerBinding,
    /// The seconds the reads of the recent comments took in this tick, which share one cap.
    recent_spent: Cell<f64>,
    /// Opened before the tick, failing at its first read, where the prototype's state fails.
    store: Result<Store, String>,
    /// The state folder, where the budget policy's local override is read.
    state: Option<PathBuf>,
    started: Instant,
    deadline_s: f64,
    /// The clock, when the run fixes it.
    fixed: Option<WallTime>,
}

/// The dossier of the prototype, while the prototype exists: a second place for `tracker.json`, and the
/// home of the budget policy's file.
fn dossier_dir() -> Option<PathBuf> {
    std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// The folder of the user's settings: the variable's, else the system's.
fn settings_dir() -> Option<PathBuf> {
    std::env::var_os(env_var(SETTINGS_DIR_SUFFIX)).filter(|value| !value.is_empty()).map(PathBuf::from).or_else(config_dir)
}

/// The sources of a tick that may take `deadline_s` seconds, with the state's clock fixed at `fixed` when
/// one is given.
///
/// # Errors
///
/// When the tracker's configuration cannot be read.
pub fn live_sources(deadline_s: f64, fixed: Option<WallTime>) -> Result<LiveSources, String> {
    live_sources_in(None, deadline_s, fixed)
}

/// As [`live_sources`], on the state folder `folder` when one is given, in place of the one the environment names:
/// a shadow run's copy, whose precheck writes its detector and its budget's cache there and never in the live
/// folder (ADR 0001 §7).
///
/// # Errors
///
/// When the tracker's configuration cannot be read.
pub fn live_sources_in(folder: Option<&Path>, deadline_s: f64, fixed: Option<WallTime>) -> Result<LiveSources, String> {
    let state = folder.map(Path::to_path_buf).or_else(state_dir);
    let config = TrackerConfigV1::load(state.as_deref(), dossier_dir().as_deref(), config_dir().as_deref()).map_err(|error| error.to_string())?;
    let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
    let store = match folder {
        Some(folder) => Store::at(folder),
        None => Store::open(),
    };
    let store = store.map_err(|error| error.to_string()).map(|store| match fixed {
        Some(at) => store.with_clock(ManualClock::fixed(at)),
        None => store,
    });
    Ok(LiveSources {
        tracker: Arc::new(GhTracker::new(&config)),
        binding,
        recent_spent: Cell::new(0.0),
        store,
        state,
        started: Instant::now(),
        deadline_s,
        fixed,
    })
}

/// The seconds a read of the recent comments may take (#393): its own cap, what is left of the tick's cap for all
/// of them, and what the tick's deadline leaves past its margin. Zero or less: no read.
fn recent_budget(left_s: f64, spent_s: f64) -> f64 {
    let words = &format().answers;
    words.recent_s.min(words.recent_total_s - spent_s).min(left_s - words.recent_margin_s)
}

impl LiveSources {
    fn within_deadline(&self) -> Result<(), String> {
        let left = self.deadline_s - self.started.elapsed().as_secs_f64();
        if left <= DEADLINE_MARGIN_S { Err("the tick's deadline ran out".to_owned()) } else { Ok(()) }
    }

    fn store(&self) -> Result<&Store, String> {
        self.store.as_ref().map_err(Clone::clone)
    }

    fn now(&self) -> WallTime {
        self.fixed.unwrap_or_else(WallTime::now)
    }

    fn detector_cursor(project: &str) -> Result<(ProjectId, CursorName), String> {
        let id = ProjectId::new(project).map_err(|error| error.to_string())?;
        let name = CursorName::new(format().detector.cursor.clone()).map_err(|error| error.to_string())?;
        Ok((id, name))
    }
}

/// An instant of the store's clock for the policy's override, which reads its expiry in chrono.
fn utc(at: WallTime) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(at.unix_micros()).unwrap_or_default()
}

/// A lease's task as a number, when it is one: `3.0` is `3`, as Python's `in` finds it.
fn task_number(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value.as_u64().or_else(|| value.as_f64().filter(|number| number.fract() == 0.0 && *number >= 0.0).map(|number| number as u64))
}

/// Whether `name` is a report's: it ends in `.md`, without regard to case where the system's names have
/// none, as the prototype's pattern matches.
fn is_report(name: &str) -> bool {
    if cfg!(windows) { name.to_lowercase().ends_with(".md") } else { name.ends_with(".md") }
}

/// When a file was last written: the float of seconds Python makes of it, rounded to the microsecond as
/// `datetime.fromtimestamp` rounds it, so a date a step recorded reads back equal.
fn modified(path: &Path) -> Result<WallTime, String> {
    let at = std::fs::metadata(path).and_then(|metadata| metadata.modified()).map_err(|error| format!("{}: {error}", path.display()))?;
    let secs = match at.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as f64 + f64::from(after.subsec_nanos()) * 1e-9,
        Err(before) => -before.duration().as_secs_f64(),
    };
    WallTime::from_unix_secs_f64(secs).ok_or_else(|| format!("{}: a date out of range", path.display()))
}

impl Sources for LiveSources {
    fn issues(&self) -> Result<Vec<Issue>, String> {
        self.within_deadline()?;
        let raw = self.tracker.open_issues().map_err(|error| error.to_string())?;
        let mut issues = Vec::with_capacity(raw.len());
        for mut issue in raw {
            let body = std::mem::take(&mut issue.body);
            // An option the configuration does not bind is no status the precheck knows: such an issue is
            // neither active nor an event, as a name the prototype does not know.
            let option = issue.status_option_id.take();
            let number = issue.number;
            let mut task = self.binding.resolve(issue).map_err(|error| error.to_string())?;
            task.status = self.binding.status(number, option.as_deref()).ok().flatten();
            let registry = task.objective.then(|| {
                let registry = Registry::parse(&body);
                ObjectiveRegistry {
                    wip: registry.wip().map(str::to_owned),
                    priority: registry.priority().map(str::to_owned),
                    folder: registry.folder().map(str::to_owned),
                }
            });
            issues.push(Issue { task, registry });
        }
        Ok(issues)
    }

    fn comments(&self, number: u64) -> Result<Vec<RawComment>, String> {
        self.within_deadline()?;
        match self.tracker.comments(number).map_err(|error| error.to_string())? {
            Some(mut comments) => {
                let skip = comments.len().saturating_sub(LAST_COMMENTS);
                comments.drain(..skip);
                Ok(comments)
            }
            None => Err(format!("the tracker has no issue #{number}")),
        }
    }

    /// The prototype's `commenti_recenti` through the read of the checks, which takes every issue updated in the
    /// window and pages through them, kept to those with the project's label: what the prototype asks for, without
    /// its cap of the hundred issues updated last. The read runs in a thread with its deadline; past it, or when it
    /// fails, there is no answer, and the thread is left to end with the process.
    fn recent_comments(&self, project: &str, since: WallTime) -> Option<Vec<RecentComment>> {
        let spent = self.recent_spent.get();
        let budget = recent_budget(self.deadline_s - self.started.elapsed().as_secs_f64(), spent);
        if budget <= 0.0 {
            return None;
        }
        let since = DateTime::<Utc>::from_timestamp_micros(since.unix_micros())?;
        let (tracker, (sender, receiver)) = (Arc::clone(&self.tracker), mpsc::channel());
        let begun = Instant::now();
        thread::spawn(move || {
            let _ = sender.send(tracker.recent_comments(since));
        });
        let answer = receiver.recv_timeout(Duration::from_secs_f64(budget));
        self.recent_spent.set(spent + begun.elapsed().as_secs_f64());
        let issues = answer.ok()?.ok()?;
        let label = format!("{}{project}", project_label_prefix());
        Some(
            issues
                .into_iter()
                .filter(|issue| issue.labels.contains(&label))
                .flat_map(|issue| {
                    let number = issue.number;
                    issue.comments.into_iter().filter_map(move |comment| {
                        Some(RecentComment { number, id: comment.id?, created_at: comment.created_at, body: comment.body })
                    })
                })
                .collect(),
        )
    }

    fn events_cursor(&self, project: &str) -> Result<Value, String> {
        let id = ProjectId::new(project).map_err(|error| error.to_string())?;
        self.store()?.cursor(&id, &CursorName::events()).map_err(|error| error.to_string())
    }

    fn lock(&self, project: &str) -> Result<Option<HeldLock>, String> {
        let id = ProjectId::new(project).map_err(|error| error.to_string())?;
        let field = |view: &regia_store::LockView, field| view.field(field).cloned().unwrap_or(Value::Null);
        Ok(match self.store()?.lock_status(&id).map_err(|error| error.to_string())? {
            LockStatus::Held(view) => Some(HeldLock {
                contended: false,
                holder: field(&view, ViewField::Holder),
                taken_at: field(&view, ViewField::TakenAt),
                renewed_at: field(&view, ViewField::RenewedAt),
                expires_at: field(&view, ViewField::ExpiresAt),
            }),
            LockStatus::Contended => Some(HeldLock { contended: true, ..HeldLock::default() }),
            LockStatus::Free | LockStatus::Expired(_) | LockStatus::Unreadable => None,
        })
    }

    fn signal(&self, project: &str, name: &str) -> Result<Value, String> {
        let id = ProjectId::new(project).map_err(|error| error.to_string())?;
        let name = CursorName::new(name).map_err(|error| error.to_string())?;
        self.store()?.cursor(&id, &name).map_err(|error| error.to_string())
    }

    fn expired_leases(&self) -> Result<Vec<ExpiredLease>, String> {
        let leases = self.store()?.leases(true).map_err(|error| error.to_string())?;
        Ok(leases
            .into_iter()
            .map(|entry| match entry {
                LeaseEntry::Lease(view) => ExpiredLease { task: task_number(view.task()), project: view.project().map(str::to_owned) },
                LeaseEntry::Unreadable { task } => ExpiredLease { task: task_number(Some(&task)), project: None },
            })
            .collect())
    }

    fn reports(&self, folder: &str) -> Result<Vec<Report>, String> {
        let dir = reports_folder(Path::new(folder));
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let entries = std::fs::read_dir(&dir).map_err(|error| format!("reports not readable in {}: {error}", dir.display()))?;
        let mut reports = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| format!("reports not readable in {}: {error}", dir.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_report(&name) {
                reports.push(Report { modified: modified(&entry.path())?, name });
            }
        }
        reports.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(reports)
    }

    /// The prototype's `carica`: the dossier's file, else the defaults, and the override of the state
    /// folder, in the machine's time zone. A state folder that does not resolve has no override.
    fn policy(&self) -> Policy {
        let file = match dossier_dir() {
            Some(dossier) => Policy::from_file(std::fs::read(dossier.join(file_path()))),
            None => Policy::defaults(),
        };
        let Some(folder) = &self.state else { return file };
        file.with_local(read_local(read_state_file(folder, local_file_name()), utc(self.now()), &chrono::Local))
    }

    fn probe_consent(&self) -> Result<ProbeConsent, String> {
        let Some(dir) = settings_dir() else { return Ok(ProbeConsent::Refused) };
        Settings::load(&dir).map(|settings| settings.probe_consent()).map_err(|error| error.to_string())
    }

    fn platform(&self) -> Platform {
        Platform::current()
    }

    fn claude_usage(&self) -> Result<Option<ProviderUsage>, String> {
        let store = self.store()?;
        let probes = UsageProbes::new(self.probe_consent()?, Env::of_this_process(), Https::new());
        let probes = match self.fixed {
            Some(at) => probes.with_clock(ManualClock::fixed(at)),
            None => probes,
        };
        let reading = store.budget(PROBE_INTERVAL_S, false, || Ok::<_, Infallible>(probes.read())).map_err(|error| error.to_string())?;
        Ok(reading.provider(&format().budget.provider).cloned())
    }

    fn detector(&self, project: &str) -> Result<Value, String> {
        let (id, name) = Self::detector_cursor(project)?;
        self.store()?.cursor(&id, &name).map_err(|error| error.to_string())
    }

    fn write_detector(&self, project: &str, value: Value) -> Result<(), String> {
        let (id, name) = Self::detector_cursor(project)?;
        self.store()?.write_cursor(&id, &name, value).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lease_s_task_is_a_whole_number() {
        assert_eq!(task_number(Some(&Value::from(3))), Some(3));
        assert_eq!(task_number(Some(&Value::from(3.0))), Some(3));
        assert_eq!(task_number(Some(&Value::from(3.5))), None);
        assert_eq!(task_number(Some(&Value::from("3"))), None);
        assert_eq!(task_number(None), None);
    }

    /// `test_un_tracker_lento_non_allunga_il_giro` and `test_piu_progetti_lenti_dividono_un_solo_tetto` of the
    /// prototype, in their sums: a read's own cap, the tick's cap shared by every project, the deadline's margin.
    #[test]
    fn the_recent_comments_share_one_cap_and_keep_off_the_deadline() {
        assert!((recent_budget(100.0, 0.0) - 12.0).abs() < 1e-9, "a read's own cap");
        assert!((recent_budget(100.0, 10.0) - 5.0).abs() < 1e-9, "what the tick's cap leaves");
        assert!(recent_budget(100.0, 15.0) <= 0.0, "the tick's cap spent");
        assert!((recent_budget(25.0, 0.0) - 5.0).abs() < 1e-9, "the deadline's margin");
        assert!(recent_budget(20.0, 0.0) <= 0.0, "too close to the deadline");
    }

    #[test]
    fn a_report_is_a_markdown_file() {
        assert!(is_report("2026-01-10-3-x.md"));
        assert!(!is_report("2026-01-10-3-x.txt"));
        assert_eq!(is_report("X.MD"), cfg!(windows), "the system's case rule");
    }
}
