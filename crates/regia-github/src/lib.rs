//! The GitHub adapter. Everything GitHub-specific stays in this crate. The user's `gh` does the
//! authentication, so the product never sees a token (ADR 0001, "GitHub").

use std::path::PathBuf;

use regia_core::{IssueState, RawIssue, Tracker, TrackerConfigV1, TrackerError};
use regia_exec::{Pin, Program};
use serde::Deserialize;
use serde_json::{Value, json};

const ISSUES_QUERY: &str = r#"query($o:String!,$r:String!,$after:String){repository(owner:$o,name:$r){
  issues(first:100,after:$after,states:[OPEN],orderBy:{field:CREATED_AT,direction:ASC}){
    pageInfo{hasNextPage endCursor}
    nodes{number title state stateReason url updatedAt createdAt closedAt
      labels(first:20){nodes{name}}
      parent{number}
      subIssues(first:50){nodes{number state}}
      blockedBy(first:20){totalCount nodes{number state}}
      projectItems(first:5){nodes{project{id} fieldValues(first:50){nodes{... on ProjectV2ItemFieldSingleSelectValue{optionId field{... on ProjectV2SingleSelectField{id}}}}}}}
    }}}}"#;

const BLOCKERS_QUERY: &str = r#"query($o:String!,$r:String!,$n:Int!,$after:String){repository(owner:$o,name:$r){issue(number:$n){
  blockedBy(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{number state}}}}}"#;

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
        let gh = self.gh.as_ref().map_err(|error| TrackerError::Unavailable(error.clone()))?;
        // The whole request, with typed variables, goes on stdin: no value reaches gh's argv.
        let mut variables = json!({"o": self.owner, "r": self.repo});
        if let Some(number) = number {
            variables["n"] = json!(number);
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
        let envelope: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| TrackerError::BadResponse(error.to_string()))?;
        if let Some(errors) = envelope.get("errors").filter(|errors| !errors.is_null()) {
            return Err(TrackerError::BadResponse(errors.to_string()));
        }
        serde_json::from_value(envelope["data"].clone())
            .map_err(|error| TrackerError::BadResponse(error.to_string()))
    }

    /// The list query reads the first 20 `blockedBy`. Issues above the limit read the rest here. If
    /// that fails the total stays above what was read, and the rules treat the task as incomplete.
    fn complete_blockers(&self, issue: &mut RawIssue) {
        if issue.blocked_by_total <= issue.blocked_by.len() as u64 {
            return;
        }
        let mut after: Option<String> = None;
        loop {
            let page: Result<BlockersData, _> =
                self.graphql(BLOCKERS_QUERY, Some(issue.number), after.as_deref());
            let Ok(page) = page else { return };
            let connection = page.repository.issue.blocked_by;
            for node in connection.nodes {
                if !issue.blocked_by.iter().any(|(number, _)| *number == node.number) {
                    issue.blocked_by.push((node.number, node.state));
                }
            }
            if !connection.page_info.has_next_page {
                return;
            }
            after = connection.page_info.end_cursor;
        }
    }

    fn to_raw(&self, node: IssueNode, items_seen: &mut u64) -> RawIssue {
        // The status field is found by its id, so its name can be anything, in any language. The last
        // item of the configured Project wins; other Projects and other fields do not count.
        let items: Vec<ProjectItem> =
            node.project_items.nodes.into_iter().filter(|item| item.project.id == self.project_id).collect();
        *items_seen += items.len() as u64;
        let status_option_id = items
            .into_iter()
            .flat_map(|item| item.field_values.nodes)
            .filter(|value| value.field.as_ref().is_some_and(|field| field.id.as_deref() == Some(&self.status_field_id)))
            .filter_map(|value| value.option_id)
            .next_back();
        RawIssue {
            number: node.number,
            title: node.title,
            state: node.state,
            state_reason: node.state_reason,
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
                let mut raw = self.to_raw(node, &mut items_seen);
                self.complete_blockers(&mut raw);
                issues.push(raw);
            }
            if !connection.page_info.has_next_page {
                break;
            }
            after = connection.page_info.end_cursor;
        }
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
    url: String,
    updated_at: String,
    created_at: Option<String>,
    closed_at: Option<String>,
    labels: Nodes<LabelNode>,
    parent: Option<NumberNode>,
    sub_issues: Nodes<StateNode>,
    blocked_by: BlockedByList,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockedByList {
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

/// Empty for a value of a field that is not a single select.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FieldValue {
    option_id: Option<String>,
    field: Option<FieldRef>,
}

#[derive(Deserialize)]
struct FieldRef {
    id: Option<String>,
}

#[derive(Deserialize)]
struct BlockersData {
    repository: BlockersRepository,
}

#[derive(Deserialize)]
struct BlockersRepository {
    issue: BlockersIssue,
}

#[derive(Deserialize)]
struct BlockersIssue {
    #[serde(rename = "blockedBy")]
    blocked_by: BlockersConnection,
}

#[derive(Deserialize)]
struct BlockersConnection {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<StateNode>,
}
