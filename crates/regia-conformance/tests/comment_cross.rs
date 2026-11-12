//! #445: the signature of a comment, `comment` and `declare` in the binary, against the prototype.
//!
//! - **The signature** (M1, M2 of the conformance study): the vectors of the prototype's
//!   `test_firma_commenti.py`, byte for byte, on the product's one signer and one reader.
//! - **The cases** (Acceptance 2): the binary with no prototype, so the native engine answers, on each case
//!   of `comment` and `declare`, gives the prototype's exit code, its JSON through the table or its Italian
//!   text, its writes to the tracker in order, its calls to `gh`, reads included, and the rows of the state
//!   database a case records. The normalizations, declared: stderr is each side's own words, so both are
//!   empty or both are not; a body on stdin has its line breaks as LF, as in every case of the harness.

use std::fs;

use regia_conformance::{Case, CliRun, Recorded, json_diff, lf, run_case_on_dossier, state_rows, translate_python};
use regia_core::{SignatureRole, sign, signature_role};
use regia_identity::env_var;
use serde_json::Value;

/// The session of the prototype's `TestFirma`.
const SESSION: &str = "sess-orchestratore";

fn marker(role: &str, session: &str) -> String {
    format!("<!-- regia:autore ruolo={role} sessione={session} -->")
}

/// The vectors of `TestRuoloFirma`, through the product's one reader of the marker (M2). The prototype's
/// twelfth, a body that is `None`, has no counterpart in a `&str`.
#[test]
fn the_marker_reads_as_the_prototype_reads_it() {
    let cases: [(&str, Option<&str>); 11] = [
        ("x\n<!-- regia:autore ruolo=worker sessione=a -->", Some("worker")),
        ("x\n<!-- regia:autore ruolo=titolare sessione=- -->\n\n  \n", Some("titolare")),
        ("x\n<!--regia:autore  ruolo=chat-->", Some("chat")),
        ("x\n<!-- regia:autore ruolo=orchestratore -->", Some("orchestratore")),
        ("x", None),
        ("", None),
        ("<!-- regia:autore ruolo=worker sessione=a -->\nsegue testo", None),
        ("x\n<!-- regia:autore ruolo= sessione=a -->", None),
        ("x\n<!-- regia:autore sessione=a -->", None),
        ("x\n<!-- regia:autore ruolo=worker sessione=a", None),
        ("x <!-- regia:autore ruolo=worker -->", None),
    ];
    for (body, role) in cases {
        assert_eq!(signature_role(body).as_deref(), role, "{body:?}");
    }
}

/// The bodies of `TestFirma`, from the product's signer: each role's visible line and marker, the session
/// as `-` without one, a body already signed left as it is, a marker quoted in the middle signed after it.
#[test]
fn the_signature_is_the_prototype_s_byte_for_byte() {
    use SignatureRole::{Chat, Orchestrator, Owner, Worker};
    assert_eq!(sign("ciao", Chat, Some(SESSION)), format!("ciao\n\n— regia · chat\n{}", marker("chat", SESSION)));
    assert_eq!(sign("ciao", Worker, Some(SESSION)), format!("ciao\n\n— regia · worker\n{}", marker("worker", SESSION)));
    assert_eq!(
        sign("per #7 scelgo A", Owner, Some(SESSION)),
        format!("per #7 scelgo A\n\n— il titolare, via chat\n{}", marker("titolare", SESSION))
    );
    assert_eq!(sign("ciao", Orchestrator, Some(SESSION)), format!("ciao\n\n— regia · orchestratore\n{}", marker("orchestratore", SESSION)));
    assert_eq!(sign("x", Chat, None).lines().last(), Some(marker("chat", "-").as_str()));
    let already = "ciao\n\n— regia · worker\n<!-- regia:autore ruolo=worker sessione=abc -->\n";
    assert_eq!(sign(already, Chat, Some(SESSION)), already);
    let quoted = sign("il marcatore è `<!-- regia:autore ruolo=x -->`\naltro testo", Chat, Some(SESSION));
    assert_eq!((quoted.matches("regia:autore").count(), signature_role(&quoted).as_deref()), (2, Some("chat")));
    assert!(sign("dal file\n", Worker, Some(SESSION)).starts_with("dal file\n\n— regia · worker\n<!-- regia:autore ruolo=worker"));
    for role in SignatureRole::ALL {
        let word = role.word();
        assert_eq!(signature_role(&sign("x", role, Some(SESSION))).as_deref(), Some(word));
    }
}

/// The real binary on `case`, natively: no prototype is named, so the table's group falls back on it. Its
/// text is Italian, the prototype's language.
fn native(label: &str, case: &Case) -> CliRun {
    run_case_on_dossier(label, case, |_| vec![(env_var("LANG"), "it".into())])
}

/// The calls the fake `gh` answered in a run, reads included, by their two words.
fn gh_calls(run: &CliRun) -> Vec<String> {
    let log = fs::read_to_string(run.state.join("fake-gh").join("calls.log")).unwrap_or_default();
    log.lines().map(str::to_owned).collect()
}

/// Every way the native run differs from what the prototype did on `case`, and from the calls it makes.
fn native_differences(case: &Case, run: &CliRun, calls: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    let stdout = lf(&String::from_utf8_lossy(&run.stdout));
    match case.recorded() {
        Recorded::Json(python) => match (serde_json::from_str::<Value>(&stdout), translate_python(&case.command_name(), &python)) {
            (Ok(rust), Ok(python)) => out.extend(json_diff(&rust, &python).into_iter().map(|line| format!("stdout {line}"))),
            (Err(error), _) => out.push(format!("stdout: rust printed no JSON ({error}): {stdout}")),
            (_, Err(error)) => out.push(format!("stdout: the table does not cover the prototype's output: {error}")),
        },
        Recorded::Text(python) => {
            if stdout != python {
                out.push(format!("stdout: rust {stdout:?}, python {python:?}"));
            }
        }
    }
    if run.stderr.trim().is_empty() != case.recorded_stderr().trim().is_empty() {
        out.push(format!("stderr: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
    }
    out.extend(json_diff(&Value::Array(run.writes.clone()), &Value::Array(case.writes())).into_iter().map(|line| format!("writes {line}")));
    if gh_calls(run) != calls {
        out.push(format!("gh calls: rust {:?}, expected {calls:?}", gh_calls(run)));
    }
    if case.spec.state_after {
        let python = case.state_after().unwrap_or(Value::Null);
        out.extend(json_diff(&state_rows(&run.state), &python).into_iter().map(|line| format!("state {line}")));
    }
    out
}

/// The calls the prototype makes for a case of `comment`: the labels, then the comment, unless it stops
/// before them.
fn comment_calls(case: &Case) -> Vec<&'static str> {
    if case.spec.exit == 0 { vec!["issue view", "issue comment"] } else { Vec::new() }
}

/// Acceptance 2 on `comment`: each case, natively, is what the prototype did, its calls to `gh` included.
#[test]
fn every_comment_case_natively_gives_the_prototype_s_outcome() {
    let cases = Case::of_family("comment");
    assert!(cases.len() >= 5, "the cases of `comment` are missing: {}", cases.len());
    for case in &cases {
        let run = native(&format!("native-{}", case.name), case);
        let diff = native_differences(case, &run, &comment_calls(case));
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}
