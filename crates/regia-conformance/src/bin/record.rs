//! Local only. Expands each synthetic scenario into GraphQL responses, runs the Python prototype on
//! them through the fake `gh`, and stores its raw output. The prototype is not modified and the real
//! tracker is never read: a scenario's URLs must come back, or recording stops.
//!
//!     cargo run -p regia-conformance --bin record -- --dossier <path> [--python <interpreter>]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{Case, fixture_for_python, fixtures_dir, log_var, prepare_state_dir, read_json};
use regia_core::TrackerConfigV1;
use regia_exec::git::{self, CommitField};
use regia_exec::{Args, DossierPath, GitRef, Pin, Program};
use regia_facade::{PythonPrototype, Table};
use serde::Deserialize;
use serde_json::{Value, json};

const URL_PREFIX: &str = "https://tracker.invalid/";

#[derive(Deserialize)]
struct Scenario {
    project_id: String,
    page_size: usize,
    issues: Vec<ScenarioIssue>,
    #[serde(default)]
    extra_blockers: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct ScenarioIssue {
    number: u64,
    title: String,
    /// The label of the status option, as the tracker shows it.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    parent: Option<u64>,
    #[serde(default)]
    sub: Vec<(u64, String)>,
    #[serde(default)]
    blocked_by: Vec<(u64, String)>,
    #[serde(default)]
    blocked_by_total: Option<u64>,
    #[serde(default)]
    state_reason: Option<String>,
    #[serde(default)]
    also_in_other_project: bool,
    #[serde(default)]
    body: Option<String>,
}

fn pairs(list: &[(u64, String)]) -> Vec<Value> {
    list.iter().map(|(number, state)| json!({"number": number, "state": state})).collect()
}

fn issue_node(issue: &ScenarioIssue, scenario: &Scenario, config: &TrackerConfigV1) -> Value {
    // Both ways of reading the status: the prototype by the field's name, the product by its id.
    let mut items = Vec::new();
    if issue.also_in_other_project {
        items.push(json!({
            "project": {"id": "OTHER_PROJECT"},
            "fieldValueByName": {"name": "Inbox", "optionId": "other-project-option"},
            "fieldValues": {"nodes": [{"optionId": "other-project-option", "field": {"id": "OTHER_FIELD"}}]},
        }));
    }
    if let Some(status) = &issue.status {
        let option = config.status_options.get(status).unwrap_or_else(|| panic!("unknown status {status}"));
        items.push(json!({
            "project": {"id": scenario.project_id},
            "fieldValueByName": {"name": status, "optionId": option},
            "fieldValues": {"nodes": [
                {"title": "a text field, not a single select"},
                {"optionId": "opt-another-select", "field": {"id": "OTHER_FIELD"}},
                {"optionId": option, "field": {"id": config.status_field_id}},
            ]},
        }));
    }
    let labels: Vec<Value> = issue.labels.iter().map(|name| json!({"name": name})).collect();
    json!({
        "number": issue.number,
        "title": issue.title,
        "state": "OPEN",
        "stateReason": issue.state_reason,
        "body": issue.body.clone().unwrap_or_default(),
        "url": format!("{URL_PREFIX}issues/{}", issue.number),
        "updatedAt": "2026-01-02T03:04:05Z",
        "createdAt": "2026-01-01T00:00:00Z",
        "closedAt": null,
        "labels": {"nodes": labels},
        "parent": issue.parent.map(|number| json!({"number": number})),
        "subIssues": {"nodes": pairs(&issue.sub)},
        "blockedBy": {
            "totalCount": issue.blocked_by_total.unwrap_or(issue.blocked_by.len() as u64),
            "nodes": pairs(&issue.blocked_by),
        },
        "projectItems": {"nodes": items},
    })
}

fn responses_of(scenario: &Scenario, config: &TrackerConfigV1) -> Value {
    let chunks: Vec<&[ScenarioIssue]> = if scenario.issues.is_empty() {
        vec![&[]]
    } else {
        scenario.issues.chunks(scenario.page_size).collect()
    };
    let pages = chunks.len();
    let issue_pages: Vec<Value> = chunks
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let nodes: Vec<Value> = chunk.iter().map(|issue| issue_node(issue, scenario, config)).collect();
            let more = i + 1 < pages;
            json!({"repository": {"issues": {
                "pageInfo": {"hasNextPage": more, "endCursor": more.then(|| format!("page-{}", i + 1))},
                "nodes": nodes,
            }}})
        })
        .collect();
    let mut blocker_pages = serde_json::Map::new();
    for (number, value) in &scenario.extra_blockers {
        let entry = match value {
            Value::String(_) => value.clone(),
            Value::Array(extra_pages) => {
                let count = extra_pages.len();
                Value::Array(
                    extra_pages
                        .iter()
                        .enumerate()
                        .map(|(i, page)| {
                            let nodes: Vec<Value> = page
                                .as_array()
                                .expect("a page is a list")
                                .iter()
                                .map(|pair| json!({"number": pair[0], "state": pair[1]}))
                                .collect();
                            let more = i + 1 < count;
                            json!({"repository": {"issue": {"blockedBy": {
                                "pageInfo": {"hasNextPage": more, "endCursor": more.then(|| format!("page-{}", i + 1))},
                                "nodes": nodes,
                            }}}})
                        })
                        .collect(),
                )
            }
            other => panic!("extra_blockers.{number}: unexpected {other}"),
        };
        blocker_pages.insert(number.clone(), entry);
    }
    json!({
        "expect": {"o": config.owner, "r": config.repo},
        "issue_pages": issue_pages,
        "blocker_pages": blocker_pages,
    })
}

