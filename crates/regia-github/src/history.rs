//! The changes of Status of the issues with a label, open and closed, as the prototype's
//! `nuove_ultimi_7_giorni` reads them to count a week's Proposals: its query `_NUOVE_Q`, a page of 100
//! issues at a time. Like the prototype, it reads at most 50 changes of an issue, and from any Project.

use chrono::{DateTime, SecondsFormat, Utc};
use regia_core::{StatusHistory, TrackerError};
use serde::Deserialize;
use serde_json::json;

use crate::{GhTracker, LabelNode, Nodes, PageInfo};

/// The prototype's `_NUOVE_Q`, with the label as a variable where the prototype writes it into the text.
const HISTORIES_QUERY: &str = r#"query($o:String!,$r:String!,$label:String!,$since:DateTime!,$after:String){repository(owner:$o,name:$r){
  issues(first:100,after:$after,filterBy:{labels:[$label]}){
    pageInfo{hasNextPage endCursor}
    nodes{number labels(first:20){nodes{name}}
      timelineItems(first:50,itemTypes:[PROJECT_V2_ITEM_STATUS_CHANGED_EVENT],since:$since){
        nodes{... on ProjectV2ItemStatusChangedEvent{createdAt status}}}}}}}"#;

impl GhTracker {
    /// The changes of Status since `since` of every issue labelled `label`.
    pub(crate) fn read_status_histories(
        &self,
        label: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<StatusHistory>, TrackerError> {
        let variables = [
            ("label", json!(label)),
            ("since", json!(iso_seconds(since))),
        ];
        histories(|after| self.graphql_with(HISTORIES_QUERY, &variables, after))
    }
}

/// `since` as the prototype writes it with `strftime("%Y-%m-%dT%H:%M:%SZ")`: ISO 8601, cut to the second, with `Z`.
fn iso_seconds(since: DateTime<Utc>) -> String {
    since.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Every page from the first, each asked with the cursor the one before ended at. A page that fails
/// fails the whole reading: the prototype then has no weekly count, never the count of the pages read.
fn histories(
    mut page: impl FnMut(Option<&str>) -> Result<HistoriesData, TrackerError>,
) -> Result<Vec<StatusHistory>, TrackerError> {
    let mut histories = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let issues = page(after.as_deref())?.repository.issues;
        histories.extend(issues.nodes.into_iter().map(HistoryNode::into_history));
        if !issues.page_info.has_next_page {
            return Ok(histories);
        }
        after = issues.page_info.end_cursor;
    }
}

#[derive(Deserialize)]
struct HistoriesData {
    repository: HistoriesRepository,
}

#[derive(Deserialize)]
struct HistoriesRepository {
    issues: HistoryPage,
}

#[derive(Deserialize)]
struct HistoryPage {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<HistoryNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryNode {
    number: u64,
    labels: Nodes<LabelNode>,
    timeline_items: Nodes<StatusChange>,
}

/// A `ProjectV2ItemStatusChangedEvent`: the name of the option the Status took, and when.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusChange {
    created_at: String,
    status: Option<String>,
}

impl HistoryNode {
    /// Every issue, an Objective too: the core leaves out what does not count. The changes keep the order
    /// they came in, oldest first.
    fn into_history(self) -> StatusHistory {
        let labels = self.labels.nodes.into_iter().map(|label| label.name);
        let changes = self.timeline_items.nodes.into_iter();
        StatusHistory {
            number: self.number,
            labels: labels.collect(),
            changes: changes.map(StatusChange::into_change).collect(),
        }
    }
}

impl StatusChange {
    /// GraphQL declares `status` as `String!`, as it does `previousStatus`, which is `""` before an issue's
    /// first Status: an emptied Status comes as `""`, and is `None` here, as a `null` would be.
    fn into_change(self) -> (Option<String>, String) {
        (self.status.filter(|name| !name.is_empty()), self.created_at)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    /// `_NUOVE_Q` as `tools/politica_budget.py` of the dossier has it: `LABEL` is replaced by the label, in
    /// JSON, before the query is sent.
    const PROTOTYPE_QUERY: &str = r#"query($o:String!,$r:String!,$since:DateTime!,$after:String){repository(owner:$o,name:$r){
  issues(first:100,after:$after,filterBy:{labels:[LABEL]}){
    pageInfo{hasNextPage endCursor}
    nodes{number labels(first:20){nodes{name}}
      timelineItems(first:50,itemTypes:[PROJECT_V2_ITEM_STATUS_CHANGED_EVENT],since:$since){
        nodes{... on ProjectV2ItemStatusChangedEvent{createdAt status}}}}}}}"#;

    /// The labels of a task of project `p`.
    const TASK: &[&str] = &["progetto:p"];

    type Read = Result<Vec<StatusHistory>, TrackerError>;

    /// What GraphQL answers for a page of `nodes`, with the cursor of the next page if there is one.
    fn answer(nodes: &[Value], next: Option<&str>) -> Vec<u8> {
        let page_info = json!({"hasNextPage": next.is_some(), "endCursor": next.unwrap_or("last")});
        json!({"data": {"repository": {"issues": {"pageInfo": page_info, "nodes": nodes}}}})
            .to_string()
            .into_bytes()
    }

