//! Acceptance 1 of #449: `lease` and `step` in the binary, natively, give on each case of their families what
//! the prototype gave, refusals with exit 3 included, and leave the state database and the dossier's repo
//! as the prototype left them. The run is the real binary with no prototype, so the native engine answers,
//! on the case's rows and dossier, at the fixtures' instant.
//!
//! The normalizations, declared:
//! - **The reasons** are the product's own sentences, in English: the prototype's are translated by the
//!   tables of `words` before the comparison, those of a lock or a lease and those of the dossier.
//! - **The checks** of `step finish` are not ported (#448): natively they are `{unavailable: <why>}`, and
//!   the prototype's are not compared.
//! - **A generated holder**, `passo-` and 12 random hex digits, reads as `passo-<generated>` on both sides,
//!   as in every comparison of the harness.
//! - **The text** of the product is the JSON's keys, one leaf a line: for a case recorded in text, the
//!   facts its lines name are compared (who, what state, until when, which outcome of the dossier).
//! - **stderr** is each side's own words: both are empty, or both are not.
//! - **A case of a class only** (`same_class_only`): the dossier's reason is each side's own sentence about
//!   a tracker that does not answer, and is left out on both sides; the rest is compared.
//! - **The monotonic stamp** of a lock or a lease is each implementation's own: the rows are compared
//!   without it, and whether each row has one.

mod words;

use regia_conformance::{
    Case, CliRun, Recorded, generated_holders_as_one as generated_text, generated_holders_as_one_in as generated, json_diff, lf,
    run_case_on_dossier, state_rows, translate_python, unplace_json, unplace_text,
};
use regia_i18n::{Catalog, Language};
use regia_identity::env_var;
use serde_json::{Value, json};
use words::{in_the_product_s_words, outcome_of_line, state_reason_in_the_product_s_words};

/// The cases where the product differs from the prototype on purpose, each with a test below that says
/// how: an events cursor in the old form, the instant alone, which the facade cannot translate (#442/V7).
const DIVERGENT: [&str; 1] = ["step-start-old-cursor"];

fn state_cases() -> Vec<Case> {
    [Case::of_family("lease"), Case::of_family("step")].concat()
}

/// The real binary on `case`, natively: no prototype is named, so the table's group falls back on it.
fn native(label: &str, case: &Case, extra: Vec<(String, std::ffi::OsString)>) -> CliRun {
    run_case_on_dossier(label, case, |_| extra)
}

/// The prototype's JSON in the product's words: the table's names, the reasons translated, the
/// checks left out.
fn python_in_the_product_s_words(case: &Case, python: &Value) -> Result<Value, String> {
    let mut python = translate_python(&case.command_name(), python).map_err(|error| error.to_string())?;
    if let Some(object) = python.as_object_mut() {
        if let Some(Value::String(reason)) = object.get("reason") {
            let reason = state_reason_in_the_product_s_words(reason);
            object.insert("reason".to_owned(), json!(reason));
        }
        if let Some(Value::String(reason)) = object.get("dossier").and_then(|dossier| dossier.get("reason")) {
            let reason = in_the_product_s_words(reason);
            object["dossier"]["reason"] = json!(reason);
        }
        object.remove("checks");
    }
    Ok(generated(&python))
}

/// The native checks: absent when the prototype's are, and otherwise not ported, with why.
fn checks_differences(case: &Case, rust: &mut Value) -> Vec<String> {
    let python_has = matches!(case.recorded(), Recorded::Json(python) if python.get("controlli").is_some());
    let Some(object) = rust.as_object_mut() else { return Vec::new() };
    match (object.remove("checks"), python_has) {
        (None, false) => Vec::new(),
        (Some(checks), true) => {
            let reason = checks.get("unavailable").and_then(Value::as_str).unwrap_or_default();
            let alone = checks.as_object().is_some_and(|fields| fields.len() == 1);
            if reason.is_empty() || !alone { vec![format!("checks: rust {checks}, not unavailable with a reason")] } else { Vec::new() }
        }
        (rust, _) => vec![format!("checks: rust {rust:?}, python has them: {python_has}")],
    }
}

