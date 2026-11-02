//! What waits for the owner, what is ready, what is running. A pure function of the tracker's issues.
//!
//! This slice does not port the sections that read the local machine (controller state, dossier git
//! state, commitments outside the tracker): see ADR 0001, "Conformance".

use std::collections::BTreeMap;

use serde::Serialize;

use crate::graph::{GraphOrder, Metrics, graph_order};
use crate::model::{Status, Task};
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphWarnings {
    pub cycles: Vec<Vec<u64>>,
    pub truncated: Vec<u64>,
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
    pub not_ported: [&'static str; 3],
}

#[derive(Debug, Clone, Default)]
pub struct BriefOptions {
    pub project: Option<String>,
}

fn ordered(mut tasks: Vec<Task>, graph: &GraphOrder) -> Vec<BriefTask> {
    tasks.sort_by_key(|task| graph.sort_key(task.number));
    tasks
        .into_iter()
        .map(|task| {
            let metrics = graph.metrics_of(task.number);
            BriefTask { task, metrics: Some(metrics) }
        })
        .collect()
}

/// `tasks` are all the open issues: the scope filters the sections, not the graph.
pub fn build_brief(tasks: Vec<Task>, binding: &TrackerBinding, options: &BriefOptions) -> Brief {
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
    for task in scoped {
        if task.objective {
            continue;
        }
        if task.status.is_some_and(Status::waits_on_owner) {
            waiting.push(task);
        } else if task.is_ready() {
            ready.push(task);
        } else if task.status == Some(Status::InProgress) {
            in_progress.push(BriefTask { task, metrics: None });
        }
    }
    let waiting_on_owner = ordered(waiting, &graph);
    let session = waiting_on_owner
        .iter()
        .filter(|entry| entry.task.labels.iter().any(|l| binding.is_session_label(l)))
        .cloned()
        .collect();
    let warnings = (!graph.cycles.is_empty() || !graph.truncated.is_empty()).then(|| GraphWarnings {
        cycles: graph.cycles.clone(),
        truncated: graph.truncated.clone(),
    });

    Brief {
        waiting_on_owner,
        ready: ordered(ready, &graph),
        in_progress,
        by_project,
        session,
        graph: warnings,
        not_ported: NOT_PORTED,
    }
}
