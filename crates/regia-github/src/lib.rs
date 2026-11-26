//! The GitHub adapter. Everything GitHub-specific stays in this crate. The user's `gh` does the
//! authentication, so the product never sees a token (ADR 0001, "GitHub").

mod checks;
mod history;
mod init;
mod issue;
mod limit;
mod read;
mod write;

pub use init::GhInit;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use regia_core::{
    CloseReason, ClosedIssue, IssueRef, IssueState, RawComment, RawIssue, RecentComments, StatusHistory, Tracker, TrackerConfigV1, TrackerError,
};
use regia_exec::{ExecError, Output, Pin, Program};
use serde::Deserialize;
use serde_json::{Map, Value, json};

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

const EDITED_QUERY: &str = r#"query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){issue(number:$n){lastEditedAt}}}"#;

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
        let mut variables = Map::new();
        variables.insert("o".to_owned(), json!(self.owner));
        variables.insert("r".to_owned(), json!(self.repo));
        for (name, value) in extra {
            variables.insert((*name).to_owned(), value.clone());
        }
        if let Some(after) = after {
            variables.insert("after".to_owned(), json!(after));
        }
        Self::data_of(self.envelope(query, &variables)?)
    }

    /// `variables` as they are through the adapter's one run of `gh api graphql` ([`graphql_through`]), for
    /// the reads and the writes. What comes back is the envelope GraphQL answered, `data` and `errors` as it
    /// sent them: what an `errors` means is for the caller to say (`data_of` for the reads, `write::answer`
    /// for the writes).
    fn envelope(&self, query: &str, variables: &Map<String, Value>) -> Result<Value, TrackerError> {
        let gh = self.gh.as_ref().map_err(|error| TrackerError::Unavailable(error.clone()))?;
        self.envelope_through(query, variables, |body| {
            graphql_launch(gh, &self.work_dir, body)
                .map_err(|error| TrackerError::Unavailable(error.to_string()))
        })
    }

    /// [`envelope`](Self::envelope) with the launch of `gh` given: it gets the body of the request and runs
    /// while the call holds its place. The tests give it a launch that does not start a process.
    fn envelope_through(
        &self,
        query: &str,
        variables: &Map<String, Value>,
        launch: impl FnOnce(String) -> Result<Reply, TrackerError>,
    ) -> Result<Value, TrackerError> {
        envelope_of(&graphql_through(&self.calls, query, variables, launch)?)
    }

    /// The `data` of an envelope, for the reads: an answer with `errors`, whatever they are but `null`, is a
    /// bad response with the errors as JSON, and so is a `data` that does not read.
    fn data_of<T: for<'de> Deserialize<'de>>(envelope: Value) -> Result<T, TrackerError> {
        if let Some(errors) = envelope.get("errors").filter(|errors| !errors.is_null()) {
            return Err(TrackerError::BadResponse(errors.to_string()));
        }
        serde_json::from_value(envelope["data"].clone())
            .map_err(|error| TrackerError::BadResponse(error.to_string()))
    }

    /// The `data` of what `gh` printed, as the reads take it: the envelope, then [`data_of`](Self::data_of).
    #[cfg(test)]
    fn data<T: for<'de> Deserialize<'de>>(stdout: &[u8]) -> Result<T, TrackerError> {
        Self::data_of(parse_envelope(stdout)?)
    }
}

