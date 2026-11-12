//! The GitHub adapter. Everything GitHub-specific stays in this crate. The user's `gh` does the
//! authentication, so the product never sees a token (ADR 0001, "GitHub").

mod history;
mod issue;
mod write;

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use regia_core::{
    CloseReason, IssueRef, IssueState, RawComment, RawIssue, StatusHistory, Tracker, TrackerConfigV1, TrackerError,
    add_children_from_parents,
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
        let output = gh
            .launch(regia_exec::gh::graphql(), &self.work_dir)
            .stdin(body)
            .run()
            .map_err(|error| TrackerError::Unavailable(error.to_string()))?;
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

    /// Reads one connection of an issue from its first page, and adds the nodes the list lacks. A page
    /// that fails stops the reading and keeps what came before it.
    fn read_pages(&self, query: &str, number: u64, list: &mut Vec<(u64, IssueState)>) {
        let mut after: Option<String> = None;
        loop {
            let page: Result<ConnectionData, _> = self.graphql(query, Some(number), after.as_deref());
            let Ok(page) = page else { return };
            let connection = page.repository.issue.connection;
            let seen: Vec<u64> = list.iter().map(|(number, _)| *number).collect();
            list.extend(connection.nodes.into_iter().filter(|node| !seen.contains(&node.number)).map(|n| (n.number, n.state)));
            if !connection.page_info.has_next_page {
                return;
            }
            after = connection.page_info.end_cursor;
        }
    }

    /// The list query reads the first 20 `blockedBy`. Issues above the limit read the rest here. If
    /// that fails the total stays above what was read, and the rules treat the task as incomplete.
    fn complete_blockers(&self, issue: &mut RawIssue) {
        if issue.blocked_by_total > issue.blocked_by.len() as u64 {
            self.read_pages(BLOCKERS_QUERY, issue.number, &mut issue.blocked_by);
        }
    }

    /// The list query reads the first 50 sub-issues. Issues above the limit read the rest here. If that
    /// fails, the open children in the list still reach their parent (`add_children_from_parents`).
    fn complete_sub_issues(&self, issue: &mut RawIssue, total: u64) {
        if total > issue.sub_issues.len() as u64 {
            self.read_pages(SUB_ISSUES_QUERY, issue.number, &mut issue.sub_issues);
        }
    }

    fn to_raw(&self, node: IssueNode, items_seen: &mut u64) -> RawIssue {
        // The status field is found by its id, so its name can be anything, in any language. The last
        // item of the configured Project wins; other Projects and other fields do not count. The other
        // single-select values keep their field's name, which gives them a meaning in the core.
        let items: Vec<ProjectItem> =
            node.project_items.nodes.into_iter().filter(|item| item.project.id == self.project_id).collect();
        *items_seen += items.len() as u64;
        let values: Vec<FieldValue> = items.into_iter().flat_map(|item| item.field_values.nodes).collect();
        let status_option_id = values
            .iter()
            .filter(|value| value.field.as_ref().is_some_and(|field| field.id.as_deref() == Some(&self.status_field_id)))
            .filter_map(|value| value.option_id.clone())
            .next_back();
        let field_values = values
            .into_iter()
            .filter_map(|value| Some((value.field?.name?, value.name?)))
            .collect();
        RawIssue {
            number: node.number,
            title: node.title,
            state: node.state,
            state_reason: node.state_reason,
            body: node.body,
            url: node.url,
            updated_at: node.updated_at,
            created_at: node.created_at,
            closed_at: node.closed_at,
            labels: node.labels.nodes.into_iter().map(|label| label.name).collect(),
            parent: node.parent.map(|parent| parent.number),
            sub_issues: node.sub_issues.nodes.into_iter().map(|n| (n.number, n.state)).collect(),
            blocked_by_total: node.blocked_by.total_count,
            blocked_by: node.blocked_by.nodes.into_iter().map(|n| (n.number, n.state)).collect(),
            status_option_id,
            field_values,
        }
    }
}

impl GhTracker {
    /// The issues `states` lets through, in tracker order, each with its blockers and sub-issues read in
    /// full where the list cuts them.
    fn issues(&self, states: &str) -> Result<Vec<RawIssue>, TrackerError> {
        let query = ISSUES_QUERY.replace("{STATES}", states);
        let mut issues: Vec<RawIssue> = Vec::new();
        let mut items_seen = 0;
        let mut after: Option<String> = None;
        loop {
            let data: IssuesData = self.graphql(&query, None, after.as_deref())?;
            let connection = data.repository.issues;
            for node in connection.nodes {
                let sub_issues_total = node.sub_issues.total_count;
                let mut raw = self.to_raw(node, &mut items_seen);
                self.complete_blockers(&mut raw);
                self.complete_sub_issues(&mut raw, sub_issues_total);
                issues.push(raw);
            }
            if !connection.page_info.has_next_page {
                break;
            }
            after = connection.page_info.end_cursor;
        }
        add_children_from_parents(&mut issues);
        // Every task in the Project without a status means the config is wrong, not that the plan is
        // empty: reporting that as an empty brief would be a false all-clear.
        if items_seen > 0 && issues.iter().all(|issue| issue.status_option_id.is_none()) {
            return Err(TrackerError::BadResponse(format!(
                "none of the {items_seen} items in the Project has a value for status field {}: check tracker.json",
                self.status_field_id
            )));
        }
        Ok(issues)
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

#[derive(Deserialize)]
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
