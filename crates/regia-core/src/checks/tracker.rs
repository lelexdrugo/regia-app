//! The checks of the tracker: the comments after a close and the decisions signed by a chat, read from the recent
//! comments, and the issues that wait with no reason the tracker shows (Inbox, Approved, In progress) or whose
//! Project field says another project than their label.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use super::{CheckId, Finding, clean, days_between, instant, why, words};
use crate::model::{IssueState, Status, Task};
use crate::pystr;
use crate::tracker::RawComment;

/// An issue updated in the window of the comments, with its last comments: what the checks read of the tracker's
/// comments. The project is the one its label names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentedIssue {
    pub number: u64,
    pub state: IssueState,
    pub closed_at: Option<String>,
    pub project: Option<String>,
    pub comments: Vec<RawComment>,
}

/// `ruolo_del_commento`: the role of the author's marker on the last line of the body that is not blank, the lines
/// split on `\n` only; `None` without one.
pub fn comment_role(body: &str) -> Option<String> {
    let body = clean(body);
    let last = body.split('\n').map(pystr::strip).rfind(|line| !line.is_empty())?;
    crate::comment::marker_role(last)
}

/// The body without the blank lines and the author's marker at its end, as one line.
fn comment_text(body: &str) -> String {
    let body = clean(body);
    let mut lines: Vec<&str> = body.split('\n').collect();
    while lines.last().is_some_and(|line| pystr::strip(line).is_empty() || crate::comment::marker_role(pystr::strip(line)).is_some()) {
        lines.pop();
    }
    lines.into_iter().map(pystr::strip).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ")
}

fn comment_location(number: u64, comment: &RawComment) -> String {
    let id = comment.id.map_or_else(|| "None".to_owned(), |id| id.to_string());
    why("comment_where", &[("number", &number.to_string()), ("id", &id)])
}

/// `commenti_dopo_chiusura`: the comments made after an issue of the project closed, in the last `days`, whose
/// author is not the regia's own (an orchestrator or a worker): the owner's, a chat's, or with no marker.
pub fn comments_after_close(data: &[CommentedIssue], project: &str, now: DateTime<Utc>, days: u64) -> Vec<Finding> {
    let mut out = Vec::new();
    for issue in data {
        let Some(closed) = instant(issue.closed_at.as_deref()) else { continue };
        #[allow(clippy::cast_precision_loss)]
        let window = days as f64;
        if issue.project.as_deref() != Some(project) || issue.state != IssueState::Closed || days_between(now, closed).is_none_or(|age| age > window) {
            continue;
        }
        for comment in &issue.comments {
            let role = comment_role(&comment.body);
            let own = role.as_ref().is_some_and(|role| words().own_roles.contains(role));
            if instant(Some(&comment.created_at)).is_some_and(|when| when > closed) && !own {
                let who = role.as_ref().map_or_else(|| why("comment_without_marker", &[]), |role| why("comment_of_role", &[("role", role)]));
                let date = closed.format("%Y-%m-%d").to_string();
                let reason = why("comment_after_close", &[("who", &who), ("number", &issue.number.to_string()), ("date", &date)]);
                let finding = Finding::new(CheckId::CommentAfterClose, project, comment_location(issue.number, comment), &comment_text(&comment.body), reason);
                out.push(finding.with_url(Some(&comment.url).filter(|url| !url.is_empty()).map(String::as_str)));
            }
        }
    }
    out
}

/// The comments of the last `days`, on open or closed issues of the project, signed by a chat
/// and beginning with the decision's word: a decision of the owner relayed with no flag, which the controller does
/// not see.
pub fn decision_without_flag(data: &[CommentedIssue], project: &str, now: DateTime<Utc>, days: u64) -> Vec<Finding> {
    let mut out = Vec::new();
    #[allow(clippy::cast_precision_loss)]
    let window = days as f64;
    for issue in data.iter().filter(|issue| issue.project.as_deref() == Some(project)) {
        for comment in &issue.comments {
            let Some(when) = instant(Some(&comment.created_at)) else { continue };
            if days_between(now, when).is_none_or(|age| age > window) || comment_role(&comment.body).as_deref() != Some(words().chat_role.as_str()) {
                continue;
            }
            let body = clean(&comment.body);
            let start = body.trim_start_matches(|c| pystr::is_space(c) || c == '*');
            if start.to_lowercase().starts_with(&words().decision_prefix) {
                let finding = Finding::new(CheckId::DecisionWithoutFlag, project, comment_location(issue.number, comment), &comment_text(&comment.body), why("decision_without_flag", &[]));
                out.push(finding.with_url(Some(&comment.url).filter(|url| !url.is_empty()).map(String::as_str)));
            }
        }
    }
    out
}

