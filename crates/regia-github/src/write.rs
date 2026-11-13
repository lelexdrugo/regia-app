//! The tracker's writes through GraphQL, and the reads they need: an issue's node, its item in the
//! Project, its blockers and the count of its sub-issues (T10, T11, T13 and G1 of the dossier's
//! conformance study). Each query and mutation is the prototype's text, its variables' names included:
//! the order of the calls and what each carries are part of the behaviour, and the conformance tests
//! compare them call by call.

use regia_core::{CloseReason, IssueRef, IssueState, TrackerError};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::GhTracker;

const ISSUE_NODE_QUERY: &str = "query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){issue(number:$n){id state}}}";

const PROJECT_ITEMS_QUERY: &str = "query($id:ID!){node(id:$id){... on Issue{projectItems(first:10){nodes{id project{id}}}}}}";

const ADD_ITEM: &str = "mutation($p:ID!,$c:ID!){addProjectV2ItemById(input:{projectId:$p,contentId:$c}){item{id}}}";

const SET_STATUS: &str = "mutation($p:ID!,$i:ID!,$f:ID!,$o:String!){updateProjectV2ItemFieldValue(input:{
        projectId:$p,itemId:$i,fieldId:$f,value:{singleSelectOptionId:$o}}){projectV2Item{id}}}";

const CLOSE: &str = "mutation($i:ID!,$r:IssueClosedStateReason!){closeIssue(input:{issueId:$i,stateReason:$r}){issue{id}}}";

const REOPEN: &str = "mutation($i:ID!){reopenIssue(input:{issueId:$i}){issue{id}}}";

const ADD_SUB_ISSUE: &str = "mutation($p:ID!,$c:ID!){addSubIssue(input:{issueId:$p,subIssueId:$c}){issue{id}}}";

/// The same link, taking the child from the parent it had.
const MOVE_SUB_ISSUE: &str = "mutation($p:ID!,$c:ID!){addSubIssue(input:{issueId:$p,subIssueId:$c,replaceParent:true}){issue{id}}}";

const ADD_BLOCKED_BY: &str = "mutation($i:ID!,$b:ID!){addBlockedBy(input:{issueId:$i,blockingIssueId:$b}){issue{id}}}";

const REMOVE_BLOCKED_BY: &str = "mutation($i:ID!,$b:ID!){removeBlockedBy(input:{issueId:$i,blockingIssueId:$b}){issue{id}}}";

const SUB_ISSUE_COUNT_QUERY: &str =
    "query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){issue(number:$n){subIssues(first:1){totalCount}}}}";

impl GhTracker {
    /// `gh api graphql` with `variables` as they are, the whole request on stdin: the `data` of the answer.
    /// Errors in the answer are the tracker's refusal, their messages joined by `; `.
    fn request(&self, query: &str, variables: Map<String, Value>) -> Result<Value, TrackerError> {
        let gh = self.gh.as_ref().map_err(|error| TrackerError::Unavailable(error.clone()))?;
        let body = json!({"query": query, "variables": variables}).to_string();
        let output = gh
            .launch(regia_exec::gh::graphql(), &self.work_dir)
            .stdin(body)
            .run()
            .map_err(|error| TrackerError::Unavailable(error.to_string()))?;
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_owned();
        if !output.status.success() {
            let stderr = text(&output.stderr);
            let said = if stderr.is_empty() { text(&output.stdout) } else { stderr };
            return Err(TrackerError::Unavailable(format!("gh api graphql: {said}")));
        }
        let mut envelope: Value =
            serde_json::from_slice(&output.stdout).map_err(|error| TrackerError::BadResponse(error.to_string()))?;
        if let Some(errors) = envelope.get("errors").and_then(Value::as_array).filter(|errors| !errors.is_empty()) {
            let messages: Vec<&str> = errors.iter().map(|error| error["message"].as_str().unwrap_or_default()).collect();
            return Err(TrackerError::Refused(messages.join("; ")));
        }
        Ok(envelope["data"].take())
    }

    /// A mutation with string variables.
    fn mutate(&self, query: &str, variables: &[(&str, &str)]) -> Result<Value, TrackerError> {
        let variables = variables.iter().map(|(name, value)| ((*name).to_owned(), Value::String((*value).to_owned()))).collect();
        self.request(query, variables)
    }

    /// The owner, the repo and the issue's number, as the queries of one issue take them.
    fn issue_variables(&self, number: u64) -> Map<String, Value> {
        let mut variables = Map::new();
        variables.insert("o".to_owned(), json!(self.owner));
        variables.insert("r".to_owned(), json!(self.repo));
        variables.insert("n".to_owned(), json!(number));
        variables
    }

    pub(crate) fn node_of_issue(&self, number: u64) -> Result<Option<IssueRef>, TrackerError> {
        let data = self.request(ISSUE_NODE_QUERY, self.issue_variables(number))?;
        let issue: Option<NodeAndState> = parse(&data["repository"]["issue"])?;
        Ok(issue.map(|issue| IssueRef { id: issue.id, state: issue.state }))
    }

