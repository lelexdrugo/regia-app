//! A tick of the precheck: the registry of the active projects, each project observed, and the decision
//! among them. The order of the reads is the prototype's, so the first fault is the same one.

use std::collections::BTreeSet;

use regia_core::{GraphOrder, IssueState, Status, Task, graph_order, signature_role};
use regia_store::WallTime;

use super::cursor::{self, Since};
use super::line::{Json, Reason};
use super::v1::format;
use super::{Fault, Issue, Options, Sources, Verdict};

/// The statuses of an active project's Objective.
const ACTIVE: [Status; 3] = [Status::Approved, Status::InProgress, Status::Decision];

/// A project's wip and priority when its registry does not give them.
const DEFAULT_WIP: i64 = 2;
const DEFAULT_PRIORITY: i64 = 3;

/// An active project, from its Objective.
struct Project<'a> {
    id: String,
    wip: Option<&'a str>,
    folder: Option<&'a str>,
}

/// What a project's observation found: its reason, the instant of its cursor as the line writes it, and
/// its counts in the order the line writes them.
struct Observation {
    id: String,
    reason: Reason,
    t: Option<String>,
    counts: Vec<(&'static str, i64)>,
    tasks: Vec<u64>,
}

impl Observation {
    /// The number of events: the counts written before the first optional one.
    fn events(&self) -> i64 {
        let words = &format().project;
        let events = [&words.ready, &words.reports, &words.answers, &words.inbox, &words.leases, &words.silent];
        self.counts.iter().filter(|(key, _)| events.iter().any(|event| event.as_str() == *key)).map(|(_, count)| count).sum()
    }

