use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Canonical status ids (ADR 0017). Bound to the option id of the GitHub field, never to its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Inbox,
    Proposal,
    Approved,
    InProgress,
    Decision,
    Done,
    Discarded,
}

impl Status {
    pub fn id(self) -> &'static str {
        match self {
            Status::Inbox => "inbox",
            Status::Proposal => "proposal",
            Status::Approved => "approved",
            Status::InProgress => "in_progress",
            Status::Decision => "decision",
            Status::Done => "done",
            Status::Discarded => "discarded",
        }
    }

    /// The statuses that wait for the owner.
    pub fn waits_on_owner(self) -> bool {
        matches!(self, Status::Proposal | Status::Decision)
    }

    /// Approved work, whether it waits to start, runs, or waits for a decision: what `unblocks` counts.
    pub fn is_approved_work(self) -> bool {
        matches!(self, Status::Approved | Status::InProgress | Status::Decision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Maturity {
    Draft,
    Ready,
}

/// The issue state as GitHub reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum IssueState {
    Open,
    Closed,
}

/// The due date a task's spec declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DueDate {
    Date(NaiveDate),
    /// A value that is not a date: ignored, and reported as it was written.
    Malformed(String),
}

/// An issue of the tracker with its project field and labels resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub state_reason: Option<String>,
    pub status: Option<Status>,
    /// From the project label, the source of truth.
    pub project: Option<String>,
    /// The Project field that mirrors the label, as it is: a drift from the label is reported, not fixed.
    pub project_field: Option<String>,
    pub maturity: Option<Maturity>,
    pub objective: bool,
    pub labels: Vec<String>,
    pub parent: Option<u64>,
    pub sub_issues: Vec<(u64, IssueState)>,
    pub blocked_by: Vec<(u64, IssueState)>,
    pub blocked_by_total: u64,
    pub url: String,
    pub updated_at: String,
    pub created_at: Option<String>,
    pub closed_at: Option<String>,
    /// The brief shows it through its own keys, only where it applies.
    #[serde(skip)]
    pub due: Option<DueDate>,
}

impl Task {
    /// Approved, ready, and with every dependency read and closed.
    pub fn is_ready(&self) -> bool {
        !self.objective
            && self.state == IssueState::Open
            && self.status == Some(Status::Approved)
            && self.maturity == Some(Maturity::Ready)
            && self.blocked_by.iter().all(|(_, state)| *state == IssueState::Closed)
            && self.blocked_by_total <= self.blocked_by.len() as u64
    }
}
