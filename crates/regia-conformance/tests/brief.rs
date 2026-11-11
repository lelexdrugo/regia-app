//! The acceptance test of the slice: an empty diff between the Rust brief and the Python one passed
//! through the facade table, on recorded fixtures. The negative controls prove the test can fail.

use std::fs;

use regia_conformance::{
    Case, bin_dir, fixture_for_rust, json_diff, prepare_state_dir, project_query, read_json, regia_binary, run_rust,
    take_not_ported, translate_python,
};
use regia_identity::env_var;
use serde_json::{Value, json};

/// The Rust brief, with its declaration of what is not ported checked and removed.
fn rust_brief(label: &str, case: &Case, responses: &std::path::Path) -> Value {
    let mut rust = run_rust(label, &case.spec.args, responses).expect("the Rust brief runs");
    let not_ported = take_not_ported(&mut rust).expect("the marker");
    assert_eq!(not_ported, ["controller", "dossier", "outside_tracker"]);
    rust
}

fn diff_of(case: &Case) -> Vec<String> {
    let rust = rust_brief(&format!("test-{}", case.name), case, &case.responses_path());
    let python = translate_python(&case.command_name(), &case.python_output()).expect("the table covers the output");
    json_diff(&rust, &python)
}

fn write_responses(name: &str, responses: &Value) -> std::path::PathBuf {
    let path = bin_dir().join("conformance-state").join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_string(responses).unwrap()).unwrap();
    path
}