fn git_output(dossier: &Path, args: Args) -> String {
    let output = Pin::resolve(Program::Git).expect("git").launch(args, dossier).run().expect("git");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write_json(path: &Path, value: &Value) {
    let text = serde_json::to_string_pretty(value).expect("json");
    fs::write(path, format!("{text}\n")).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter().position(|arg| arg == name).map(|i| args[i + 1].clone())
    };
    let dossier = PathBuf::from(flag("--dossier").expect("--dossier <path>"));
    let prototype = PythonPrototype::in_dossier(&dossier, flag("--python"));
    assert!(prototype.script.is_file(), "{} not found", prototype.script.display());
    let config = TrackerConfigV1::read(&fixtures_dir().join("tracker.json")).expect("tracker.json");
    let table = Table::embedded();

    for case in Case::all() {
        let scenario: Scenario = read_json(&fixtures_dir().join("scenarios").join(format!("{}.json", case.spec.scenario)));
        assert_eq!(scenario.project_id, config.project_id, "scenario and tracker.json must agree");
        write_json(&case.responses_path(), &responses_of(&scenario, &config));

        let state = prepare_state_dir(&format!("record-{}", case.name));
        let log = state.join("gh.log");
        let argv = table
            .python_argv(case.command_name(), &case.spec.args[1..])
            .expect("the table knows the case's arguments");
        let launch = prototype.launch(&argv).expect("the prototype");
        let output = fixture_for_python(launch, &case.responses_path(), &state)
            .env(&log_var(), &log)
            .run()
            .expect("run the prototype");
        assert!(
            output.status.success(),
            "{}: the prototype failed: {}",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );

        // Proof that the fake answered and no real data came through.
        let calls = fs::read_to_string(&log).unwrap_or_default();
        assert!(!calls.is_empty(), "{}: the fake gh was not called", case.name);
        let python: Value = serde_json::from_slice(&output.stdout).expect("the prototype printed JSON");
        let text = String::from_utf8_lossy(&output.stdout);
        let urls = text.matches("\"url\"").count();
        assert_eq!(text.matches(&format!("\"url\": \"{URL_PREFIX}")).count(), urls, "{}: foreign URL", case.name);
        write_json(&case.dir.join("python.it.json"), &python);
        println!("{}: {} calls to the fake gh, {} URLs, all synthetic", case.name, calls.lines().count(), urls);
    }

    let head = GitRef::head();
    let script = DossierPath::parse("tools/regia.py").expect("a dossier path");
    write_json(
        &fixtures_dir().join("RECORDED.json"),
        &json!({
            "oracle_repository": "lelexdrugo/regia",
            "oracle_commit": git_output(&dossier, git::rev_parse(&head)),
            "oracle_last_change_to_regia_py": git_output(&dossier, git::last_commit(CommitField::Hash, &head, Some(&script))),
            "recorded_at": git_output(&dossier, git::last_commit(CommitField::Date, &head, None)),
        }),
    );
}