    /// An issue with its labels and its changes, each a (status, createdAt).
    fn issue(number: u64, labels: &[&str], changes: &[(Value, &str)]) -> Value {
        let labels: Vec<Value> = labels.iter().map(|name| json!({"name": name})).collect();
        let changes: Vec<Value> = changes
            .iter()
            .map(|(status, at)| json!({"createdAt": at, "status": status}))
            .collect();
        json!({"number": number, "labels": {"nodes": labels}, "timelineItems": {"nodes": changes}})
    }

    /// What the reading gives on these answers, one a page, and the cursor each page was asked with.
    fn reading(answers: Vec<Vec<u8>>) -> (Read, Vec<Option<String>>) {
        let mut asked = Vec::new();
        let mut answers = answers.into_iter();
        let read = histories(|after| {
            asked.push(after.map(str::to_owned));
            GhTracker::data(&answers.next().expect("no more pages than answers"))
        });
        (read, asked)
    }

    fn numbers(histories: &[StatusHistory]) -> Vec<u64> {
        histories.iter().map(|history| history.number).collect()
    }

    /// `since` goes to GitHub as the prototype sends it: to the second, cut and not rounded, with `Z`.
    #[test]
    fn since_is_written_to_the_second_with_z() {
        let since = DateTime::parse_from_rfc3339("2026-11-05T16:40:00.999+01:00").unwrap();
        assert_eq!(
            iso_seconds(since.with_timezone(&Utc)),
            "2026-11-05T15:40:00Z"
        );
    }

    /// The query is the prototype's, which reads closed issues too: only the label becomes a variable.
    #[test]
    fn the_query_is_the_prototype_s_with_the_label_as_a_variable() {
        let expected = PROTOTYPE_QUERY
            .replace("$since:DateTime!", "$label:String!,$since:DateTime!")
            .replace("[LABEL]", "[$label]");
        assert_eq!(HISTORIES_QUERY, expected);
    }

    /// The second page is asked with the cursor the first ended at, and the issues keep the tracker's order.
    #[test]
    fn two_pages_follow_the_cursor() {
        let first = answer(&[issue(3, TASK, &[]), issue(5, TASK, &[])], Some("c1"));
        let inbox = [(json!("Inbox"), "2026-01-06T10:00:00Z")];
        let second = answer(&[issue(4, TASK, &inbox)], None);
        let (read, asked) = reading(vec![first, second]);
        assert_eq!(numbers(&read.unwrap()), [3, 5, 4]);
        assert_eq!(asked, [None, Some("c1".to_owned())]);
    }

    /// An issue whose Status did not change since `since` is still read, with no change.
    #[test]
    fn an_issue_without_changes_is_read_with_none() {
        let (read, _) = reading(vec![answer(&[issue(7, TASK, &[])], None)]);
        assert_eq!(
            read.unwrap(),
            [StatusHistory {
                number: 7,
                labels: vec!["progetto:p".to_owned()],
                changes: vec![]
            }]
        );
    }

    /// The changes keep the order they came in; an emptied Status is `None`, written `null` or `""`.
    #[test]
    fn an_emptied_status_is_none() {
        let changes = [
            (json!("Proposta"), "2026-01-05T10:00:00Z"),
            (Value::Null, "2026-01-06T10:00:00Z"),
            (json!(""), "2026-01-07T10:00:00Z"),
            (json!("Approvato"), "2026-01-08T10:00:00Z"),
        ];
        let (read, _) = reading(vec![answer(&[issue(9, TASK, &changes)], None)]);
        let at = |day: u8| format!("2026-01-0{day}T10:00:00Z");
        let named = |name: &str| Some(name.to_owned());
        assert_eq!(
            read.unwrap()[0].changes,
            [
                (named("Proposta"), at(5)),
                (None, at(6)),
                (None, at(7)),
                (named("Approvato"), at(8))
            ]
        );
    }

    /// The adapter does not filter: an Objective stays, with its labels, for the core to leave out.
    #[test]
    fn an_objective_stays_with_its_labels() {
        let proposal = [(json!("Proposta"), "2026-01-05T10:00:00Z")];
        let objective = issue(1, &["progetto:p", "tipo:obiettivo"], &proposal);
        let page = answer(&[objective, issue(2, TASK, &proposal)], None);
        let read = reading(vec![page]).0.unwrap();
        assert_eq!(numbers(&read), [1, 2]);
        assert_eq!(read[0].labels, ["progetto:p", "tipo:obiettivo"]);
        assert_eq!(read[0].changes, read[1].changes);
    }

    /// An answer with `errors`, or one that does not read, is a bad response on any page, and the reading
    /// keeps nothing of the pages before it.
    #[test]
    fn an_answer_with_errors_is_a_bad_response() {
        let errors = br#"{"data":null,"errors":[{"type":"NOT_FOUND","message":"Could not resolve to a Repository"}]}"#;
        let (read, _) = reading(vec![errors.to_vec()]);
        assert!(
            matches!(read, Err(TrackerError::BadResponse(message)) if message.contains("Could not resolve"))
        );
        let first = answer(&[issue(3, TASK, &[])], Some("c1"));
        let (read, asked) = reading(vec![first.clone(), errors.to_vec()]);
        assert!(
            matches!(read, Err(TrackerError::BadResponse(_))),
            "{read:?}"
        );
        assert_eq!(asked.len(), 2);
        let (read, _) = reading(vec![first, b"<html>".to_vec()]);
        assert!(
            matches!(read, Err(TrackerError::BadResponse(_))),
            "{read:?}"
        );
    }
}
