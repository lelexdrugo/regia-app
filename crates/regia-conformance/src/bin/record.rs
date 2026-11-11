//! Local only. Expands each synthetic scenario into GraphQL responses, writes each case's state rows,
//! runs the Python prototype on them through the fake `gh`, at the fixtures' fixed instant, and stores
//! its raw output: JSON or text, its stderr if any, and the writes it made to the tracker. The
//! prototype is not modified and the real tracker is never read: a scenario's URLs must come back, or
//! recording stops, and so does an exit code other than the case's or an argv other than the one it
//! expects. `--family` records only the families named, so a slice leaves the others' files alone.
//!
//!     cargo run -p regia-conformance --bin record -- --dossier <path> [--python <interpreter>] [--family <name>]…

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{
    Case, FIXTURE_NOW, FIXTURE_TODAY, dossier_after, fixed_now_driver, fixture_for_python, fixtures_dir, lf, log_var,
    place_args, prepare_case_state, prepare_dossier, read_json, unplace_json, unplace_text, writes_of_run,
};
use regia_core::{TrackerBinding, TrackerConfigV1};
use regia_exec::git::{self, CommitField};
use regia_exec::{Args, DossierPath, GitRef, Pin, Program, python};
use regia_facade::{PROTOTYPE_LIMITS, PythonPrototype, Table};
use serde::Deserialize;
use serde_json::{Map, Value, json};

const URL_PREFIX: &str = "https://tracker.invalid/";

/// GitHub's name for the status field. The prototype reads the status by this name, the product by the
/// field's id.
const STATUS_FIELD_NAME: &str = "Status";
const PROJECT_FIELD_ID: &str = "FIELD_PROJECT_FIXTURE";

#[derive(Deserialize)]
struct Scenario {
    project_id: String,
    page_size: usize,
    issues: Vec<ScenarioIssue>,
    #[serde(default)]
    extra_blockers: BTreeMap<String, Value>,
    #[serde(default)]
    extra_sub_issues: BTreeMap<String, Value>,
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
    /// More sub-issues than the list query reads, with the rest in `extra_sub_issues`.
    #[serde(default)]
    sub_total: Option<u64>,
    #[serde(default)]
    blocked_by: Vec<(u64, String)>,
    #[serde(default)]
    blocked_by_total: Option<u64>,
    #[serde(default)]
    state_reason: Option<String>,
    /// A closed issue is in the list of every state only, with this date of closing.
    #[serde(default)]
    closed_at: Option<String>,
    #[serde(default)]
    also_in_other_project: bool,
    #[serde(default)]
    body: Option<String>,
    /// The option of the Project field that mirrors the project label, if the item has one.
    #[serde(default)]
    project_field: Option<String>,
    /// Recent comments, which the checks read.
    #[serde(default)]
    comments: Vec<ScenarioComment>,
}

#[derive(Deserialize)]
struct ScenarioComment {
    id: u64,
    created_at: String,
    body: String,
}

