use std::collections::{BTreeSet, HashMap};

use crate::model::IssueState;

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

#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("tracker unavailable: {0}")]
    Unavailable(String),
    #[error("unexpected tracker response: {0}")]
    BadResponse(String),
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

    #[test]
    fn a_parent_outside_the_list_changes_nothing() {
        let mut issues = vec![raw(3, Some(99), &[])];
        add_children_from_parents(&mut issues);
        assert_eq!(issues, vec![raw(3, Some(99), &[])]);
    }
}
