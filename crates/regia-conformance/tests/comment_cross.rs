//! #445: the signature of a comment, `comment` and `declare` in the binary, against the prototype.
//!
//! - **The signature** (M1, M2 of the conformance study): the vectors of the prototype's
//!   `test_firma_commenti.py`, byte for byte, on the product's one signer and one reader.
//! - **The cases** (Acceptance 2): the binary with no prototype, so the native engine answers, on each case
//!   of `comment` and `declare`, gives the prototype's exit code, its JSON through the table or its Italian
//!   text, its writes to the tracker in order, its calls to `gh`, reads included, and the rows of the state
//!   database a case records. The normalizations, declared: stderr is each side's own words, so both are
//!   empty or both are not; a body on stdin has its line breaks as LF, as in every case of the harness.
//!
//! Against a copy of the prototype, named by `REGIA_HOME` (`git archive` of the dossier's `tools/` and
//! `policies/`), locally:
//!
//!     REGIA_HOME=<copy> cargo test -p regia-conformance --test comment_cross -- --ignored
//!
//! - **Acceptance 1.** The comments the product writes, one per role, are read by the prototype's own
//!   `ruolo_firma`, and its real `precheck.py`, reading them from the tracker, counts only the owner's as an
//!   answer. A marker broken by hand makes a comment the owner's again: the control.
//! - **X.** The step the prototype opens takes the id of the comment the product writes in that session,
//!   and the prototype's step end copies it into the events cursor, where the precheck reads it: a comment
//!   with that id is not new, even unsigned. The same with the product's step around it.
//! - **The calls.** The prototype, run on each case of `comment`, makes the calls to `gh` the product makes.

use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{
    Case, CliRun, FIXTURE_NOW, Recorded, StateRow, fixed_now_driver, fixture_for_python, fixture_for_rust, json_diff, lf, log_var,
    prepare_case_state, prepare_state_dir, regia_binary, run_case_on_dossier, state_rows, translate_python, workspace_root, write_state,
    writes_of_run,
};
use regia_core::{SignatureRole, sign, signature_role};
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::{env_var, prototype_env_var};
use serde_json::{Value, json};

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

/// The session that holds the step in the proofs across the languages.
const STEP_SESSION: &str = "session-of-the-step";
/// Any other session.
const OTHER_SESSION: &str = "session-of-a-worker";

/// A run's exit code, stdout and stderr.
type Ran = (Option<i32>, String, String);

fn ran(output: regia_exec::Output) -> Ran {
    (output.status.code(), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
}

fn v1() -> toml::Table {
    let path = workspace_root().join("crates").join("regia-store").join("formats").join("store_v1.toml");
    toml::from_str(&fs::read_to_string(path).expect("the v1 state format")).expect("TOML")
}

fn name(section: &str, key: &str) -> String {
    v1()[section][key].as_str().unwrap_or_else(|| panic!("{section}.{key} in the v1 format")).to_owned()
}

/// The copy of the prototype in `REGIA_HOME`.
fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype", prototype_env_var("HOME")));
    assert!(home.join("tools").join("regia.py").is_file(), "no tools/regia.py in {}", home.display());
    home
}

/// The product with no prototype, on the state folder `dir` and the tracker of `responses`, in `session`.
fn product(dir: &Path, responses: &Path, session: Option<&str>, args: &[&str]) -> Ran {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut launch, _) = fixture_for_rust(regia_exec::harness::launch(&regia_binary(), &args, dir), responses, dir);
    if let Some(session) = session {
        launch = launch.env("CLAUDE_CODE_SESSION_ID", session);
    }
    ran(launch.run().expect("run regia"))
}

