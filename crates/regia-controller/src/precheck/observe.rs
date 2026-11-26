//! A tick of the precheck: the registry of the active projects, each project observed, and the decision
//! among them. The order of the reads is the prototype's, so the first fault is the same one.

use std::collections::{BTreeMap, BTreeSet};

use regia_agents::{WorkerBeat, WorkerThresholds};
use regia_core::{GraphOrder, IssueState, Lever, LeverValue, Local, LocalStatus, Origin, Policy, Status, Task, graph_order, signature_role};
use regia_store::WallTime;

use super::args::python_int;
use super::budget::{self, BudgetSource, Streak};
use super::series;
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
    /// The first numbers of the backlog to formulate, in the graph's order.
    backlog: Vec<u64>,
    /// The tasks whose worker went silent, then blocked, in a new episode (#210, #641): their numbers only, Orca's
    /// ids stay in its adapter.
    silent_tasks: Vec<u64>,
    blocked_tasks: Vec<u64>,
    /// The first tasks of the owner's answers outside Decision and Proposal (#393), by number.
    answer_tasks: Vec<u64>,
    /// Whether the tick counts towards a stall: new activity, or a held lock (#466).
    countable: bool,
    /// The digest of what a countable tick observed.
    digest: Option<String>,
    /// The Objective is in decision: a stall would be reported already.
    objective_in_decision: bool,
    /// The series after this tick.
    accounted: series::Accounted,
}

impl Observation {
    /// The number of events: the counts written before the first optional one, the workers' included.
    fn events(&self) -> i64 {
        let words = &format().project;
        let events = [&words.ready, &words.reports, &words.answers, &words.inbox, &words.leases, &words.silent, &words.blocked];
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
        let numbers = |numbers: &[u64]| Json::List(numbers.iter().map(|number| Json::Int(i64::try_from(*number).unwrap_or(i64::MAX))).collect());
        if !self.tasks.is_empty() {
            fields.push((words.tasks.clone(), numbers(&self.tasks)));
        }
        if !self.backlog.is_empty() {
            fields.push((words.backlog_tasks.clone(), numbers(&self.backlog)));
        }
        // The workers' tasks, then the answers', each only when it applies.
        for (key, list) in [(&words.silent_tasks, &self.silent_tasks), (&words.blocked_tasks, &self.blocked_tasks), (&words.answer_tasks, &self.answer_tasks)] {
            if !list.is_empty() {
                fields.push((key.clone(), numbers(list)));
            }
        }
        if self.accounted.count > 0 {
            fields.push((format().series.key.clone(), Json::Int(self.accounted.count)));
        }
        if self.accounted.postponed {
            fields.push((format().series.postponed.clone(), Json::Bool(true)));
        }
        // #466: the dossier's warnings follow here.
        Json::Object(fields)
    }
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

/// A consultation (#68 of the dossier): a comment whose text starts with its word, after the blanks Python's
/// `lstrip` takes, is context and never wakes, whoever signs it.
fn consultation(body: &str) -> bool {
    body.trim_start_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)).starts_with(&format().answers.consultation)
}

/// An issue that waits for the owner and did not change since the cursor (#589 of the dossier): its `updatedAt`
/// no later than the step's instant less the margin. A new or edited comment moves `updatedAt`, so a stale issue
/// has no answer to read. Without a cursor, without `updatedAt` or with one that does not read it is not stale: in
/// doubt it is read.
fn stale(task: &Task, t: Option<WallTime>) -> bool {
    let (Some(t), Some(updated)) = (t, WallTime::parse_iso(&task.updated_at)) else { return false };
    t.checked_add_secs(-format().answers.stale_margin_s).is_some_and(|limit| updated <= limit)
}

/// Whether an item can be drained from the backlog (#211): not a parent with open sub-issues, not blocked
/// nor with its dependencies read only in part, and not parked. A new Inbox item must be so too to be an
/// event (#393): one born parked or blocked wakes nobody.
fn drainable(task: &Task) -> bool {
    !task.sub_issues.iter().any(|(_, state)| *state == IssueState::Open)
        && !task.blocked_by.iter().any(|(_, state)| *state == IssueState::Open)
        && task.blocked_by_total <= u64::try_from(task.blocked_by.len()).unwrap_or(u64::MAX)
        && !task.labels.iter().any(|label| *label == format().backlog.parked)
}

/// The episodes of the workers of the project's tasks in progress (#210, #641), each a task with the instant its
/// threshold passed: the silent ones, which wait for nobody and are not blocked, then the blocked ones. An episode
/// counts once its instant is past `now` and after the cursor's `t`: the step that looks moves `t`, and the same
/// episode wakes no more; a worker that writes again and stops is another instant. Of several dispatches of a task
/// the most recent counts.
fn episodes(beats: Option<Vec<WorkerBeat>>, wanted: &BTreeSet<u64>, in_progress: &BTreeSet<u64>, now: WallTime, t: Option<WallTime>, thresholds: WorkerThresholds) -> (Vec<(u64, WallTime)>, Vec<(u64, WallTime)>) {
    let mut last: BTreeMap<u64, WorkerBeat> = BTreeMap::new();
    for beat in beats.into_iter().flatten().filter(|beat| wanted.contains(&beat.task)) {
        if last.get(&beat.task).is_none_or(|seen| beat.last > seen.last) {
            last.insert(beat.task, beat);
        }
    }
    let (mut silent, mut blocked) = (Vec::new(), Vec::new());
    for (task, beat) in last.iter().filter(|(task, _)| in_progress.contains(task)) {
        let new = |threshold: f64| beat.last.checked_add_secs(threshold).filter(|at| now > *at && t.is_none_or(|t| *at > t));
        if !beat.waiting && beat.blocked.is_none()
            && let Some(at) = new(thresholds.silence_s)
        {
            silent.push((*task, at));
        }
        if beat.blocked.is_some()
            && let Some(at) = new(thresholds.block_s)
        {
            blocked.push((*task, at));
        }
    }
    (silent, blocked)
}

