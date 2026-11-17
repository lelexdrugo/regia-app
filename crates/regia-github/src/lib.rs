//! The GitHub adapter. Everything GitHub-specific stays in this crate. The user's `gh` does the
//! authentication, so the product never sees a token (ADR 0001, "GitHub").

mod history;
mod init;
mod issue;
mod limit;
mod read;
mod write;

pub use init::GhInit;

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use regia_core::{
    CloseReason, IssueRef, IssueState, RawComment, RawIssue, StatusHistory, Tracker, TrackerConfigV1, TrackerError,
};
use regia_exec::{Pin, Program};
use serde::Deserialize;
use serde_json::{Value, json};

/// The list of issues; `{STATES}` is the filter on their state, empty for every state.
const ISSUES_QUERY: &str = r#"query($o:String!,$r:String!,$after:String){repository(owner:$o,name:$r){
  issues(first:100,after:$after,{STATES}orderBy:{field:CREATED_AT,direction:ASC}){
    pageInfo{hasNextPage endCursor}
    nodes{number title state stateReason body url updatedAt createdAt closedAt
      labels(first:20){nodes{name}}
      parent{number}
      subIssues(first:50){totalCount nodes{number state}}
      blockedBy(first:20){totalCount nodes{number state}}
      projectItems(first:5){nodes{project{id} fieldValues(first:50){nodes{... on ProjectV2ItemFieldSingleSelectValue{optionId name field{... on ProjectV2SingleSelectField{id name}}}}}}}
    }}}}"#;

/// Where the pages of the list end, and nothing else: the walk that gives the full pages their cursors
/// (`read`). It has the arguments of the list, so the same pages end at the same cursors.
const ENDS_QUERY: &str = r#"query($o:String!,$r:String!,$after:String){repository(owner:$o,name:$r){
  issues(first:100,after:$after,{STATES}orderBy:{field:CREATED_AT,direction:ASC}){
    pageInfo{hasNextPage endCursor}}}}"#;

const BLOCKERS_QUERY: &str = r#"query($o:String!,$r:String!,$n:Int!,$after:String){repository(owner:$o,name:$r){issue(number:$n){
  blockedBy(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{number state}}}}}"#;

const COMMENTS_QUERY: &str = r#"query($o:String!,$r:String!,$n:Int!,$after:String){repository(owner:$o,name:$r){issue(number:$n){
  comments(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{databaseId createdAt url body}}}}}"#;

const SUB_ISSUES_QUERY: &str = r#"query($o:String!,$r:String!,$n:Int!,$after:String){repository(owner:$o,name:$r){issue(number:$n){
  subIssues(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{number state}}}}}"#;

pub struct GhTracker {
    /// Resolved once, to an absolute path. A gh that cannot be found fails the first call.
    gh: Result<Pin, String>,
    work_dir: PathBuf,
    owner: String,
    repo: String,
    project_id: String,
    status_field_id: String,
    /// The calls running at once, at most [`limit::MAX_CALLS`].
    calls: limit::Calls,
}

impl GhTracker {
    pub fn new(config: &TrackerConfigV1) -> Self {
        Self {
            gh: Pin::resolve(Program::Gh).map_err(|error| error.to_string()),
            work_dir: regia_exec::neutral_folder(),
            owner: config.owner.clone(),
            repo: config.repo.clone(),
            project_id: config.project_id.clone(),
            status_field_id: config.status_field_id.clone(),
            calls: limit::Calls::new(limit::MAX_CALLS),
        }
    }

    fn graphql<T: for<'de> Deserialize<'de>>(
        &self,
        query: &str,
        number: Option<u64>,
        after: Option<&str>,
    ) -> Result<T, TrackerError> {
        let number = number.map(|number| ("n", json!(number)));
        self.graphql_with(query, number.as_slice(), after)
    }

    /// `graphql` with the query's own variables, besides the repository and the cursor.
    fn graphql_with<T: for<'de> Deserialize<'de>>(
        &self,
        query: &str,
        extra: &[(&str, Value)],
        after: Option<&str>,
    ) -> Result<T, TrackerError> {
        let gh = self.gh.as_ref().map_err(|error| TrackerError::Unavailable(error.clone()))?;
        // The whole request, with typed variables, goes on stdin: no value reaches gh's argv.
        let mut variables = json!({"o": self.owner, "r": self.repo});
        for (name, value) in extra {
            variables[*name] = value.clone();
        }
        if let Some(after) = after {
            variables["after"] = json!(after);
        }
        let body = json!({"query": query, "variables": variables}).to_string();
        let place = self.calls.start();
        let output = gh
            .launch(regia_exec::gh::graphql(), &self.work_dir)
            .stdin(body)
            .run()
            .map_err(|error| TrackerError::Unavailable(error.to_string()));
        drop(place);
        let output = output?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TrackerError::Unavailable(format!("gh api graphql: {}", stderr.trim())));
        }
        Self::data(&output.stdout)
    }

    /// The `data` of what GraphQL answered: an answer that does not read, or that has `errors`, is a bad
    /// response.
    fn data<T: for<'de> Deserialize<'de>>(stdout: &[u8]) -> Result<T, TrackerError> {
        let envelope: Value = serde_json::from_slice(stdout)
            .map_err(|error| TrackerError::BadResponse(error.to_string()))?;
        if let Some(errors) = envelope.get("errors").filter(|errors| !errors.is_null()) {
            return Err(TrackerError::BadResponse(errors.to_string()));
        }
        serde_json::from_value(envelope["data"].clone())
            .map_err(|error| TrackerError::BadResponse(error.to_string()))
    }
}