/// The checks' read of recent comments: one page with the issues that have comments.
fn comment_pages(scenario: &Scenario) -> Value {
    let nodes: Vec<Value> = scenario
        .issues
        .iter()
        .filter(|issue| !issue.comments.is_empty())
        .map(|issue| {
            let labels: Vec<Value> = issue.labels.iter().map(|name| json!({"name": name})).collect();
            let comments: Vec<Value> = issue
                .comments
                .iter()
                .map(|comment| {
                    let url = format!("{URL_PREFIX}issues/{}#issuecomment-{}", issue.number, comment.id);
                    json!({"databaseId": comment.id, "createdAt": comment.created_at, "body": comment.body, "url": url})
                })
                .collect();
            json!({"number": issue.number, "state": "OPEN", "closedAt": null, "labels": {"nodes": labels}, "comments": {"nodes": comments}})
        })
        .collect();
    json!([{"repository": {"issues": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": nodes}}}])
}

fn pairs(list: &[(u64, String)]) -> Vec<Value> {
    list.iter().map(|(number, state)| json!({"number": number, "state": state})).collect()
}

/// A single-select value, both as `fieldValues` lists it and as `fieldValueByName` answers it.
fn select_value(field_id: &str, field_name: &str, option_id: &str, option: &str) -> (Value, Value) {
    let node = json!({"optionId": option_id, "name": option, "field": {"id": field_id, "name": field_name}});
    (node, json!({"name": option, "optionId": option_id}))
}

/// An item of a Project. Both ways of reading a field are there: the prototype asks by the field's name,
/// the product reads the list with each field's id and name. Values of other fields and of a field that is
/// not a single select are in the list too, and must not count.
fn project_item(project_id: &str, values: &[(Value, Value, &str)]) -> Value {
    let mut nodes = vec![
        json!({"title": "a text field, not a single select"}),
        json!({"optionId": "opt-another-select", "name": "Medium", "field": {"id": "OTHER_FIELD", "name": "Priority"}}),
    ];
    let mut by_name = Map::new();
    for (node, answer, field_name) in values {
        nodes.push(node.clone());
        by_name.insert((*field_name).to_owned(), answer.clone());
    }
    json!({"project": {"id": project_id}, "fieldValueByName": by_name, "fieldValues": {"nodes": nodes}})
}

fn issue_node(issue: &ScenarioIssue, scenario: &Scenario, config: &TrackerConfigV1, binding: &TrackerBinding) -> Value {
    let project_field = binding.project_field_name();
    let mut items = Vec::new();
    if issue.also_in_other_project {
        let (status_node, status) = select_value("OTHER_FIELD", STATUS_FIELD_NAME, "other-project-option", "Inbox");
        let (project_node, project) = select_value("OTHER_PROJECT_FIELD", project_field, "other-project-value", "omega");
        items.push(project_item(
            "OTHER_PROJECT",
            &[(status_node, status, STATUS_FIELD_NAME), (project_node, project, project_field)],
        ));
    }
    let mut values = Vec::new();
    if let Some(status) = &issue.status {
        let option = config.status_options.get(status).unwrap_or_else(|| panic!("unknown status {status}"));
        let (node, answer) = select_value(&config.status_field_id, STATUS_FIELD_NAME, option, status);
        values.push((node, answer, STATUS_FIELD_NAME));
    }
    if let Some(value) = &issue.project_field {
        let (node, answer) = select_value(PROJECT_FIELD_ID, project_field, &format!("opt-project-{value}"), value);
        values.push((node, answer, project_field));
    }
    if !values.is_empty() {
        items.push(project_item(&scenario.project_id, &values));
    }
    let labels: Vec<Value> = issue.labels.iter().map(|name| json!({"name": name})).collect();
    json!({
        "number": issue.number,
        "title": issue.title,
        "state": state(issue),
        "stateReason": issue.state_reason,
        "body": issue.body.clone().unwrap_or_default(),
        "url": format!("{URL_PREFIX}issues/{}", issue.number),
        "updatedAt": "2026-01-02T03:04:05Z",
        "createdAt": "2026-01-01T00:00:00Z",
        "closedAt": issue.closed_at,
        "labels": {"nodes": labels},
        "parent": issue.parent.map(|number| json!({"number": number})),
        "subIssues": {
            "totalCount": issue.sub_total.unwrap_or(issue.sub.len() as u64),
            "nodes": pairs(&issue.sub),
        },
        "blockedBy": {
            "totalCount": issue.blocked_by_total.unwrap_or(issue.blocked_by.len() as u64),
            "nodes": pairs(&issue.blocked_by),
        },
        "projectItems": {"nodes": items},
    })
}

/// The pages of a connection read one issue at a time (`blockedBy`, `subIssues`), from a scenario's lists
/// of `[number, state]` pairs. A string instead of the pages makes the read fail.
fn extra_pages(extra: &BTreeMap<String, Value>, connection: &str) -> Map<String, Value> {
    let mut out = Map::new();
    for (number, value) in extra {
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
                            json!({"repository": {"issue": {connection: {
                                "pageInfo": {"hasNextPage": more, "endCursor": more.then(|| format!("page-{}", i + 1))},
                                "nodes": nodes,
                            }}}})
                        })
                        .collect(),
                )
            }
            other => panic!("extra {connection} of #{number}: unexpected {other}"),
        };
        out.insert(number.clone(), entry);
    }
    out
}

fn state(issue: &ScenarioIssue) -> &'static str {
    if issue.closed_at.is_some() { "CLOSED" } else { "OPEN" }
}

/// The pages of the list of `issues`, `page_size` at a time.
fn list_pages(issues: &[&ScenarioIssue], scenario: &Scenario, config: &TrackerConfigV1, binding: &TrackerBinding) -> Vec<Value> {
    let chunks: Vec<&[&ScenarioIssue]> = if issues.is_empty() { vec![&[]] } else { issues.chunks(scenario.page_size).collect() };
    let pages = chunks.len();
    chunks
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let nodes: Vec<Value> = chunk.iter().map(|issue| issue_node(issue, scenario, config, binding)).collect();
            let more = i + 1 < pages;
            json!({"repository": {"issues": {
                "pageInfo": {"hasNextPage": more, "endCursor": more.then(|| format!("page-{}", i + 1))},
                "nodes": nodes,
            }}})
        })
        .collect()
}

/// The node id the fixture gives an issue.
fn issue_id(number: u64) -> String {
    format!("ISSUE_{number}")
}

