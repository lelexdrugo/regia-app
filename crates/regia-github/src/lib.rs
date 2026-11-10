//! The GitHub adapter. Everything GitHub-specific stays in this crate. The user's `gh` does the
//! authentication, so the product never sees a token (ADR 0001, "GitHub").

use std::process::Command;

use regia_core::{IssueState, RawIssue, Tracker, TrackerConfigV1, TrackerError, add_children_from_parents};
use serde::Deserialize;
use serde_json::Value;

const ISSUES_QUERY: &str = r#"query($o:String!,$r:String!,$after:String){repository(owner:$o,name:$r){
  issues(first:100,after:$after,states:[OPEN],orderBy:{field:CREATED_AT,direction:ASC}){
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

const SUB_ISSUES_QUERY: &str = r#"query($o:String!,$r:String!,$n:Int!,$after:String){repository(owner:$o,name:$r){issue(number:$n){
  subIssues(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{number state}}}}}"#;

pub struct GhTracker {
    program: String,
    owner: String,
    repo: String,
    project_id: String,
    status_field_id: String,
}

impl GhTracker {
    pub fn new(config: &TrackerConfigV1) -> Self {
        Self {
            program: "gh".to_owned(),
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
        let mut command = Command::new(&self.program);
        command
            .args(["api", "graphql", "-f"])
            .arg(format!("query={query}"))
            .args(["-f", &format!("o={}", self.owner), "-f", &format!("r={}", self.repo)]);
        if let Some(number) = number {
            command.args(["-F", &format!("n={number}")]);
        }
        if let Some(after) = after {
            command.args(["-f", &format!("after={after}")]);
        }
        // The release app has no console: without this flag every call would flash a window.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command
            .output()
            .map_err(|error| TrackerError::Unavailable(format!("cannot run gh: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TrackerError::Unavailable(format!("gh api graphql: {}", stderr.trim())));
        }
        let envelope: Value = serde_json::from_slice(&output.stdout)
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

impl Tracker for GhTracker {
    fn open_issues(&self) -> Result<Vec<RawIssue>, TrackerError> {
        let mut issues: Vec<RawIssue> = Vec::new();
        let mut items_seen = 0;
        let mut after: Option<String> = None;
        loop {
            let data: IssuesData = self.graphql(ISSUES_QUERY, None, after.as_deref())?;
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