    fn to_json(&self) -> Json {
        let words = &format().project;
        let mut fields = vec![
            (words.id.clone(), Json::Str(self.id.clone())),
            (words.outcome.clone(), Json::Str(self.reason.outcome().word().to_owned())),
            (words.reason.clone(), Json::Str(self.reason.word().to_owned())),
            (words.events.clone(), Json::Int(self.events())),
        ];
        fields.extend(self.counts.iter().map(|(key, count)| ((*key).to_owned(), Json::Int(*count))));
        if !self.tasks.is_empty() {
            let tasks = self.tasks.iter().map(|number| Json::Int(i64::try_from(*number).unwrap_or(i64::MAX))).collect();
            fields.push((words.tasks.clone(), Json::List(tasks)));
        }
        // #466: the backlog's tasks, the silent workers' tasks, the series of a stall, a postponement and
        // the dossier's warnings follow here, each only when it applies.
        Json::Object(fields)
    }
}

/// A number of the registry as Python's `int()` reads it: blanks around it, a sign, `_` between digits.
fn python_int(text: &str) -> Option<i64> {
    let text = regia_core::pystr::strip(text);
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
    if !well_formed {
        return None;
    }
    let value: i64 = digits.replace('_', "").parse().ok()?;
    Some(if negative { -value } else { value })
}

/// A number of the registry, or `default` when it is absent or empty; anything else fails the tick.
fn registry_number(value: Option<&str>, name: &str, default: i64) -> Result<i64, Fault> {
    match value {
        None | Some("") => Ok(default),
        Some(text) => python_int(text).ok_or_else(|| Fault::unreadable(format!("{name} is not a number in the registry: {text:?}"))),
    }
}

/// An Inbox item is an event only when created after the step's instant. Without an instant everything is
/// new, and so is an issue without a creation date: in doubt, one step more.
fn created_after(task: &Task, t: Option<WallTime>) -> Result<bool, Fault> {
    let (Some(t), Some(created)) = (t, task.created_at.as_deref().filter(|created| !created.is_empty())) else { return Ok(true) };
    let created = WallTime::parse_iso(created).ok_or_else(|| Fault::unreadable(format!("not a valid instant: {created:?}")))?;
    Ok(created > t)
}

/// A comment signed by the regia: its marker reads, with a role that is not the owner's. In doubt it is
/// the owner's: with no marker, written on the site, or a marker that does not read, one step more is
/// better than an answer lost.
fn signed_by_the_regia(body: &str) -> bool {
    signature_role(body).is_some_and(|role| role != regia_core::SignatureRole::Owner.word())
}

/// Observes one active project. `issues` are every open issue, of every project.
fn observe(sources: &dyn Sources, project: &Project<'_>, issues: &[Issue], graph: &GraphOrder, leases: &[super::ExpiredLease]) -> Result<Observation, Fault> {
    let wip = registry_number(project.wip, "wip", DEFAULT_WIP)?;
    let mine: Vec<&Task> = issues.iter().map(|issue| &issue.task).filter(|task| task.project.as_deref() == Some(project.id.as_str())).collect();
    let work: Vec<&Task> = mine.iter().copied().filter(|task| !task.objective).collect();

    let value = sources.events_cursor(&project.id).map_err(Fault::runtime)?;
    let Since { t, own_comments } = cursor::since(&value)?;
    let t_text = t.map(WallTime::to_iso_seconds);
    // #466: an Objective in decision tells the stall already reported.

    if sources.lock_busy(&project.id).map_err(Fault::runtime)? {
        // #466: a held lock is counted for a stall, by its digest.
        return Ok(Observation { id: project.id.clone(), reason: Reason::Busy, t: t_text, counts: Vec::new(), tasks: Vec::new() });
    }

    let in_progress: BTreeSet<u64> = work.iter().filter(|task| task.status == Some(Status::InProgress)).map(|task| task.number).collect();
    // Among more ready tasks than free places, those first in the graph's order: longest chain, most
    // unblocked, then number.
    let mut ready_all: Vec<&Task> = work.iter().copied().filter(|task| task.is_ready()).collect();
    ready_all.sort_by_key(|task| graph.sort_key(task.number));
    let free = usize::try_from((wip - i64::try_from(in_progress.len()).unwrap_or(i64::MAX)).max(0)).unwrap_or(usize::MAX);
    let ready: Vec<u64> = ready_all.iter().take(free).map(|task| task.number).collect();
    let mut inbox = 0;
    for task in work.iter().filter(|task| task.status == Some(Status::Inbox)) {
        if created_after(task, t)? {
            inbox += 1;
        }
    }
    let expired = leases
        .iter()
        .filter(|lease| lease.project.as_deref() == Some(project.id.as_str()) && lease.task.is_some_and(|task| in_progress.contains(&task)))
        .count();
    // A report the step listed as processed is not new while its date is the one recorded; rewritten
    // after, it is (#406).
    let processed = cursor::processed_reports(&value)?;
    let reports = match project.folder {
        Some(folder) => sources.reports(folder).map_err(Fault::unreadable)?,
        None => Vec::new(),
    };
    let reports = reports
        .iter()
        .filter(|report| t.is_none_or(|t| report.modified > t) && !processed.get(&report.name).is_some_and(|date| report.modified <= *date))
        .count();
    let waiting: Vec<&Task> = mine.iter().copied().filter(|task| task.status.is_some_and(Status::waits_on_owner)).collect();
    let mut answers = 0;
    for task in &waiting {
        for comment in sources.comments(task.number).map_err(Fault::tracker)? {
            // A comment the step wrote itself, then one the regia signed, is not an answer.
            if comment.id.is_some_and(|id| own_comments.contains(&i128::from(id))) || signed_by_the_regia(&comment.body) {
                continue;
            }
            let new = match t {
                None => true,
                Some(t) => {
                    let created = WallTime::parse_iso(&comment.created_at);
                    created.ok_or_else(|| Fault::unreadable(format!("not a valid instant in a comment: {:?}", comment.created_at)))? > t
                }
            };
            answers += usize::from(new);
        }
    }
    // #466: the silent workers of the tasks in progress are read here, after the comments.

    let words = &format().project;
    let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
    let mut counts: Vec<(&'static str, i64)> = vec![
        (words.ready.as_str(), count(ready.len())),
        (words.reports.as_str(), count(reports)),
        (words.answers.as_str(), count(answers)),
        (words.inbox.as_str(), count(inbox)),
        (words.leases.as_str(), count(expired)),
    ];
    if ready_all.len() > ready.len() {
        counts.push((words.blocked_by_wip.as_str(), count(ready_all.len() - ready.len())));
    }
    // #466: the backlog to formulate is counted here, and the caps of the Proposals hold the Inbox back.
    let by_event = |reason: Reason| match reason {
        Reason::Answers => answers,
        Reason::Reports => reports,
        Reason::Leases => expired,
        Reason::Ready => ready.len(),
        Reason::Inbox => inbox,
        _ => 0,
    };
    let event = Reason::EVENTS_BY_PRIORITY.into_iter().find(|reason| by_event(*reason) > 0);
    let reason = if let Some(event) = event {
        event
    } else if ready_all.len() > ready.len() {
        Reason::WipLimit
    } else if !in_progress.is_empty() {
        // Work in progress and no event is not waiting on the owner; the count says if it also does.
        counts.push((words.in_progress.as_str(), count(in_progress.len())));
        if !waiting.is_empty() {
            counts.push((words.waiting_on_owner.as_str(), count(waiting.len())));
        }
        Reason::WorkInProgress
    } else if !waiting.is_empty() {
        Reason::WaitingOnOwner
    } else {
        Reason::NoEvent
    };
    let tasks = ready.into_iter().take(words.max_tasks).collect();
    Ok(Observation { id: project.id.clone(), reason, t: t_text, counts, tasks })
}

/// The tick: the outcome of every active project, in the order of their priority and then of their ids,
/// and the verdict among them. A fault anywhere is the tick's error.
#[must_use]
pub fn evaluate(sources: &dyn Sources, options: &Options) -> Verdict {
    match tick(sources, options) {
        Ok(verdict) => verdict,
        Err(fault) => Verdict::of_fault(&fault),
    }
}

fn tick(sources: &dyn Sources, options: &Options) -> Result<Verdict, Fault> {
    let issues = sources.issues().map_err(Fault::tracker)?;
    // The registry is in the body of each Objective; one without a project fails the tick, even when it
    // is not active.
    let mut registry = Vec::new();
    for issue in issues.iter().filter(|issue| issue.task.objective) {
        let id = issue.task.project.clone().filter(|id| !id.is_empty());
        let id = id.ok_or_else(|| Fault::unreadable(format!("Objective #{} without a project label", issue.task.number)))?;
        registry.push((id, issue));
    }
    let mut active = Vec::new();
    for (id, issue) in &registry {
        let status = issue.task.status;
        if status.is_some_and(|status| ACTIVE.contains(&status)) && (options.projects.is_empty() || options.projects.contains(id)) {
            let fields = issue.registry.as_ref();
            let priority = registry_number(fields.and_then(|fields| fields.priority.as_deref()), "priority", DEFAULT_PRIORITY)?;
            active.push((priority, Project { id: id.clone(), wip: fields.and_then(|fields| fields.wip.as_deref()), folder: fields.and_then(|fields| fields.folder.as_deref()) }));
        }
    }
    active.sort_by(|(a, left), (b, right)| (a, &left.id).cmp(&(b, &right.id)));
    let known: BTreeSet<&str> = registry.iter().map(|(id, _)| id.as_str()).collect();
    let unknown: BTreeSet<&str> = options.projects.iter().map(String::as_str).filter(|id| !known.contains(id)).collect();
    if !unknown.is_empty() {
        return Err(Fault::internal(format!("unknown project: {}", unknown.into_iter().collect::<Vec<_>>().join(", "))));
    }
    let leases = sources.expired_leases().map_err(Fault::runtime)?;
    // The graph's measures, once a tick, on every open issue of every project.
    let tasks: Vec<Task> = issues.iter().map(|issue| issue.task.clone()).filter(|task| task.state == IssueState::Open).collect();
    let graph = graph_order(&tasks);

    let mut observations = Vec::new();
    for (_, project) in &active {
        observations.push(observe(sources, project, &issues, &graph, &leases)?);
    }
    // #466: the budget's gate on the projects with new activity, then the series of each project and the
    // stall it finds, which would come first.

    let cursor_words = &format().cursor;
    let cursor = observations
        .iter()
        .map(|observation| {
            let value = observation.t.clone().map_or(Json::Null, Json::Str);
            Json::Object(vec![(cursor_words.project.clone(), Json::Str(observation.id.clone())), (cursor_words.value.clone(), value)])
        })
        .collect();
    let projects = observations.iter().map(Observation::to_json).collect();
    // #466: the budget, the policy's source and its local override follow the projects.
    let payload = Json::Object(vec![(format().payload.projects.clone(), Json::List(projects))]);
    let first_new = observations.iter().find(|observation| observation.reason.outcome() == super::Outcome::NewActivity);
    let reason = match first_new {
        Some(observation) => observation.reason,
        None => {
            let reasons: Vec<Reason> = observations.iter().map(|observation| observation.reason).collect();
            Reason::IDLE_BY_PRIORITY.into_iter().find(|reason| reasons.contains(reason)).unwrap_or(Reason::NoEvent)
        }
    };
    Ok(Verdict { reason, cursor: Json::List(cursor), payload })
}