/// `_bloccata`: an issue with a blocker still open waits for a reason the tracker shows.
fn blocked(task: &Task) -> bool {
    task.blocked_by.iter().any(|(_, state)| *state == IssueState::Open)
}

/// The task of the project, open, not an Objective, with this status.
fn waiting(task: &Task, project: &str, status: Status) -> bool {
    task.project.as_deref() == Some(project) && !task.objective && task.state == IssueState::Open && task.status == Some(status)
}

/// The days a stale issue has waited, past `days`, as Python's `int()` cuts them.
fn stale_days(now: DateTime<Utc>, since: Option<&str>, days: u64) -> Option<String> {
    let age = days_between(now, instant(since)?)?;
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let (window, whole) = (days as f64, age.trunc() as i64);
    (age > window).then(|| whole.to_string())
}

fn issue_finding(check: CheckId, task: &Task, project: &str, reason: String) -> Finding {
    Finding::new(check, project, format!("#{}", task.number), &task.title, reason).with_url(Some(&task.url).filter(|url| !url.is_empty()).map(String::as_str))
}

/// `inbox_ferme`: in Inbox for more than `days` since its creation, unless it waits for an open blocker.
pub fn inbox_stale(issues: &[Task], project: &str, now: DateTime<Utc>, days: u64) -> Vec<Finding> {
    let mut out = Vec::new();
    for task in issues.iter().filter(|task| waiting(task, project, Status::Inbox) && !blocked(task)) {
        if let Some(age) = stale_days(now, task.created_at.as_deref(), days) {
            out.push(issue_finding(CheckId::InboxStale, task, project, why("inbox_stale", &[("age", &age), ("days", &days.to_string())])));
        }
    }
    out
}

/// `approvati_mai_partiti`: Approved, created more than `days` ago, with no report, no open blocker and no
/// sub-issue (an umbrella's work goes through its children). The third criterion of the prototype's spec, never
/// In progress, needs the history of the statuses, which the wiring does not read: its warning says so.
pub fn approved_never_started(issues: &[Task], project: &str, now: DateTime<Utc>, days: u64, with_report: &BTreeSet<u64>) -> Vec<Finding> {
    let mut out = Vec::new();
    let candidates = issues
        .iter()
        .filter(|task| waiting(task, project, Status::Approved) && !with_report.contains(&task.number) && !blocked(task) && task.sub_issues.is_empty());
    for task in candidates {
        if let Some(age) = stale_days(now, task.created_at.as_deref(), days) {
            let reason = why("approved_never_started", &[("age", &age), ("days", &days.to_string())]);
            out.push(issue_finding(CheckId::ApprovedNeverStarted, task, project, reason));
        }
    }
    out
}

/// In progress with no live lease and the issue not updated for more than `days`: the dispatches
/// of Orca take no lease, so without the second condition every running worker would be a finding.
pub fn in_progress_stalled(issues: &[Task], project: &str, now: DateTime<Utc>, days: u64, live_leases: &BTreeSet<u64>) -> Vec<Finding> {
    let mut out = Vec::new();
    for task in issues.iter().filter(|task| waiting(task, project, Status::InProgress) && !live_leases.contains(&task.number)) {
        if let Some(age) = stale_days(now, Some(&task.updated_at), days) {
            let reason = why("in_progress_stalled", &[("age", &age), ("days", &days.to_string())]);
            out.push(issue_finding(CheckId::InProgressStalled, task, project, reason));
        }
    }
    out
}

/// The issues with the project's label whose Project field is empty or names another project. An
/// issue with no label is no finding: on the board it waits in the field's empty column, to be sorted.
pub fn project_field(issues: &[Task], project: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for task in issues.iter().filter(|task| task.project.as_deref() == Some(project)) {
        let field = task.project_field.as_deref().filter(|field| !field.is_empty());
        if field == Some(project) {
            continue;
        }
        let reason = match field {
            None => why("project_field_empty", &[("project", project)]),
            Some(field) => why("project_field_other", &[("project", project), ("field", field)]),
        };
        out.push(issue_finding(CheckId::ProjectField, task, project, reason + &why("project_field_fix", &[])));
    }
    out
}