/// A Python script of the prototype's copy, `regia.py` at the fixtures' instant as `record` starts it, on
/// `dir` and `responses`, in `session`; the fake `gh` logs its calls in `log`.
fn python_run(home: &Path, dir: &Path, responses: &Path, session: Option<&str>, script: &Path, args: &[&str], log: &Path) -> Ran {
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    let text = script.display().to_string();
    if script.file_name().is_some_and(|file| file == "regia.py") {
        argv.extend([fixed_now_driver().display().to_string(), text, FIXTURE_NOW.to_owned()]);
    } else {
        argv.push(text);
    }
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let launch = regia_exec::harness::program(&interpreter, &argv, home).limits(PROTOTYPE_LIMITS);
    let mut launch = fixture_for_python(launch, responses, dir).env(&log_var(), log);
    if let Some(session) = session {
        launch = launch.env("CLAUDE_CODE_SESSION_ID", session);
    }
    ran(launch.run().expect("run the prototype"))
}

/// `regia.py <args>` of the copy.
fn prototype_regia(home: &Path, dir: &Path, responses: &Path, session: Option<&str>, args: &[&str]) -> Ran {
    python_run(home, dir, responses, session, &home.join("tools").join("regia.py"), args, &dir.join("python-calls.log"))
}

/// The prototype's own reader of the signature, `ruolo_firma`, on each body.
fn prototype_roles(home: &Path, dir: &Path, bodies: &[String]) -> Vec<Option<String>> {
    let script = dir.join("roles.py");
    let text = [
        "# Written by the conformance test of #445: the prototype's reader of the signature, on the product's bodies.",
        "import json, sys",
        "sys.path.insert(0, sys.argv[1])",
        "import regia",
        "bodies = json.load(open(sys.argv[2], encoding=\"utf-8\"))",
        "print(json.dumps([regia.ruolo_firma(body) for body in bodies]))",
    ];
    fs::write(&script, text.join("\n") + "\n").expect("the script");
    let input = dir.join("bodies.json");
    fs::write(&input, serde_json::to_string(bodies).expect("JSON")).expect("the bodies");
    let tools = home.join("tools").display().to_string();
    let empty = dir.join("no-responses.json");
    fs::write(&empty, "{}").expect("empty responses");
    let args = [tools.as_str(), &input.display().to_string()];
    let (exit, stdout, stderr) = python_run(home, dir, &empty, None, &script, &args, &dir.join("roles.log"));
    assert_eq!(exit, Some(0), "the prototype's reader\nstdout: {stdout}\nstderr: {stderr}");
    serde_json::from_str(stdout.trim()).expect("JSON")
}

/// The responses of the comment's scenario, with #5's comments as `comments` (id, body), each created after
/// the events cursor's instant; written in `dir`.
fn responses_with_comments(dir: &Path, comments: &[(u64, &str)]) -> PathBuf {
    let text = fs::read_to_string(Case::named("comment-owner").responses_path()).expect("responses");
    let mut responses: Value = serde_json::from_str(&text).expect("JSON");
    let nodes: Vec<Value> = comments
        .iter()
        .enumerate()
        .map(|(i, (id, body))| {
            let url = format!("https://tracker.invalid/issues/5#issuecomment-{id}");
            json!({"databaseId": id, "createdAt": format!("2026-01-10T09:00:{:02}Z", i + 10), "body": body, "url": url})
        })
        .collect();
    responses["issues"]["5"]["comments"]["nodes"] = Value::Array(nodes);
    let path = dir.join("responses-with-comments.json");
    fs::write(&path, serde_json::to_string(&responses).expect("JSON")).expect("the responses");
    path
}

/// The events cursor of alpha: begun before the comments, with `own` as the step's own comments.
fn events_cursor(dir: &Path, own: &[u64]) {
    let cursor = json!({
        name("cursor", "value"): {
            name("events", "version"): 1,
            name("events", "t"): "2026-01-10T09:00:00Z",
            name("events", "own"): {name("events", "comments"): own},
        },
        name("cursor", "updated"): "2026-01-10T09:00:00.000+00:00",
    });
    write_state(dir, &[StateRow { kind: name("kinds", "cursor"), key: "alpha".to_owned(), doc: cursor }]);
}

