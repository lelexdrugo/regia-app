//! Acceptance 1 of #449 and of #562: `lock`, `lease` and `step` in the binary, natively, give on each case of
//! their families what the prototype gave, refusals with exit 3 included, and leave the state database and
//! the dossier's repo as the prototype left them. The run is the real binary with no prototype, so the
//! native engine answers, on the case's rows and dossier, at the fixtures' instant.
//!
//! A database held by another transaction beyond the wait (#442/V5) has no recorded case: a case cannot
//! hold a transaction while `record` runs the prototype. Here another process holds it during the run, and
//! the tests compare the product with what the prototype's code answers then; an ignored test runs the
//! prototype under the same hold.
//!
//! The normalizations, declared:
//! - **The reasons** are the product's own sentences, in English: the prototype's are translated by the
//!   tables of `words` before the comparison, those of a lock or a lease and those of the dossier.
//! - **The line of the checks** in the text of `step finish` (#701) is the prototype's, character for character, with the checks' ids and the command of its hint in the contract's words; see `step_finish_prints_the_prototype_s_line_of_the_checks`.
//! - **The checks** of `step finish` (#448) are compared whole, through the table, when the prototype ran them;
//!   when it could not, each side says why in its own words, and only that both could not is compared.
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

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use regia_conformance::{
    Case, CliRun, Recorded, fixture_for_rust, generated_holders_as_one as generated_text, generated_holders_as_one_in as generated,
    json_diff, lf, prepare_state_dir, prototype_at_fixture_now, regia_binary, run_case_on_dossier, state_rows, translate_python,
    unplace_json, unplace_text,
};
use regia_exec::harness::{self, Running};
use regia_i18n::{Catalog, Language};
use regia_identity::{env_var, prototype_env_var};
use serde_json::{Value, json};
use words::{in_the_product_s_words, outcome_of_line, state_reason_in_the_product_s_words};

const STORE_PROBE: &str = env!("CARGO_BIN_EXE_store-probe");

/// The cases where the product differs from the prototype on purpose, each with a test below that says
/// how: an events cursor in the old form, the instant alone, which the facade cannot translate (#442/V7).
const DIVERGENT: [&str; 1] = ["step-start-old-cursor"];

fn state_cases() -> Vec<Case> {
    [Case::of_family("lock"), Case::of_family("lease"), Case::of_family("step")].concat()
}

/// The real binary on `case`, natively: no prototype is named, so the table's group falls back on it.
fn native(label: &str, case: &Case, extra: Vec<(String, std::ffi::OsString)>) -> CliRun {
    run_case_on_dossier(label, case, |_| extra)
}

/// The prototype's JSON in the product's words: the table's names, the reasons translated, the
/// checks left out, for `checks_differences`.
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

