//! The facade's engine (#440). A command that is not ported runs on the prototype through the table,
//! with its exit code, its JSON in English or its text as it is; without a prototype the CLI says so
//! from the catalog. In CI a stand-in interpreter replays what the prototype printed for each case,
//! after checking the argv the table built. The writes to the tracker are proven where the prototype
//! is, by the last test, which runs the real binary on the real prototype and needs `REGIA_HOME`.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{
    Case, CliRun, FAKE_PYTHON_WRONG_ARGV, Recorded, Replay, Writes, bin_dir, differences, json_diff, path_with_fake_gh,
    replay_prototype, responses_var, run_case, translate_python, writes_log, writes_var,
};
use regia_facade::Table;
use regia_i18n::{Catalog, Language};
use regia_identity::{env_var, prototype_env_var};
use serde_json::{Value, json};

const FAKE_GH: &str = env!("CARGO_BIN_EXE_fake-gh");

/// The cases of the commands that go through the facade.
fn facade_cases() -> Vec<Case> {
    Case::all().into_iter().filter(|case| case.spec.python.is_some()).collect()
}

fn os(pairs: Vec<(String, PathBuf)>) -> Vec<(String, OsString)> {
    pairs.into_iter().map(|(name, value)| (name, value.into_os_string())).collect()
}

/// The real binary on `case`, with the stand-in interpreter replaying `replay`.
fn replayed(label: &str, case: &Case, replay: &Replay) -> CliRun {
    run_case(label, case, |state| os(replay_prototype(state, replay)))
}

