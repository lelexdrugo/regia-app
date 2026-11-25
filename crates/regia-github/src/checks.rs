//! The two reads of the checks (#448), with the prototype's queries (`commenti_recenti` and `issue_chiuse`): the
//! issues updated in the window, open and closed, with their last twenty comments, and every closed issue, with no
//! children, blockers or fields. Each is one query, paged a hundred issues at a time.

use chrono::{DateTime, Utc};
use regia_core::{ClosedIssue, IssueState, RawComment, RecentComments, TrackerError};
use serde::Deserialize;
use serde_json::json;

use crate::{GhTracker, PageInfo};

/// The prototype's `COMMENTI_Q`.
pub(crate) const RECENT_COMMENTS_QUERY: &str = r#"query($o:String!,$r:String!,$since:DateTime!,$after:String){repository(owner:$o,name:$r){
  issues(first:100,after:$after,filterBy:{since:$since},orderBy:{field:UPDATED_AT,direction:DESC}){
    pageInfo{hasNextPage endCursor}
    nodes{number state closedAt labels(first:20){nodes{name}}
      comments(last:20){nodes{databaseId createdAt body url}}}}}}"#;

/// The prototype's `CHIUSE_Q`.
pub(crate) const CLOSED_QUERY: &str = r#"query($o:String!,$r:String!,$after:String){repository(owner:$o,name:$r){
  issues(first:100,after:$after,states:[CLOSED]){
    pageInfo{hasNextPage endCursor}
    nodes{number state stateReason closedAt url}}}}"#;

#[derive(Deserialize)]
pub(crate) struct Data<T> {
    repository: Repository<T>,
}

#[derive(Deserialize)]
struct Repository<T> {
    issues: Page<T>,
}