    /// The item of the configured Project among the issue's first ten, or a new one.
    pub(crate) fn item_of_issue(&self, issue: &str) -> Result<String, TrackerError> {
        let mut variables = Map::new();
        variables.insert("id".to_owned(), json!(issue));
        let data = self.request(PROJECT_ITEMS_QUERY, variables)?;
        let items: Items = parse(&data["node"]["projectItems"])?;
        if let Some(item) = items.nodes.into_iter().find(|item| item.project.id == self.project_id) {
            return Ok(item.id);
        }
        let data = self.mutate(ADD_ITEM, &[("p", &self.project_id), ("c", issue)])?;
        let added: Id = parse(&data["addProjectV2ItemById"]["item"])?;
        Ok(added.id)
    }

    pub(crate) fn status_option(&self, item: &str, option: &str) -> Result<(), TrackerError> {
        let variables = [("p", self.project_id.as_str()), ("i", item), ("f", self.status_field_id.as_str()), ("o", option)];
        self.mutate(SET_STATUS, &variables).map(|_| ())
    }

    pub(crate) fn close(&self, issue: &str, reason: CloseReason) -> Result<(), TrackerError> {
        let reason = match reason {
            CloseReason::Completed => "COMPLETED",
            CloseReason::NotPlanned => "NOT_PLANNED",
        };
        self.mutate(CLOSE, &[("i", issue), ("r", reason)]).map(|_| ())
    }

    pub(crate) fn reopen(&self, issue: &str) -> Result<(), TrackerError> {
        self.mutate(REOPEN, &[("i", issue)]).map(|_| ())
    }

    pub(crate) fn sub_issue(&self, parent: &str, child: &str, replace_parent: bool) -> Result<(), TrackerError> {
        let mutation = if replace_parent { MOVE_SUB_ISSUE } else { ADD_SUB_ISSUE };
        self.mutate(mutation, &[("p", parent), ("c", child)]).map(|_| ())
    }

    pub(crate) fn blocked_by(&self, issue: &str, blocker: &str, add: bool) -> Result<(), TrackerError> {
        let mutation = if add { ADD_BLOCKED_BY } else { REMOVE_BLOCKED_BY };
        self.mutate(mutation, &[("i", issue), ("b", blocker)]).map(|_| ())
    }

    /// The blockers of an issue, a page of a hundred at a time, as long as the tracker says there are more.
    pub(crate) fn blockers_of_issue(&self, number: u64) -> Result<Option<Vec<u64>>, TrackerError> {
        let mut numbers = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut variables = self.issue_variables(number);
            if let Some(after) = &after {
                variables.insert("after".to_owned(), json!(after));
            }
            let data = self.request(crate::BLOCKERS_QUERY, variables)?;
            let Some(issue) = parse::<Option<BlockersIssue>>(&data["repository"]["issue"])? else { return Ok(None) };
            let page = issue.blocked_by;
            numbers.extend(page.nodes.into_iter().map(|node| node.number));
            if !page.page_info.has_next_page {
                return Ok(Some(numbers));
            }
            after = page.page_info.end_cursor;
        }
    }

    pub(crate) fn sub_issues_of_issue(&self, number: u64) -> Result<Option<u64>, TrackerError> {
        let data = self.request(SUB_ISSUE_COUNT_QUERY, self.issue_variables(number))?;
        let issue: Option<SubIssueCount> = parse(&data["repository"]["issue"])?;
        Ok(issue.map(|issue| issue.sub_issues.total_count))
    }
}

fn parse<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, TrackerError> {
    T::deserialize(value).map_err(|error| TrackerError::BadResponse(error.to_string()))
}

#[derive(Deserialize)]
struct NodeAndState {
    id: String,
    state: IssueState,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct Items {
    nodes: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    id: String,
    project: Id,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockersIssue {
    blocked_by: BlockersPage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockersPage {
    page_info: crate::PageInfo,
    nodes: Vec<Number>,
}

#[derive(Deserialize)]
struct Number {
    number: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubIssueCount {
    sub_issues: Count,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Count {
    total_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mutations name their variables as the prototype's do, and carry `replaceParent` in the text.
    #[test]
    fn the_mutations_are_the_prototype_s() {
        assert_eq!(call(CLOSE), ("closeIssue", "input:{issueId:$i,stateReason:$r}".to_owned()));
        assert_eq!(call(SET_STATUS).1, "input:{projectId:$p,itemId:$i,fieldId:$f,value:{singleSelectOptionId:$o}}");
        assert_eq!(call(MOVE_SUB_ISSUE).1, "input:{issueId:$p,subIssueId:$c,replaceParent:true}");
        assert_eq!(call(ADD_SUB_ISSUE).1, "input:{issueId:$p,subIssueId:$c}");
        assert_eq!(call(REMOVE_BLOCKED_BY), ("removeBlockedBy", "input:{issueId:$i,blockingIssueId:$b}".to_owned()));
    }

    /// The first field of a mutation and its arguments without blanks, as the conformance harness reads them.
    fn call(query: &str) -> (&str, String) {
        let body = &query[query.find('{').expect("a body") + 1..];
        let open = body.find('(').expect("arguments");
        let name = body[..open].trim();
        let mut depth = 0;
        let mut end = open;
        for (at, c) in body[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + at;
                        break;
                    }
                }
                _ => {}
            }
        }
        (name, body[open + 1..end].chars().filter(|c| !c.is_whitespace()).collect())
    }
}
