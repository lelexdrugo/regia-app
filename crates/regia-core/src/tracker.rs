use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, SecondsFormat, Utc};

use crate::model::{CloseReason, IssueState};

/// What an adapter reads from the tracker, before labels and option ids get a meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawIssue {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub state_reason: Option<String>,
    pub body: String,
    pub url: String,
    pub updated_at: String,
    pub created_at: Option<String>,
    pub closed_at: Option<String>,
    pub labels: Vec<String>,
    pub parent: Option<u64>,
    pub sub_issues: Vec<(u64, IssueState)>,
    pub blocked_by: Vec<(u64, IssueState)>,
    pub blocked_by_total: u64,
    /// The option id of the Status field in the configured Project, if the issue has one.
    pub status_option_id: Option<String>,
    /// Every single-select value of the configured Project, as (field name, option name), in the order read.
    pub field_values: Vec<(String, String)>,
}

/// A comment of an issue, as the adapter reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawComment {
    /// GitHub's `databaseId`, which a comment's URL ends with.
    pub id: Option<u64>,
    pub created_at: String,
    pub url: String,
    pub body: String,
}

/// An issue, open or closed, updated in the window of the checks, with its last comments (`commenti_recenti`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentComments {
    pub number: u64,
    pub state: IssueState,
    pub closed_at: Option<String>,
    pub labels: Vec<String>,
    /// The last twenty, oldest first.
    pub comments: Vec<RawComment>,
}

/// A closed issue as the checks read it (`issue_chiuse`): its state, why it closed, when, and its URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedIssue {
    pub number: u64,
    pub state: IssueState,
    pub state_reason: Option<String>,
    pub closed_at: Option<String>,
    pub url: Option<String>,
}

/// The changes of an issue's Status in the configured Project, as its timeline records them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusHistory {
    pub number: u64,
    /// The issue's labels: an Objective's changes are not a task's.
    pub labels: Vec<String>,
    /// Oldest first: the name of the option the Status took, `None` when it was emptied, and when, in
    /// ISO 8601.
    pub changes: Vec<(Option<String>, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("tracker unavailable: {0}")]
    Unavailable(String),
    #[error("unexpected tracker response: {0}")]
    BadResponse(String),
    /// The tracker answered with errors: their messages, joined by `; ` (G1 of the dossier's study).
    #[error("the tracker refused: {0}")]
    Refused(String),
}

/// The port to the tracker.
pub trait Tracker {
    /// Every open issue, in tracker order.
    fn open_issues(&self) -> Result<Vec<RawIssue>, TrackerError>;

    /// Every issue, open and closed, in tracker order.
    fn all_issues(&self) -> Result<Vec<RawIssue>, TrackerError> {
        Err(TrackerError::Unavailable("this tracker reads the open issues only".to_owned()))
    }

    /// The comments of an issue, oldest first; `None` if the tracker has no such issue.
    fn comments(&self, number: u64) -> Result<Option<Vec<RawComment>>, TrackerError> {
        Err(TrackerError::Unavailable(format!("this tracker does not read the comments of #{number}")))
    }

    /// The instant of the last edit of an issue's body, in ISO 8601, or `None` inside if the body was never
    /// edited (`Issue.lastEditedAt`, not `updatedAt`, which a comment moves too); `None` outside if the
    /// tracker has no such issue.
    fn body_edited_at(&self, number: u64) -> Result<Option<Option<String>>, TrackerError> {
        Err(TrackerError::Unavailable(format!("this tracker does not read when the body of #{number} was edited")))
    }

    /// The changes of Status since `since` of every issue labelled `label`, open and closed, in tracker
    /// order: where the Proposals of a week are counted. The Status does not move an issue's `updatedAt`.
    fn status_histories(&self, label: &str, since: DateTime<Utc>) -> Result<Vec<StatusHistory>, TrackerError> {
        let since = since.to_rfc3339_opts(SecondsFormat::Secs, true);
        Err(TrackerError::Unavailable(format!("this tracker does not read the changes of Status of `{label}` since {since}")))
    }

    /// The issues, open and closed, updated since `since`, newest first, each with its last twenty comments: what
    /// the checks read of the comments, in one paged read for every project (`commenti_recenti`).
    fn recent_comments(&self, since: DateTime<Utc>) -> Result<Vec<RecentComments>, TrackerError> {
        let since = since.to_rfc3339_opts(SecondsFormat::Secs, true);
        Err(TrackerError::Unavailable(format!("this tracker does not read the comments since {since}")))
    }