/// The answers the prototype's real precheck counts on alpha, reading `responses`.
fn precheck_answers(home: &Path, dir: &Path, responses: &Path) -> i64 {
    let script = home.join("tools").join("precheck.py");
    let (exit, stdout, stderr) = python_run(home, dir, responses, None, &script, &["--progetto", "alpha"], &dir.join("precheck.log"));
    assert!(matches!(exit, Some(0 | 3)), "the precheck ended with {exit:?}\nstdout: {stdout}\nstderr: {stderr}");
    let last = stdout.lines().rev().find(|line| line.starts_with('{')).unwrap_or_else(|| panic!("no outcome line: {stdout}"));
    let outcome: Value = serde_json::from_str(last).expect("the outcome line is JSON");
    let alpha = outcome["payload"]["progetti"].as_array().and_then(|projects| projects.iter().find(|project| project["id"] == "alpha"));
    alpha.and_then(|alpha| alpha["gate"].as_i64()).unwrap_or_else(|| panic!("no count of answers for alpha: {last}"))
}

/// The open step of alpha, `session`'s, as the prototype writes it.
fn open_step(session: &str) -> StateRow {
    let doc = json!({
        name("cursor", "value"): {
            name("step", "version"): 1,
            name("step", "holder"): session,
            name("step", "session"): session,
            name("step", "t"): "2026-01-10T08:30:00Z",
            name("step", "comments"): [],
        },
        name("cursor", "updated"): "2026-01-10T08:30:00.000+00:00",
    });
    StateRow { kind: name("kinds", "cursor"), key: format!("alpha:{}", name("cursor", "step")), doc }
}

/// Acceptance 1: one comment per role, written by the product on #5, read by the prototype. Its reader of
/// the signature reads each role, and its precheck counts only the owner's as an answer; with a marker
/// broken by hand, that comment is the owner's again.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_precheck_tells_the_owner_s_comment_from_the_regia_s() {
    let home = prototype();
    let dir = prepare_state_dir("precheck-reads-the-product-s-signature");
    write_state(&dir, &[open_step(STEP_SESSION)]);
    let responses = Case::named("comment-owner").responses_path();
    let writes: [(&[&str], &str); 4] = [
        (&["comment", "5", "--body", "Decisione: B.", "--on-behalf-of-owner"], OTHER_SESSION),
        (&["comment", "5", "--body", "A report.", "--role", "worker"], OTHER_SESSION),
        (&["comment", "5", "--body", "A note."], OTHER_SESSION),
        (&["comment", "5", "--body", "A step's note.", "--role", "worker"], STEP_SESSION),
    ];
    for (args, session) in writes {
        let (exit, stdout, stderr) = product(&dir, &responses, Some(session), args);
        assert_eq!(exit, Some(0), "{args:?}\nstdout: {stdout}\nstderr: {stderr}");
    }
    let bodies: Vec<String> = writes_of_run(&dir).iter().map(|write| write["stdin"].as_str().expect("a body on stdin").to_owned()).collect();
    assert_eq!(bodies.len(), 4, "one write per comment");
    let roles = prototype_roles(&home, &dir, &bodies);
    let expected = ["titolare", "worker", "chat", "orchestratore"].map(|role| Some(role.to_owned()));
    assert_eq!(roles, expected, "the prototype reads the role the product signed");
    events_cursor(&dir, &[]);
    let numbered: Vec<(u64, &str)> = bodies.iter().enumerate().map(|(i, body)| (9301 + i as u64, body.as_str())).collect();
    assert_eq!(precheck_answers(&home, &dir, &responses_with_comments(&dir, &numbered)), 1, "only the owner's is an answer");
    assert_eq!(precheck_answers(&home, &dir, &responses_with_comments(&dir, &numbered[1..])), 0, "the regia's are not");
    // The control: a worker's marker that does not read, and the comment is the owner's.
    let broken = bodies[1].replace("regia:autore", "regia:autor");
    let mut altered = numbered.clone();
    altered[1].1 = &broken;
    assert_eq!(precheck_answers(&home, &dir, &responses_with_comments(&dir, &altered)), 2, "a broken marker is the owner's");
}

