//! What waits for the owner, what is ready, what is running. A pure function of the tracker's issues and of
//! the day the due dates count from.
//!
//! This slice does not port the sections that read the local machine (controller state, dossier git
//! state, commitments outside the tracker): see ADR 0001, "Conformance".

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;

use crate::due::{DueMark, IgnoredDue, with_due_dates};
use crate::graph::{GraphOrder, Metrics, graph_order};
use crate::model::{DueDate, Status, Task};
use crate::v1::TrackerBinding;

/// The per-project key for tasks with no status.
pub const UNSET_STATUS: &str = "unset";

/// Sections of the contract this implementation does not produce. They mean "nothing to report" when
/// absent, so absence alone would read as an all-clear: the output names them instead.
pub const NOT_PORTED: [&str; 3] = ["controller", "dossier", "outside_tracker"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BriefTask {
    #[serde(flatten)]
    pub task: Task,
    /// Present on the sections ordered by the graph.
    #[serde(flatten)]
    pub metrics: Option<Metrics>,
    #[serde(flatten)]
    pub due: DueMark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphWarnings {
    pub cycles: Vec<Vec<u64>>,
    pub truncated: Vec<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children_waiting: Vec<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Brief {
    pub waiting_on_owner: Vec<BriefTask>,
    pub ready: Vec<BriefTask>,
    pub in_progress: Vec<BriefTask>,
    pub by_project: BTreeMap<String, BTreeMap<String, u64>>,
    /// The tasks of `waiting_on_owner` that need a working session with the owner.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub session: Vec<BriefTask>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphWarnings>,
    /// Due dates that could not be read, of every open task in scope, in or out of the sections.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub due_ignored: Vec<IgnoredDue>,
    pub not_ported: [&'static str; 3],
}

#[derive(Debug, Clone, Default)]
pub struct BriefOptions {
    pub project: Option<String>,
    /// The day due dates count from. `None` is today's date on this machine.
    pub today: Option<NaiveDate>,
}

fn ordered(mut tasks: Vec<Task>, graph: &GraphOrder) -> Vec<BriefTask> {
    tasks.sort_by_key(|task| graph.sort_key(task.number));
    tasks
        .into_iter()
        .map(|task| {
            let metrics = graph.metrics_of(task.number);
            BriefTask { task, metrics: Some(metrics), due: DueMark::default() }
        })
        .collect()
}

/// `tasks` are all the open issues: the scope filters the sections, not the graph.
pub fn build_brief(tasks: Vec<Task>, binding: &TrackerBinding, options: &BriefOptions) -> Brief {
    let today = options.today.unwrap_or_else(|| chrono::Local::now().date_naive());
    let graph = graph_order(&tasks);
    // An empty name scopes nothing and counts nowhere, as in the prototype.
    let scope = options.project.as_deref().filter(|project| !project.is_empty());
    let scoped: Vec<Task> = tasks
        .into_iter()
        .filter(|task| scope.is_none() || task.project.as_deref() == scope)
        .collect();

    let mut by_project: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for task in &scoped {
        if let Some(project) = task.project.as_ref().filter(|project| !project.is_empty()) {
            let counts = by_project.entry(project.clone()).or_default();
            if !task.objective {
                let key = task.status.map_or(UNSET_STATUS, Status::id).to_owned();
                *counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    let mut waiting = Vec::new();
    let mut ready = Vec::new();
    let mut in_progress = Vec::new();
    // A task outside the sections does not change the brief, but a due date it cannot read is listed.
    let mut due_ignored = Vec::new();
    for task in scoped {
        if task.objective {
            continue;
        }
        if task.status.is_some_and(Status::waits_on_owner) {
            waiting.push(task);
        } else if task.is_ready() {
            ready.push(task);
        } else if task.status == Some(Status::InProgress) {
            in_progress.push(BriefTask { task, metrics: None, due: DueMark::default() });
        } else if let Some(DueDate::Malformed(value)) = &task.due {
            due_ignored.push(IgnoredDue { number: task.number, value: value.clone() });
        }
    }
    let mut sections = [ordered(waiting, &graph), ordered(ready, &graph), in_progress];
    for section in &mut sections {
        let (entries, ignored) = with_due_dates(std::mem::take(section), today);
        *section = entries;
        due_ignored.extend(ignored);
    }
    let [waiting_on_owner, ready, in_progress] = sections;
    due_ignored.sort_by_key(|ignored| ignored.number);

    let session = waiting_on_owner
        .iter()
        .filter(|entry| entry.task.labels.iter().any(|l| binding.is_session_label(l)))
        .cloned()
        .collect();
    let warnings = (!graph.cycles.is_empty() || !graph.truncated.is_empty() || !graph.children_waiting.is_empty())
        .then(|| GraphWarnings {
            cycles: graph.cycles.clone(),
            truncated: graph.truncated.clone(),
            children_waiting: graph.children_waiting.clone(),
        });

    Brief {
        waiting_on_owner,
        ready,
        in_progress,
        by_project,
        session,
        graph: warnings,
        due_ignored,
        not_ported: NOT_PORTED,
    }
}
