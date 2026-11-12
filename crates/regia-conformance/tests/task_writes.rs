//! #446: the writes of the plan in the binary, against the prototype.
//!
//! - **The cases** (Acceptance 1): the binary with no prototype, so the native engine answers, on each case
//!   of a ported write of the `task` family, gives the prototype's exit code, its JSON through the table or
//!   its Italian text, its writes to the tracker in order, and the rows of the state database a case
//!   records. stderr is each side's own words: both are empty or both are not. Where a case lists them
//!   below, its calls to `gh`, reads included, are checked too.
//! - **T11**: a closing status writes the status a second time, three seconds after the first.
//! - **The control** (Acceptance 2): a recording with two writes swapped does not compare equal.
//!
//! Against a copy of the prototype, named by `REGIA_HOME` (`git archive` of the dossier's `tools/` and
//! `policies/`), locally:
//!
//!     REGIA_HOME=<copy> cargo test -p regia-conformance --test task_writes -- --ignored
//!
//! - **The calls.** The prototype, run on each case, calls `gh` as the product does: the same calls, reads
//!   included, in the same order, and the same writes.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use regia_conformance::{
    Case, CliRun, FIXTURE_NOW, Recorded, fixed_now_driver, fixture_for_python, json_diff, lf, log_var, place_args, prepare_case_state,
    prepare_dossier, run_case_on_dossier, state_rows, translate_python, writes_of_run,
};
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::{env_var, prototype_env_var};
use serde_json::Value;

/// The writes of the `task` family this binary runs natively.
const PORTED: [&str; 1] = ["task status"];

/// The calls to `gh` of some cases, reads included, which the tests check without the prototype: by kind
/// as the fake logs them. The cross test checks every case against the prototype's own.
const CALLS: [(&str, &[&str]); 5] = [
    (
        "status-done",
        &[
            "issue 3",
            "mutation closeIssue",
            "node ISSUE_3",
            "mutation updateProjectV2ItemFieldValue",
            "node ISSUE_3",
            "mutation updateProjectV2ItemFieldValue",
        ],
    ),
    ("status-approved-text", &["issue 8", "node ISSUE_8", "mutation updateProjectV2ItemFieldValue"]),
    (
        "status-not-in-project",
        &["issue 8", "node ISSUE_8", "mutation addProjectV2ItemById", "mutation updateProjectV2ItemFieldValue"],
    ),
    ("status-comment-owner", &["issue 5", "node ISSUE_5", "mutation updateProjectV2ItemFieldValue", "issue view", "issue comment"]),
    ("status-missing", &["issue 77"]),
];

/// The cases of the ported writes.
fn cases() -> Vec<Case> {
    Case::of_family("task").into_iter().filter(|case| PORTED.contains(&case.command_name().as_str())).collect()
}

/// The real binary on `case`, natively: no prototype is named, so the table's group falls back on it. Its
/// text is Italian, the prototype's language.
fn native(label: &str, case: &Case) -> CliRun {
    run_case_on_dossier(label, case, |_| vec![(env_var("LANG"), "it".into())])
}

/// The calls the fake `gh` answered in a run, reads included, by kind.
fn gh_calls(state: &Path, file: &str) -> Vec<String> {
    let log = fs::read_to_string(state.join(file)).unwrap_or_default();
    log.lines().map(str::to_owned).collect()
}

/// Every way the native run differs from what the prototype did on `case`, whose writes are `writes`.
fn differences(case: &Case, run: &CliRun, writes: &[Value]) -> Vec<String> {
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
    out.extend(json_diff(&Value::Array(run.writes.clone()), &Value::Array(writes.to_vec())).into_iter().map(|line| format!("writes {line}")));
    if let Some((_, calls)) = CALLS.iter().find(|(name, _)| *name == case.name) {
        let rust = gh_calls(&run.state, "fake-gh/calls.log");
        if rust != *calls {
            out.push(format!("gh calls: rust {rust:?}, expected {calls:?}"));
        }
    }
    if case.spec.state_after {
        let python = case.state_after().unwrap_or(Value::Null);
        out.extend(json_diff(&state_rows(&run.state), &python).into_iter().map(|line| format!("state {line}")));
    }
    out
}

