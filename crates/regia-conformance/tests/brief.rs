//! The acceptance test of the slice: an empty diff between the Rust brief and the Python one passed
//! through the facade table, on recorded fixtures. The negative controls prove the test can fail.

use std::fs;

use regia_conformance::{Case, bin_dir, json_diff, read_json, run_rust, translate_python};
use serde_json::{Value, json};

fn diff_of(case: &Case) -> Vec<String> {
    let rust = run_rust(&format!("test-{}", case.name), &case.spec.args, &case.responses_path())
        .expect("the Rust brief runs");
    let python = translate_python(case.command_name(), &case.python_output()).expect("the table covers the output");
    json_diff(&rust, &python)
}

#[test]
fn rust_and_python_agree_on_every_recorded_case() {
    for case in Case::all() {
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
}

/// Negative control: every alteration of the expected output must surface as a difference.
#[test]
fn an_altered_expectation_produces_a_diff() {
    let case = Case::named("core");
    let rust = run_rust("negative-rust", &case.spec.args, &case.responses_path()).unwrap();
    let original = case.python_output();

    let mut changed_number = original.clone();
    changed_number["aspetta_me"][0]["number"] = json!(999_999);
    let mut swapped_order = original.clone();
    swapped_order["aspetta_me"].as_array_mut().unwrap().swap(0, 1);
    let mut dropped_key = original.clone();
    dropped_key["pronti"][0].as_object_mut().unwrap().remove("catena");
    let mut changed_status = original.clone();
    changed_status["in_corso"][0]["stato"] = json!("Fatto");

    for (name, altered) in [
        ("changed number", changed_number),
        ("swapped order", swapped_order),
        ("dropped key", dropped_key),
        ("changed status", changed_status),
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
    let path = bin_dir().join("conformance-state").join("negative-responses.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_string(&responses).unwrap()).unwrap();

    let rust = run_rust("negative-tracker", &case.spec.args, &path).unwrap();
    let python = translate_python("brief", &case.python_output()).unwrap();
    assert!(!json_diff(&rust, &python).is_empty());
}

/// The fixtures go wherever the repo goes: only synthetic hosts and names.
#[test]
fn fixtures_are_synthetic() {
    for case in Case::all() {
        for file in ["responses.json", "python.it.json"] {
            let text = fs::read_to_string(case.dir.join(file)).unwrap();
            for forbidden in ["github.com", "lelexdrugo", "regia-tasks", "paper", "orca\\\\projects"] {
                assert!(!text.contains(forbidden), "{}/{file} contains {forbidden}", case.name);
            }
        }
    }
}
