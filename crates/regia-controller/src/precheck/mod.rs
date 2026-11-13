//! The precheck (P1-P5 of the dossier's conformance study, `tools/precheck.py` of the prototype): before
//! each tick of the controller, what happened in each active project since its last step, and whether a
//! step has work. It reads and decides; it writes nothing: not the tracker, not a cursor, not a lock.
//!
//! A project is active when its Objective is approved, in progress or in decision. For each one, in the
//! order of its priority and then of its id:
//!
//! - **events**: comments of the owner on an issue that waits for the owner, newer than the step's instant
//!   and neither written by a step nor signed by the regia (M2, P4); reports newer than that instant,
//!   except those the step listed as processed with the same date (#406); expired leases of tasks in
//!   progress; ready tasks, as many as the wip has room for; Inbox items created after the instant. A
//!   task closed or reopened is no event of its own: a closed dependency makes its dependents ready, and
//!   a reopened task counts again by its status;
//! - **decision**: the first event names the project's new activity; without one it is idle, because its
//!   lock may be held, its wip is full, work is in progress, it waits for the owner, or nothing happened.
//!
//! Then the **budget's gate**, asked once a tick and only when a project has new activity ([`budget`]):
//! with the budget unknown or full, no project starts; backfill (new Inbox items and the backlog) wants
//! the budget known and under its own lower threshold.
//!
//! The tick's outcome is the first project's new activity, or the strongest reason to stay idle; any
//! fault of a source or of a project fails the whole tick closed, with exit 1. The only thing the precheck
//! writes is its detector's own accounting: the count of the budget's failed readings.
//!
//! Each project also counts its **backlog to formulate** (#211): every Inbox item and every approved draft
//! that can be drained, the last event in priority (`da-formulare`). The **cap of the open Proposals**
//! holds back the Inbox part of it, and a new Inbox item (`proposte-esaurite`).
//!
//! **Not here yet (#466):** the series that finds a stall, the silent workers and the dossier's warnings.
//! Each has its place below, marked, and none of their reasons or keys is produced.

mod args;
mod budget;
mod cursor;
mod line;
mod observe;
mod sources;
mod v1;

use regia_agents::{Platform, ProbeConsent};
use regia_core::{Policy, RawComment, Task};
use regia_store::{ProviderUsage, WallTime};
use serde_json::Value;

pub use args::{Options, parse_options};
pub use budget::BudgetSource;
pub use line::{Json, Outcome, Reason, error_payload, outcome_line, python_repr};
pub use observe::evaluate;
pub use sources::{LiveSources, SETTINGS_DIR_SUFFIX, live_sources};

/// An issue of the tracker as the precheck reads it, resolved; an Objective also carries what its
/// registry says.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub task: Task,
    pub registry: Option<ObjectiveRegistry>,
}

/// The keys of an Objective's registry the precheck reads, as written there: the numbers are read later,
/// where the prototype reads them, so a value that is not one fails at the same point.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectiveRegistry {
    pub wip: Option<String>,
    pub priority: Option<String>,
    /// The project's folder, `None` when the registry names none or an empty one.
    pub folder: Option<String>,
}

/// An expired lease, as the state lists it: the task's number when its key reads as one, and the project
/// it names, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiredLease {
    pub task: Option<u64>,
    pub project: Option<String>,
}

/// A report on disk: its file name, and when it was last written, to the microsecond as Python reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub name: String,
    pub modified: WallTime,
}

/// Where the precheck reads, each read able to fail with a sentence: the tracker (the open issues and
/// the comments of one), the state (a project's events cursor and lock, the expired leases) and the
/// reports of a project's folder. The engine says which class each failure is.
pub trait Sources {
    /// Every open issue, in tracker order.
    ///
    /// # Errors
    ///
    /// When the tracker cannot be read.
    fn issues(&self) -> Result<Vec<Issue>, String>;

    /// The comments of an issue, oldest first: the last hundred, as the prototype reads them.
    ///
    /// # Errors
    ///
    /// When the tracker cannot be read, or has no such issue.
    fn comments(&self, number: u64) -> Result<Vec<RawComment>, String>;

