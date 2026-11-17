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
    /// `variables` as they are through the one `gh api graphql` of the adapter ([`GhTracker::envelope`]): the
    /// `data` of the answer, `answer` of its errors.
    fn request(&self, query: &str, variables: Map<String, Value>) -> Result<Value, TrackerError> {
        answer(self.envelope(query, &variables)?)
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

/// The `data` of an envelope, for the writes. An envelope that is not an object (`[]`, a number, a string,
/// `true`, `null`) is a bad response with the envelope as JSON, as the reads take it, and is looked at before
/// anything else: indexing it by `data` would panic. A non-empty array of `errors` is the tracker's refusal,
/// their messages joined by `; `; an empty one, or anything else, is not (the reads take `errors` otherwise).
/// An object with no `data` key (`{}`, `{"errors": []}`, `{"extensions": {}}`) is a bad response with the
/// envelope as JSON, as the prototype's `return data["data"]` is a `KeyError` (#674): a mutation that drops
/// the answer would have taken it for done. A `data` that is there and is `null` is `Ok(Null)`: the prototype
/// returns `None` and goes on, and `parse` refuses it where a caller expects a type.
fn answer(mut envelope: Value) -> Result<Value, TrackerError> {
    if !envelope.is_object() {
        return Err(TrackerError::BadResponse(envelope.to_string()));
    }
    if let Some(errors) = envelope.get("errors").and_then(Value::as_array).filter(|errors| !errors.is_empty()) {
        let messages: Vec<&str> = errors.iter().map(|error| error["message"].as_str().unwrap_or_default()).collect();
        return Err(TrackerError::Refused(messages.join("; ")));
    }
    match envelope.get_mut("data") {
        Some(data) => Ok(data.take()),
        None => Err(TrackerError::BadResponse(envelope.to_string())),
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

    /// Difference 2 and 4 of #595, the writes' side: a non-empty array of `errors` is a refusal with the
    /// messages joined by `; `, and an empty one, or one that is not an array, is a success, as it was.
    #[test]
    fn the_writes_take_errors_as_a_refusal_with_their_messages() {
        let errors = json!([{"type": "FORBIDDEN", "message": "no"}, {"message": "nor this"}, {"type": "NO_MESSAGE"}]);
        assert!(matches!(
            answer(json!({"data": null, "errors": errors})),
            Err(TrackerError::Refused(text)) if text == "no; nor this; "
        ));
        assert_eq!(answer(json!({"data": {"x": 1}, "errors": []})).unwrap(), json!({"x": 1}));
        assert_eq!(answer(json!({"data": {"x": 1}, "errors": null})).unwrap(), json!({"x": 1}));
        assert_eq!(answer(json!({"data": {"x": 1}, "errors": {"message": "m"}})).unwrap(), json!({"x": 1}));
        assert_eq!(answer(json!({"data": {"x": 1}})).unwrap(), json!({"x": 1}));
    }

    /// #671: an envelope that is not an object is a bad response with the envelope as JSON, not a panic of the
    /// mutable index on `data`. One test for each shape, so that a panic names it; the reads' side of the same
    /// shapes is `lib.rs`'s test of `data_of`.
    fn bad_response(envelope: Value, text: &str) {
        let write = answer(envelope.clone());
        assert!(matches!(&write, Err(TrackerError::BadResponse(said)) if said == text), "{envelope}: {write:?}");
    }

    #[test]
    fn a_write_answers_an_array_envelope_as_a_bad_response() {
        bad_response(json!([]), "[]");
        bad_response(json!([{"message": "m"}]), r#"[{"message":"m"}]"#);
    }

    #[test]
    fn a_write_answers_a_number_envelope_as_a_bad_response() {
        bad_response(json!(3), "3");
    }

    #[test]
    fn a_write_answers_a_string_envelope_as_a_bad_response() {
        bad_response(json!("x"), r#""x""#);
    }

    #[test]
    fn a_write_answers_a_boolean_envelope_as_a_bad_response() {
        bad_response(json!(true), "true");
    }

    /// `null` did not panic (the index makes it an object), but it was a success with no data: a mutation
    /// would have taken it as done.
    #[test]
    fn a_write_answers_a_null_envelope_as_a_bad_response() {
        bad_response(Value::Null, "null");
    }

    /// #674: an envelope that is an object with no `data` key is a bad response with the envelope as JSON, as
    /// the prototype's `KeyError: 'data'` is a failure; a refusal in `errors` still comes first.
    #[test]
    fn an_object_without_data_is_a_bad_response() {
        bad_response(json!({}), "{}");
        bad_response(json!({"errors": []}), r#"{"errors":[]}"#);
        bad_response(json!({"errors": null}), r#"{"errors":null}"#);
    }

    #[test]
    fn an_object_with_other_keys_but_no_data_is_a_bad_response() {
        bad_response(json!({"extensions": {}}), r#"{"extensions":{}}"#);
        bad_response(json!({"extensions": {"cost": 1}, "errors": []}), r#"{"errors":[],"extensions":{"cost":1}}"#);
        bad_response(json!({"Data": {"x": 1}}), r#"{"Data":{"x":1}}"#);
    }

    #[test]
    fn a_refusal_comes_before_the_missing_data() {
        let write = answer(json!({"errors": [{"message": "no"}]}));
        assert!(matches!(write, Err(TrackerError::Refused(ref text)) if text == "no"), "{write:?}");
    }

    /// `data: null` is there: the prototype returns `None` and the write goes on, so it is a success with
    /// `null` (measured in `.regia/prove/674/prototipo.txt`), with or without an empty `errors`.
    #[test]
    fn a_null_data_is_a_success_with_null() {
        assert_eq!(answer(json!({"data": null})).unwrap(), Value::Null);
        assert_eq!(answer(json!({"data": null, "errors": []})).unwrap(), Value::Null);
    }

    /// The two sides give the same envelope their own classification and their own text: where they part is
    /// the point of keeping the mapping with the caller.
    #[test]
    fn each_side_keeps_its_own_classification_of_the_same_envelope() {
        let envelope = json!({"data": null, "errors": [{"message": "Could not resolve"}]});
        let read = crate::GhTracker::data_of::<Value>(envelope.clone());
        let write = answer(envelope);
        assert!(matches!(read, Err(TrackerError::BadResponse(text)) if text == r#"[{"message":"Could not resolve"}]"#));
        assert!(matches!(write, Err(TrackerError::Refused(text)) if text == "Could not resolve"));
        // `errors: []`: a success for a write, an error for a read, as today.
        let empty = json!({"data": {"x": 1}, "errors": []});
        assert!(answer(empty.clone()).is_ok());
        assert!(crate::GhTracker::data_of::<Value>(empty).is_err());
    }

    /// Difference 1 of #595: a write takes a place among the running calls, as a read does. The `gh` here is
    /// this test binary, which fails at once on `api graphql --input=-` (a run that fails is enough): with the
    /// only place held, `request` does not start it until the place is given back. The wait for "it did not
    /// answer" can only pass too soon on a slow machine, never fail for it.
    #[test]
    fn a_write_waits_for_a_place() {
        use std::sync::mpsc;
        use std::time::Duration;

        let tracker = crate::tests::tracker_running_this_binary(1);
        let place = tracker.calls.start();
        let (done, hear) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| done.send(tracker.request(REOPEN, tracker.issue_variables(1))).unwrap());
            assert!(hear.recv_timeout(Duration::from_millis(1500)).is_err(), "the write ran without a place");
            drop(place);
            let result = hear.recv_timeout(Duration::from_secs(60)).expect("the write runs once it has a place");
            assert!(matches!(result, Err(TrackerError::Unavailable(ref text)) if text.starts_with("gh api graphql: ")), "{result:?}");
        });
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
