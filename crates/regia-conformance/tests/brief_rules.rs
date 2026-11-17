//! Rules of the brief that the recorded cases pin only if their scenarios hold the edge (#559, the
//! residues of #435): what the fake `gh` was called for, the day to the day, the Done and the Discarded
//! downstream, the first label of each kind, and the two differences the product declares.

use std::fs;

use regia_conformance::{
    Case, bin_dir, fixture_for_rust, json_diff, prepare_state_dir, read_json, regia_binary, take_not_ported, translate_python,
};
use regia_identity::env_var;
use serde_json::{Value, json};

/// One run of the binary on `responses`, on `day` if given: its output, its exit code and the lines the
/// fake `gh` logged, one per call.
fn run(label: &str, case: &Case, responses: &std::path::Path, day: Option<&str>) -> (Option<i32>, Value, String, Vec<String>) {
    let state = prepare_state_dir(label);
    let launch = regia_exec::harness::launch(&regia_binary(), &case.spec.args, &state);
    let (launch, log) = fixture_for_rust(launch, responses, &state);
    let launch = match day {
        Some(day) => launch.env(&env_var("TODAY"), day),
        None => launch,
    };
    let output = launch.run().expect("run regia");
    let calls = fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_owned).collect();
    let json = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (output.status.code(), json, String::from_utf8_lossy(&output.stderr).into_owned(), calls)
}

/// The prototype's recorded output, in the product's words.
fn python(case: &Case) -> Value {
    translate_python("brief", &case.python_output()).unwrap()
}

/// How many times the product asks `gh`, by case: one call a page of the list, one a page of what the list
/// cuts (twenty blockers, fifty sub-issues). The numbers are what the prototype did when it was recorded
/// (`record` prints them); a call more is a defect, a call less is a rule that stopped being read (#435/V5).
#[test]
fn the_product_calls_gh_as_often_as_the_prototype_did() {
    for (name, expected) in [("core", 14), ("filtered", 14), ("edges", 4), ("truncated", 1), ("empty", 1)] {
        let case = Case::named(name);
        let (code, _, stderr, calls) = run(&format!("calls-{name}"), &case, &case.responses_path(), None);
        assert_eq!(code, Some(0), "{name}: {stderr}");
        assert_eq!(calls.len(), expected, "{name}: {calls:?}");
    }
    // `edges` shows where the extra calls come from: two pages of the list, then the twenty-first blocker of
    // #9 and the fifty-first sub-issue of #11 each in a page of its own, and none for #8 and #10, which the
    // list reads whole.
    let case = Case::named("edges");
    let (_, _, _, calls) = run("calls-edges-kinds", &case, &case.responses_path(), None);
    let kinds: Vec<&str> = calls.iter().map(|call| call.split(' ').next().unwrap()).collect();
    assert_eq!(kinds.iter().filter(|kind| **kind == "issues").count(), 2);
    assert!(calls.contains(&"blockers 9 0".to_owned()) && calls.contains(&"sub-issues 11 0".to_owned()), "{calls:?}");
    assert!(!calls.iter().any(|call| call.starts_with("blockers 8") || call.starts_with("sub-issues 10")), "{calls:?}");
}

/// Done and Discarded count in `unblocks_total`, as any status does, and not in `unblocks`; the first project
/// label and the first maturity label win, whatever else the issue carries.
#[test]
fn done_and_discarded_downstream_count_in_the_total_and_the_first_label_wins() {
    let case = Case::named("edges");
    let (_, rust, _, _) = run("rules-edges", &case, &case.responses_path(), None);
    let mut rust = rust;
    take_not_ported(&mut rust).unwrap();
    assert!(json_diff(&rust, &python(&case)).is_empty(), "the product and the prototype differ on the edges");
    let entry = |number: u64| {
        ["waiting_on_owner", "ready", "in_progress"]
            .iter()
            .flat_map(|key| rust[key].as_array().unwrap())
            .find(|entry| entry["number"] == number)
            .cloned()
    };
    let root = entry(1).expect("#1 is ready");
    // #2 Done, #3 Discarded, #4 approved, #5 Inbox: all four count in the total, one only in the work.
    assert_eq!((root["unblocks"].clone(), root["unblocks_total"].clone()), (json!(1), json!(4)));
    // #6 has `maturità:abbozzo` before `maturità:pronto`: not ready. #7 has them the other way round, and
    // the project label `beta` before `alpha`.
    assert!(entry(6).is_none(), "#6 is a draft: it is in no section");
    let seventh = entry(7).expect("#7 is ready");
    assert_eq!((seventh["project"].clone(), seventh["maturity"].clone()), (json!("beta"), json!("ready")));
}

