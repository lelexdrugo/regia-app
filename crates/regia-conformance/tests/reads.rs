//! Acceptance 1 and 2 of #443: `projects`, `task show` and `task list` in the binary, natively, give on each
//! case of their families what the prototype gave, through the facade table. The run is the real binary with
//! no prototype, so the native engine answers, against the fake `gh` at the fixtures' instant.
//!
//! Three normalizations, declared:
//! - **JSON** is compared whole, as the brief's: an empty diff, key order aside.
//! - **Text** is not a contract (ADR 0001 §11): the product's is the contract's keys and the catalog's words,
//!   not the prototype's. For a case recorded in text, the issue numbers `#<n>` and the URLs the two print are
//!   compared, in order, and so is the exit code.
//! - **An error** is the product's own sentence: for an exit other than 0, both print nothing on stdout and
//!   something on stderr. With exit 0, stderr is empty on both sides or on neither.

use std::fs;
use std::path::PathBuf;

use regia_conformance::{Case, CliRun, Recorded, bin_dir, json_diff, lf, read_json, run_case, translate_python};
use serde_json::{Value, json};

const COMMANDS: [&str; 3] = ["projects", "task show", "task list"];

fn read_cases() -> Vec<Case> {
    Case::all().into_iter().filter(|case| COMMANDS.contains(&case.command_name().as_str())).collect()
}

fn native(label: &str, case: &Case) -> CliRun {
    run_case(label, case, |_| Vec::new())
}

/// The issue numbers and the URLs of a text, in order: what both texts say, in different words.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | ';' | '[' | ']')) {
        if word.contains("://") {
            out.push(word.to_owned());
        } else if let Some(digits) = word.strip_prefix('#').filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())) {
            out.push(format!("#{digits}"));
        }
    }
    out
}