#[test]
fn rust_and_python_agree_on_every_recorded_case() {
    for case in Case::of_family("brief") {
        let diff = diff_of(&case);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

#[test]
fn the_cases_exercise_what_they_claim() {
    let core = translate_python("brief", &Case::named("core").python_output()).unwrap();
    for key in ["waiting_on_owner", "ready", "in_progress", "session"] {
        assert!(!core[key].as_array().unwrap().is_empty(), "core has no {key}");
    }
    assert!(!core["graph"]["cycles"].as_array().unwrap().is_empty(), "core has no cycle");
    assert!(core["by_project"].as_object().unwrap().len() >= 2, "core has one project");
    let counts: Vec<&Value> = core["by_project"].as_object().unwrap().values().collect();
    assert!(counts.iter().any(|c| c.get("unset").is_some()), "core has no task without a status");
    assert!(counts.iter().any(|c| c.as_object().unwrap().is_empty()), "core has no objective-only project");
    let pages: Value = read_json(&Case::named("core").responses_path());
    assert!(pages["issue_pages"].as_array().unwrap().len() >= 2, "core is not paginated");
    assert!(!pages["blocker_pages"].as_object().unwrap().is_empty(), "core does not page blockers");

    let truncated = translate_python("brief", &Case::named("truncated").python_output()).unwrap();
    assert!(!truncated["graph"]["truncated"].as_array().unwrap().is_empty());
    let filtered = translate_python("brief", &Case::named("filtered").python_output()).unwrap();
    assert_eq!(filtered["by_project"].as_object().unwrap().len(), 1);
    // The scope filters the sections, not the graph: #21 of alpha still counts #29 of beta, an Inbox, in
    // its total. Without this the case could not tell a graph computed after the filter.
    let of_21 = |brief: &Value, key: &str| {
        brief["waiting_on_owner"].as_array().unwrap().iter().find(|t| t["number"] == 21).unwrap()[key].clone()
    };
    assert_eq!(of_21(&filtered, "unblocks_total"), of_21(&core, "unblocks_total"));
    assert_eq!(of_21(&filtered, "unblocks_total"), json!(2));
    assert_eq!(of_21(&filtered, "unblocks"), json!(0), "#22 and #29 are in Inbox: no approved work");
}

/// The rules of #244, #407 and #338 of the dossier, and of its #505, each have a case that shows them.
#[test]
fn the_cases_exercise_the_graph_due_dates_and_project_field() {
    let core = translate_python("brief", &Case::named("core").python_output()).unwrap();
    let entries: Vec<&Value> = ["waiting_on_owner", "ready", "in_progress"]
        .iter()
        .flat_map(|key| core[key].as_array().unwrap())
        .collect();
    let has = |test: &dyn Fn(&Value) -> bool| entries.iter().any(|entry| test(entry));
    assert!(has(&|e| e["unblocks"] != e["unblocks_total"]), "no entry with work in Inbox downstream");
    assert!(has(&|e| e["project_field"].is_string()) && has(&|e| e["project_field"].is_null()));
    assert!(has(&|e| e["project_field"].is_string() && e["project_field"] != e["project"]), "no drift from the label");
    assert!(has(&|e| e.get("overdue").is_some()), "no overdue entry");
    assert!(has(&|e| e.get("due").is_some() && e.get("due_soon").is_none() && e.get("overdue").is_none()));
    for key in ["waiting_on_owner", "ready", "in_progress", "session"] {
        let first = &core[key][0];
        assert_eq!(first["due_soon"], json!(true), "the first of {key} is not due soon");
    }
    let soon: Vec<&Value> = core["waiting_on_owner"].as_array().unwrap().iter().filter(|e| e.get("due_soon").is_some()).collect();
    assert!(soon.windows(2).any(|pair| pair[0]["due"] == pair[1]["due"]), "no two due the same day");

    let in_sections: Vec<&Value> = entries.iter().map(|entry| &entry["number"]).collect();
    let ignored = core["due_ignored"].as_array().unwrap();
    assert!(ignored.iter().any(|i| in_sections.contains(&&i["number"])), "no malformed date in a section");
    assert!(ignored.iter().any(|i| !in_sections.contains(&&i["number"])), "no malformed date outside them");
    let filtered = translate_python("brief", &Case::named("filtered").python_output()).unwrap();
    assert!(filtered["due_ignored"].as_array().unwrap().len() < ignored.len(), "the scope does not filter them");

    let waiting = core["graph"]["children_waiting"].as_array().unwrap();
    assert!(waiting.len() >= 4, "the single arc and the three that close a cycle only together");

    let pages: Value = read_json(&Case::named("core").responses_path());
    assert!(!pages["sub_issue_pages"].as_object().unwrap().is_empty(), "core does not page sub-issues");
    let longest = |brief: &Value| {
        ["waiting_on_owner", "ready"]
            .iter()
            .flat_map(|key| brief[key].as_array().unwrap())
            .map(|entry| entry["sub_issues"].as_array().unwrap().len())
            .max()
            .unwrap()
    };
    assert!(longest(&core) > 50, "the rest of the sub-issues is not read");
    let truncated = translate_python("brief", &Case::named("truncated").python_output()).unwrap();
    assert!(longest(&truncated) > 50, "the open child is not added when the rest cannot be read");
}

/// The fake answers like a server: a field the query does not select is not in the answer.
#[test]
fn the_fake_gh_returns_only_what_the_query_selects() {
    let page = json!({"repository": {"a": 1, "b": {"c": 2, "d": 3}, "e": [{"f": 4, "g": 5}], "h": null}});
    let query = r#"query($o:String!){repository(owner:$o,name:"x{"){ a b(first:1){c} e{g ... on T{f z}} h{i} }}"#;
    let projected = project_query(query, &page).unwrap();
    assert_eq!(projected, json!({"repository": {"a": 1, "b": {"c": 2}, "e": [{"g": 5, "f": 4}], "h": null}}));
    assert!(project_query("{repository{nope}}", &page).is_err(), "an unknown field is an error");

    // A value by field name answers under its alias, and a field with no value answers null.
    let item = json!({"fieldValueByName": {"Status": {"name": "Inbox", "optionId": "x"}, "Other": {"name": "y"}}});
    let query = r#"{fieldValueByName(name:"Status"){name} other:fieldValueByName(name: "Other"){name} none:fieldValueByName(name:"Nope"){name}}"#;
    let projected = project_query(query, &item).unwrap();
    assert_eq!(projected, json!({"fieldValueByName": {"name": "Inbox"}, "other": {"name": "y"}, "none": null}));
}

/// The status field is found by id, so a field with another name works, and a wrong id is an error
/// instead of an empty plan.
#[test]
fn a_status_field_nobody_can_find_is_an_error_not_an_empty_brief() {
    let case = Case::named("core");
    let mut responses: Value = read_json(&case.responses_path());
    for page in responses["issue_pages"].as_array_mut().unwrap() {
        for issue in page["repository"]["issues"]["nodes"].as_array_mut().unwrap() {
            for item in issue["projectItems"]["nodes"].as_array_mut().unwrap() {
                for value in item["fieldValues"]["nodes"].as_array_mut().unwrap() {
                    if value.get("field").is_some() {
                        value["field"]["id"] = json!("SOME_OTHER_FIELD");
                    }
                }
            }
        }
    }
    let path = write_responses("negative-field.json", &responses);
    let outcome = run_rust("negative-field", &case.spec.args, &path);
    assert!(outcome.is_err_and(|error| error.contains("status field")));
}

/// Negative control: every alteration of the expected output must surface as a difference.
#[test]
fn an_altered_expectation_produces_a_diff() {
    let case = Case::named("core");
    let rust = rust_brief("negative-rust", &case, &case.responses_path());
    let original = case.python_output();

    let mut changed_number = original.clone();
    changed_number["aspetta_me"][0]["number"] = json!(999_999);
    let mut swapped_order = original.clone();
    swapped_order["aspetta_me"].as_array_mut().unwrap().swap(0, 1);
    let mut dropped_key = original.clone();
    dropped_key["pronti"][0].as_object_mut().unwrap().remove("catena");
    let mut changed_status = original.clone();
    changed_status["in_corso"][0]["stato"] = json!("Fatto");
    let mut changed_total = original.clone();
    let total = changed_total["aspetta_me"][0]["sblocca_totale"].as_u64().expect("a measure");
    changed_total["aspetta_me"][0]["sblocca_totale"] = json!(total + 1);
    let mut dropped_due = original.clone();
    dropped_due["pronti"][0].as_object_mut().unwrap().remove("entro_vicino").expect("the first ready is due soon");

    for (name, altered) in [
        ("changed number", changed_number),
        ("swapped order", swapped_order),
        ("dropped key", dropped_key),
        ("changed status", changed_status),
        ("changed unblocks_total", changed_total),
        ("dropped due_soon", dropped_due),
    ] {
        let python = translate_python("brief", &altered).unwrap();
        assert!(!json_diff(&rust, &python).is_empty(), "the harness did not notice: {name}");
    }

    let mut unknown_key = original.clone();
    unknown_key["chiave_nuova"] = json!(1);
    assert!(translate_python("brief", &unknown_key).is_err(), "an unknown key must fail closed");
}

/// Negative control on the other side: changing what the tracker answers changes the Rust brief.
#[test]
fn an_altered_tracker_response_produces_a_diff() {
    let case = Case::named("core");
    let mut responses: Value = read_json(&case.responses_path());
    let node = &mut responses["issue_pages"][0]["repository"]["issues"]["nodes"];
    // Issue 3 is a task in a section; an Objective would be excluded and change nothing.
    let waiting = node.as_array_mut().unwrap().iter_mut().find(|issue| issue["number"] == 3).unwrap();
    waiting["title"] = json!("altered by the negative control");
    let path = write_responses("negative-responses.json", &responses);

    let rust = rust_brief("negative-tracker", &case, &path);
    let python = translate_python("brief", &case.python_output()).unwrap();
    assert!(!json_diff(&rust, &python).is_empty());
}

/// Negative control on the day: the fixed day reaches the binary, and another day changes the brief. A
/// value that is not a date stops the brief instead of falling back to the clock.
#[test]
fn another_day_produces_a_diff_and_a_bad_day_is_an_error() {
    let case = Case::named("core");
    let run = |label: &str, day: &str| {
        let state = prepare_state_dir(label);
        let launch = regia_exec::harness::launch(&regia_binary(), &case.spec.args, &state);
        let (launch, _) = fixture_for_rust(launch, &case.responses_path(), &state);
        launch.env(&env_var("TODAY"), day).run().expect("run regia")
    };
    let later = run("negative-day", "2026-01-12");
    assert!(later.status.success());
    let mut rust: Value = serde_json::from_slice(&later.stdout).unwrap();
    take_not_ported(&mut rust).unwrap();
    let python = translate_python("brief", &case.python_output()).unwrap();
    assert!(!json_diff(&rust, &python).is_empty(), "two days later, nothing changed");

    let bad = run("negative-bad-day", "12/01/2026");
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("12/01/2026"));
}

/// The fixtures go wherever the repo goes: only synthetic hosts and names, in every file of every case
/// of every family. Two exceptions, both the prototype's own words: the URLs it builds on GitHub from the
/// fixture's owner (`task new`, the origin of `project new`), as `record` counts them, and the default
/// name of the tracker's repo, which `init` alone prints.
#[test]
fn fixtures_are_synthetic() {
    for case in Case::all() {
        for entry in fs::read_dir(&case.dir).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut text = fs::read_to_string(&path).unwrap().replace("https://github.com/example/", "");
            if case.command_name() == "init" && file == "python.it.json" {
                text = text.replace("\"repo\": \"regia-tasks\"", "");
            }
            for forbidden in ["github.com", "lelexdrugo", "regia-tasks", "paper", "orca\\\\projects"] {
                assert!(!text.contains(forbidden), "{}/{file} contains {forbidden}", case.name);
            }
        }
    }
}