#[test]
fn every_case_through_the_facade_replays_with_its_exit_and_output() {
    let cases = facade_cases();
    assert!(cases.len() >= 2, "the engine's cases are missing");
    for case in &cases {
        let run = replayed(&format!("replay-{}", case.name), case, &Replay::of(case));
        // The stand-in makes no writes: they are proven by the last test.
        let diff = differences(case, &run, Writes::Skip);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

#[test]
fn every_case_names_a_command_of_the_table_once() {
    let table = Table::embedded();
    let cases = Case::all();
    for case in &cases {
        assert!(table.split_command(&case.spec.args).is_some(), "{}/{}: unknown command", case.family, case.name);
        assert_eq!(cases.iter().filter(|other| other.name == case.name).count(), 1, "case name {} is used twice", case.name);
    }
}

#[test]
fn the_engine_cases_show_what_they_claim() {
    let renew = Case::named("renew-foreign");
    assert_eq!(renew.spec.exit, 3, "a refusal");
    assert!(!renew.spec.state.is_empty(), "the refusal comes from the synthetic state database");
    let outcome = translate_python("lock renew", &renew.python_output()).unwrap();
    assert_eq!((&outcome["status"], &outcome["current"]["holder"]), (&json!("foreign"), &json!("worker-b")));
    assert_eq!(outcome["current"]["expired"], true, "expired long ago, whatever the clock reads");
    let maturity = Case::named("maturity-ready");
    assert!(matches!(maturity.recorded(), Recorded::Text(_)), "a command without JSON prints text");
    assert_eq!(maturity.writes().len(), 1, "one edit of the labels");
}

/// Negative controls: an exit code, an output or an argv other than the case's is a difference, and
/// a key the table does not know fails closed.
#[test]
fn an_altered_exit_output_or_argv_is_noticed() {
    let renew = Case::named("renew-foreign");
    let mut exit = Replay::of(&renew);
    exit.exit = 0;
    let diff = differences(&renew, &replayed("negative-exit", &renew, &exit), Writes::Skip);
    assert!(diff.iter().any(|line| line.starts_with("exit")), "an exit 0 in place of 3 went unnoticed: {diff:?}");

    let mut key = Replay::of(&renew);
    key.stdout = key.stdout.replacen("\"ok\"", "\"chiave_nuova\": 1, \"ok\"", 1);
    let run = replayed("negative-key", &renew, &key);
    assert_eq!(run.exit, Some(1), "an unknown key must fail closed: {}", run.stderr);
    assert!(run.stderr.contains("chiave_nuova"), "{}", run.stderr);

    let maturity = Case::named("maturity-ready");
    let mut text = Replay::of(&maturity);
    text.stdout = text.stdout.replace("pronto", "abbozzo");
    let diff = differences(&maturity, &replayed("negative-text", &maturity, &text), Writes::Skip);
    assert!(diff.iter().any(|line| line.starts_with("stdout")), "an altered text went unnoticed: {diff:?}");

    let mut argv = Replay::of(&maturity);
    *argv.argv.last_mut().unwrap() = "abbozzo".to_owned();
    let run = replayed("negative-argv", &maturity, &argv);
    assert_eq!(run.exit, Some(i32::from(FAKE_PYTHON_WRONG_ARGV)), "the stand-in took another argv: {}", run.stderr);
    assert!(!differences(&maturity, &run, Writes::Skip).is_empty());
}

/// Acceptance 3 of #440: without the prototype, or without an interpreter to start it, the CLI says so
/// in the user's language, from the catalog, and never with a traceback or a panic.
#[test]
fn without_the_prototype_the_catalog_says_so() {
    let case = Case::named("maturity-ready");
    for language in Language::ALL {
        let catalog = Catalog::new(language);
        let tag = || (env_var("LANG"), OsString::from(language.tag()));
        let missing = run_case(&format!("no-prototype-{language}"), &case, |_| vec![tag()]);
        let not_found = run_case(&format!("no-python-{language}"), &case, |state| {
            let mut env = os(replay_prototype(state, &Replay::of(&case)));
            env.retain(|(name, _)| *name != env_var("PYTHON"));
            env.extend([(env_var("PYTHON"), OsString::from("no-such-python")), tag()]);
            env
        });
        let brief = Case::named("core");
        let forced = run_case(&format!("forced-python-{language}"), &brief, |_| vec![(env_var("ENGINE"), "python".into()), tag()]);
        for (run, id, command) in [
            (&missing, "facade-not-ported", "task maturity"),
            (&not_found, "facade-no-python", "task maturity"),
            (&forced, "facade-no-prototype", "brief"),
        ] {
            assert_eq!(run.exit, Some(1), "{language}, {id}: {}", run.stderr);
            assert!(run.stderr.contains(&catalog.message_with(id, "command", command)), "{language}, {id}: {}", run.stderr);
            assert!(!run.stderr.contains("Traceback") && !run.stderr.contains("panicked"), "{}", run.stderr);
            assert!(run.stdout.is_empty());
        }
    }
    let en = Catalog::new(Language::En).message_with("facade-not-ported", "command", "x");
    assert_ne!(en, Catalog::new(Language::It).message_with("facade-not-ported", "command", "x"));
}

/// The fake `gh` logs a write in one form for both sides: a subcommand with `--name=value` or
/// `--name value`, and a mutation sent as `-f` pairs or as a request on stdin. It answers a mutation
/// from the fixture and refuses one the fixture does not answer.
#[test]
fn the_fake_gh_logs_writes_in_one_form_for_both_sides() {
    let dir = bin_dir().join("conformance-state").join("fake-gh-writes");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let responses = dir.join("responses.json");
    let fixture = json!({
        "expect": {"o": "example", "r": "tracker"},
        "mutations": {"closeIssue": {"closeIssue": {"issue": {"id": "ISSUE_3"}}}},
        "cli": {"issue comment": "https://tracker.invalid/issues/3#issuecomment-7\n"},
    });
    fs::write(&responses, fixture.to_string()).unwrap();
    let log = dir.join("writes.jsonl");
    let gh = |args: &[&str], stdin: Option<&str>| {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let launch = regia_exec::harness::launch(Path::new(FAKE_GH), &args, &dir)
            .env(&responses_var(), &responses)
            .env(&writes_var(), &log);
        let output = stdin.map_or(launch.clone(), |body| launch.stdin(body)).run().expect("the fake gh runs");
        (output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned())
    };
    assert!(gh(&["issue", "edit", "3", "-R", "example/tracker", "--add-label", "x"], None).0);
    assert!(gh(&["issue", "edit", "3", "--repo=example/tracker", "--add-label=x"], None).0);
    let comment = gh(&["issue", "comment", "3", "-R", "example/tracker", "--body-file", "-"], Some("a body"));
    assert_eq!(comment, (true, "https://tracker.invalid/issues/3#issuecomment-7\n".to_owned()));
    let mutation = "mutation($i:ID!,$r:IssueClosedStateReason!){closeIssue(input:{issueId:$i,stateReason:$r}){issue{id}}}";
    let pairs = gh(&["api", "graphql", "-f", &format!("query={mutation}"), "-f", "i=ISSUE_3", "-f", "r=COMPLETED"], None);
    let request = json!({"query": mutation, "variables": {"i": "ISSUE_3", "r": "COMPLETED"}}).to_string();
    let body = gh(&["api", "graphql", "--input=-"], Some(&request));
    assert_eq!(pairs, body);
    assert_eq!(serde_json::from_str::<Value>(&pairs.1).unwrap(), json!({"data": {"closeIssue": {"issue": {"id": "ISSUE_3"}}}}));
    assert!(!gh(&["api", "graphql", "-f", "query=mutation{reopenIssue(input:{}){issue{id}}}"], None).0, "no fixture answer");
    assert!(!gh(&["issue", "delete", "3"], None).0, "an unknown subcommand is refused");

    let lines: Vec<Value> = fs::read_to_string(&log).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.len(), 6, "{lines:?}");
    assert_eq!(lines[0], lines[1], "`--name=value` and `-R` log as `--name value` and `--repo`");
    assert_eq!(lines[2]["stdin"], "a body");
    assert_eq!(lines[3], lines[4], "the two forms of a mutation log the same");
    assert_eq!(lines[3], json!({"mutation": "closeIssue", "variables": {"i": "ISSUE_3", "r": "COMPLETED"}}));
}

/// The writes, where the prototype is. The real binary goes through the facade to the real prototype,
/// against the fake `gh`, and must end with the recorded exit code and output and make exactly the
/// recorded writes; a sequence of writes altered by hand must differ. Local only:
///
///     REGIA_HOME=<dossier> cargo test -p regia-conformance --test facade -- --ignored
#[test]
#[ignore = "needs the prototype's dossier in REGIA_HOME"]
fn the_prototype_through_the_binary_makes_the_recorded_writes() {
    let dossier = PathBuf::from(std::env::var_os(prototype_env_var("HOME")).expect("REGIA_HOME names the dossier"));
    let mut writes = 0;
    for case in facade_cases() {
        let run = run_case(&format!("live-{}", case.name), &case, |state| {
            vec![
                (prototype_env_var("HOME"), dossier.clone().into_os_string()),
                ("PATH".to_owned(), path_with_fake_gh()),
                (responses_var(), case.responses_path().into_os_string()),
                (writes_var(), writes_log(state).into_os_string()),
            ]
        });
        let diff = differences(&case, &run, Writes::Compare);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
        println!("{}: exit {:?}, {} writes, as recorded", case.name, run.exit, run.writes.len());
        if let Some(first) = case.writes().first() {
            let mut altered = case.writes();
            let labels = first["argv"].as_array().unwrap().len();
            altered[0]["argv"].as_array_mut().unwrap().swap(labels - 3, labels - 1);
            assert!(!json_diff(&Value::Array(run.writes.clone()), &Value::Array(altered)).is_empty(), "a swapped write went unnoticed");
            writes += run.writes.len();
        }
    }
    assert!(writes > 0, "no case made a write");
}