    /// The value of the project's events cursor, `null` without one.
    ///
    /// # Errors
    ///
    /// When the state cannot be read, or the project's id is not one.
    fn events_cursor(&self, project: &str) -> Result<Value, String>;

    /// Whether someone may hold the project's lock: held and not expired, or the state held by another
    /// process past the wait.
    ///
    /// # Errors
    ///
    /// When the state cannot be read.
    fn lock_busy(&self, project: &str) -> Result<bool, String>;

    /// Every expired lease, and every row of a lease that does not read.
    ///
    /// # Errors
    ///
    /// When the state cannot be read.
    fn expired_leases(&self) -> Result<Vec<ExpiredLease>, String>;

    /// The reports in the project's `folder`, none when it has no reports folder.
    ///
    /// # Errors
    ///
    /// When the folder exists and cannot be listed.
    fn reports(&self, folder: &str) -> Result<Vec<Report>, String>;

    /// The budget policy in use: the dossier's file, else the defaults, with the machine's override.
    fn policy(&self) -> Policy;

    /// The user's answer to the usage probes, read only when the budget is asked.
    ///
    /// # Errors
    ///
    /// When the user's settings do not read.
    fn probe_consent(&self) -> Result<ProbeConsent, String>;

    /// The system the probes run on.
    fn platform(&self) -> Platform;

    /// Claude's reading from the cache of the probes, which asks them when its last attempt is old;
    /// `None` when the cache has no reading of Claude.
    ///
    /// # Errors
    ///
    /// When the state cannot be read or written.
    fn claude_usage(&self) -> Result<Option<ProviderUsage>, String>;

    /// The value of the detector's row of `project`, `null` without one.
    ///
    /// # Errors
    ///
    /// When the state cannot be read, or the project's id is not one.
    fn detector(&self, project: &str) -> Result<Value, String>;

    /// Writes the detector's row of `project`: the only write of the precheck.
    ///
    /// # Errors
    ///
    /// When the state cannot be written.
    fn write_detector(&self, project: &str, value: Value) -> Result<(), String>;
}

/// Why the precheck could not decide: the reason of its error, and a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub reason: Reason,
    pub detail: String,
}

impl Fault {
    fn of(reason: Reason, detail: String) -> Fault {
        Fault { reason, detail }
    }

    pub(crate) fn tracker(detail: String) -> Fault {
        Fault::of(Reason::Tracker, detail)
    }

    pub(crate) fn runtime(detail: String) -> Fault {
        Fault::of(Reason::Runtime, detail)
    }

    pub(crate) fn unreadable(detail: String) -> Fault {
        Fault::of(Reason::Unreadable, detail)
    }

    #[must_use]
    pub fn internal(detail: String) -> Fault {
        Fault::of(Reason::Internal, detail)
    }
}

/// What a tick decided: the reason, whose outcome it belongs to, the cursors of the projects observed
/// (`null` for an error), and the payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub reason: Reason,
    pub cursor: Json,
    pub payload: Json,
}

impl Verdict {
    /// The verdict of a fault: an error, with its detail.
    #[must_use]
    pub fn of_fault(fault: &Fault) -> Verdict {
        Verdict { reason: fault.reason, cursor: Json::Null, payload: error_payload(&fault.detail) }
    }

    /// The exit code and the outcome line, at `now`.
    #[must_use]
    pub fn line(&self, now: WallTime) -> (u8, String) {
        outcome_line(self.reason, &self.cursor, &self.payload, &now.to_iso_seconds())
    }
}

/// The first line the command prints, before the outcome line: the format's word, then what the run
/// depends on, for the history of the runs.
#[must_use]
pub fn diagnostic(parts: &[(&str, String)]) -> String {
    let mut line = v1::format().line.diagnostic.clone();
    for (name, value) in parts {
        line.push_str(&format!(" {name}={value}"));
    }
    line
}