/// The native checks against the prototype's, through the table: absent on both sides, unavailable on both with a
/// reason each, or the same findings, counts and warnings.
fn checks_differences(case: &Case, rust: &mut Value) -> Vec<String> {
    let python = match case.recorded() {
        Recorded::Json(python) => python.get("controlli").map(|checks| translate_python(&case.command_name(), &json!({"controlli": checks}))),
        Recorded::Text(_) => None,
    };
    let Some(object) = rust.as_object_mut() else { return Vec::new() };
    match (object.remove("checks"), python) {
        (None, None) => Vec::new(),
        (Some(rust), Some(Ok(python))) => {
            let python = &python["checks"];
            let unavailable = |checks: &Value| checks.get("unavailable").and_then(Value::as_str).is_some_and(|reason| !reason.is_empty());
            match (unavailable(&rust), unavailable(python)) {
                (true, true) => Vec::new(),
                (false, false) => json_diff(&rust, python).into_iter().map(|line| format!("checks {line}")).collect(),
                _ => vec![format!("checks: rust {rust}, python {python}")],
            }
        }
        (_, Some(Err(error))) => vec![format!("checks: the table does not cover the prototype's: {error}")],
        (rust, python) => vec![format!("checks: rust {rust:?}, python has them: {}", python.is_some())],
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
        "step start" | "lease acquire" | "lock acquire" => {
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
            if let Some((_, replaced)) = rest.split_once(", al posto di ") {
                facts.push(format!("replaced: {replaced}"));
            }
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
        "lock status" => {
            let line = python.lines().next().unwrap_or_default();
            let rest = line.split_once(": ").map_or(line, |(_, rest)| rest);
            if let Some(rest) = rest.strip_prefix("occupato da ") {
                let (holder, rest) = rest.split_once(", scade ").unwrap_or_default();
                let (until, rest) = rest.split_once(" (tra ").unwrap_or_default();
                let seconds = rest.split_once(" s)").map_or("", |(seconds, _)| seconds);
                let facts = ["status: busy", "busy: true"].map(str::to_owned);
                [facts.to_vec(), vec![format!("holder: {holder}"), format!("expires_at: {until}"), format!("expires_in_s: {seconds}")]].concat()
            } else if let Some(holder) = rest.strip_prefix("scaduto (era di ").and_then(|rest| rest.strip_suffix(')')) {
                vec!["status: expired".to_owned(), "busy: false".to_owned(), format!("holder: {holder}")]
            } else {
                let state = [("libero", "free"), ("illeggibile", "unreadable")].iter().find(|(python, _)| *python == rest).map_or(rest, |(_, product)| product);
                vec![format!("status: {state}")]
            }
        }
        // `rilasciato` also when the lock was already free: the line says no more than that it succeeded.
        "lock release" => vec!["ok: true".to_owned()],
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
fn every_lock_lease_and_step_case_natively_gives_the_prototype_s_outcome() {
    let cases = state_cases();
    assert!(cases.len() >= 58, "the cases of the prototype's tests are missing: {}", cases.len());
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

/// The line of the checks in `step finish`'s text (#701): from `first` on, with the lines indented under it.
fn checks_block(text: &str, first: &str) -> Vec<String> {
    let mut lines = text.lines().skip_while(|line| !line.starts_with(first));
    lines.next().map(|head| head.to_owned()).into_iter().chain(lines.take_while(|line| line.starts_with("  ")).map(str::to_owned)).collect()
}

/// The contract's id of a check the prototype names, through the table.
fn contract_id(id: &str) -> String {
    let report = json!({"voci": [{"controllo": id, "progetto": "p", "dove": "d", "testo": "t", "perche": "w"}], "per_controllo": {}, "avvisi": []});
    let translated = translate_python("check", &report).unwrap_or_else(|error| panic!("the table does not know {id}: {error}"));
    translated["findings"][0]["check"].as_str().expect("an id").to_owned()
}

/// The prototype's lines of the checks with the checks' ids and its hint in the contract's words, the differences declared
/// between its text and the product's in Italian: the summary's `(id n, …)` and each finding's `[id]`.
fn in_contract_ids(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            if let Some((head, counts)) = line.split_once(" (").filter(|_| line.starts_with("controlli:")) {
                let counts: Vec<String> = counts
                    .trim_end_matches(')')
                    .split(", ")
                    .map(|count| count.rsplit_once(' ').map_or_else(|| count.to_owned(), |(id, n)| format!("{} {n}", contract_id(id))))
                    .collect();
                format!("{head} ({})", counts.join(", "))
            } else if let Some((head, rest)) = line.split_once(": [").filter(|_| line.starts_with("  ")) {
                let (id, tail) = rest.split_once(']').expect("a closing bracket");
                format!("{head}: [{}]{tail}", contract_id(id))
            } else if let Some(count) = line.strip_prefix("  … e altre ").and_then(|rest| rest.strip_suffix(" (regia controlla --progetto …)")) {
                // The prototype's hint names its own command; the product's is `check --project`.
                format!("  … e altre {count} (regia check --project …)")
            } else {
                line.clone()
            }
        })
        .collect()
}

/// #701: in text, `step finish` prints the prototype's line of the checks, `controlli: N voci (…)`, the first
/// three of them and how many more, between the cursor and the dossier. In Italian, the catalog the
/// prototype's sentences are in, the lines are the prototype's own, character for character, but for the checks' ids and the
/// command its hint names, which are the contract's provisional names (#223), as in `check`'s text; the JSON of
/// `checks` is compared by the cases in JSON. In English only the words of the catalog change.
#[test]
fn step_finish_prints_the_prototype_s_line_of_the_checks() {
    let italian = vec![(env_var("LANG"), OsString::from("it"))];
    for (name, findings) in [("step-finish-retry-text", 2), ("step-finish-checks-text", 5), ("step-finish-checks-none-text", 0)] {
        let case = Case::named(name);
        let Recorded::Text(python) = case.recorded() else { panic!("{name} is recorded in text") };
        let prototype = checks_block(&lf(&python), "controlli:");
        let expected = in_contract_ids(&prototype);
        assert_eq!(prototype.len(), if findings == 0 { 1 } else { findings.min(3) + 1 + usize::from(findings > 3) }, "{name}: the recording");
        let run = native(&format!("native-{name}-it"), &case, italian.clone());
        assert_eq!(run.exit, Some(0), "{name}: {}", run.stderr);
        let stdout = lf(&String::from_utf8_lossy(&run.stdout));
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(checks_block(&stdout, "controlli:"), expected, "{name}: the text of the product
{stdout}");
        // Where the prototype puts it: after the cursor, before the dossier, with nothing of the checks' keys around it.
        let at = lines.iter().position(|line| line.starts_with("controlli:")).expect("the line");
        assert!(lines[..at].iter().any(|line| line.starts_with("cursor.t:")), "{name}: after the cursor
{stdout}");
        assert!(lines[at + expected.len()].starts_with("dossier."), "{name}: before the dossier
{stdout}");
        assert!(!stdout.contains("checks."), "{name}: the checks' keys are not printed as leaves
{stdout}");
        assert_eq!(native_differences(&case, &run), Vec::<String>::new(), "{name}");
        // English: the catalog's words, the same shape and the same findings.
        let run = native(&format!("native-{name}-en"), &case, vec![(env_var("LANG"), OsString::from("en"))]);
        let english = checks_block(&lf(&String::from_utf8_lossy(&run.stdout)), "checks:");
        assert_eq!(english.len(), expected.len(), "{name}: {english:?}");
        assert!(english[0].starts_with(&format!("checks: {findings} findings")), "{name}: {english:?}");
        assert_eq!(english[1..expected.len().min(4)], expected[1..expected.len().min(4)], "{name}: the findings are the same sentences");
        if findings > 3 {
            assert_eq!(english.last().map(String::as_str), Some(&*format!("  … and {} more (regia check --project …)", findings - 3)));
        }
    }
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
    let refusals = ["acquire-busy", "release-foreign", "renew-foreign", "renew-absent", "release-unreadable", "lease-acquire-busy"];
    for name in refusals.into_iter().chain(["lease-release-foreign", "step-start-busy", "step-renew-foreign", "step-finish-foreign"]) {
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
    // A row that is not a lock document is nobody's: not busy, not anyone's to release, and taken by the
    // first who asks, in place of no holder. A renewal moves the expiry and keeps when the lock was taken.
    let words = |name: &str| {
        let case = Case::named(name);
        python_in_the_product_s_words(&case, &case.python_output()).expect("the table covers it")
    };
    assert_eq!(words("status-unreadable"), json!({"project": "alpha", "busy": false, "status": "unreadable"}));
    assert_eq!(words("release-unreadable"), json!({"ok": false, "status": "foreign"}));
    let taken = words("acquire-unreadable");
    assert_eq!((&taken["status"], &taken["replaced"], &rows("acquire-unreadable")[0]["generation"]), (&json!("stolen"), &Value::Null, &json!(2)));
    let renewed = words("renew-own");
    assert_eq!(
        (&renewed["renewed_at"], &renewed["taken_at"], &renewed["ttl_s"]),
        (&json!("2026-01-10T09:00:00.000+00:00"), &json!("2026-01-10T08:55:00.000+00:00"), &json!(60.0))
    );
}

/// A write transaction on the state database of `state`, held by another process until it ends: a holder
/// stopped halfway through a mutation.
fn hold(state: &Path) -> Running {
    let args = ["--dir".to_owned(), state.display().to_string(), "hold-tx".to_owned()];
    let mut running = harness::start(harness::launch(Path::new(STORE_PROBE), &args, state)).expect("the holder starts");
    assert_eq!(running.read_line().expect("a line").trim(), "\"ready\"", "the transaction is held");
    running
}

/// The real binary on `case`, with the variables of `extra`, while another process holds the state
/// database's write transaction; and how long the run took, from its launch.
fn held(label: &str, case: &Case, extra: impl FnOnce(&Path) -> Vec<(String, OsString)>) -> (CliRun, Duration) {
    let (mut holder, mut started) = (None, None);
    let run = run_case_on_dossier(label, case, |state| {
        holder = Some(hold(state));
        started = Some(Instant::now());
        extra(state)
    });
    let took = started.expect("the run started").elapsed();
    let ended = holder.expect("the holder started").finish().expect("the holder ends");
    assert!(ended.success(), "the holder ended with {ended:?}");
    (run, took)
}

/// What the prototype answers to a mutation of the lock that `case` records refused as busy, when the
/// transaction stays held beyond the wait (`_esito_slot`): busy and contended, with the lock as a read
/// without the write lock finds it, the one the case recorded.
fn contended(case: &Case) -> Value {
    let Recorded::Json(mut python) = case.recorded() else { panic!("{} is recorded in JSON", case.name) };
    python["motivo"] = json!("lock in contesa, riprova");
    python_in_the_product_s_words(case, &python).expect("the table covers it")
}

/// What the prototype's `lock status` answers when the transaction stays held beyond the wait (`stato_lock`):
/// the check fails closed, busy and unreadable.
fn contended_status() -> Value {
    translate_python("lock status", &json!({"progetto": "alpha", "occupato": true, "stato": "illeggibile"})).expect("the table covers it")
}

/// #442/V5: a transaction held beyond the wait makes `lock acquire` refuse with 3, busy and contended, after
/// waiting it out, and writes nothing.
#[test]
fn a_transaction_held_beyond_the_wait_refuses_a_mutation() {
    let case = Case::named("acquire-busy");
    let (run, took) = held("native-held-acquire", &case, |_| Vec::new());
    assert_eq!(run.exit, Some(3), "{}", run.stderr);
    assert!(took >= Duration::from_millis(2900), "the busy wait was not waited out: {took:?}");
    let rust: Value = serde_json::from_slice(&run.stdout).expect("JSON");
    assert_eq!(json_diff(&rust, &contended(&case)), Vec::<String>::new());
    assert_eq!(rust["reason"], "lock contended, try again");
    assert_eq!(json_diff(&state_rows(&run.state), &case.state_after().expect("state.json")), Vec::<String>::new(), "nothing written");
}

/// #442/V5: under the same hold `lock status` answers busy and unreadable, with exit 0.
#[test]
fn a_transaction_held_beyond_the_wait_makes_the_status_busy_and_unreadable() {
    let (run, took) = held("native-held-status", &Case::named("status-free"), |_| Vec::new());
    assert_eq!(run.exit, Some(0), "{}", run.stderr);
    assert!(took >= Duration::from_millis(2900), "the busy wait was not waited out: {took:?}");
    let rust: Value = serde_json::from_slice(&run.stdout).expect("JSON");
    assert_eq!(json_diff(&rust, &contended_status()), Vec::<String>::new());
}

/// #442/V5 on the prototype: under the same hold, the real prototype through the binary, at the fixtures'
/// instant, answers what the two tests above expect of it. Local only:
///
///     REGIA_HOME=<dossier> cargo test -p regia-conformance --test state -- --ignored
#[test]
#[ignore = "needs the prototype's dossier in REGIA_HOME"]
fn the_prototype_under_a_held_transaction_answers_as_the_product() {
    let dossier = PathBuf::from(std::env::var_os(prototype_env_var("HOME")).expect("REGIA_HOME names the dossier"));
    let prototype = |state: &Path| vec![(prototype_env_var("HOME"), prototype_at_fixture_now(state, &dossier).into_os_string())];
    let reason_in_words = |mut value: Value| {
        if let Some(Value::String(reason)) = value.get("reason") {
            value["reason"] = json!(state_reason_in_the_product_s_words(reason));
        }
        value
    };
    let case = Case::named("acquire-busy");
    let (run, took) = held("live-held-acquire", &case, prototype);
    assert_eq!(run.exit, Some(3), "{}", run.stderr);
    assert!(took >= Duration::from_millis(2900), "the busy wait was not waited out: {took:?}");
    let python = reason_in_words(serde_json::from_slice(&run.stdout).expect("JSON through the facade"));
    assert_eq!(json_diff(&python, &contended(&case)), Vec::<String>::new());
    assert_eq!(json_diff(&state_rows(&run.state), &case.state_after().expect("state.json")), Vec::<String>::new(), "nothing written");
    let (run, _) = held("live-held-status", &Case::named("status-free"), prototype);
    assert_eq!(run.exit, Some(0), "{}", run.stderr);
    let python: Value = serde_json::from_slice(&run.stdout).expect("JSON through the facade");
    assert_eq!(json_diff(&python, &contended_status()), Vec::<String>::new());
}

/// The lock's writes record a line each in the sessions' log, with the prototype's words for the command and
/// the project as given, before they run and also when they refuse; `lock status`, a read, records nothing,
/// and neither does a ttl the prototype's parser refuses (`ATTIVITA_REGISTRATA`, `_registra_attivita`).
#[test]
fn the_lock_s_writes_record_a_line_each() {
    let dir = prepare_state_dir("native-lock-activity");
    let responses = Case::named("status-free").responses_path();
    let commands: [(&[&str], i32); 6] = [
        (&["lock", "acquire", "alpha", "--holder", "x", "--ttl", "60"], 0),
        (&["lock", "acquire", "alpha", "--holder", "y", "--ttl", "60"], 3),
        (&["lock", "acquire", "alpha", "--holder", "y", "--ttl", "soon"], 1),
        (&["lock", "status", "alpha"], 0),
        (&["lock", "renew", "alpha", "--holder", "x", "--ttl", "60"], 0),
        (&["lock", "release", "alpha", "--holder", "x"], 0),
    ];
    for (args, exit) in commands {
        let argv: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (launch, _) = fixture_for_rust(harness::launch(&regia_binary(), &argv, &dir), &responses, &dir);
        let output = launch.env("CLAUDE_CODE_SESSION_ID", "claude-1").run().expect("run regia");
        assert_eq!(output.status.code(), Some(exit), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }
    let mut summary: Vec<Value> = fs::read_dir(dir.join("dichiarazioni"))
        .expect("the sessions' log")
        .flat_map(|entry| fs::read_to_string(entry.expect("a log file").path()).expect("a log file").lines().map(str::to_owned).collect::<Vec<_>>())
        .map(|line| {
            let event: Value = serde_json::from_str(&line).expect("a JSON line");
            json!([event["comando"], event["progetto"], event.get("task").cloned().unwrap_or(Value::Null), event["sessione"]])
        })
        .collect();
    summary.sort_by_key(Value::to_string);
    let line = |command: &str| json!([command, "alpha", null, "claude-1"]);
    assert_eq!(summary, [line("lock prendi"), line("lock prendi"), line("lock rilascia"), line("lock rinnova")]);
}
