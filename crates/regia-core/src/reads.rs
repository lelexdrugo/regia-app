//! `task list` and `task show`: the tracker's issues as the agents read them (§3.1 and §3.2 of the
//! dossier's conformance study). Pure functions of the issues read.

use serde::Serialize;

use crate::comment::Comment;
use crate::graph::{GraphOrder, Metrics, graph_order};
use crate::model::{Status, Task};

/// The scope and the order of `task list`.
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    /// Only this project; an empty name scopes nothing, as in the prototype.
    pub project: Option<String>,
    /// Only these statuses. An empty list keeps nothing.
    pub statuses: Option<Vec<Status>>,
    pub ready: bool,
    pub waiting_on_owner: bool,
    /// The Objectives too.
    pub objectives: bool,
    /// Longest chain first. The ready ones always come in this order.
    pub by_graph: bool,
}

/// An issue of `task list`: the task and its measures in the graph, which are empty outside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListedTask {
    #[serde(flatten)]
    pub task: Task,
    #[serde(flatten)]
    pub metrics: Metrics,
}

/// The tasks `options` ask for, in the tracker's order or the graph's, with the graph they were measured
/// on. The graph is of every issue read, other projects' too: the scope filters the list, not the graph.
pub fn task_list(tasks: Vec<Task>, options: &ListOptions) -> (Vec<ListedTask>, GraphOrder) {
    let graph = graph_order(&tasks);
    let project = options.project.as_deref().filter(|project| !project.is_empty());
    let mut kept: Vec<Task> = tasks
        .into_iter()
        .filter(|task| project.is_none() || task.project.as_deref() == project)
        .filter(|task| options.statuses.as_ref().is_none_or(|statuses| task.status.is_some_and(|status| statuses.contains(&status))))
        .filter(|task| !options.ready || task.is_ready())
        .filter(|task| !options.waiting_on_owner || task.status.is_some_and(Status::waits_on_owner))
        .filter(|task| options.objectives || !task.objective)
        .collect();
    if options.ready || options.by_graph {
        kept.sort_by_key(|task| graph.sort_key(task.number));
    }
    let listed = kept.into_iter().map(|task| ListedTask { metrics: graph.metrics_of(task.number), task }).collect();
    (listed, graph)
}

/// An issue of `task show`: the task with its body, and its comments when they were asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShownTask {
    #[serde(flatten)]
    pub task: Task,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<Vec<Comment>>,
    /// Only with `--since` (#557): when the body was last edited and whether that is after `--since`.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub body_edit: Option<BodyEdit>,
}

/// The two keys `task show --since` adds: the instant of the last edit of the body, `null` for one never
/// edited, and whether it is after the instant given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BodyEdit {
    pub body_edited_at: Option<String>,
    pub body_edited_after_since: bool,
}