/// Acceptance 1: each case of a ported write, natively, is what the prototype did.
#[test]
fn every_write_case_natively_gives_the_prototype_s_outcome() {
    let cases = cases();
    assert!(cases.len() >= 10, "the cases of the writes are missing: {}", cases.len());
    for case in &cases {
        let run = native(&format!("native-{}", case.name), case);
        let diff = differences(case, &run, &case.writes());
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

/// T11: done writes the status, waits three seconds, and writes it again; a status that does not close
/// writes it once, without the pause.
#[test]
fn a_closing_status_is_written_again_after_three_seconds() {
    let started = Instant::now();
    let done = native("native-t11-done", &Case::named("status-done"));
    let took = started.elapsed();
    assert_eq!(done.exit, Some(0), "{}", done.stderr);
    let updates = |run: &CliRun| run.writes.iter().filter(|write| write["mutation"] == "updateProjectV2ItemFieldValue").count();
    assert_eq!(updates(&done), 2);
    assert!(took >= Duration::from_secs(3), "the second write came after {took:?}");
    let started = Instant::now();
    let approved = native("native-t11-approved", &Case::named("status-approved-text"));
    assert_eq!((approved.exit, updates(&approved)), (Some(0), 1), "{}", approved.stderr);
    assert!(started.elapsed() < Duration::from_secs(3), "no pause without closing");
}

/// Acceptance 2: the comparison sees the order of the writes. With the close and the first write of the
/// status swapped in the recording, the same native run no longer compares equal.
#[test]
fn a_write_out_of_order_fails_the_comparison() {
    let case = Case::named("status-done");
    let mut swapped = case.writes();
    swapped.swap(0, 1);
    let run = native("native-control-swapped", &case);
    assert!(differences(&case, &run, &case.writes()).is_empty(), "the run itself compares equal");
    let diff = differences(&case, &run, &swapped);
    println!("the control's differences:\n{}", diff.join("\n"));
    assert!(diff.iter().any(|line| line.starts_with("writes ")), "a swap must show: {diff:?}");
}

/// The copy of the prototype in `REGIA_HOME`.
fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype", prototype_env_var("HOME")));
    assert!(home.join("tools").join("regia.py").is_file(), "no tools/regia.py in {}", home.display());
    home
}

/// `regia.py` of the copy at the fixtures' instant, as `record` starts it, on the case's state and
/// responses, with its session; the fake `gh` logs its calls in `python-calls.log`.
fn prototype_run(home: &Path, case: &Case, dir: &Path) -> (Option<i32>, String) {
    let argv = case.spec.python.clone().expect("a case of the facade");
    let argv = case.spec.dossier.as_ref().map_or_else(|| argv.clone(), |spec| place_args(&argv, &prepare_dossier(dir, spec)));
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut full: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    let script = home.join("tools").join("regia.py").display().to_string();
    full.extend([fixed_now_driver().display().to_string(), script, FIXTURE_NOW.to_owned()]);
    full.extend(argv);
    let launch = regia_exec::harness::program(&interpreter, &full, home).limits(PROTOTYPE_LIMITS);
    let mut launch = fixture_for_python(launch, &case.responses_path(), dir).env(&log_var(), dir.join("python-calls.log"));
    for (name, value) in case.spec.session_env() {
        launch = launch.env(&name, value);
    }
    let output = launch.run().expect("run the prototype");
    (output.status.code(), String::from_utf8_lossy(&output.stderr).into_owned())
}

/// The calls: the prototype, run on each case of a ported write, calls `gh` as the product does, reads
/// included, in the same order, and writes the same.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_calls_gh_as_the_product_does_on_each_write_case() {
    let home = prototype();
    for case in cases() {
        let dir = prepare_case_state(&format!("python-calls-{}", case.name), &case);
        let (exit, stderr) = prototype_run(&home, &case, &dir);
        assert_eq!(exit, Some(i32::from(case.spec.exit)), "{}: {stderr}", case.name);
        let rust = native(&format!("native-calls-{}", case.name), &case);
        let python = gh_calls(&dir, "python-calls.log");
        assert_eq!(gh_calls(&rust.state, "fake-gh/calls.log"), python, "{}: the calls", case.name);
        let writes = json_diff(&Value::Array(rust.writes.clone()), &Value::Array(writes_of_run(&dir)));
        assert_eq!(writes, Vec::<String>::new(), "{}: the writes", case.name);
    }
}