/// Every way the native run differs from what the prototype printed for `case`, with `python` in place of
/// the recording when it is given.
fn native_differences(case: &Case, run: &CliRun, python: Option<&Recorded>) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    let stdout = lf(&String::from_utf8_lossy(&run.stdout));
    let recorded = python.cloned().unwrap_or_else(|| case.recorded());
    if case.spec.exit != 0 {
        let python_stdout = match &recorded {
            Recorded::Text(text) => text.clone(),
            Recorded::Json(value) => value.to_string(),
        };
        if !stdout.is_empty() || !python_stdout.is_empty() {
            out.push(format!("stdout of an error: rust {stdout:?}, python {python_stdout:?}"));
        }
        if run.stderr.trim().is_empty() || case.recorded_stderr().trim().is_empty() {
            out.push(format!("stderr of an error: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
        }
        return out;
    }
    match &recorded {
        Recorded::Json(python) => match (serde_json::from_str::<Value>(&stdout), translate_python(&case.command_name(), python)) {
            (Ok(rust), Ok(python)) => out.extend(json_diff(&rust, &python).into_iter().map(|line| format!("stdout {line}"))),
            (Err(error), _) => out.push(format!("stdout: rust printed no JSON ({error}): {stdout}")),
            (_, Err(error)) => out.push(format!("stdout: the table does not cover the prototype's output: {error}")),
        },
        Recorded::Text(python) => {
            let (rust, python) = (tokens(&stdout), tokens(python));
            if rust != python {
                out.push(format!("stdout: rust names {rust:?}, python {python:?}"));
            }
        }
    }
    if run.stderr.trim().is_empty() != case.recorded_stderr().trim().is_empty() {
        out.push(format!("stderr: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
    }
    out
}

#[test]
fn every_read_case_natively_gives_the_prototype_s_output() {
    let cases = read_cases();
    assert!(cases.len() >= 15, "the read cases are missing: {}", cases.len());
    for case in &cases {
        let run = native(&format!("native-{}", case.name), case);
        let diff = native_differences(case, &run, None);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

/// The comparison can fail: a status, an order, a key and a number altered in the recordings are each a
/// difference, and so is an exit code.
#[test]
fn an_altered_recording_is_noticed() {
    let list = Case::named("list-all-graph");
    let run = native("negative-list", &list);
    let Recorded::Json(python) = list.recorded() else { panic!("list-all-graph is JSON") };
    let mut status = python.clone();
    status[0]["stato"] = json!("Inbox");
    let mut order = python.clone();
    order.as_array_mut().unwrap().swap(0, 1);
    let mut key = python.clone();
    key[0].as_object_mut().unwrap().remove("catena");
    for (what, altered) in [("a status", status), ("the order", order), ("a missing key", key)] {
        assert!(!native_differences(&list, &run, Some(&Recorded::Json(altered))).is_empty(), "{what} went unnoticed");
    }
    let text = Case::named("list-waiting-text");
    let run = native("negative-text", &text);
    let Recorded::Text(python) = text.recorded() else { panic!("list-waiting-text is text") };
    let altered = Recorded::Text(python.replace("#5", "#6"));
    assert!(!native_differences(&text, &run, Some(&altered)).is_empty(), "an altered number went unnoticed");
    let mut wrong_exit = text.clone();
    wrong_exit.spec.exit = 1;
    assert!(!native_differences(&wrong_exit, &run, None).is_empty(), "an altered exit went unnoticed");
}

/// A copy of `case` whose responses `change` alters, under the build's folder.
fn with_responses(case: &Case, label: &str, change: impl FnOnce(&mut Value)) -> Case {
    let dir: PathBuf = bin_dir().join("conformance-cases").join(label);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut responses: Value = read_json(&case.responses_path());
    change(&mut responses);
    fs::write(dir.join("responses.json"), responses.to_string()).unwrap();
    Case { dir, ..case.clone() }
}

/// `edit` on the body of every Objective in the responses, open lists and lists of every state alike.
fn edit_objective_bodies(responses: &mut Value, edit: impl Fn(&str) -> String) {
    for key in ["issue_pages", "all_pages"] {
        for page in responses[key].as_array_mut().into_iter().flatten() {
            for node in page["repository"]["issues"]["nodes"].as_array_mut().into_iter().flatten() {
                let objective = node["labels"]["nodes"].as_array().is_some_and(|labels| labels.iter().any(|label| label["name"] == "tipo:obiettivo"));
                if objective {
                    node["body"] = json!(edit(node["body"].as_str().unwrap()));
                }
            }
        }
    }
}

/// Acceptance 2: a line added by hand to a project's registry does not break `projects`: the key passes as
/// data, with its name and its value as they are written, and the other keys are as they were. Through the
/// facade the same line passes too, since the entry's shape is open.
#[test]
fn a_new_line_in_a_registry_does_not_break_projects() {
    let case = Case::named("projects-registry");
    let before = native("registry-before", &case);
    assert!(native_differences(&case, &before, None).is_empty());
    let added = with_responses(&case, "registry-line", |responses| {
        edit_objective_bodies(responses, |body| body.replacen("-->", "a_new_line: by hand, with: colons\n-->", 1));
    });
    let after = native("registry-after", &added);
    assert_eq!(after.exit, Some(0), "{}", after.stderr);
    let (before, after): (Value, Value) = (serde_json::from_slice(&before.stdout).unwrap(), serde_json::from_slice(&after.stdout).unwrap());
    let entries = after.as_array().unwrap();
    assert!(!entries.is_empty());
    for (old, new) in before.as_array().unwrap().iter().zip(entries) {
        assert_eq!(new["a_new_line"], json!("by hand, with: colons"));
        let mut new = new.clone();
        new.as_object_mut().unwrap().remove("a_new_line");
        assert_eq!(&new, old, "the other keys changed");
    }
    let Recorded::Json(mut python) = case.recorded() else { panic!("projects-registry is JSON") };
    python[0]["a_new_line"] = json!("by hand, with: colons");
    assert_eq!(translate_python("projects", &python).unwrap()[0]["a_new_line"], json!("by hand, with: colons"));
    // A hand-written key that lands on a name of the contract is refused on both sides, never merged.
    let clash = with_responses(&case, "registry-clash", |responses| {
        edit_objective_bodies(responses, |body| body.replacen("-->", "title: by hand\n-->", 1));
    });
    let refused = native("registry-clash", &clash);
    assert_eq!((refused.exit, refused.stdout.is_empty()), (Some(1), true), "{}", refused.stderr);
    python[0]["title"] = json!("by hand");
    assert!(translate_python("projects", &python).is_err());
}

/// The cases show what Acceptance 1 asks: a list read in pages, closed issues, the graph's order, a closed
/// issue found by `task show`, comments signed in each way, and the registry's keys.
#[test]
fn the_read_cases_show_what_they_claim() {
    let cases = read_cases();
    let paged = cases.iter().filter(|case| {
        let responses: Value = read_json(&case.responses_path());
        ["issue_pages", "all_pages"].iter().any(|key| responses[key].as_array().is_some_and(|pages| pages.len() >= 2))
    });
    let paged: Vec<&str> = paged.map(|case| case.name.as_str()).collect();
    assert!(paged.iter().any(|name| name.starts_with("list-")), "no list read in pages: {paged:?}");
    assert!(paged.iter().any(|name| name.starts_with("show-")), "no show read in pages: {paged:?}");
    for name in ["list-all-graph", "list-statuses", "show-closed", "show-comments", "show-missing", "projects-registry"] {
        assert!(cases.iter().any(|case| case.name == name), "{name} is missing");
    }
    let shown = translate_python("task show", &Case::named("show-comments").python_output()).unwrap();
    let roles: Vec<&str> = shown["comments"].as_array().unwrap().iter().map(|comment| comment["role"].as_str().unwrap()).collect();
    for role in ["titolare", "chat", "worker"] {
        assert!(roles.contains(&role), "no comment of role {role}: {roles:?}");
    }
    let projects = translate_python("projects", &Case::named("projects-registry").python_output()).unwrap();
    for key in ["folder", "playbook", "priority", "wip", "auto_defaults", "repo", "products"] {
        assert!(projects[0].get(key).is_some(), "the registry has no {key}");
    }
}