impl read::Wire for GhTracker {
    fn list(&self, states: &str, after: Option<&str>) -> Result<IssueConnection, TrackerError> {
        let data: IssuesData = self.graphql(&ISSUES_QUERY.replace("{STATES}", states), None, after)?;
        Ok(data.repository.issues)
    }

    fn list_ends(&self, states: &str, after: Option<&str>) -> Result<PageInfo, TrackerError> {
        let data: EndsData = self.graphql(&ENDS_QUERY.replace("{STATES}", states), None, after)?;
        Ok(data.repository.issues.page_info)
    }

    fn connection(&self, connection: read::Connection, number: u64, after: Option<&str>) -> Result<PagedConnection, TrackerError> {
        let query = match connection {
            read::Connection::Blockers => BLOCKERS_QUERY,
            read::Connection::SubIssues => SUB_ISSUES_QUERY,
        };
        let data: ConnectionData = self.graphql(query, Some(number), after)?;
        Ok(data.repository.issue.connection)
    }
}

impl GhTracker {
    /// The issues `states` lets through, in tracker order, each with its blockers and sub-issues read in
    /// full where the list cuts them.
    fn issues(&self, states: &str) -> Result<Vec<RawIssue>, TrackerError> {
        let project = read::Project { id: &self.project_id, status_field_id: &self.status_field_id };
        read::issues(self, &project, states)
    }
}

impl Tracker for GhTracker {
    fn open_issues(&self) -> Result<Vec<RawIssue>, TrackerError> {
        self.issues("states:[OPEN],")
    }

    fn all_issues(&self) -> Result<Vec<RawIssue>, TrackerError> {
        self.issues("")
    }

    fn comments(&self, number: u64) -> Result<Option<Vec<RawComment>>, TrackerError> {
        let mut comments = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let data: CommentsData = self.graphql(COMMENTS_QUERY, Some(number), after.as_deref())?;
            let Some(issue) = data.repository.issue else { return Ok(None) };
            let page = issue.comments;
            comments.extend(page.nodes.into_iter().map(|node| RawComment {
                id: node.database_id,
                created_at: node.created_at,
                url: node.url,
                body: node.body.unwrap_or_default(),
            }));
            if !page.page_info.has_next_page {
                return Ok(Some(comments));
            }
            after = page.page_info.end_cursor;
        }
    }

    fn status_histories(&self, label: &str, since: DateTime<Utc>) -> Result<Vec<StatusHistory>, TrackerError> {
        self.read_status_histories(label, since)
    }

    fn labels(&self, number: u64) -> Result<Vec<String>, TrackerError> {
        self.issue_labels(number)
    }

    fn add_comment(&self, number: u64, body: &str) -> Result<String, TrackerError> {
        self.issue_comment(number, body)
    }

    fn add_comment_inline(&self, number: u64, body: &str) -> Result<(), TrackerError> {
        self.issue_comment_inline(number, body)
    }

    fn issue_node(&self, number: u64) -> Result<Option<IssueRef>, TrackerError> {
        self.node_of_issue(number)
    }

    fn project_item(&self, issue: &str) -> Result<String, TrackerError> {
        self.item_of_issue(issue)
    }

    fn set_status_option(&self, item: &str, option: &str) -> Result<(), TrackerError> {
        self.status_option(item, option)
    }

    fn close_issue(&self, issue: &str, reason: CloseReason) -> Result<(), TrackerError> {
        self.close(issue, reason)
    }

    fn reopen_issue(&self, issue: &str) -> Result<(), TrackerError> {
        self.reopen(issue)
    }

    fn add_sub_issue(&self, parent: &str, child: &str, replace_parent: bool) -> Result<(), TrackerError> {
        self.sub_issue(parent, child, replace_parent)
    }

    fn add_blocked_by(&self, issue: &str, blocker: &str) -> Result<(), TrackerError> {
        self.blocked_by(issue, blocker, true)
    }

    fn remove_blocked_by(&self, issue: &str, blocker: &str) -> Result<(), TrackerError> {
        self.blocked_by(issue, blocker, false)
    }

    fn blocker_numbers(&self, number: u64) -> Result<Option<Vec<u64>>, TrackerError> {
        self.blockers_of_issue(number)
    }

    fn sub_issue_count(&self, number: u64) -> Result<Option<u64>, TrackerError> {
        self.sub_issues_of_issue(number)
    }

    fn create_issue(&self, title: &str, body: &str, labels: &[String]) -> Result<String, TrackerError> {
        self.issue_create(title, body, labels)
    }

    fn edit_labels(&self, number: u64, add: &str, remove: Option<&str>) -> Result<(), TrackerError> {
        self.issue_edit_labels(number, add, remove)
    }

    fn issue_url(&self, number: u64) -> Option<String> {
        Some(format!("https://github.com/{}/{}/issues/{number}", self.owner, self.repo))
    }
}