/// The sections as a set of entries, without what depends on the day: its marks, and the order, which puts
/// the due soon first.
fn without_the_day(brief: &Value) -> Value {
    let mut out = brief.clone();
    for key in ["waiting_on_owner", "ready", "in_progress", "session"] {
        let Some(entries) = out[key].as_array_mut() else { continue };
        for entry in entries.iter_mut() {
            for mark in ["due_soon", "overdue"] {
                entry.as_object_mut().unwrap().remove(mark);
            }
            // The graph's order is the order in the section: a number that moves keeps its measures.
        }
        entries.sort_by_key(|entry| entry["number"].as_u64());
    }
    out
}

/// The fixed day is the right day, not only a day: one day earlier or later the brief differs from the
/// prototype's, and only in what depends on the day: the marks of the due dates and the order they give
/// (#435/V5). A day that is wrong by one is as wrong as a day that is missing.
#[test]
fn the_day_one_day_off_changes_the_brief_and_only_what_depends_on_the_day() {
    let case = Case::named("core");
    let recorded = python(&case);
    for day in ["2026-01-09", "2026-01-11"] {
        let (code, mut rust, stderr, _) = run(&format!("day-{day}"), &case, &case.responses_path(), Some(day));
        assert_eq!(code, Some(0), "{stderr}");
        take_not_ported(&mut rust).unwrap();
        assert!(!json_diff(&rust, &recorded).is_empty(), "{day}: the fixtures do not tell the day from the day before or after");
        let diff = json_diff(&without_the_day(&rust), &without_the_day(&recorded));
        assert!(diff.is_empty(), "{day}: more than the day moved: {diff:?}");
    }
}

/// Declared difference, #435/V7 (#559): a tracker that has an item for its tasks in the Project and a value
/// of the status field for none. The prototype prints the brief, every task with no status; the product
/// refuses, because that is what a wrong `status_field_id` in `tracker.json` looks like, and the brief it
/// would print instead (everything unset, nothing waiting, nothing ready) reads as an all-clear. The guard
/// protects from a fault, so it stays: the product tells the owner to check the configuration.
#[test]
fn a_project_with_items_and_no_status_is_a_brief_in_the_prototype_and_an_error_here() {
    let case = Case::named("nostatus");
    let recorded = python(&case);
    assert_eq!(recorded["by_project"], json!({"alpha": {"unset": 2}}));
    for key in ["waiting_on_owner", "ready", "in_progress"] {
        assert_eq!(recorded[key], json!([]), "the prototype's brief has a task in {key}");
    }
    let (code, rust, stderr, calls) = run("nostatus", &case, &case.responses_path(), None);
    assert_ne!(code, Some(0), "the product printed a brief");
    assert_eq!(rust, Value::Null, "the product printed something on stdout");
    assert!(stderr.contains("status field") && stderr.contains("tracker.json"), "{stderr}");
    assert_eq!(calls.len(), 1, "it reads the list once and then refuses");
}

/// Declared difference, #435/V8 (#559): a page of the rest of an issue's blockers that says the issue is not
/// there (`issue: null`, an issue deleted between two reads). The prototype's reading ends in a `TypeError` that
/// nothing catches, and it exits 1 (the review of #435 ran it); the product ends the reading of that issue and
/// keeps what it read, as it does for a page that fails: the issue stays incomplete (`blocked_by_total` above
/// the blockers read) and so is not ready, never ready by its first twenty. An unhandled exception is not
/// behavior to copy.
#[test]
fn a_page_that_says_the_issue_is_not_there_leaves_it_incomplete_and_not_ready() {
    let case = Case::named("core");
    let mut responses: Value = read_json(&case.responses_path());
    responses["blocker_pages"]["23"][0] = json!({"repository": {"issue": null}});
    let path = bin_dir().join("conformance-state").join("null-blocker-page.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_string(&responses).unwrap()).unwrap();
    let (code, mut rust, stderr, calls) = run("null-page", &case, &path, None);
    assert_eq!(code, Some(0), "{stderr}");
    take_not_ported(&mut rust).unwrap();
    let ready: Vec<&Value> = rust["ready"].as_array().unwrap().iter().map(|entry| &entry["number"]).collect();
    assert!(!ready.contains(&&json!(23)), "#23 is ready on twenty of its twenty-three blockers");
    // The reading of #23 stopped at its first page: one call, no second page.
    assert!(calls.contains(&"blockers 23 0".to_owned()) && !calls.contains(&"blockers 23 1".to_owned()), "{calls:?}");
}
