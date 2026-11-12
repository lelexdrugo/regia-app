//! Acceptance 1 and 2 of #447: `checkpoint` in the binary, natively, gives on each case of its family the
//! outcome the prototype gave on the same repo, and leaves the repo as the prototype left it, commit ids
//! included. The run is the real binary with no prototype, so the native engine answers, against the
//! fake `gh` and on the case's dossier built as the recording built it, at the fixtures' instant.
//!
//! Two normalizations, declared:
//! - **The reason** is the product's own sentence, in English: the prototype's is translated by
//!   [`words::in_the_product_s_words`] before the comparison. What git said, its last line, is compared as it
//!   is.
//! - **The text** of the product is the contract's keys, one a line, as `budget` prints them: for a
//!   case recorded in text, the outcome its line names is compared, and the repo.

use std::path::{Path, PathBuf};

mod words;

use regia_conformance::{Case, CliRun, Recorded, bin_dir, json_diff, lf, run_case_on_dossier, translate_python, unplace_json, unplace_text};
use words::{in_the_product_s_words, outcome_of_line};
use regia_identity::env_var;
use serde_json::{Value, json};

/// The cases where the product differs from the prototype on purpose, each with a test below that says
/// how: a name with a space, which the prototype commits and a dossier path cannot spell (#571/V5).
const DIVERGENT: [&str; 1] = ["checkpoint-file-with-space"];

fn checkpoint_cases() -> Vec<Case> {
    Case::of_family("checkpoint")
}

/// The real binary on `case`, natively: no prototype is named, so the table's group falls back on it.
fn native(label: &str, case: &Case, extra: Vec<(String, std::ffi::OsString)>) -> CliRun {
    run_case_on_dossier(label, case, |_| extra)
}

