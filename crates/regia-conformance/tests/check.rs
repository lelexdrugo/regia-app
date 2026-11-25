//! Acceptance 1 of #448: `check` in the binary, natively, gives on each case of its families what the prototype gave:
//! on the tracker alone (`fixtures/check/`), and on a synthetic dossier with a finding for each of the twelve checks,
//! the state file of the last commit against the one before and of HEAD against the disk, `remaining-unreconciled`
//! through the closed issues (#442/V6) and the project field configured (`fixtures/check-dossier/`). The run is the
//! real binary with no prototype, so the native engine answers, on the case's dossier, at the fixtures' instant.
//!
//! The normalizations, declared:
//! - **The JSON** is compared whole with the prototype's through the facade table: the findings' texts and reasons
//!   and the warnings are the prototype's sentences on both sides.
//! - **The text** is the catalog's: for a case recorded in text, each finding line of the prototype, its check's id
//!   in the contract's words, must be a line of the product's, and there must be as many.
//! - **stderr** is each side's own words: both are empty, or both are not.

use regia_conformance::{Case, CliRun, Recorded, json_diff, lf, run_case_on_dossier, translate_python, unplace_json, unplace_text};
use serde_json::{Value, json};

fn cases() -> Vec<Case> {
    [Case::of_family("check"), Case::of_family("check-dossier")].concat()
}

/// The real binary on `case`, natively, in a state folder of its own: two tests of one case run at once.
fn native(label: &str, case: &Case) -> CliRun {
    run_case_on_dossier(&format!("{label}-{}", case.name), case, |_| Vec::new())
}

/// The contract's id of a check the prototype names.
fn english_id(id: &str) -> String {
    let report = json!({"voci": [{"controllo": id, "progetto": "p", "dove": "d", "testo": "t", "perche": "w"}], "per_controllo": {}, "avvisi": []});
    let translated = translate_python("check", &report).unwrap_or_else(|error| panic!("the table does not know {id}: {error}"));
    translated["findings"][0]["check"].as_str().unwrap().to_owned()
}

/// The finding lines of a text, `  <project>: [<id>] …`.
fn finding_lines(text: &str) -> Vec<&str> {
    text.lines().filter(|line| line.starts_with("  ") && line.contains(": [")).collect()
}

/// The prototype's finding lines, with the contract's ids.
fn prototype_lines(text: &str) -> Vec<String> {
    finding_lines(text)
        .into_iter()
        .map(|line| {
            let (head, rest) = line.split_once(": [").unwrap();
            let (id, tail) = rest.split_once(']').unwrap();
            format!("{head}: [{}]{tail}", english_id(id))
        })
        .collect()
}

fn differences(case: &Case, run: &CliRun) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    let stdout = lf(&String::from_utf8_lossy(&run.stdout));
    let stdout = run.dossier.as_ref().map_or_else(|| stdout.clone(), |dossier| unplace_text(&stdout, dossier));
    match case.recorded() {
        Recorded::Json(python) => match (serde_json::from_str::<Value>(&stdout), translate_python("check", &python)) {
            (Ok(rust), Ok(python)) => {
                let rust = run.dossier.as_ref().map_or_else(|| rust.clone(), |dossier| unplace_json(&rust, dossier));
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
            let (lines, wanted) = (finding_lines(&stdout), prototype_lines(&python));
            if lines.len() != wanted.len() {
                out.push(format!("stdout: rust has {} finding lines, python {}:\n{stdout}", lines.len(), wanted.len()));
            }
            for line in wanted.iter().filter(|line| !stdout.lines().any(|rust| rust == line.as_str())) {
                out.push(format!("stdout: rust has no line {line:?}:\n{stdout}"));
            }
        }
    }
    if run.stderr.trim().is_empty() != case.recorded_stderr().trim().is_empty() {
        out.push(format!("stderr: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
    }
    out
}

#[test]
fn every_check_case_natively_gives_the_prototype_s_findings() {
    let cases = cases();
    assert!(cases.len() >= 7, "the cases of check are missing: {}", cases.len());
    for case in &cases {
        let run = native("native", case);
        let diff = differences(case, &run);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

/// The case of Acceptance 1 is what its note says: at least one finding for each of the twelve checks, through the
/// table; a finding altered by hand is a difference.
#[test]
fn the_dossier_case_has_a_finding_for_each_check_and_a_difference_shows() {
    let case = Case::named("check-dossier-commit");
    let Recorded::Json(python) = case.recorded() else { panic!("the case is recorded in JSON") };
    let translated = translate_python("check", &python).unwrap();
    let checks: std::collections::BTreeSet<&str> = translated["findings"].as_array().unwrap().iter().map(|finding| finding["check"].as_str().unwrap()).collect();
    assert_eq!(checks.len(), 12, "{checks:?}");
    let run = native("native-control", &case);
    let mut rust: Value = serde_json::from_slice(&run.stdout).expect("JSON");
    assert!(json_diff(&rust, &translated).is_empty());
    rust["findings"][0]["why"] = json!("altro");
    assert!(!json_diff(&rust, &translated).is_empty(), "an altered finding is a difference");
}