/// X: the prototype opens the step of a session; the product comments in that session, reads the
/// prototype's step, signs as its orchestrator and records the id; the prototype's end copies the id into
/// the events cursor, and its precheck does not count a comment with that id, even unsigned. The same with
/// the product's own step around the comment.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_reads_the_comment_the_product_records_in_the_step() {
    let home = prototype();
    let responses = Case::named("comment-orchestrator").responses_path();
    for (label, prototype_s_step) in [("x-prototype-step", true), ("x-product-step", false)] {
        let dir = prepare_state_dir(label);
        let (exit, stdout, stderr) = if prototype_s_step {
            prototype_regia(&home, &dir, &responses, Some(STEP_SESSION), &["--json", "passo", "inizio", "--progetto", "alpha"])
        } else {
            product(&dir, &responses, Some(STEP_SESSION), &["step", "start", "--project", "alpha", "--json"])
        };
        assert_eq!(exit, Some(0), "{label}: the step opens\nstdout: {stdout}\nstderr: {stderr}");
        let (exit, stdout, stderr) = product(&dir, &responses, Some(STEP_SESSION), &["comment", "5", "--body", "A step's note.", "--json"]);
        assert_eq!((exit, stderr.as_str()), (Some(0), ""), "{label}: {stdout}");
        let printed: Value = serde_json::from_str(&stdout).expect("JSON");
        assert_eq!(printed["id"], json!(9204), "{label}");
        let body = writes_of_run(&dir)[0]["stdin"].as_str().expect("the body").to_owned();
        assert_eq!(prototype_roles(&home, &dir, &[body]), [Some("orchestratore".to_owned())], "{label}: the orchestrator's");
        let (exit, stdout, stderr) = if prototype_s_step {
            prototype_regia(&home, &dir, &responses, Some(STEP_SESSION), &["--json", "passo", "fine", "--progetto", "alpha", "--no-push"])
        } else {
            product(&dir, &responses, Some(STEP_SESSION), &["step", "finish", "--project", "alpha", "--no-push", "--json"])
        };
        assert_eq!(exit, Some(0), "{label}: the step closes\nstdout: {stdout}\nstderr: {stderr}");
        let closed: Value = serde_json::from_str(&stdout).expect("JSON");
        let own = if prototype_s_step { &closed["cursore"]["propri"]["commenti"] } else { &closed["cursor"]["own"]["comments"] };
        assert_eq!(own, &json!([9204]), "{label}: the end copies the product's id");
        let unsigned = [(9204, "Unsigned, and the step's own.")];
        assert_eq!(precheck_answers(&home, &dir, &responses_with_comments(&dir, &unsigned)), 0, "{label}: not new, by its id");
        let another = [(9205, "Unsigned, and not the step's.")];
        assert_eq!(precheck_answers(&home, &dir, &responses_with_comments(&dir, &another)), 1, "{label}: the control");
    }
}

/// Acceptance 2, the calls: the prototype, on each case of `comment`, calls `gh` as the product does, reads
/// included, in the same order.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_calls_gh_as_the_product_does_on_each_comment_case() {
    let home = prototype();
    for case in Case::of_family("comment") {
        let dir = prepare_case_state(&format!("python-calls-{}", case.name), &case);
        let argv = case.spec.python.clone().expect("a case of the facade");
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let session = case.spec.env.get("CLAUDE_CODE_SESSION_ID").map(String::as_str);
        let (exit, _, stderr) = prototype_regia(&home, &dir, &case.responses_path(), session, &argv);
        assert_eq!(exit, Some(i32::from(case.spec.exit)), "{}: {stderr}", case.name);
        let python: Vec<String> = fs::read_to_string(dir.join("python-calls.log")).unwrap_or_default().lines().map(str::to_owned).collect();
        let rust = native(&format!("native-calls-{}", case.name), &case);
        assert_eq!(gh_calls(&rust), python, "{}: the calls", case.name);
        let writes = json_diff(&Value::Array(rust.writes.clone()), &Value::Array(writes_of_run(&dir)));
        assert_eq!(writes, Vec::<String>::new(), "{}: the writes", case.name);
    }
}