/// Each issue by its number, as one read of it answers: its id and state, the count of its sub-issues,
/// its comments in one page.
fn issues_by_number(scenario: &Scenario) -> Map<String, Value> {
    scenario
        .issues
        .iter()
        .map(|issue| {
            let comments: Vec<Value> = issue
                .comments
                .iter()
                .map(|comment| {
                    let url = format!("{URL_PREFIX}issues/{}#issuecomment-{}", issue.number, comment.id);
                    json!({"databaseId": comment.id, "createdAt": comment.created_at, "url": url, "body": comment.body})
                })
                .collect();
            let entry = json!({
                "id": issue_id(issue.number),
                "state": state(issue),
                "subIssues": {"totalCount": issue.sub_total.unwrap_or(issue.sub.len() as u64)},
                "comments": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": comments},
            });
            (issue.number.to_string(), entry)
        })
        .collect()
}

fn responses_of(scenario: &Scenario, config: &TrackerConfigV1, binding: &TrackerBinding) -> Value {
    let all: Vec<&ScenarioIssue> = scenario.issues.iter().collect();
    let open: Vec<&ScenarioIssue> = all.iter().copied().filter(|issue| issue.closed_at.is_none()).collect();
    json!({
        "expect": {"o": config.owner, "r": config.repo},
        "issue_pages": list_pages(&open, scenario, config, binding),
        "all_pages": list_pages(&all, scenario, config, binding),
        "issues": issues_by_number(scenario),
        "blocker_pages": extra_pages(&scenario.extra_blockers, "blockedBy"),
        "sub_issue_pages": extra_pages(&scenario.extra_sub_issues, "subIssues"),
        "comment_pages": comment_pages(scenario),
        // No scenario that runs the checks has a closed issue.
        "closed_pages": [{"repository": {"issues": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": []}}}],
    })
}

fn git_output(dossier: &Path, args: Args) -> String {
    let output = Pin::resolve(Program::Git).expect("git").launch(args, dossier).run().expect("git");
    // An empty answer must mean nothing to report, never a git that failed.
    assert!(output.status.success(), "git failed: {}", String::from_utf8_lossy(&output.stderr));
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
    let only: BTreeSet<String> = args.windows(2).filter(|pair| pair[0] == "--family").map(|pair| pair[1].clone()).collect();
    // Absolute, because the prototype runs from the dossier's root and the driver takes the script's path.
    let dossier = std::path::absolute(PathBuf::from(flag("--dossier").expect("--dossier <path>"))).expect("the dossier");
    let prototype = PythonPrototype::in_dossier(&dossier, flag("--python"));
    assert!(prototype.script.is_file(), "{} not found", prototype.script.display());
    // RECORDED.json cites a commit: the prototype that answers must be that commit, not a working copy.
    let tools = DossierPath::parse("tools").expect("a dossier path");
    let local_changes = git_output(&dossier, git::status(&tools));
    assert!(local_changes.is_empty(), "the dossier's tools/ has local changes:\n{local_changes}");
    let config = TrackerConfigV1::read(&fixtures_dir().join("tracker.json")).expect("tracker.json");
    let binding = TrackerBinding::new(&config).expect("tracker.json binds every status");
    let table = Table::embedded();
    // The interpreter, its flags and its limits are the facade's: the oracle answers as the product
    // would start it. Only the driver comes in between, to fix the clock.
    let interpreter = prototype.interpreter().expect("a Python interpreter");
    let driver = fixed_now_driver().display().to_string();
    let script = prototype.script.display().to_string();
    let owner_url = format!("https://github.com/{}/", config.owner);

    let mut families = BTreeSet::new();
    let cases: Vec<Case> = Case::all().into_iter().filter(|case| only.is_empty() || only.contains(&case.family)).collect();
    let known: BTreeSet<&String> = cases.iter().map(|case| &case.family).collect();
    assert!(only.iter().all(|family| known.contains(family)), "--family names a family without cases: {only:?}");
    for case in cases {
        let responses = match &case.spec.scenario {
            Some(name) => {
                let scenario: Scenario = read_json(&case.family_dir().join("scenarios").join(format!("{name}.json")));
                assert_eq!(scenario.project_id, config.project_id, "scenario and tracker.json must agree");
                responses_of(&scenario, &config, &binding)
            }
            None => json!({"expect": {"o": config.owner, "r": config.repo}}),
        };
        write_json(&case.responses_path(), &responses);

        let state = prepare_case_state(&format!("record-{}", case.name), &case);
        let log = state.join("gh.log");
        let command = case.command_name();
        let words = command.split(' ').count();
        // The placeholder of the dossier is a value like any other for the table.
        let argv = table.python_argv(&command, &case.spec.args[words..]).expect("the table knows the case's arguments");
        if let Some(expected) = &case.spec.python {
            assert_eq!(&argv, expected, "{}: the table does not build the argv the case expects", case.name);
        }
        let folder = case.spec.dossier.as_ref().map(|spec| prepare_dossier(&state, spec));
        let mut launch_args: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
        launch_args.extend([driver.clone(), script.clone(), FIXTURE_NOW.to_owned()]);
        launch_args.extend(folder.as_ref().map_or_else(|| argv.clone(), |folder| place_args(&argv, folder)));
        let launch = regia_exec::harness::program(&interpreter, &launch_args, &dossier).limits(PROTOTYPE_LIMITS);
        let mut launch = fixture_for_python(launch, &case.responses_path(), &state).env(&log_var(), &log);
        for (name, value) in case.spec.session_env() {
            launch = launch.env(&name, value);
        }
        let output = launch.run().expect("run the prototype");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(output.status.code(), Some(i32::from(case.spec.exit)), "{}: the prototype ended otherwise: {stderr}", case.name);

        // Proof that the fake answered and no real data came through.
        let calls = fs::read_to_string(&log).unwrap_or_default();
        assert!(case.spec.scenario.is_none() || !calls.is_empty(), "{}: the fake gh was not called", case.name);
        let writes = writes_of_run(&state);
        let text = String::from_utf8(output.stdout).expect("the prototype printed UTF-8");
        let recorded = [text.as_str(), stderr.as_str(), &serde_json::to_string(&writes).expect("JSON")].concat();
        // The prototype builds the URL of an issue it creates, and of a repo, from the tracker's owner:
        // those of the fixture's owner are synthetic too.
        let urls = recorded.matches("https://").count() + recorded.matches("http://").count();
        let synthetic = recorded.matches(URL_PREFIX).count() + recorded.matches(&owner_url).count();
        assert_eq!(synthetic, urls, "{}: foreign URL", case.name);
        // An error prints nothing on stdout, even with `--json`: the facade passes it as text, empty.
        let json = argv.first().is_some_and(|arg| arg == "--json") && table.has_output(&command) && !text.trim().is_empty();
        let parsed = json.then(|| serde_json::from_str::<Value>(&text).expect("the prototype printed JSON"));
        // No path of this machine is stored: the dossier's folder becomes the placeholder.
        let (parsed, text, stderr) = match &folder {
            Some(folder) => (parsed.map(|value| unplace_json(&value, folder)), unplace_text(&text, folder), unplace_text(&stderr, folder)),
            None => (parsed, text, stderr),
        };
        replace(&case.dir.join("python.it.json"), parsed);
        replace_text(&case.dir.join("python.it.txt"), (!json).then(|| lf(&text)).as_deref());
        replace_text(&case.dir.join("python.it.stderr.txt"), Some(lf(&stderr)).filter(|text| !text.is_empty()).as_deref());
        replace(&case.dir.join("calls.json"), (!writes.is_empty()).then(|| Value::Array(writes.clone())));
        replace(&case.dir.join("dossier.json"), case.spec.dossier.as_ref().and_then(|spec| dossier_after(&state, spec)));
        println!(
            "{}/{}: exit {}, {} calls to the fake gh, {} writes, {urls} URLs, all synthetic",
            case.family,
            case.name,
            case.spec.exit,
            calls.lines().count(),
            writes.len()
        );
        families.insert(case.family_dir());
    }

    let head = GitRef::head();
    let regia_py = DossierPath::parse("tools/regia.py").expect("a dossier path");
    let recorded = json!({
        "oracle_repository": "lelexdrugo/regia",
        "oracle_commit": git_output(&dossier, git::rev_parse(&head)),
        "oracle_last_change_to_regia_py": git_output(&dossier, git::last_commit(CommitField::Hash, &head, Some(&regia_py))),
        "recorded_at": git_output(&dossier, git::last_commit(CommitField::Date, &head, None)),
        "today": FIXTURE_TODAY,
        "now": FIXTURE_NOW,
    });
    for family in families {
        write_json(&family.join("RECORDED.json"), &recorded);
    }
}

/// `value` as the file's content, or no file: a recording leaves nothing of the previous one.
fn replace(path: &Path, value: Option<Value>) {
    match value {
        Some(value) => write_json(path, &value),
        None => remove(path),
    }
}

fn replace_text(path: &Path, text: Option<&str>) {
    match text {
        Some(text) => fs::write(path, text).unwrap_or_else(|error| panic!("{}: {error}", path.display())),
        None => remove(path),
    }
}

fn remove(path: &Path) {
    if path.exists() {
        fs::remove_file(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
}
