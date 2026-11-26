//! The maintenance of the machine's state (#471, F29 of the dossier's plan; R15 and R16 of its inventory), a
//! scheduled operation with no chat and no session:
//! - the log of declarations compacted by month (`regia_store::compact_declarations`): nothing is deleted from it,
//!   since the moviola's `sync` reads declarations again for as long as Orca lists their sessions;
//! - the tasks' work files removed 30 days after the task closed (`regia_store::clean_work`), the closed tasks read
//!   from the tracker only when there is a task's folder to look at.
//!
//! Its first run, and the run after an error, is a dry run: it counts and changes nothing. A part that fails does not
//! stop the other; the error goes in the operation's entry and in the tick's line.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use regia_core::{Tracker, TrackerConfigV1};
use regia_github::GhTracker;
use regia_identity::{cache_dir, config_dir, prototype_env_var};
use regia_store::{CompactionPolicy, WallTime, WorkPolicy, clean_work, compact_declarations, log_folder, work_folder};
use serde_json::{Value, json};

/// The closed tasks, with when each closed.
pub type ClosedTasks = Arc<dyn Fn() -> Result<BTreeMap<u64, WallTime>, String> + Send + Sync>;

/// What the maintenance works on.
#[derive(Clone)]
pub struct Maintenance {
    declarations: PathBuf,
    work: Option<PathBuf>,
    closed: ClosedTasks,
    always_dry: bool,
}

/// What a run did: each part's counts, and the errors.
#[derive(Debug, Clone, PartialEq)]
pub struct MaintenanceRun {
    pub dry_run: bool,
    pub declarations: Value,
    pub work: Value,
    pub errors: Vec<String>,
}

impl Maintenance {
    /// The log in `declarations`, the work folder `work` (none: no cleaning), the closed tasks from `closed`.
    #[must_use]
    pub fn new(declarations: PathBuf, work: Option<PathBuf>, closed: ClosedTasks) -> Maintenance {
        Maintenance { declarations, work, closed, always_dry: false }
    }

    /// The machine's own: the log under the state folder `state`, the work folder in the product's cache, the closed
    /// tasks from the tracker the state folder's configuration names.
    #[must_use]
    pub fn live(state: &Path) -> Maintenance {
        let folder = state.to_path_buf();
        let closed: ClosedTasks = Arc::new(move || closed_tasks(&folder));
        Maintenance::new(log_folder(state), cache_dir().map(|cache| work_folder(&cache)), closed)
    }

    /// Every run is a dry run: a run that must change nothing, as the shadow's.
    #[must_use]
    pub fn always_dry(mut self) -> Maintenance {
        self.always_dry = true;
        self
    }

    /// One run at `now`; `dry` unless the operation's last run went well.
    #[must_use]
    pub fn run(&self, now: WallTime, dry: bool) -> MaintenanceRun {
        let dry_run = dry || self.always_dry;
        let mut errors = Vec::new();
        let declarations = match compact_declarations(&self.declarations, &CompactionPolicy::at(now, dry_run)) {
            Ok(done) => json!({
                "merged": done.merged, "lines": done.lines, "months": done.months, "kept_recent": done.kept_recent,
                "kept_unclean": done.kept_unclean, "kept_spanning": done.kept_spanning, "kept_order": done.kept_order,
                "kept_changed": done.kept_changed, "recovered": done.recovered,
            }),
            Err(error) => {
                errors.push(error.to_string());
                Value::Null
            }
        };
        let work = match &self.work {
            None => Value::Null,
            Some(root) => match clean_work(root, self.closed.as_ref(), &WorkPolicy::at(now, dry_run)) {
                Ok(done) => json!({
                    "removed": done.removed, "kept_open": done.kept_open, "kept_recent": done.kept_recent,
                    "kept_reparse": done.kept_reparse, "skipped": done.skipped,
                }),
                Err(error) => {
                    errors.push(error.to_string());
                    Value::Null
                }
            },
        };
        MaintenanceRun { dry_run, declarations, work, errors }
    }
}

/// The closed tasks of the tracker, as the checks read them (`closed_issues`): an issue closed without an instant
/// that reads is left out, so its folder stays.
fn closed_tasks(state: &Path) -> Result<BTreeMap<u64, WallTime>, String> {
    let dossier = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let config = TrackerConfigV1::load(Some(state), dossier.as_deref(), config_dir().as_deref()).map_err(|error| error.to_string())?;
    let issues = GhTracker::new(&config).closed_issues().map_err(|error| error.to_string())?;
    Ok(issues.into_iter().filter_map(|issue| Some((issue.number, WallTime::parse_iso(issue.closed_at.as_deref()?)?))).collect())
}