#[derive(Deserialize)]
struct Page<T> {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentNode {
    number: u64,
    state: IssueState,
    closed_at: Option<String>,
    labels: Names,
    comments: Comments,
}

#[derive(Deserialize)]
struct Names {
    nodes: Vec<Name>,
}

#[derive(Deserialize)]
struct Name {
    name: String,
}

#[derive(Deserialize)]
struct Comments {
    nodes: Vec<CommentNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentNode {
    database_id: Option<u64>,
    created_at: String,
    body: Option<String>,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClosedNode {
    number: u64,
    state: IssueState,
    state_reason: Option<String>,
    closed_at: Option<String>,
    url: Option<String>,
}

/// Every page of a query, from the first: `fetch` reads the page after a cursor.
fn pages<T>(mut fetch: impl FnMut(Option<&str>) -> Result<Data<T>, TrackerError>) -> Result<Vec<T>, TrackerError> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = fetch(after.as_deref())?.repository.issues;
        out.extend(page.nodes);
        if !page.page_info.has_next_page {
            return Ok(out);
        }
        after = page.page_info.end_cursor;
    }
}

pub(crate) fn recent(fetch: impl FnMut(Option<&str>) -> Result<Data<RecentNode>, TrackerError>) -> Result<Vec<RecentComments>, TrackerError> {
    let nodes = pages(fetch)?;
    Ok(nodes
        .into_iter()
        .map(|node| RecentComments {
            number: node.number,
            state: node.state,
            closed_at: node.closed_at,
            labels: node.labels.nodes.into_iter().map(|label| label.name).collect(),
            comments: node
                .comments
                .nodes
                .into_iter()
                .map(|c| RawComment { id: c.database_id, created_at: c.created_at, url: c.url, body: c.body.unwrap_or_default() })
                .collect(),
        })
        .collect())
}

pub(crate) fn closed(fetch: impl FnMut(Option<&str>) -> Result<Data<ClosedNode>, TrackerError>) -> Result<Vec<ClosedIssue>, TrackerError> {
    let nodes = pages(fetch)?;
    Ok(nodes
        .into_iter()
        .map(|node| ClosedIssue { number: node.number, state: node.state, state_reason: node.state_reason, closed_at: node.closed_at, url: node.url })
        .collect())
}

impl GhTracker {
    pub(crate) fn read_recent_comments(&self, since: DateTime<Utc>) -> Result<Vec<RecentComments>, TrackerError> {
        // The window's start as the prototype writes it: seconds, in UTC.
        let since = json!(since.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        recent(|after| self.graphql_with(RECENT_COMMENTS_QUERY, &[("since", since.clone())], after))
    }

    pub(crate) fn read_closed_issues(&self) -> Result<Vec<ClosedIssue>, TrackerError> {
        closed(|after| self.graphql_with(CLOSED_QUERY, &[], after))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn page<T: for<'de> Deserialize<'de>>(nodes: Value, next: Option<&str>) -> Data<T> {
        serde_json::from_value(json!({"repository": {"issues": {"pageInfo": {"hasNextPage": next.is_some(), "endCursor": next}, "nodes": nodes}}})).unwrap()
    }

    /// Two pages, read with the cursor of the first: the issues in the order given, their labels and comments.
    #[test]
    fn the_recent_comments_are_read_a_page_at_a_time() {
        let first = json!([{"number": 1, "state": "CLOSED", "closedAt": "2026-01-02T00:00:00Z", "labels": {"nodes": [{"name": "project:p"}]},
            "comments": {"nodes": [{"databaseId": 10, "createdAt": "2026-01-03T00:00:00Z", "body": "x", "url": "u10"}]}}]);
        let second = json!([{"number": 2, "state": "OPEN", "closedAt": null, "labels": {"nodes": []},
            "comments": {"nodes": [{"databaseId": null, "createdAt": "2026-01-04T00:00:00Z", "body": null, "url": "u"}]}}]);
        let mut asked = Vec::new();
        let read = recent(|after| {
            asked.push(after.map(str::to_owned));
            Ok(if after.is_none() { page(first.clone(), Some("c1")) } else { page(second.clone(), None) })
        })
        .unwrap();
        assert_eq!(asked, [None, Some("c1".to_owned())]);
        assert_eq!(read.iter().map(|issue| issue.number).collect::<Vec<_>>(), [1, 2]);
        assert_eq!((read[0].state, read[0].closed_at.as_deref(), read[0].labels.as_slice()), (IssueState::Closed, Some("2026-01-02T00:00:00Z"), &["project:p".to_owned()][..]));
        assert_eq!((read[0].comments[0].id, read[1].comments[0].id, read[1].comments[0].body.as_str()), (Some(10), None, ""));
    }

    #[test]
    fn the_closed_issues_keep_their_reason_and_a_failure_is_the_tracker_s() {
        let nodes = json!([{"number": 5, "state": "CLOSED", "stateReason": "NOT_PLANNED", "closedAt": "2026-01-02T00:00:00Z", "url": "u5"}]);
        let read = closed(|_| Ok(page(nodes.clone(), None))).unwrap();
        assert_eq!(read, [ClosedIssue { number: 5, state: IssueState::Closed, state_reason: Some("NOT_PLANNED".to_owned()), closed_at: Some("2026-01-02T00:00:00Z".to_owned()), url: Some("u5".to_owned()) }]);
        let failed = closed(|_| Err(TrackerError::Unavailable("gh: no network".to_owned())));
        assert!(matches!(failed, Err(TrackerError::Unavailable(reason)) if reason.contains("no network")));
    }

    /// The queries are the prototype's: the window of the comments, their order, the twenty comments, the light read
    /// of the closed issues.
    #[test]
    fn the_queries_are_the_prototype_s() {
        for part in ["filterBy:{since:$since}", "orderBy:{field:UPDATED_AT,direction:DESC}", "comments(last:20)", "labels(first:20)"] {
            assert!(RECENT_COMMENTS_QUERY.contains(part), "{part}");
        }
        assert!(CLOSED_QUERY.contains("states:[CLOSED]") && CLOSED_QUERY.contains("stateReason closedAt url"));
        assert!(!CLOSED_QUERY.contains("subIssues") && !CLOSED_QUERY.contains("blockedBy") && !CLOSED_QUERY.contains("projectItems"));
    }
}