/// Observes one active project at `now`. `issues` are every open issue, of every project, and `in_progress_all`
/// their tasks in progress, whose workers are read once a tick.
#[allow(clippy::too_many_arguments)]
fn observe(
    sources: &dyn Sources,
    project: &Project<'_>,
    issues: &[Issue],
    in_progress_all: &BTreeSet<u64>,
    graph: &GraphOrder,
    leases: &[super::ExpiredLease],
    policy: &Policy,
    now: WallTime,
) -> Result<Observation, Fault> {
    let wip = registry_number(project.wip, "wip", DEFAULT_WIP)?;
    let mine: Vec<&Task> = issues.iter().map(|issue| &issue.task).filter(|task| task.project.as_deref() == Some(project.id.as_str())).collect();
    let work: Vec<&Task> = mine.iter().copied().filter(|task| !task.objective).collect();

    let value = sources.events_cursor(&project.id).map_err(Fault::runtime)?;
    let Since { t, own_comments } = cursor::since(&value)?;
    let t_text = t.map(WallTime::to_iso_seconds);
    let objective_in_decision = mine.iter().any(|task| task.objective && task.status == Some(Status::Decision));

    if let Some(lock) = sources.lock(&project.id).map_err(Fault::runtime)? {
        // A held lock counts for a stall, by its state and times: renewed, it is another digest.
        let words = &format().series;
        let state = if lock.contended { &words.contended } else { &words.held };
        let facts = [Json::Str(words.lock.clone()), Json::Str(state.clone())]
            .into_iter()
            .chain([&lock.holder, &lock.taken_at, &lock.renewed_at, &lock.expires_at].map(json_of))
            .collect();
        return Ok(Observation {
            id: project.id.clone(),
            reason: Reason::Busy,
            t: t_text,
            counts: Vec::new(),
            tasks: Vec::new(),
            backlog: Vec::new(),
            silent_tasks: Vec::new(),
            blocked_tasks: Vec::new(),
            answer_tasks: Vec::new(),
            countable: true,
            digest: Some(series::digest(&Json::List(facts))),
            objective_in_decision,
            accounted: series::Accounted::default(),
        });
    }

    let in_progress: BTreeSet<u64> = work.iter().filter(|task| task.status == Some(Status::InProgress)).map(|task| task.number).collect();
    // Among more ready tasks than free places, those first in the graph's order: longest chain, most
    // unblocked, then number.
    let mut ready_all: Vec<&Task> = work.iter().copied().filter(|task| task.is_ready()).collect();
    ready_all.sort_by_key(|task| graph.sort_key(task.number));
    let free = usize::try_from((wip - i64::try_from(in_progress.len()).unwrap_or(i64::MAX)).max(0)).unwrap_or(usize::MAX);
    let ready: Vec<u64> = ready_all.iter().take(free).map(|task| task.number).collect();
    let mut inbox_items = Vec::new();
    for task in work.iter().filter(|task| task.status == Some(Status::Inbox)) {
        if created_after(task, t)? && drainable(task) {
            inbox_items.push(task.number);
        }
    }
    let inbox = inbox_items.len();
    let mut expired_tasks: Vec<u64> = leases
        .iter()
        .filter(|lease| lease.project.as_deref() == Some(project.id.as_str()))
        .filter_map(|lease| lease.task.filter(|task| in_progress.contains(task)))
        .collect();
    expired_tasks.sort_unstable();
    let expired = expired_tasks.len();
    // A report the step listed as processed is not new while its date is the one recorded; rewritten
    // after, it is (#406).
    let processed = cursor::processed_reports(&value)?;
    let reports = match project.folder {
        Some(folder) => sources.reports(folder).map_err(Fault::unreadable)?,
        None => Vec::new(),
    };
    let mut new_reports: Vec<&str> = reports
        .iter()
        .filter(|report| t.is_none_or(|t| report.modified > t) && !processed.get(&report.name).is_some_and(|date| report.modified <= *date))
        .map(|report| report.name.as_str())
        .collect();
    new_reports.sort_unstable();
    let reports = new_reports.len();
    if !in_progress.is_empty() {
        // While the comments are read (#210).
        sources.start_workers(in_progress_all, now);
    }
    let waiting: Vec<&Task> = mine.iter().copied().filter(|task| task.status.is_some_and(Status::waits_on_owner)).collect();
    let mut answer_ids: Vec<i64> = Vec::new();
    // The issues whose comments this loop read: a stale one (#589) is left to the read of the recent comments,
    // which goes by `updatedAt` too.
    let mut read: BTreeSet<u64> = BTreeSet::new();
    for task in &waiting {
        if stale(task, t) {
            continue;
        }
        read.insert(task.number);
        for comment in sources.comments(task.number).map_err(Fault::tracker)? {
            // A comment the step wrote itself, then one the regia signed or a consultation, is not an answer.
            if comment.id.is_some_and(|id| own_comments.contains(&i128::from(id))) || signed_by_the_regia(&comment.body) || consultation(&comment.body) {
                continue;
            }
            let new = match t {
                None => true,
                Some(t) => {
                    let created = WallTime::parse_iso(&comment.created_at);
                    created.ok_or_else(|| Fault::unreadable(format!("not a valid instant in a comment: {:?}", comment.created_at)))? > t
                }
            };
            if new {
                answer_ids.push(comment.id.and_then(|id| i64::try_from(id).ok()).unwrap_or(i64::MIN));
            }
        }
    }
    // The owner's answers outside Decision and Proposal (#393): who answers and changes the status at once takes the
    // issue off the list above, and steering on an Objective, an approved task, one in progress or one just closed
    // was never on it. The same rule as above, once a comment, one read a project; an instant that does not read
    // skips its comment, as nothing of this read fails the tick.
    let mut answer_tasks: BTreeSet<u64> = BTreeSet::new();
    if let Some(t) = t {
        for comment in sources.recent_comments(&project.id, t).unwrap_or_default() {
            let id = i64::try_from(comment.id).unwrap_or(i64::MAX);
            if read.contains(&comment.number)
                || own_comments.contains(&i128::from(comment.id))
                || answer_ids.contains(&id)
                || signed_by_the_regia(&comment.body)
                || consultation(&comment.body)
            {
                continue;
            }
            if WallTime::parse_iso(&comment.created_at).is_some_and(|created| created > t) {
                answer_ids.push(id);
                answer_tasks.insert(comment.number);
            }
        }
    }
    answer_ids.sort_unstable();
    let answers = answer_ids.len();
    // After the comments of the tracker: the read of the workers started before them.
    let beats = if in_progress.is_empty() { None } else { sources.workers() };
    let (silent, blocked) = episodes(beats, in_progress_all, &in_progress, now, t, sources.worker_thresholds());

    let words = &format().project;
    let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
    let mut counts: Vec<(&'static str, i64)> = vec![
        (words.ready.as_str(), count(ready.len())),
        (words.reports.as_str(), count(reports)),
        (words.answers.as_str(), count(answers)),
        (words.inbox.as_str(), count(inbox)),
        (words.leases.as_str(), count(expired)),
    ];
    for (key, episodes) in [(&words.silent, &silent), (&words.blocked, &blocked)] {
        if !episodes.is_empty() {
            counts.push((key.as_str(), count(episodes.len())));
        }
    }
    if ready_all.len() > ready.len() {
        counts.push((words.blocked_by_wip.as_str(), count(ready_all.len() - ready.len())));
    }
    // The cap of the open Proposals (#141) holds the Inbox back: the drafts it does not.
    let proposals = work.iter().filter(|task| task.status == Some(Status::Proposal)).count();
    let at_cap = policy.proposal_caps && u64::try_from(proposals).unwrap_or(u64::MAX) >= policy.open_per_project;
    let (backlog, held) = if policy.drain {
        let items: Vec<&Task> = work.iter().copied().filter(|task| drainable(task)).collect();
        let from_inbox: Vec<&Task> = items.iter().copied().filter(|task| task.status == Some(Status::Inbox)).collect();
        let mut chosen: Vec<&Task> = if free > 0 {
            items.iter().copied().filter(|task| task.status == Some(Status::Approved) && task.maturity == Some(regia_core::Maturity::Draft)).collect()
        } else {
            Vec::new()
        };
        if !at_cap {
            chosen.extend(from_inbox.iter().copied());
        }
        chosen.sort_by_key(|task| graph.sort_key(task.number));
        (chosen.iter().map(|task| task.number).collect::<Vec<u64>>(), at_cap && !from_inbox.is_empty())
    } else {
        (Vec::new(), false)
    };
    let by_event = |reason: Reason| match reason {
        Reason::Answers => answers,
        Reason::Reports => reports,
        Reason::Leases => expired,
        // The blocked first, then the silent: one reason.
        Reason::SilentWorkers => blocked.len() + silent.len(),
        Reason::Ready => ready.len(),
        Reason::Inbox if !at_cap => inbox,
        _ => 0,
    };
    let mut event = Reason::EVENTS_BY_PRIORITY.into_iter().find(|reason| by_event(*reason) > 0);
    if !backlog.is_empty() {
        counts.push((words.backlog.as_str(), count(backlog.len())));
        // The last in priority (#211).
        event = event.or(Some(Reason::Backlog));
    }
    let reason = if let Some(event) = event {
        event
    } else if held || (inbox > 0 && at_cap) {
        // Only Inbox items and the Proposals at their cap (#141): backfill does not start.
        counts.push((words.proposals.as_str(), count(proposals)));
        Reason::ProposalsExhausted
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
    let countable = reason.outcome() == super::Outcome::NewActivity;
    // The digest holds the whole backlog: a step that drains one item changes it, and is no stall (#211).
    // The cursor is not in it (W4): a step that only moves the cursor leaves it as it was.
    let digest = countable.then(|| {
        let numbers = |numbers: &[u64]| Json::List(numbers.iter().map(|number| Json::Int(i64::try_from(*number).unwrap_or(i64::MAX))).collect());
        let series = &format().series;
        let mut facts = vec![
            Json::Str(series.events.clone()),
            numbers(&ready),
            Json::List(new_reports.iter().map(|name| Json::Str((*name).to_owned())).collect()),
            Json::List(answer_ids.iter().map(|id| Json::Int(*id)).collect()),
            numbers(&inbox_items),
            numbers(&expired_tasks),
        ];
        // An episode is its task and the instant its threshold passed.
        for (tag, episodes) in [(&series.silent, &silent), (&series.blocked, &blocked)] {
            if !episodes.is_empty() {
                let pairs = episodes.iter().map(|(task, at)| Json::List(vec![Json::Int(i64::try_from(*task).unwrap_or(i64::MAX)), Json::Str(at.to_iso_seconds())]));
                facts.extend([Json::Str(tag.clone()), Json::List(pairs.collect())]);
            }
        }
        if !backlog.is_empty() {
            let mut sorted = backlog.clone();
            sorted.sort_unstable();
            facts.extend([Json::Str(series.backlog.clone()), numbers(&sorted)]);
        }
        series::digest(&Json::List(facts))
    });
    let tasks = ready.into_iter().take(words.max_tasks).collect();
    let backlog = backlog.into_iter().take(words.max_tasks).collect();
    Ok(Observation {
        id: project.id.clone(),
        reason,
        t: t_text,
        counts,
        tasks,
        backlog,
        silent_tasks: silent.iter().map(|(task, _)| *task).collect(),
        blocked_tasks: blocked.iter().map(|(task, _)| *task).collect(),
        answer_tasks: answer_tasks.into_iter().take(words.max_tasks).collect(),
        countable,
        digest,
        objective_in_decision,
        accounted: series::Accounted::default(),
    })
}

/// The policy's source as the prototype names it: `file` or `default`, with `+locale` when an override
/// applies.
fn policy_source(policy: &Policy) -> String {
    let words = &format().policy;
    let origin = match policy.origin {
        Origin::File => &words.file,
        Origin::Default => &words.default,
    };
    if policy.with_local { format!("{origin}{}", words.with_local) } else { origin.clone() }
}

/// A value of the override's file, as it was: data.
fn json_of(value: &serde_json::Value) -> Json {
    match value {
        serde_json::Value::Null => Json::Null,
        serde_json::Value::Bool(flag) => Json::Bool(*flag),
        serde_json::Value::Number(number) => match number.as_i64() {
            Some(whole) if !number.is_f64() => Json::Int(whole),
            _ => Json::Float(number.as_f64().unwrap_or(f64::NAN)),
        },
        serde_json::Value::String(text) => Json::Str(text.clone()),
        serde_json::Value::Array(items) => Json::List(items.iter().map(json_of).collect()),
        serde_json::Value::Object(fields) => Json::Object(fields.iter().map(|(key, value)| (key.clone(), json_of(value))).collect()),
    }
}

/// The local override as the prototype's `leggi_locale` describes it, in its keys and order: its status,
/// then what the file said, then why it was not applied.
fn local_json(local: &Local) -> Json {
    let words = &format().policy;
    let statuses = &words.statuses;
    let status = match local.status {
        LocalStatus::Active => &statuses.active,
        LocalStatus::Expired => &statuses.expired,
        LocalStatus::Unreadable => &statuses.unreadable,
        LocalStatus::Ignored => &statuses.ignored,
    };
    let mut fields = vec![(words.status.clone(), Json::Str(status.clone()))];
    if let Some(until) = &local.until {
        fields.push((words.until.clone(), json_of(until)));
    }
    if let Some(values) = &local.values {
        let levers = &words.levers;
        let values = values
            .iter()
            .map(|(lever, value)| {
                let key = match lever {
                    Lever::AutomaticMaxPct => &levers.automatic_max_pct,
                    Lever::BackfillMaxPct => &levers.backfill_max_pct,
                    Lever::OpenPerProject => &levers.open_per_project,
                    Lever::NewPerWeek => &levers.new_per_week,
                    Lever::ProposalCaps => &levers.proposal_caps,
                    Lever::Backfill => &levers.backfill,
                };
                let value = match value {
                    LeverValue::Pct(pct) => Json::Float(pct),
                    LeverValue::Count(count) => Json::Int(i64::try_from(count).unwrap_or(i64::MAX)),
                    LeverValue::Switch(on) => Json::Bool(on),
                };
                (key.clone(), value)
            })
            .collect();
        fields.push((words.values.clone(), Json::Object(values)));
    }
    for (key, value) in [(&words.note, &local.note), (&words.set_at, &local.set_at)] {
        if let Some(value) = value {
            fields.push((key.clone(), json_of(value)));
        }
    }
    if let Some(reason) = &local.reason {
        fields.push((words.reason.clone(), Json::Str(reason.clone())));
    }
    Json::Object(fields)
}

/// The budget's gate on the projects with new activity, once a tick: unknown or full, none starts; known
/// and under the lower threshold of backfill, backfill starts too. Neither counts towards a stall. The
/// count of the failed readings lives in the detector's row of the project reserved for all of them.
/// `payload.budget` as the gate decided it, `None` when nothing asked for it.
fn budget_gate(sources: &dyn Sources, observations: &mut [Observation], policy: &Policy, max: f64, now: WallTime) -> Result<Option<Json>, Fault> {
    let candidates: Vec<usize> = (0..observations.len()).filter(|at| observations[*at].reason.outcome() == super::Outcome::NewActivity).collect();
    if candidates.is_empty() {
        return Ok(None);
    }
    let consent = sources.probe_consent().map_err(Fault::unreadable)?;
    let source = match BudgetSource::before_reading(consent, sources.platform()) {
        Some(source) => source,
        None => BudgetSource::Read(sources.claude_usage().map_err(Fault::runtime)?.map(Box::new)),
    };
    let check = budget::check(&source, max, now);
    let all = &format().detector.global_project;
    let previous = Streak::from_row(&sources.detector(all).map_err(Fault::runtime)?);
    let streak = Streak::after(previous.as_ref(), &check.kind, &now.to_iso_seconds());
    if streak != previous {
        let reason = match &check.kind {
            budget::Kind::Unknown(reason) => Some(reason.as_str()),
            budget::Kind::Known | budget::Kind::NotProbed => None,
        };
        sources.write_detector(all, Streak::row(streak.as_ref(), reason)).map_err(Fault::runtime)?;
    }
    for at in candidates {
        let observation = &mut observations[at];
        let backfill = matches!(observation.reason, Reason::Inbox | Reason::Backlog);
        if !check.open || (backfill && !check.pct.is_some_and(|pct| pct < policy.backfill_max_pct)) {
            (observation.reason, observation.countable) = (Reason::Budget, false);
        }
    }
    let mut detail = check.detail;
    if let Some(streak) = streak.filter(Streak::alarms) {
        detail.extend(streak.alarm_detail());
    }
    Ok(Some(Json::Object(detail)))
}

/// The tick at `now`: the outcome of every active project, in the order of their priority and then of
/// their ids, and the verdict among them. A fault anywhere is the tick's error.
#[must_use]
pub fn evaluate(sources: &dyn Sources, options: &Options, now: WallTime) -> Verdict {
    match tick(sources, options, now) {
        Ok(verdict) => verdict,
        Err(fault) => Verdict::of_fault(&fault),
    }
}

fn tick(sources: &dyn Sources, options: &Options, now: WallTime) -> Result<Verdict, Fault> {
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
    // The tasks in progress of every project, whose workers are read once a tick.
    let in_progress_all: BTreeSet<u64> =
        issues.iter().map(|issue| &issue.task).filter(|task| !task.objective && task.status == Some(Status::InProgress)).map(|task| task.number).collect();

    let policy = sources.policy();
    let mut observations = Vec::new();
    for (_, project) in &active {
        observations.push(observe(sources, project, &issues, &in_progress_all, &graph, &leases, &policy, now)?);
    }
    let max = options.budget_max.unwrap_or(policy.automatic_max_pct);
    let budget = budget_gate(sources, &mut observations, &policy, max, now)?;
    // Then the series of each project, and the stall it finds, which comes first: it is rare, and nobody
    // would see it otherwise.
    for observation in &mut observations {
        let accounted = series::account(sources, &observation.id, observation.countable, observation.digest.as_deref(), now)?;
        if observation.countable && series::stalled(&accounted, options.stall_n, options.stall_s, now)? {
            let digest = observation.digest.clone().unwrap_or_default();
            let reported = series::reported(sources, &observation.id, observation.objective_in_decision, &digest, options.realert_s, now)?;
            observation.reason = if reported { Reason::WaitingOnOwner } else { Reason::Stall };
        }
        observation.accounted = accounted;
    }

    let cursor_words = &format().cursor;
    let cursor = observations
        .iter()
        .map(|observation| {
            let value = observation.t.clone().map_or(Json::Null, Json::Str);
            Json::Object(vec![(cursor_words.project.clone(), Json::Str(observation.id.clone())), (cursor_words.value.clone(), value)])
        })
        .collect();
    let projects = observations.iter().map(Observation::to_json).collect();
    let payload_words = &format().payload;
    let mut payload = vec![(payload_words.projects.clone(), Json::List(projects))];
    if let Some(budget) = budget {
        payload.push((payload_words.budget.clone(), budget));
    }
    payload.push((payload_words.policy.clone(), Json::Str(policy_source(&policy))));
    if let Some(reason) = policy.reason.as_ref().filter(|reason| !reason.is_empty()) {
        payload.push((payload_words.policy_reason.clone(), Json::Str(reason.clone())));
    }
    if let Some(local) = &policy.local {
        payload.push((payload_words.policy_local.clone(), local_json(local)));
    }
    let payload = Json::Object(payload);
    let first = |outcome| observations.iter().find(|observation| observation.reason.outcome() == outcome);
    let first_new = first(super::Outcome::TerminalState).or_else(|| first(super::Outcome::NewActivity));
    let reason = match first_new {
        Some(observation) => observation.reason,
        None => {
            let reasons: Vec<Reason> = observations.iter().map(|observation| observation.reason).collect();
            Reason::IDLE_BY_PRIORITY.into_iter().find(|reason| reasons.contains(reason)).unwrap_or(Reason::NoEvent)
        }
    };
    Ok(Verdict { reason, cursor: Json::List(cursor), payload })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use regia_agents::{Platform, ProbeConsent, WorkerBeat, WorkerThresholds};
    use regia_core::{Maturity, RawComment, SignatureRole, sign};
    use regia_store::{ProviderUsage, Served, Window};
    use serde_json::{Value, json};

    use super::*;
    use crate::precheck::{ExpiredLease, ObjectiveRegistry, RecentComment, Report};

    /// Claude's reading as the cache gives it: `pct` of the 5-hour window, read `age_s` ago.
    fn usage(pct: f64, age_s: i64) -> ProviderUsage {
        let window = Window::read(pct, Some(("2026-01-10T11:00:00+00:00".to_owned(), 7200)));
        let mut usage = ProviderUsage::new("claude", None, vec![(format().budget.window.clone(), window)], None, None);
        usage.served = Some(Served { age_s: Some(age_s), from_cache: true, update_failed: None });
        usage
    }

    /// Sources that answer from memory, and remember which comments were read and what was written.
    struct Fake {
        issues: Vec<Issue>,
        comments: BTreeMap<u64, Vec<RawComment>>,
        cursors: BTreeMap<String, Value>,
        busy: Vec<String>,
        read: RefCell<Vec<u64>>,
        consent: ProbeConsent,
        platform: Platform,
        usage: Option<ProviderUsage>,
        /// How many times the cache was asked.
        asked: RefCell<usize>,
        detectors: RefCell<BTreeMap<String, Value>>,
        /// The cursors of the controller and of the alerting step, by `<project>:<name>`.
        signals: BTreeMap<String, Value>,
        /// The answer of the read of the recent comments, `None` for no answer; and the projects it was asked for.
        recent: Option<Vec<RecentComment>>,
        recent_asked: RefCell<Vec<String>>,
        /// What Orca's adapter answers, and the reads it was asked for.
        beats: Option<Vec<WorkerBeat>>,
        started: RefCell<Vec<BTreeSet<u64>>>,
    }

    impl Default for Fake {
        fn default() -> Fake {
            Fake {
                recent: Some(Vec::new()),
                recent_asked: RefCell::default(),
                beats: None,
                started: RefCell::default(),
                issues: Vec::new(),
                comments: BTreeMap::new(),
                cursors: BTreeMap::new(),
                busy: Vec::new(),
                read: RefCell::default(),
                consent: ProbeConsent::Granted,
                platform: Platform::Other,
                usage: Some(usage(10.0, 0)),
                asked: RefCell::default(),
                detectors: RefCell::default(),
                signals: BTreeMap::new(),
            }
        }
    }

    impl Sources for Fake {
        fn issues(&self) -> Result<Vec<Issue>, String> {
            Ok(self.issues.clone())
        }

        fn comments(&self, number: u64) -> Result<Vec<RawComment>, String> {
            self.read.borrow_mut().push(number);
            Ok(self.comments.get(&number).cloned().unwrap_or_default())
        }

        fn recent_comments(&self, project: &str, _since: WallTime) -> Option<Vec<RecentComment>> {
            self.recent_asked.borrow_mut().push(project.to_owned());
            self.recent.clone()
        }

        fn worker_thresholds(&self) -> WorkerThresholds {
            WorkerThresholds::from_policy(None)
        }

        fn start_workers(&self, wanted: &BTreeSet<u64>, _now: WallTime) {
            if self.started.borrow().is_empty() {
                self.started.borrow_mut().push(wanted.clone());
            }
        }

        fn workers(&self) -> Option<Vec<WorkerBeat>> {
            if self.started.borrow().is_empty() { None } else { self.beats.clone() }
        }

        fn events_cursor(&self, project: &str) -> Result<Value, String> {
            Ok(self.cursors.get(project).cloned().unwrap_or(Value::Null))
        }

        fn lock(&self, project: &str) -> Result<Option<crate::precheck::HeldLock>, String> {
            Ok(self.busy.iter().any(|busy| busy == project).then(|| crate::precheck::HeldLock { contended: true, ..Default::default() }))
        }

        fn signal(&self, project: &str, name: &str) -> Result<Value, String> {
            Ok(self.signals.get(&format!("{project}:{name}")).cloned().unwrap_or(Value::Null))
        }

        fn expired_leases(&self) -> Result<Vec<ExpiredLease>, String> {
            Ok(Vec::new())
        }

        fn reports(&self, _folder: &str) -> Result<Vec<Report>, String> {
            Ok(Vec::new())
        }

        fn policy(&self) -> Policy {
            Policy::defaults()
        }

        fn probe_consent(&self) -> Result<ProbeConsent, String> {
            Ok(self.consent)
        }

        fn platform(&self) -> Platform {
            self.platform
        }

        fn claude_usage(&self) -> Result<Option<ProviderUsage>, String> {
            *self.asked.borrow_mut() += 1;
            Ok(self.usage.clone())
        }

        fn detector(&self, project: &str) -> Result<Value, String> {
            Ok(self.detectors.borrow().get(project).cloned().unwrap_or(Value::Null))
        }

        fn write_detector(&self, project: &str, value: Value) -> Result<(), String> {
            self.detectors.borrow_mut().insert(project.to_owned(), value);
            Ok(())
        }
    }

    fn task(number: u64, project: &str, status: Status) -> Task {
        Task {
            number,
            title: String::new(),
            state: IssueState::Open,
            state_reason: None,
            status: Some(status),
            project: Some(project.to_owned()),
            project_field: None,
            maturity: Some(Maturity::Ready),
            objective: false,
            labels: Vec::new(),
            parent: None,
            sub_issues: Vec::new(),
            blocked_by: Vec::new(),
            blocked_by_total: 0,
            url: String::new(),
            updated_at: String::new(),
            created_at: None,
            closed_at: None,
            due: None,
        }
    }

    fn issue(task: Task) -> Issue {
        Issue { task, registry: None }
    }

    fn objective(number: u64, project: &str, status: Status, wip: &str, priority: &str) -> Issue {
        let mut task = task(number, project, status);
        (task.objective, task.maturity) = (true, None);
        let registry = ObjectiveRegistry { wip: Some(wip.to_owned()), priority: Some(priority.to_owned()), folder: None };
        Issue { task, registry: Some(registry) }
    }

    fn now() -> WallTime {
        WallTime::parse_iso("2026-01-10T09:00:00Z").unwrap()
    }

    fn line(fake: &Fake) -> Value {
        let (_, line) = evaluate(fake, &Options::default(), now()).line(now());
        serde_json::from_str(&line).unwrap()
    }

    fn project_of(line: &Value, id: &str) -> Value {
        let projects = line[&format().line.keys.payload][&format().payload.projects].as_array().unwrap().clone();
        projects.into_iter().find(|project| project[&format().project.id] == id).unwrap()
    }

    #[test]
    fn the_ready_tasks_go_by_the_graph_s_order_as_many_as_the_wip_has_room_for() {
        // #20 unblocks #21, which unblocks #22: its chain is the longest, so it goes first among the ready.
        let mut blocked = task(21, "p", Status::Approved);
        blocked.blocked_by = vec![(20, IssueState::Open)];
        blocked.blocked_by_total = 1;
        let mut last = task(22, "p", Status::Approved);
        last.blocked_by = vec![(21, IssueState::Open)];
        last.blocked_by_total = 1;
        let issues = vec![
            objective(1, "p", Status::Approved, "2", "3"),
            issue(task(10, "p", Status::Approved)),
            issue(task(20, "p", Status::Approved)),
            issue(blocked),
            issue(last),
            issue(task(30, "p", Status::Approved)),
        ];
        let line = line(&Fake { issues, ..Fake::default() });
        let project = project_of(&line, "p");
        assert_eq!(project[&format().project.tasks], json!([20, 10]), "the chain first, then by number");
        assert_eq!(project[&format().project.blocked_by_wip], 1);
        assert_eq!(line[&format().line.keys.reason], Reason::Ready.word());
    }

    #[test]
    fn a_wip_below_the_work_in_progress_leaves_no_place() {
        let issues = vec![objective(1, "p", Status::Approved, "-1", "3"), issue(task(2, "p", Status::InProgress)), issue(task(3, "p", Status::Approved))];
        let line = line(&Fake { issues, ..Fake::default() });
        assert_eq!(line[&format().line.keys.reason], Reason::WipLimit.word());
    }

    #[test]
    fn an_objective_in_decision_waits_for_the_owner_and_its_comments_are_read() {
        let owner = sign("Decision: B.", SignatureRole::Owner, Some("s"));
        let chat = sign("A question.", SignatureRole::Chat, Some("s"));
        let comment = |id: u64, body: &str| RawComment { id: Some(id), created_at: "2026-01-10T08:30:00Z".to_owned(), url: String::new(), body: body.to_owned() };
        let issues = vec![objective(1, "p", Status::Decision, "2", "3")];
        let cursors = BTreeMap::from([("p".to_owned(), json!({format().events.t.as_str(): "2026-01-10T08:00:00Z"}))]);
        let fake = Fake { issues: issues.clone(), comments: BTreeMap::from([(1, vec![comment(5, &chat)])]), cursors: cursors.clone(), ..Fake::default() };
        assert_eq!(line(&fake)[&format().line.keys.reason], Reason::WaitingOnOwner.word());
        assert_eq!(*fake.read.borrow(), [1]);
        let fake = Fake { issues, comments: BTreeMap::from([(1, vec![comment(5, &chat), comment(6, &owner)])]), cursors, ..Fake::default() };
        let line = line(&fake);
        assert_eq!(line[&format().line.keys.reason], Reason::Answers.word());
        assert_eq!(project_of(&line, "p")[&format().project.answers], 1);
    }

    /// `test_precheck_ferme.py` and `test_precheck_risposte.py` of the prototype (#589, #393, #68): a stale issue
    /// is not read, a consultation never wakes, and the owner's comment outside Decision and Proposal is an answer,
    /// once, with its task; an instant that does not read skips its comment, and without a cursor nothing is asked.
    #[test]
    fn stale_issues_consultations_and_the_answers_outside_the_gates() {
        let at = |minutes: i64| format!("2026-01-10T08:{minutes:02}:00Z");
        let raw = |id: u64, created: &str, body: &str| RawComment { id: Some(id), created_at: created.to_owned(), url: String::new(), body: body.to_owned() };
        let recent = |number: u64, id: u64, created: &str, body: &str| RecentComment { number, id, created_at: created.to_owned(), body: body.to_owned() };
        let waiting = |number: u64, updated: &str| {
            let mut task = task(number, "p", Status::Decision);
            task.updated_at = updated.to_owned();
            issue(task)
        };
        let cursor = BTreeMap::from([("p".to_owned(), json!({format().events.t.as_str(): "2026-01-10T08:00:00Z"}))]);
        // #2 did not change since two minutes before the cursor: not read. #3 moved: read, and its consultation
        // does not wake.
        let issues = vec![objective(1, "p", Status::Approved, "2", "3"), waiting(2, "2026-01-10T07:58:00Z"), waiting(3, &at(30))];
        let fake = Fake {
            issues: issues.clone(),
            comments: BTreeMap::from([(3, vec![raw(30, &at(31), "  Consultazione: and if?")])]),
            cursors: cursor.clone(),
            ..Fake::default()
        };
        let line = line(&fake);
        assert_eq!((line[&format().line.keys.reason].clone(), fake.read.borrow().clone()), (json!(Reason::WaitingOnOwner.word()), vec![3]));
        // The owner's comment on an approved task, on a stale issue and once only for a comment already counted;
        // the regia's, a consultation, one before the cursor and one whose instant does not read are not answers.
        let regia = sign("Done.", SignatureRole::Orchestrator, Some("s"));
        let fake = Fake {
            issues: [issues, vec![issue(task(4, "p", Status::Approved))]].concat(),
            comments: BTreeMap::from([(3, vec![raw(31, &at(31), "Decision: A")])]),
            recent: Some(vec![
                recent(4, 40, &at(10), "Decision: go"),
                recent(4, 40, &at(10), "Decision: go"),
                recent(2, 41, &at(11), "A line"),
                recent(3, 31, &at(31), "Decision: A"),
                recent(4, 42, &at(12), &regia),
                recent(4, 43, &at(13), "Consultazione: maybe"),
                recent(9, 44, "2026-01-10T07:00:00Z", "Too early"),
                recent(9, 45, "yesterday", "Unreadable"),
            ]),
            cursors: cursor,
            ..Fake::default()
        };
        let line = super::tests::line(&fake);
        let project = project_of(&line, "p");
        assert_eq!(line[&format().line.keys.reason], Reason::Answers.word());
        assert_eq!((project[&format().project.answers].clone(), project[&format().project.answer_tasks].clone()), (json!(3), json!([2, 4])));
        assert_eq!(*fake.recent_asked.borrow(), ["p"]);
        // Without a cursor every issue is read, and the recent comments are not asked.
        let fake = Fake { issues: vec![objective(1, "p", Status::Approved, "2", "3"), waiting(2, "2026-01-01T00:00:00Z")], ..Fake::default() };
        let _ = super::tests::line(&fake);
        assert_eq!((fake.read.borrow().clone(), fake.recent_asked.borrow().len()), (vec![2], 0));
    }

    /// `test_precheck_worker.py` and `test_precheck_blocco.py` of the prototype (#210, #641): a worker silent past 30
    /// minutes wakes, once an episode, unless it waits for a person; one blocked past 180 minutes is in its own list and
    /// not among the silent; the events come after the reports and before the ready; Orca is asked once a tick, for
    /// the tasks in progress of every project, and only when a project has some.
    #[test]
    fn the_silent_and_the_blocked_workers_wake_once_an_episode() {
        let ago = |minutes: f64| now().checked_add_secs(-minutes * 60.0).unwrap();
        let beat = |task: u64, minutes: f64, waiting: bool, blocked: Option<regia_agents::Blocked>| WorkerBeat { task, last: ago(minutes), waiting, blocked };
        let issues = vec![
            objective(1, "p", Status::Approved, "5", "3"),
            issue(task(2, "p", Status::InProgress)),
            issue(task(3, "p", Status::InProgress)),
            issue(task(4, "p", Status::InProgress)),
            issue(task(5, "p", Status::InProgress)),
            issue(task(6, "p", Status::Approved)),
            issue(task(9, "q", Status::InProgress)),
        ];
        let beats = vec![
            beat(2, 31.0, false, None),
            beat(3, 29.0, false, None),
            beat(4, 200.0, false, Some(regia_agents::Blocked::OpenCall)),
            beat(5, 40.0, true, None),
            beat(5, 45.0, false, None),
        ];
        let fake = Fake { issues: issues.clone(), beats: Some(beats.clone()), ..Fake::default() };
        let line = line(&fake);
        let project = project_of(&line, "p");
        let words = &format().project;
        assert_eq!(line[&format().line.keys.reason], Reason::SilentWorkers.word(), "before the ready");
        assert_eq!((project[&words.silent].clone(), project[&words.blocked].clone(), project[&words.events].clone()), (json!(1), json!(1), json!(3)));
        assert_eq!((project[&words.silent_tasks].clone(), project[&words.blocked_tasks].clone()), (json!([2]), json!([4])));
        assert_eq!(*fake.started.borrow(), [BTreeSet::from([2, 3, 4, 5, 9])], "every project's tasks in progress, once");
        // The step looked after both thresholds passed: the same episodes wake no more, and the ready task does.
        let cursors = BTreeMap::from([("p".to_owned(), json!({format().events.t.as_str(): ago(0.5).to_iso_seconds()}))]);
        let fake = Fake { issues: issues.clone(), beats: Some(beats), cursors, ..Fake::default() };
        let project = project_of(&super::tests::line(&fake), "p");
        assert_eq!((project[&words.reason].clone(), project.get(&words.silent_tasks)), (json!(Reason::Ready.word()), None));
        // No data is no silence; and without tasks in progress Orca is not asked.
        let fake = Fake { issues: issues.clone(), ..Fake::default() };
        assert_eq!(project_of(&super::tests::line(&fake), "p")[&words.reason], Reason::Ready.word());
        let fake = Fake { issues: vec![objective(1, "p", Status::Approved, "2", "3")], beats: Some(Vec::new()), ..Fake::default() };
        let _ = super::tests::line(&fake);
        assert!(fake.started.borrow().is_empty());
    }

    #[test]
    fn the_tick_takes_the_first_new_activity_or_the_strongest_reason_to_wait() {
        let issues = || {
            vec![
                objective(1, "a", Status::Approved, "2", "1"),
                issue(task(2, "a", Status::Decision)),
                objective(3, "b", Status::Approved, "2", "2"),
                issue(task(4, "b", Status::Approved)),
            ]
        };
        // `a` waits for the owner and `b` is held: held is the stronger reason.
        let line = line(&Fake { issues: issues(), busy: vec!["b".to_owned()], ..Fake::default() });
        assert_eq!(line[&format().line.keys.reason], Reason::Busy.word());
        let ids: Vec<Value> = line[&format().line.keys.cursor].as_array().unwrap().iter().map(|entry| entry[&format().cursor.project].clone()).collect();
        assert_eq!(ids, [json!("a"), json!("b")], "by priority, then id");
        assert_eq!(project_of(&line, "b").as_object().unwrap().len(), 5, "a held project has only its id, outcome, reason, events and series");
        // `b` has a ready task: the tick's new activity, though `a` comes first.
        let fake = Fake { issues: issues(), ..Fake::default() };
        assert_eq!(super::tests::line(&fake)[&format().line.keys.reason], Reason::Ready.word());
    }

    #[test]
    fn a_priority_that_is_not_a_number_fails_only_for_an_active_project() {
        let issues = vec![objective(1, "a", Status::Proposal, "2", "high"), objective(2, "b", Status::Approved, "2", "")];
        assert_eq!(line(&Fake { issues, ..Fake::default() })[&format().line.keys.reason], Reason::NoEvent.word());
        let issues = vec![objective(1, "a", Status::Approved, "2", "high")];
        let verdict = evaluate(&Fake { issues, ..Fake::default() }, &Options::default(), now());
        assert_eq!((verdict.reason, verdict.cursor), (Reason::Unreadable, Json::Null));
    }

    fn project_reason(line: &Value) -> Value {
        project_of(line, "p")[&format().project.reason].clone()
    }

    fn inbox_item(number: u64) -> Issue {
        let mut item = task(number, "p", Status::Inbox);
        item.maturity = None;
        issue(item)
    }

    /// S1 of the dossier's `da-decidere.md`, Acceptance 2: with the probes refused the candidates start,
    /// backfill does not, nothing is read and nothing counts towards an alarm.
    #[test]
    fn with_the_probes_off_the_ready_start_and_backfill_waits_without_an_alarm() {
        let ready = || vec![objective(1, "p", Status::Approved, "2", "3"), issue(task(2, "p", Status::Approved))];
        let fake = Fake { issues: ready(), consent: ProbeConsent::Refused, usage: None, ..Fake::default() };
        let line = line(&fake);
        assert_eq!(line[&format().line.keys.reason], Reason::Ready.word());
        let budget = &line[&format().line.keys.payload][&format().payload.budget];
        assert_eq!(*budget, json!({format().budget.probe.as_str(): format().budget.probes_off.as_str()}));
        assert_eq!(*fake.asked.borrow(), 0, "no cache read");
        assert!(!fake.detectors.borrow().contains_key(&format().detector.global_project), "no count written");
        let backfill = Fake { issues: vec![objective(1, "p", Status::Approved, "2", "3"), inbox_item(2)], consent: ProbeConsent::Refused, ..Fake::default() };
        let line = super::tests::line(&backfill);
        assert_eq!((line[&format().line.keys.reason].clone(), project_reason(&line)), (json!(Reason::Budget.word()), json!(Reason::Budget.word())));
        assert!(line[&format().line.keys.payload][&format().payload.budget].get(&format().budget.alarm).is_none());
    }

    /// #463/V11, decided on #466: on macOS, with the consent, Claude reads unknown by design. It goes as
    /// the refused probes go, without reading the cache, however many ticks in a row.
    #[test]
    fn claude_never_read_on_this_system_goes_as_the_probes_off() {
        let issues = vec![objective(1, "p", Status::Approved, "2", "3"), issue(task(2, "p", Status::Approved))];
        let fake = Fake { issues, platform: Platform::MacOs, usage: None, ..Fake::default() };
        for _ in 0..6 {
            let line = line(&fake);
            assert_eq!(line[&format().line.keys.reason], Reason::Ready.word());
            let budget = &line[&format().line.keys.payload][&format().payload.budget];
            assert_eq!(*budget, json!({format().budget.probe.as_str(): format().budget.never_read.as_str()}));
        }
        assert_eq!(*fake.asked.borrow(), 0);
        assert!(!fake.detectors.borrow().contains_key(&format().detector.global_project));
    }

    /// Change 3 and Acceptance 3: a reading that fails, with the consent given, keeps every step from
    /// starting; from the fourth tick in a row the alarm names the setting. With the probes then turned
    /// off, the candidates start and the count goes.
    #[test]
    fn a_failed_reading_four_ticks_in_a_row_raises_the_alarm_until_the_probes_are_off() {
        let words = &format().budget;
        let issues = vec![objective(1, "p", Status::Approved, "2", "3"), issue(task(2, "p", Status::Approved))];
        let failed = ProviderUsage::unknown("claude", None, "network unreachable", None);
        let mut fake = Fake { issues, usage: Some(failed), ..Fake::default() };
        for round in 1..=5 {
            let line = line(&fake);
            assert_eq!((line[&format().line.keys.reason].clone(), line[&format().line.keys.exit].clone()), (json!(Reason::Budget.word()), json!(3)), "tick {round}");
            let budget = &line[&format().line.keys.payload][&format().payload.budget];
            assert_eq!(budget[&words.pct], Value::Null);
            match budget.get(&words.alarm) {
                Some(Value::String(text)) => {
                    assert!(round >= 4 && text.contains("agents.toml") && text.contains("refused"), "tick {round}: {text}");
                    assert_eq!(budget[&words.rounds], json!(round));
                }
                _ => assert!(round < 4, "tick {round}: no alarm"),
            }
        }
        fake.consent = ProbeConsent::Refused;
        let line = line(&fake);
        assert_eq!(line[&format().line.keys.reason], Reason::Ready.word());
        assert_eq!(Streak::from_row(&fake.detectors.borrow()[&format().detector.global_project]), None, "the count went");
        // A known reading ends the count too, and a tick without candidates leaves it.
        fake.consent = ProbeConsent::Granted;
        fake.usage = Some(ProviderUsage::unknown("claude", None, "timeout", None));
        let _ = super::tests::line(&fake);
        let idle = Fake { issues: vec![objective(1, "p", Status::Approved, "2", "3")], detectors: RefCell::new(fake.detectors.borrow().clone()), ..Fake::default() };
        let _ = super::tests::line(&idle);
        assert_eq!(Streak::from_row(&idle.detectors.borrow()[&format().detector.global_project]).map(|streak| streak.rounds), Some(1));
        fake.usage = Some(usage(80.0, 0));
        let line = super::tests::line(&fake);
        assert_eq!(line[&format().line.keys.reason], Reason::Budget.word(), "full and known");
        assert_eq!(Streak::from_row(&fake.detectors.borrow()[&format().detector.global_project]), None);
    }

    fn at(minutes: f64) -> WallTime {
        now().checked_add_secs(minutes * 60.0).unwrap()
    }

    fn tick(fake: &Fake, minutes: f64) -> Value {
        let (_, line) = evaluate(fake, &Options::default(), at(minutes)).line(at(minutes));
        serde_json::from_str(&line).unwrap()
    }

    /// `TestStallo`: the same work three ticks in a row and an hour is a stall, and a reported one waits
    /// for the owner; a postponement of the controller does not count, once.
    #[test]
    fn the_same_work_three_ticks_and_an_hour_is_a_stall() {
        let issues = vec![objective(1, "p", Status::Approved, "2", "3"), issue(task(2, "p", Status::Approved))];
        let fake = Fake { issues: issues.clone(), ..Fake::default() };
        let reasons: Vec<Value> = [30.0, 60.0, 90.0].iter().map(|minutes| tick(&fake, *minutes)[&format().line.keys.reason].clone()).collect();
        assert_eq!(reasons, [json!(Reason::Ready.word()), json!(Reason::Ready.word()), json!(Reason::Stall.word())]);
        let line = tick(&fake, 120.0);
        assert_eq!(project_of(&line, "p")[&format().series.key], 4);
        // The Objective in decision: reported already, the stall waits for the owner.
        let mut decided = issues.clone();
        decided[0] = objective(1, "p", Status::Decision, "2", "3");
        let fake = Fake { issues: decided, detectors: RefCell::new(fake.detectors.borrow().clone()), ..Fake::default() };
        assert_eq!(tick(&fake, 150.0)[&format().line.keys.reason], Reason::WaitingOnOwner.word());
        // Postponed by the controller after each tick: never a stall.
        let words = &format().series;
        let mut fake = Fake { issues, ..Fake::default() };
        for round in 1..=5 {
            let line = tick(&fake, f64::from(round) * 30.0);
            assert_eq!(project_of(&line, "p")[&words.key], 1, "round {round}");
            assert_eq!(project_of(&line, "p").get(&words.postponed).is_some(), round > 1, "round {round}");
            let signal = json!({words.postponement_t.as_str(): at(f64::from(round) * 30.0).to_iso_seconds(), words.postponement_id.as_str(): format!("r{round}")});
            fake.signals.insert(format!("p:{}", words.postponement), signal);
        }
    }

    /// An idle tick that is not a held lock ends the series; budget held is no stall.
    #[test]
    fn a_tick_that_waits_ends_the_series() {
        let mut fake = Fake { issues: vec![objective(1, "p", Status::Approved, "2", "3")], busy: vec!["p".to_owned()], ..Fake::default() };
        assert_eq!(project_of(&tick(&fake, 0.0), "p")[&format().series.key], 1);
        assert_eq!(project_of(&tick(&fake, 40.0), "p")[&format().series.key], 2);
        fake.busy.clear();
        assert!(project_of(&tick(&fake, 80.0), "p").get(&format().series.key).is_none());
        let row = fake.detectors.borrow()["p"].clone();
        assert_eq!(row, json!({format().detector.version.as_str(): 1, format().series.key.as_str(): null}));
        let full = Fake { issues: vec![objective(1, "p", Status::Approved, "2", "3"), issue(task(2, "p", Status::Approved))], usage: Some(usage(90.0, 0)), ..Fake::default() };
        for round in 0..5 {
            assert_eq!(tick(&full, f64::from(round) * 40.0)[&format().line.keys.reason], Reason::Budget.word());
        }
    }

    /// The prototype's `test_budget` and `TestBudgetDiRiempimento`: the threshold of the automatic work, the
    /// lower one of backfill, and the flag over the policy.
    #[test]
    fn the_thresholds_of_the_automatic_work_and_of_backfill() {
        let ready = || vec![objective(1, "p", Status::Approved, "2", "3"), issue(task(2, "p", Status::Approved))];
        let inbox = || vec![objective(1, "p", Status::Approved, "2", "3"), inbox_item(2)];
        for (issues, pct, reason) in [
            (ready(), 74.9, Reason::Ready),
            (ready(), 75.0, Reason::Budget),
            (inbox(), 49.9, Reason::Inbox),
            (inbox(), 50.0, Reason::Budget),
            (inbox(), 74.9, Reason::Budget),
        ] {
            let line = line(&Fake { issues, usage: Some(usage(pct, 0)), ..Fake::default() });
            assert_eq!(line[&format().line.keys.reason], reason.word(), "{pct}");
        }
        let fake = Fake { issues: ready(), usage: Some(usage(40.0, 0)), ..Fake::default() };
        let options = Options { budget_max: Some(30.0), ..Options::default() };
        let (exit, _) = evaluate(&fake, &options, now()).line(now());
        assert_eq!(exit, 3, "--budget-max over the policy");
    }
}