/// The lines a text recording names, as the product's text must have them; a line ending with `*` is a
/// prefix.
fn text_facts(command: &str, python: &str) -> Vec<String> {
    let python = python.trim_end();
    if let Some(reason) = python.strip_prefix("RIFIUTATO: ") {
        return vec!["ok: false".to_owned(), format!("reason: {}", state_reason_in_the_product_s_words(reason))];
    }
    let states = [("preso", "taken"), ("rinnovato", "renewed"), ("rubato", "stolen")];
    match command {
        // The catalog's word for an empty list, in the language both the binary and this test detect.
        "lease list" if python.starts_with("(nessuna lease") => vec![Catalog::new(Language::detect()).message("brief-nothing")],
        "lease list" => python.lines().map(|line| format!("{} *", line.split_whitespace().next().unwrap_or_default())).collect(),
        "step start" | "lease acquire" => {
            let mut lines = python.lines();
            let first = lines.next().unwrap_or_default();
            let (state, rest) = first.split_once(": ").and_then(|(_, rest)| rest.split_once(" da ")).unwrap_or_default();
            let (holder, rest) = rest.split_once(", scade ").unwrap_or_default();
            let (until, rest) = rest.split_once(" (tra ").unwrap_or_default();
            let seconds = rest.split_once(" s)").map_or("", |(seconds, _)| seconds);
            let state = states.iter().find(|(python, _)| *python == state).map_or(state, |(_, product)| product);
            let mut facts = vec![
                format!("status: {state}"),
                format!("holder: {}", holder.trim_end_matches(" (id generato)")),
                format!("expires_at: {until}"),
                format!("expires_in_s: {seconds}"),
            ];
            for line in lines {
                if let Some(t) = line.strip_prefix("t del passo: ") {
                    facts.push(format!("t: {t}"));
                }
                if line.starts_with("cursore eventi: assente") {
                    facts.push("cursor: -".to_owned());
                }
            }
            facts
        }
        "step finish" => {
            let mut facts = Vec::new();
            for line in python.lines() {
                if line.ends_with("concluso, lock rilasciato") {
                    facts.push("lock: released".to_owned());
                }
                if let Some(t) = line.strip_prefix("cursore eventi: t=").and_then(|rest| rest.split_whitespace().next()) {
                    facts.push(format!("cursor.t: {t}"));
                }
                if let Some(dossier) = line.strip_prefix("dossier: ") {
                    facts.push(format!("dossier.outcome: {}", outcome_of_line(&format!("x: {dossier}"))));
                    if let Some(sha) = dossier.strip_prefix("pushato ").and_then(|rest| rest.split_whitespace().next()) {
                        facts.push(format!("dossier.sha: {sha}*"));
                    }
                    if let Some(retried) = dossier.split_once(" (").and_then(|(_, rest)| rest.strip_suffix(')')) {
                        facts.push(format!("dossier.reason: {}", in_the_product_s_words(retried)));
                    }
                }
            }
            facts
        }
        _ => vec![format!("(no facts for the text of {command})")],
    }
}

fn has_fact(text: &str, fact: &str) -> bool {
    match fact.strip_suffix('*') {
        Some(prefix) => text.lines().any(|line| line.starts_with(prefix.trim_end())),
        None => text.lines().any(|line| line == fact),
    }
}