/// Every way the native run differs from what the prototype did on `case`: exit code, output through
/// the table with the reason in the product's words, stderr, and the repo after the run.
fn native_differences(case: &Case, run: &CliRun) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    // A case of a class only (#571/V8): the sentence of an error is each side's own.
    if case.spec.same_class_only {
        out.extend(regia_conformance::class_differences(case, run));
        let after = |value: Option<Value>| value.unwrap_or(Value::Null);
        out.extend(json_diff(&after(run.dossier_after.clone()), &after(case.dossier_after())).into_iter().map(|line| format!("dossier {line}")));
        return out;
    }
    // A case with no dossier has no folder to put back as the placeholder.
    let unplace = |value: &Value| run.dossier.as_ref().map_or_else(|| value.clone(), |dossier| unplace_json(value, dossier));
    let unplace_line = |text: &str| run.dossier.as_ref().map_or_else(|| text.to_owned(), |dossier| unplace_text(text, dossier));
    let stdout = lf(&String::from_utf8_lossy(&run.stdout));
    match case.recorded() {
        Recorded::Json(python) => {
            let mut python = translate_python("checkpoint", &python).expect("the table covers the prototype's output");
            if let Some(reason) = python["dossier"]["reason"].as_str() {
                python["dossier"]["reason"] = json!(in_the_product_s_words(reason));
            }
            match serde_json::from_str::<Value>(&stdout) {
                Ok(rust) => out.extend(json_diff(&unplace(&rust), &python).into_iter().map(|line| format!("stdout {line}"))),
                Err(error) => out.push(format!("stdout: rust printed no JSON ({error}): {stdout}")),
            }
        }
        Recorded::Text(python) => {
            let line = format!("dossier.outcome: {}", outcome_of_line(python.trim_end()));
            if !unplace_line(&stdout).lines().any(|rust| rust == line) {
                out.push(format!("stdout: rust {stdout:?} has no `{line}`, python {python:?}"));
            }
        }
    }
    if !run.stderr.is_empty() || !case.recorded_stderr().is_empty() {
        out.push(format!("stderr: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
    }
    let after = |value: Option<Value>| value.unwrap_or(Value::Null);
    out.extend(json_diff(&after(run.dossier_after.clone()), &after(case.dossier_after())).into_iter().map(|line| format!("dossier {line}")));
    out
}

#[test]
fn every_checkpoint_case_natively_gives_the_prototype_s_outcome() {
    let cases = checkpoint_cases();
    assert!(cases.len() >= 15, "the cases of #442/V4 and of the prototype's tests are missing");
    for case in cases.iter().filter(|case| !DIVERGENT.contains(&case.name.as_str())) {
        let run = native(&format!("native-{}", case.name), case, Vec::new());
        let diff = native_differences(case, &run);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

/// Negative controls: on the machine's clock the commit has another id, and an outcome or a repo
/// altered by hand is a difference.
#[test]
fn the_comparison_notices_a_difference() {
    let case = Case::named("checkpoint-pushed");
    let run = native("native-machine-clock", &case, vec![(env_var("NOW"), "".into())]);
    let diff = native_differences(&case, &run);
    assert!(diff.iter().any(|line| line.contains("sha")) && diff.iter().any(|line| line.starts_with("dossier")), "{diff:?}");

    let run = native("native-altered", &case, Vec::new());
    assert!(native_differences(&case, &run).is_empty());
    let mut altered = run;
    altered.stdout = String::from_utf8_lossy(&altered.stdout).replace("\"pushed\"", "\"nothing\"").into_bytes();
    assert!(native_differences(&case, &altered).iter().any(|line| line.contains("outcome")), "an altered outcome went unnoticed");
    altered.dossier_after.as_mut().unwrap()["status"] = json!([]);
    assert!(native_differences(&case, &altered).iter().any(|line| line.starts_with("dossier")), "an altered repo went unnoticed");
}

/// The rows of the state database of a run, as they are after it.
fn state_rows(state: &Path) -> Vec<(String, String, String)> {
    let connection = rusqlite::Connection::open(state.join(regia_store::database_name())).expect("the state database");
    let mut rows = connection.prepare("SELECT tipo, chiave, doc FROM documenti ORDER BY tipo, chiave").expect("the documents table");
    rows.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(Result::unwrap).collect()
}

/// `test_non_toglie_il_lock_a_chi_lo_ha`: a checkpoint takes no lock, and leaves the one someone holds
/// as it was.
#[test]
fn a_checkpoint_leaves_the_lock_to_whoever_holds_it() {
    let case = Case::named("checkpoint-lock-held");
    assert!(!case.spec.state.is_empty());
    let label = "native-lock-held";
    let run = native(label, &case, Vec::new());
    assert!(native_differences(&case, &run).is_empty());
    let state: PathBuf = bin_dir().join("conformance-state").join(label);
    let rows = state_rows(&state);
    let expected: Vec<(String, String, String)> =
        case.spec.state.iter().map(|row| (row.kind.clone(), row.key.clone(), row.doc.to_string())).collect();
    assert_eq!(rows, expected, "the lock row is as the case wrote it");
}

/// The cases record what their notes say: each path of the dossier the prototype takes, with its outcome,
/// its reason and the repo it leaves.
#[test]
fn the_checkpoint_cases_show_what_they_claim() {
    let out = |name: &str| {
        let case = Case::named(name);
        let value = translate_python("checkpoint", &case.python_output()).unwrap();
        let after = case.dossier_after();
        (case, value["dossier"].clone(), after)
    };
    let (_, retry, after) = out("checkpoint-retry-pushes");
    assert_eq!((&retry["outcome"], &retry["reason"], &retry["ahead"], &retry["files"]), (&json!("pushed"), &json!("ritentato: 1 commit già fatti"), &json!(0), &json!([])));
    let after = after.unwrap();
    assert_eq!(after["log"], after["remote"], "the commit left behind is on the remote, and no new one");
    assert!(after["log"][0].as_str().unwrap().ends_with("fixture: a commit not pushed"));
    assert_eq!(out("checkpoint-retry-counts-all").1["reason"], "ritentato: 2 commit già fatti");
    let (_, again, after) = out("checkpoint-retry-fails-again");
    assert_eq!((&again["outcome"], &again["ahead"]), (&json!("local_commit"), &json!(1)));
    assert!(again["reason"].as_str().unwrap().contains("; push: "), "{again}");
    assert_ne!(after.as_ref().unwrap()["log"], after.as_ref().unwrap()["remote"]);
    let (_, failed, _) = out("checkpoint-push-fails");
    assert_eq!((&failed["outcome"], &failed["ahead"]), (&json!("local_commit"), &json!(1)));
    let (_, diverged, after) = out("checkpoint-diverged");
    assert_eq!(diverged["outcome"], "local_commit");
    assert!(diverged["reason"].as_str().unwrap().ends_with("(divergenza: la risolve il titolare)"));
    let after = after.unwrap();
    assert!(after["remote"][0].as_str().unwrap().ends_with("fixture: another clone's commit"), "the remote kept its own");
    let (_, idle, _) = out("checkpoint-diverged-nothing");
    assert_eq!(idle["outcome"], "nothing");
    assert!(idle["reason"].as_str().unwrap().starts_with("fast-forward non riuscito: "));
    let (_, forward, after) = out("checkpoint-fast-forward");
    assert_eq!(forward["outcome"], "pushed");
    assert_eq!(after.as_ref().unwrap()["log"][1].as_str().unwrap().split_once(' ').unwrap().1, "fixture: another clone's commit");
    assert_eq!(out("checkpoint-detached").1["reason"], "HEAD staccato: nessun branch su cui committare");
    let (staged, theirs, after) = out("checkpoint-staged-stays");
    assert_eq!((&theirs["outcome"], staged.spec.dossier.as_ref().unwrap().staged.len()), (&json!("pushed"), 1));
    assert!(after.unwrap()["status"].as_array().unwrap().contains(&json!("A  theirs.txt")), "what someone else staged stays staged");
    let (_, branch, after) = out("checkpoint-never-another-branch");
    assert_eq!((&branch["outcome"], &after.unwrap()["remote_refs"]), (&json!("pushed"), &json!(["refs/heads/main"])));
    let (_, session, after) = out("checkpoint-other-session-commit");
    assert_eq!(session["reason"], "ritentato: 1 commit già fatti");
    assert_eq!(after.unwrap()["status"], json!([" M notes.md", "?? new.txt"]));
    assert_eq!(out("checkpoint-no-upstream-no-retry").1["outcome"], "nothing");
    assert_eq!(out("checkpoint-no-upstream").1["reason"], "il branch main non ha un upstream");
    assert_eq!(out("checkpoint-hook-refuses").1["reason"], "git commit: no");
    assert_eq!(out("checkpoint-file-outside").1["reason"], "--file '../outside.md': esce dalla cartella del progetto");
    assert_eq!(out("checkpoint-not-the-root").1["reason"], "la cartella non è la radice di un repo git");
    assert_eq!(out("checkpoint-folder-missing").1["folder"], "{dossier}/missing");
    let (registry, folder, _) = out("checkpoint-registry-no-folder");
    assert_eq!((registry.spec.exit, &folder["reason"]), (1, &json!("cartella del progetto non letta: il registro di 'alpha' non ha la cartella")));
    let (registry, folder, _) = out("checkpoint-registry-folder");
    assert_eq!((registry.spec.exit, &folder["folder"], &folder["outcome"]), (0, &json!("/no-such-folder/alpha"), &json!("unavailable")));
    for (name, product) in [
        ("ritentato: 2 commit già fatti", "retried: 2 commits already made"),
        ("fetch non riuscito: x; push: y", "fetch failed: x; push: y"),
        ("il branch main non ha un upstream", "the branch main has no upstream"),
    ] {
        assert_eq!(in_the_product_s_words(name), product);
    }
}

/// #571/V8: with a git that gives no answer, or none at all, the product commits nothing and says so; the
/// repo is as the case built it. The comparison with the prototype is by class, so this holds what the
/// product's own sentence says.
#[test]
fn a_git_that_does_not_answer_commits_nothing() {
    for (name, said) in [("checkpoint-git-timeout", "git: fake git: no answer"), ("checkpoint-git-missing", "git: git")] {
        let case = Case::named(name);
        let run = native(&format!("native-{name}"), &case, Vec::new());
        assert_eq!(run.exit, Some(0), "{name}: {}", run.stderr);
        let out: Value = serde_json::from_slice(&run.stdout).expect("JSON");
        assert_eq!(out["dossier"]["outcome"], "unavailable", "{name}");
        assert!(out["dossier"]["reason"].as_str().is_some_and(|reason| reason.starts_with(said)), "{name}: {out}");
        assert_eq!(out["dossier"]["sha"], Value::Null, "{name}: no commit was made");
    }
    let case = Case::named("checkpoint-tracker-down");
    let run = native("native-tracker-down", &case, Vec::new());
    assert_eq!(run.exit, Some(1), "{}", run.stderr);
}

/// #571/V5, the declared divergence: the prototype commits and pushes a `--file` with a space, the
/// product answers `unavailable` with the path and the rule, and the repo stays as it was.
#[test]
fn a_space_in_a_file_is_refused_where_the_prototype_commits() {
    let case = Case::named(DIVERGENT[0]);
    let python = translate_python("checkpoint", &case.python_output()).unwrap();
    assert_eq!(python["dossier"]["outcome"], "pushed", "the prototype commits it");
    let run = native("native-file-with-space", &case, Vec::new());
    assert_eq!(run.exit, Some(i32::from(case.spec.exit)), "{}", run.stderr);
    let out: Value = serde_json::from_slice(&run.stdout).expect("JSON");
    assert_eq!(out["dossier"]["outcome"], "unavailable");
    assert_eq!(out["dossier"]["reason"], "--file 'notes/two words.md': invalid dossier path: has a character outside its alphabet");
    assert_eq!(out["dossier"]["sha"], Value::Null, "nothing was committed");
    assert_ne!(run.dossier_after, case.dossier_after(), "the repo is not the one the prototype left");
}