#[derive(Deserialize)]
struct CommentsData {
    repository: CommentsRepository,
}

#[derive(Deserialize)]
struct CommentsRepository {
    issue: Option<CommentsIssue>,
}

#[derive(Deserialize)]
struct CommentsIssue {
    comments: CommentPage,
}

#[derive(Deserialize)]
struct CommentPage {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<CommentNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentNode {
    database_id: Option<u64>,
    created_at: String,
    url: String,
    body: Option<String>,
}

/// The answer of the walk that only reads where the pages end.
#[derive(Deserialize)]
struct EndsData {
    repository: EndsRepository,
}

#[derive(Deserialize)]
struct EndsRepository {
    issues: EndsConnection,
}

#[derive(Deserialize)]
struct EndsConnection {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Deserialize)]
struct IssuesData {
    repository: IssuesRepository,
}

#[derive(Deserialize)]
struct IssuesRepository {
    issues: IssueConnection,
}

#[derive(Deserialize)]
struct IssueConnection {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<IssueNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueNode {
    number: u64,
    title: String,
    state: IssueState,
    state_reason: Option<String>,
    body: String,
    url: String,
    updated_at: String,
    created_at: Option<String>,
    closed_at: Option<String>,
    labels: Nodes<LabelNode>,
    parent: Option<NumberNode>,
    sub_issues: CountedList,
    blocked_by: CountedList,
    project_items: Nodes<ProjectItem>,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
}

#[derive(Deserialize)]
struct LabelNode {
    name: String,
}

#[derive(Deserialize)]
struct NumberNode {
    number: u64,
}

#[derive(Deserialize)]
struct StateNode {
    number: u64,
    state: IssueState,
}

/// The first page of a list, with how many there are in all.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CountedList {
    total_count: u64,
    nodes: Vec<StateNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectItem {
    project: ProjectRef,
    field_values: Nodes<FieldValue>,
}

#[derive(Deserialize)]
struct ProjectRef {
    id: String,
}

/// Empty for a value of a field that is not a single select. `name` is the option's.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FieldValue {
    option_id: Option<String>,
    name: Option<String>,
    field: Option<FieldRef>,
}

#[derive(Deserialize)]
struct FieldRef {
    id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct ConnectionData {
    repository: ConnectionRepository,
}

#[derive(Deserialize)]
struct ConnectionRepository {
    issue: ConnectionIssue,
}

/// One issue's `blockedBy` or `subIssues`, a page at a time.
#[derive(Deserialize)]
struct ConnectionIssue {
    #[serde(rename = "blockedBy", alias = "subIssues")]
    connection: PagedConnection,
}

#[derive(Deserialize)]
struct PagedConnection {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<StateNode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TestTaskListJson.test_la_query_chiede_createdAt_e_closedAt` of the dossier: the list reads both dates.
    #[test]
    fn the_list_asks_for_the_dates_of_creation_and_closing() {
        assert!(ISSUES_QUERY.contains("createdAt closedAt"));
    }

    /// `TestTaskListJson.test_normalize_senza_i_campi_nuovi_non_si_rompe`: a node without the two dates reads,
    /// with none for each.
    #[test]
    fn a_node_without_the_dates_still_reads() {
        let node = json!({
            "number": 8, "title": "open", "state": "OPEN", "stateReason": null, "body": "", "url": "u", "updatedAt": "d",
            "labels": {"nodes": []}, "parent": null,
            "subIssues": {"totalCount": 0, "nodes": []}, "blockedBy": {"totalCount": 0, "nodes": []},
            "projectItems": {"nodes": []},
        });
        let node: IssueNode = serde_json::from_value(node).expect("a node without the dates");
        assert_eq!((node.number, node.created_at, node.closed_at), (8, None, None));
    }
}
