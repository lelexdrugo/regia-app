//! What the fake `gh` refuses (#559, #435/V4 and #439/D2): a query that does not read the connections as
//! the prototype does. The queries are the product's own, read from its source, so the test cannot drift from
//! them; a mutated copy stands for the wrong query a later slice could write.

use std::fs;
use std::path::Path;

use regia_conformance::{Case, bin_dir, check_connections, project_query, responses_var, workspace_root};
use serde_json::{Value, json};

const FAKE_GH: &str = env!("CARGO_BIN_EXE_fake-gh");

/// The product's list-of-issues query, with the filter of the open ones, as `regia-github` sends it.
fn issues_query() -> String {
    let source = fs::read_to_string(workspace_root().join("crates/regia-github/src/lib.rs")).unwrap().replace("\r\n", "\n");
    let start = source.find("const ISSUES_QUERY: &str = r#\"").expect("the query") + "const ISSUES_QUERY: &str = r#\"".len();
    let end = start + source[start..].find("\"#;").expect("its end");
    source[start..end].replace("{STATES}", "states:[OPEN],")
}

/// The wrong queries: each is the product's with one change, and the word the fake's reason must have.
fn mutations(query: &str) -> Vec<(&'static str, String, &'static str)> {
    let swap = |from: &str, to: &str| {
        assert!(query.contains(from), "the product's query no longer has {from}");
        query.replacen(from, to, 1)
    };
    vec![
        ("one item of the Project instead of five", swap("projectItems(first:5)", "projectItems(first:1)"), "projectItems"),
        ("six items instead of five", swap("projectItems(first:5)", "projectItems(first:6)"), "projectItems"),
        ("ten issues' labels", swap("labels(first:20)", "labels(first:10)"), "labels"),
        ("fewer sub-issues", swap("subIssues(first:50)", "subIssues(first:10)"), "subIssues"),
        ("more blockers than the prototype reads", swap("blockedBy(first:20)", "blockedBy(first:100)"), "blockedBy"),
        ("fewer field values", swap("fieldValues(first:50)", "fieldValues(first:5)"), "fieldValues"),
        ("a page of fewer issues", swap("issues(first:100,", "issues(first:50,"), "issues"),
        ("the cursor dropped", swap("first:100,after:$after,", "first:100,"), "issues"),
        ("another cursor", swap("after:$after", "after:$cursor"), "issues"),
        ("no word on the next page", swap("pageInfo{hasNextPage endCursor}", "pageInfo{endCursor}"), "pageInfo"),
        ("a fragment on another type", swap("... on ProjectV2ItemFieldSingleSelectValue", "... on ProjectV2ItemFieldTextValue"), "ProjectV2ItemFieldTextValue"),
        ("labels without a pagination", swap("labels(first:20)", "labels"), "labels"),
        ("blockers without a pagination", swap("blockedBy(first:20)", "blockedBy"), "blockedBy"),
        ("the owner and the repo swapped", swap("repository(owner:$o,name:$r)", "repository(owner:$r,name:$o)"), "repository"),
        ("a page asked backwards", swap("issues(first:100,", "issues(before:$after,first:100,"), "issues"),
        ("a connection the prototype never reads", swap("parent{number}", "parent{number} assignees(first:5){nodes{id}}"), "assignees"),
    ]
}

#[test]
fn a_query_that_does_not_read_the_connections_as_the_prototype_does_is_refused() {
    let query = issues_query();
    assert_eq!(check_connections(&query), Ok(()), "the product's own query is refused");
    for (what, wrong, word) in mutations(&query) {
        let refusal = check_connections(&wrong).err().unwrap_or_else(|| panic!("passed: {what}"));
        assert!(refusal.contains(word), "{what}: the reason does not say {word}: {refusal}");
    }
}

/// An object read with nothing selected in it is an error on the real API; the fake used to copy the whole
/// fixture value (D14). A scalar, and a list of scalars, need no selection.
#[test]
fn an_object_with_no_selection_is_refused_and_a_scalar_is_not() {
    let page = json!({"labels": {"nodes": [{"name": "a"}]}, "title": "t", "names": ["a", "b"]});
    let refusal = project_query("{labels}", &page).expect_err("an object with nothing selected");
    assert!(refusal.contains("labels"), "{refusal}");
    assert_eq!(project_query("{labels{nodes{name}} title names}", &page).unwrap(), page);
}

/// The fake itself, started as the product starts it: the request on stdin. The same fixture answers the
/// product's query and refuses the mutated ones, so a fixture richer than the query cannot hide the change.
#[test]
fn the_fake_gh_answers_the_products_query_and_refuses_the_wrong_ones() {
    let case = Case::named("core");
    let dir = bin_dir().join("conformance-state").join("fake-gh-shape");
    fs::create_dir_all(&dir).unwrap();
    let ask = |query: &str| {
        let body = json!({"query": query, "variables": {"o": "example", "r": "tracker"}}).to_string();
        regia_exec::harness::launch(Path::new(FAKE_GH), &["api".to_owned(), "graphql".to_owned(), "--input=-".to_owned()], &dir)
            .env(&responses_var(), case.responses_path())
            .stdin(body)
            .run()
            .expect("the fake runs")
    };
    let query = issues_query();
    let answered = ask(&query);
    assert!(answered.status.success(), "{}", String::from_utf8_lossy(&answered.stderr));
    let page: Value = serde_json::from_slice(&answered.stdout).unwrap();
    assert!(page["data"]["repository"]["issues"]["nodes"].as_array().is_some_and(|nodes| !nodes.is_empty()));
    for (what, wrong, word) in mutations(&query) {
        let refused = ask(&wrong);
        assert!(!refused.status.success(), "the fake answered: {what}");
        let message = String::from_utf8_lossy(&refused.stderr);
        assert!(message.contains(word), "{what}: {message}");
    }
}