/// What a run of `gh` printed and how it ended: its exit code is `None` when a signal ended it.
struct Reply {
    success: bool,
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Reply {
    fn of(output: Output) -> Self {
        Reply {
            success: output.status.success(),
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}

/// The one place that runs `gh api graphql`, for the tracker's reads and writes and for the Projects that
/// `init` reads (#595, #672): `variables` as they are, the whole request, with its typed variables, on
/// stdin so that no value reaches gh's argv. A call, whichever it is, waits for a place among the running
/// ones of `calls` ([`limit::MAX_CALLS`]), and `launch` runs while it holds it: [`graphql_launch`], or in
/// the tests a launch that does not start a process. What comes back is what `gh` printed and how it
/// ended, for each caller to read with its own errors: [`envelope_of`] for the tracker, `init`'s own for
/// its Projects.
fn graphql_through<E>(
    calls: &limit::Calls,
    query: &str,
    variables: &Map<String, Value>,
    launch: impl FnOnce(String) -> Result<Reply, E>,
) -> Result<Reply, E> {
    let body = json!({"query": query, "variables": variables}).to_string();
    let place = calls.start();
    let reply = launch(body);
    drop(place);
    reply
}

/// `gh api graphql --input=-` in `work_dir`, with `body` on stdin: the launch of [`graphql_through`]
/// outside the tests.
fn graphql_launch(gh: &Pin, work_dir: &Path, body: String) -> Result<Reply, ExecError> {
    gh.launch(regia_exec::gh::graphql(), work_dir)
        .stdin(body)
        .run()
        .map(Reply::of)
}

/// The envelope of a run, for the tracker's reads and writes alike. A run that fails is `gh` unavailable,
/// with what it said: its stderr, or its stdout when stderr is empty.
fn envelope_of(reply: &Reply) -> Result<Value, TrackerError> {
    if !reply.success {
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_owned();
        let stderr = text(&reply.stderr);
        let said = if stderr.is_empty() { text(&reply.stdout) } else { stderr };
        return Err(TrackerError::Unavailable(format!("gh api graphql: {said}")));
    }
    parse_envelope(&reply.stdout)
}

/// An answer that does not read as JSON is a bad response.
fn parse_envelope(stdout: &[u8]) -> Result<Value, TrackerError> {
    serde_json::from_slice(stdout).map_err(|error| TrackerError::BadResponse(error.to_string()))
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

    fn body_edited_at(&self, number: u64) -> Result<Option<Option<String>>, TrackerError> {
        let data: EditedData = self.graphql(EDITED_QUERY, Some(number), None)?;
        Ok(data.repository.issue.map(|issue| issue.last_edited_at))
    }

    fn status_histories(&self, label: &str, since: DateTime<Utc>) -> Result<Vec<StatusHistory>, TrackerError> {
        self.read_status_histories(label, since)
    }

    fn recent_comments(&self, since: DateTime<Utc>) -> Result<Vec<RecentComments>, TrackerError> {
        self.read_recent_comments(since)
    }

    fn closed_issues(&self) -> Result<Vec<ClosedIssue>, TrackerError> {
        self.read_closed_issues()
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

#[derive(Deserialize)]
struct EditedData {
    repository: EditedRepository,
}

#[derive(Deserialize)]
struct EditedRepository {
    issue: Option<EditedIssue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditedIssue {
    last_edited_at: Option<String>,
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

    /// A tracker that starts no process: the tests give `envelope_through` the launch, and `places` is how
    /// many calls it lets run at once.
    pub(crate) fn tracker(places: usize) -> GhTracker {
        GhTracker {
            gh: Err("no gh in the unit tests".to_owned()),
            work_dir: PathBuf::new(),
            owner: "example".to_owned(),
            repo: "tracker".to_owned(),
            project_id: "P".to_owned(),
            status_field_id: "F".to_owned(),
            calls: limit::Calls::new(places),
        }
    }

    /// The same, with this test binary as the `gh`: it does not know `api graphql --input=-`, so a call
    /// that runs it fails at once, as `gh` fails without a network.
    pub(crate) fn tracker_running_this_binary(places: usize) -> GhTracker {
        let this = std::env::current_exe().expect("the test binary");
        GhTracker {
            gh: Pin::at(Program::Gh, &this).map_err(|error| error.to_string()),
            work_dir: regia_exec::neutral_folder(),
            ..tracker(places)
        }
    }

    /// A reply as `gh` gives it: a run that succeeds exits with 0, one that fails with 1.
    pub(crate) fn reply(success: bool, stdout: &str, stderr: &str) -> Reply {
        Reply {
            success,
            code: Some(if success { 0 } else { 1 }),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// Difference 3 of #595, for both sides: a run that ends with an exit code is `gh` unavailable, with
    /// stderr, or stdout when stderr is empty. The reads used to give stderr only.
    #[test]
    fn a_failed_run_says_its_stderr_else_its_stdout() {
        let said = |stdout: &str, stderr: &str| match envelope_of(&reply(false, stdout, stderr)) {
            Err(TrackerError::Unavailable(said)) => said,
            other => panic!("not unavailable: {other:?}"),
        };
        assert_eq!(said("out", " err \n"), "gh api graphql: err");
        assert_eq!(said(" out \n", ""), "gh api graphql: out");
        assert_eq!(said("", " \n"), "gh api graphql: ");
        // The failure does not reach the reading of `errors`: whatever stdout holds, the run failed.
        assert!(matches!(
            envelope_of(&reply(false, r#"{"errors":[{"message":"m"}]}"#, "")),
            Err(TrackerError::Unavailable(said)) if said.contains("\"message\":\"m\"")
        ));
    }

    /// A run that succeeds gives the envelope whole, `errors` included, for the caller to classify; one that
    /// does not read is a bad response, for both sides.
    #[test]
    fn the_transport_gives_the_envelope_and_judges_no_errors() {
        let envelope = envelope_of(&reply(true, r#"{"data":{"x":1},"errors":[{"message":"m"}]}"#, "")).unwrap();
        assert_eq!(envelope, json!({"data": {"x": 1}, "errors": [{"message": "m"}]}));
        assert!(matches!(envelope_of(&reply(true, "<html>", "")), Err(TrackerError::BadResponse(_))));
        assert!(matches!(envelope_of(&reply(true, "", "")), Err(TrackerError::BadResponse(_))));
    }

    /// Difference 2 and 4 of #595, the reads' side: `errors` is a bad response with the errors as JSON, and
    /// whatever `errors` is but `null` counts: an empty array too.
    #[test]
    fn the_reads_take_errors_as_a_bad_response_with_their_json() {
        let read = |envelope: Value| GhTracker::data_of::<Value>(envelope);
        let errors = json!([{"type": "NOT_FOUND", "message": "Could not resolve"}]);
        let as_json = errors.to_string();
        assert!(matches!(
            read(json!({"data": null, "errors": errors})),
            Err(TrackerError::BadResponse(text)) if text == as_json
        ));
        assert!(matches!(read(json!({"data": {}, "errors": []})), Err(TrackerError::BadResponse(text)) if text == "[]"));
        assert!(matches!(read(json!({"data": {}, "errors": {}})), Err(TrackerError::BadResponse(text)) if text == "{}"));
        assert_eq!(read(json!({"data": {"x": 1}, "errors": null})).unwrap(), json!({"x": 1}));
        assert_eq!(read(json!({"data": {"x": 1}})).unwrap(), json!({"x": 1}));
        assert_eq!(read(json!({"errors": null})).unwrap(), Value::Null);
        assert!(matches!(GhTracker::data_of::<u64>(json!({"data": "text"})), Err(TrackerError::BadResponse(_))));
    }

    /// Difference 1 of #595: a call takes a place among the running ones, as every call to `gh` does, writes
    /// included, because they all go through `envelope_through`. With one place, held here, the call does not
    /// start until it is given back. The wait for "it did not start" can only pass too soon on a slow machine,
    /// never fail for it.
    #[test]
    fn a_call_waits_for_a_place_and_gives_it_back() {
        use std::sync::mpsc;
        use std::time::Duration;

        let tracker = tracker(1);
        let place = tracker.calls.start();
        let (started, hear) = mpsc::channel();
        std::thread::scope(|scope| {
            let call = scope.spawn(|| {
                tracker.envelope_through("query{x}", &Map::new(), |body| {
                    started.send(body).unwrap();
                    Ok(reply(true, r#"{"data":null}"#, ""))
                })
            });
            assert!(hear.recv_timeout(Duration::from_millis(50)).is_err(), "the call ran without a place");
            drop(place);
            let body = hear.recv().expect("the call runs once it has a place");
            assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({"query": "query{x}", "variables": {}}));
            assert_eq!(call.join().unwrap().unwrap(), json!({"data": null}));
        });
        // The place was given back, even by a call whose launch failed: taking it again would wait for ever otherwise.
        let failed = tracker.envelope_through("q", &Map::new(), |_| Err(TrackerError::Unavailable("down".to_owned())));
        assert!(matches!(failed, Err(TrackerError::Unavailable(_))));
        drop(tracker.calls.start());
    }

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