    /// Every closed issue, in the light read of the checks (`issue_chiuse`): no children, blockers or fields.
    fn closed_issues(&self) -> Result<Vec<ClosedIssue>, TrackerError> {
        Err(TrackerError::Unavailable("this tracker does not read the closed issues".to_owned()))
    }

    /// The names of an issue's labels: a comment reads its issue's project from them.
    fn labels(&self, number: u64) -> Result<Vec<String>, TrackerError> {
        Err(TrackerError::Unavailable(format!("this tracker does not read the labels of #{number}")))
    }

    /// Writes `body` as a comment on the issue, and returns what the tracker answered: the comment's URL,
    /// whose end is its id.
    fn add_comment(&self, number: u64, body: &str) -> Result<String, TrackerError> {
        let _ = body;
        Err(TrackerError::Unavailable(format!("this tracker does not write comments on #{number}")))
    }

    /// Writes `body` as a comment on the issue, passed as an argument, and reads nothing back: the form a
    /// change of status with a comment uses (G4 of the dossier's conformance study).
    fn add_comment_inline(&self, number: u64, body: &str) -> Result<(), TrackerError> {
        let _ = body;
        Err(not_written(format!("comments on #{number}")))
    }

    /// The node of issue `number`, its id and state, which the writes name it by; `None` if the tracker
    /// has no such issue.
    fn issue_node(&self, number: u64) -> Result<Option<IssueRef>, TrackerError> {
        Err(TrackerError::Unavailable(format!("this tracker does not read the node of #{number}")))
    }

    /// The id of the issue's item in the configured Project. An issue that is not in the Project is
    /// added to it first.
    fn project_item(&self, issue: &str) -> Result<String, TrackerError> {
        Err(not_written(format!("the Project's item of {issue}")))
    }

    /// Sets the status field of an item of the configured Project to the option `option`.
    fn set_status_option(&self, item: &str, option: &str) -> Result<(), TrackerError> {
        Err(not_written(format!("the status of {item} ({option})")))
    }

    fn close_issue(&self, issue: &str, reason: CloseReason) -> Result<(), TrackerError> {
        Err(not_written(format!("the closing of {issue} ({reason:?})")))
    }

    fn reopen_issue(&self, issue: &str) -> Result<(), TrackerError> {
        Err(not_written(format!("the reopening of {issue}")))
    }

    /// Makes `child` a sub-issue of `parent`; with `replace_parent`, leaving the parent it had.
    fn add_sub_issue(&self, parent: &str, child: &str, replace_parent: bool) -> Result<(), TrackerError> {
        let _ = replace_parent;
        Err(not_written(format!("{child} under {parent}")))
    }

    /// Records that `blocker` blocks `issue`.
    fn add_blocked_by(&self, issue: &str, blocker: &str) -> Result<(), TrackerError> {
        Err(not_written(format!("{blocker} blocking {issue}")))
    }

    /// Removes `blocker` from the issues that block `issue`.
    fn remove_blocked_by(&self, issue: &str, blocker: &str) -> Result<(), TrackerError> {
        Err(not_written(format!("the removal of {blocker} blocking {issue}")))
    }

    /// The numbers of the issues that block `number`, every page of them, in the tracker's order; `None`
    /// if the tracker has no such issue.
    fn blocker_numbers(&self, number: u64) -> Result<Option<Vec<u64>>, TrackerError> {
        Err(TrackerError::Unavailable(format!("this tracker does not read the blockers of #{number}")))
    }

    /// How many sub-issues `number` has, closed ones included, as the tracker's cap counts them; `None` if
    /// the tracker has no such issue.
    fn sub_issue_count(&self, number: u64) -> Result<Option<u64>, TrackerError> {
        Err(TrackerError::Unavailable(format!("this tracker does not count the sub-issues of #{number}")))
    }

    /// Creates an issue with its labels, and returns what the tracker printed: the issue's URL, last.
    fn create_issue(&self, title: &str, body: &str, labels: &[String]) -> Result<String, TrackerError> {
        let _ = (body, labels);
        Err(not_written(format!("an issue «{title}»")))
    }

    /// The page of issue `number` on the tracker's site, built, not read.
    fn issue_url(&self, number: u64) -> Option<String> {
        let _ = number;
        None
    }

    /// Adds the label `add` to issue `number`, and takes `remove` off it.
    fn edit_labels(&self, number: u64, add: &str, remove: Option<&str>) -> Result<(), TrackerError> {
        let _ = remove;
        Err(not_written(format!("the label {add} on #{number}")))
    }

