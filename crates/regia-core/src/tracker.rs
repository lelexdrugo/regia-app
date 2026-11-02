use crate::model::IssueState;

/// What an adapter reads from the tracker, before labels and option ids get a meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawIssue {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub state_reason: Option<String>,
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
}