/// Every way the native run differs from what the prototype did on `case`.
fn native_differences(case: &Case, run: &CliRun) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    let unplace = |value: &Value| run.dossier.as_ref().map_or_else(|| value.clone(), |dossier| unplace_json(value, dossier));
    let stdout = lf(&String::from_utf8_lossy(&run.stdout));
    let stdout = run.dossier.as_ref().map_or_else(|| stdout.clone(), |dossier| unplace_text(&stdout, dossier));
    match case.recorded() {
        Recorded::Json(python) => match (serde_json::from_str::<Value>(&stdout), python_in_the_product_s_words(case, &python)) {
            (Ok(rust), Ok(mut python)) => {
                let mut rust = generated(&unplace(&rust));
                out.extend(checks_differences(case, &mut rust));
                if case.spec.same_class_only {
                    for side in [&mut rust, &mut python] {
                        if let Some(dossier) = side.get_mut("dossier").and_then(Value::as_object_mut) {
                            dossier.remove("reason");
                        }
                    }
                }
                out.extend(json_diff(&rust, &python).into_iter().map(|line| format!("stdout {line}")));
            }
            (Err(error), _) => out.push(format!("stdout: rust printed no JSON ({error}): {stdout}")),
            (_, Err(error)) => out.push(format!("stdout: the table does not cover the prototype's output: {error}")),
        },
        Recorded::Text(python) if python.trim().is_empty() => {
            if !stdout.trim().is_empty() {
                out.push(format!("stdout: rust {stdout:?}, python nothing"));
            }
        }
        Recorded::Text(python) => {
            let stdout = generated_text(&stdout);
            for fact in text_facts(&case.command_name(), &generated_text(&python)) {
                if !has_fact(&stdout, &fact) {
                    out.push(format!("stdout: rust {stdout:?} has no `{fact}`, python {python:?}"));
                }
            }
        }
    }
    if run.stderr.trim().is_empty() != case.recorded_stderr().trim().is_empty() {
        out.push(format!("stderr: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
    }
    if case.spec.state_after {
        let python = case.state_after().map(|rows| generated(&rows)).unwrap_or(Value::Null);
        out.extend(json_diff(&generated(&state_rows(&run.state)), &python).into_iter().map(|line| format!("state {line}")));
    }
    let after = |value: Option<Value>| value.unwrap_or(Value::Null);
    out.extend(json_diff(&after(run.dossier_after.clone()), &after(case.dossier_after())).into_iter().map(|line| format!("dossier {line}")));
    out
}

#[test]
fn every_lease_and_step_case_natively_gives_the_prototype_s_outcome() {
    let cases = state_cases();
    assert!(cases.len() >= 30, "the cases of the prototype's tests are missing: {}", cases.len());
    for case in &cases {
        assert!(case.spec.state_after, "{}: a case of the state must record the rows after it", case.name);
    }
    for case in cases.iter().filter(|case| !DIVERGENT.contains(&case.name.as_str())) {
        let run = native(&format!("native-{}", case.name), case, Vec::new());
        let diff = native_differences(case, &run);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

/// #442/V7: with the events cursor in the old form, the instant alone, the prototype answers `step start`
/// and the facade cannot translate the cursor, so through it the step is refused with 1 after the lock was
/// taken. The product reads the old form as the precheck reads it, an instant with nothing of its own,
/// and the rest is the prototype's.
#[test]
fn an_events_cursor_in_the_old_form_reads_as_its_instant() {
    let case = Case::named("step-start-old-cursor");
    let Recorded::Json(mut python) = case.recorded() else { panic!("the case is recorded in JSON") };
    let instant = python["cursore"].as_str().expect("the old form is a string").to_owned();
    assert!(translate_python("step start", &python).is_err(), "the facade cannot translate the old form");
    python["cursore"] = json!({"t": instant});
    let run = native("native-step-start-old-cursor", &case, Vec::new());
    assert_eq!(run.exit, Some(0), "{}", run.stderr);
    let rust: Value = serde_json::from_slice(&run.stdout).expect("JSON");
    let expected = python_in_the_product_s_words(&case, &python).expect("the table covers the rest");
    assert_eq!(json_diff(&rust, &expected), Vec::<String>::new());
    assert_eq!(rust["cursor"], json!({"t": instant}));
    assert_eq!(json_diff(&state_rows(&run.state), &case.state_after().expect("state.json")), Vec::<String>::new());
}

/// Negative controls: on the machine's clock the lock expires elsewhere, and a row or an outcome altered
/// by hand is a difference.
#[test]
fn the_comparison_notices_a_difference() {
    let case = Case::named("step-start-free");
    assert!(native_differences(&case, &native("native-control", &case, Vec::new())).is_empty());
    let machine = native("native-machine-clock", &case, vec![(env_var("NOW"), "".into())]);
    let diff = native_differences(&case, &machine);
    assert!(diff.iter().any(|line| line.contains("expires_at")), "{diff:?}");
    assert!(diff.iter().any(|line| line.starts_with("state")), "{diff:?}");
    let run = native("native-control-altered", &case, Vec::new());
    let mut rows = state_rows(&run.state);
    rows[0]["generation"] = json!(9);
    assert!(!json_diff(&rows, &case.state_after().expect("state.json")).is_empty(), "an altered row is a difference");
    let mut stdout: Value = serde_json::from_slice(&run.stdout).expect("JSON");
    stdout["status"] = json!("renewed");
    let Recorded::Json(python) = case.recorded() else { panic!() };
    assert!(!json_diff(&stdout, &python_in_the_product_s_words(&case, &python).unwrap()).is_empty());
}

/// The cases are what their notes say: a refusal is 3 and never touches what it refuses, an error is 1,
/// the D3 window is the store's, and the step's cursor and the events cursor are where the precheck reads.
#[test]
fn the_state_cases_show_what_they_claim() {
    let rows = |name: &str| Case::named(name).state_after().expect("state.json");
    for name in ["lease-acquire-busy", "lease-release-foreign", "step-start-busy", "step-renew-foreign", "step-finish-foreign"] {
        let case = Case::named(name);
        assert_eq!(case.spec.exit, 3, "{name}");
        let before: Vec<Value> = case.spec.state.iter().map(|row| json!([row.kind, row.key, row.doc])).collect();
        let after: Vec<Value> = rows(name).as_array().unwrap().iter().map(|row| json!([row["kind"], row["key"], row["doc"]])).collect();
        assert_eq!(after, before, "{name}: a refusal leaves the rows as they were");
    }
    // The step's end: the events cursor at the project's key, the step emptied, the lock released.
    let finished = rows("step-finish-no-push");
    let find = |key: &str| finished.as_array().unwrap().iter().find(|row| row["key"] == key).cloned();
    assert_eq!(find("alpha").unwrap()["doc"]["valore"]["propri"]["report"], json!({"2026-01-09-3-done.md": "2026-01-10T08:00:00+00:00"}));
    assert_eq!(find("alpha:passo").unwrap()["doc"]["valore"], Value::Null);
    assert!(find("alpha").and_then(|row| row.get("stamped").cloned()) == Some(json!(false)), "a cursor has no stamp");
    assert!(finished.as_array().unwrap().iter().all(|row| row["kind"] != "lock"), "the lock is released");
    // A step that was never opened is an error, and the lock stays, renewed to a minute.
    let no_step = rows("step-finish-no-step");
    let lock = no_step.as_array().unwrap().iter().find(|row| row["kind"] == "lock").expect("the lock stays");
    assert_eq!((lock["doc"]["ttl_s"].clone(), lock["generation"].clone(), lock["stamped"].clone()), (json!(60), json!(2), json!(true)));
}