    /// Creates the label `name` with its color and description, or updates it in place when it is there (`gh label
    /// create --force`, G3 of the dossier's conformance study).
    fn create_label(&self, name: &str, color: &str, description: &str) -> Result<(), TrackerError> {
        let _ = (color, description);
        Err(not_written(format!("the label {name}")))
    }

    /// Replaces the body of issue `number`.
    fn edit_body(&self, number: u64, body: &str) -> Result<(), TrackerError> {
        let _ = body;
        Err(not_written(format!("the body of #{number}")))
    }

    /// Creates the repo `full`, `<owner>/<name>`, private, with its description (G6): a dossier's repo is never
    /// public, and no other visibility can be asked for.
    fn create_private_repo(&self, full: &str, description: &str) -> Result<(), TrackerError> {
        let _ = description;
        Err(not_written(format!("the repo {full}")))
    }

    /// Adds `topic` to the topics of the repo `full`.
    fn add_repo_topic(&self, full: &str, topic: &str) -> Result<(), TrackerError> {
        Err(not_written(format!("the topic {topic} of {full}")))
    }
}

/// The node of an issue, as the tracker's writes name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    pub id: String,
    pub state: IssueState,
}

fn not_written(what: String) -> TrackerError {
    TrackerError::Unavailable(format!("this tracker does not write {what}"))
}

/// The number of the issue whose URL the tracker printed when it created it: the last segment of the last
/// line, as the prototype reads it (G2 of the dossier's conformance study).
pub fn created_issue_number(printed: &str) -> Option<u64> {
    let last = crate::pystr::strip(printed).lines().last()?;
    let segment = last.trim_end_matches('/').rsplit('/').next()?;
    segment.parse().ok()
}

/// Adds each issue to the sub-issues of its parent when the parent's list lacks it, in list order. The
/// list query reads only the first sub-issues of a parent, and reading the rest can fail: the children
/// read in the list still reach their parent. A child already there is not added twice.
pub fn add_children_from_parents(issues: &mut [RawIssue]) {
    let position: HashMap<u64, usize> = issues.iter().enumerate().map(|(i, issue)| (issue.number, i)).collect();
    let mut known: Vec<BTreeSet<u64>> =
        issues.iter().map(|issue| issue.sub_issues.iter().map(|(number, _)| *number).collect()).collect();
    for i in 0..issues.len() {
        let Some(&parent) = issues[i].parent.as_ref().and_then(|parent| position.get(parent)) else { continue };
        let (child, state) = (issues[i].number, issues[i].state);
        if known[parent].insert(child) {
            issues[parent].sub_issues.push((child, state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(number: u64, parent: Option<u64>, sub: &[u64]) -> RawIssue {
        RawIssue {
            number,
            title: String::new(),
            state: IssueState::Open,
            state_reason: None,
            body: String::new(),
            url: String::new(),
            updated_at: String::new(),
            created_at: None,
            closed_at: None,
            labels: vec![],
            parent,
            sub_issues: sub.iter().map(|&n| (n, IssueState::Closed)).collect(),
            blocked_by: vec![],
            blocked_by_total: 0,
            status_option_id: None,
            field_values: vec![],
        }
    }

    #[test]
    fn a_child_missing_from_its_parent_is_added_once_in_list_order() {
        let mut issues = vec![raw(1, None, &[7]), raw(3, Some(1), &[]), raw(2, Some(1), &[]), raw(7, Some(1), &[])];
        add_children_from_parents(&mut issues);
        let numbers: Vec<u64> = issues[0].sub_issues.iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, [7, 3, 2], "7 was already there, closed, and stays as it was");
        assert_eq!(issues[0].sub_issues[1], (3, IssueState::Open));
    }

    /// G2: the number is the last segment of the last line `gh issue create` printed.
    #[test]
    fn a_created_issue_s_number_is_the_end_of_the_last_line() {
        assert_eq!(created_issue_number("https://tracker.invalid/issues/9\n"), Some(9));
        assert_eq!(created_issue_number("Creating issue in o/r\n\nhttps://tracker.invalid/issues/12/\n"), Some(12));
        assert_eq!(created_issue_number("https://tracker.invalid/issues/x"), None);
        assert_eq!(created_issue_number("  \n"), None);
    }

    #[test]
    fn a_parent_outside_the_list_changes_nothing() {
        let mut issues = vec![raw(3, Some(99), &[])];
        add_children_from_parents(&mut issues);
        assert_eq!(issues, vec![raw(3, Some(99), &[])]);
    }
}
