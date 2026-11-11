//! The facade's engine (#440). A command that is not ported runs on the prototype through the table,
//! with its exit code, its JSON in English or its text as it is; without a prototype the CLI says so
//! from the catalog. In CI a stand-in interpreter replays what the prototype printed for each case,
//! after checking the argv the table built. The writes to the tracker and to a case's dossier are
//! proven where the prototype is, by the last test, which runs the real binary on the real prototype at
//! the fixtures' instant and needs `REGIA_HOME`.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{
    Case, CliRun, FAKE_PYTHON_WRONG_ARGV, Recorded, Replay, Writes, bin_dir, differences, json_diff, path_with_fake_gh,
    prototype_at_fixture_now, replay_prototype, responses_var, run_case, run_case_on_dossier, translate_python, writes_log,
    writes_var,
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

/// The cases of the state commands (#442) record what their notes say, at the fixtures' instant: a
/// refusal is 3 and a step that cannot close is 1, a lock taken now expires `ttl` seconds later, a
/// processed report that is missing is a warning with exit 0, and a checkpoint commits only what is
/// named.
#[test]
fn the_state_cases_show_what_they_claim() {
    let out = |name: &str| {
        let case = Case::named(name);
        let value = translate_python(&case.command_name(), &case.python_output()).unwrap();
        (case, value)
    };
    for name in ["acquire-busy", "release-foreign", "lease-acquire-busy", "lease-heartbeat-absent", "step-start-busy", "step-renew-foreign", "step-finish-foreign"] {
        let (case, value) = out(name);
        assert_eq!((case.spec.exit, &value["ok"]), (3, &json!(false)), "{name} is a refusal");
    }
    assert_eq!(Case::named("step-start-busy-text").spec.exit, 3);
    assert_eq!(Case::named("step-finish-no-step").spec.exit, 1);
    let (_, busy) = out("step-start-busy");
    assert_eq!((&busy["status"], &busy["current"]["holder"]), (&json!("busy"), &json!("worker-b")));
    let (_, taken) = out("acquire-free");
    assert_eq!((&taken["taken_at"], &taken["expires_in_s"]), (&json!("2026-01-10T09:00:00.000+00:00"), &json!(600)));
    let (_, stolen) = out("acquire-stolen");
    assert_eq!((&stolen["status"], &stolen["replaced"]), (&json!("stolen"), &json!("worker-b")));
    let (finish, closed) = out("step-finish-no-push");
    assert_eq!(closed["cursor"]["own"], json!({"comments": [41, 43], "reports": {"2026-01-09-3-done.md": "2026-01-10T08:00:00+00:00"}}));
    assert!(finish.recorded_stderr().contains("2026-01-09-9-missing.md") && finish.spec.exit == 0, "a missing report warns");
    let (pushed, checkpoint) = out("checkpoint-pushed");
    assert_eq!(checkpoint["dossier"]["outcome"], "pushed");
    assert_eq!(checkpoint["dossier"]["files"], json!([".regia/stato.md", "docs/plan.md", ".regia/journal/2026-01-10.md"]));
    let after = pushed.dossier_after().unwrap();
    assert_eq!(after["remote"][0], after["log"][0], "pushed to the upstream");
    assert!(after["log"][0].as_str().unwrap().contains(checkpoint["dossier"]["sha"].as_str().unwrap()), "the commit it prints");
    assert_eq!(after["status"], json!([" M notes.md"]), "a file nobody named stays out");
    let (_, budget) = out("budget-cache");
    assert_eq!((&budget["provider"]["claude"]["from_cache"], &budget["cache"]["attempt_age_s"]), (&json!(true), &json!(120)));
}

/// The cases of the tracker's writes (#441) record what their notes say: the order and the form of the
/// writes, the refusals that write nothing, and what a write carries in its arguments.
#[test]
fn the_tracker_cases_show_what_they_claim() {
    let out = |name: &str| {
        let case = Case::named(name);
        let value = translate_python(&case.command_name(), &case.python_output()).unwrap();
        (case, value)
    };
    let mutations = |case: &Case| -> Vec<String> {
        case.writes().iter().map(|write| write["mutation"].as_str().unwrap_or("gh").to_owned()).collect()
    };
    for name in ["new-parent-full", "move-objective"] {
        let case = Case::named(name);
        assert_eq!((case.spec.exit, case.writes().len()), (1, 0), "{name} refuses before any write");
    }
    for name in ["unblock-none-text", "parent-already-text", "project-set-dry-text", "mirror-dry-run"] {
        assert!(Case::named(name).writes().is_empty(), "{name} writes nothing");
    }
    let (new, created) = out("new-blocked");
    assert_eq!((&created["task"], &created["parent"]), (&json!(9), &json!(1)));
    let order = ["gh", "addProjectV2ItemById", "updateProjectV2ItemFieldValue", "addSubIssue", "addBlockedBy"];
    assert_eq!(mutations(&new), order, "created, into the Project with its status, under #1, blocked by #3");
    assert_eq!(new.writes()[0]["stdin"], "A body.");
    assert_eq!(Case::named("new-file-text").writes()[0]["stdin"], "A spec, read from a file.\n\nSecond paragraph.\n");
    let blocked: Vec<Value> = Case::named("block-two-text").writes().iter().map(|write| write["variables"]["b"].clone()).collect();
    assert_eq!(blocked, [json!("ISSUE_6"), json!("ISSUE_8")]);

    let (unblock, freed) = out("unblock-worker");
    assert_eq!((&freed["unblocked"], &freed["not_blocking"], &freed["comment"]), (&json!([3]), &json!([6]), &json!(9301)));
    let comments: Vec<Value> = unblock.writes().into_iter().filter(|write| write["argv"][1] == "comment").collect();
    assert_eq!(comments.len(), 1, "one comment for the blockers removed");
    let body = comments[0]["stdin"].as_str().unwrap();
    assert!(body.starts_with("Sbloccato da #3: the gate passed") && body.ends_with("ruolo=worker sessione=fixture-worker -->"), "{body}");

    let parent = |name: &str| Case::named(name).writes().last().unwrap()["arguments"].as_str().unwrap().to_owned();
    assert!(parent("move-beta").contains("replaceParent:true"), "a move replaces the parent");
    assert!(!parent("parent-orphan-text").contains("replaceParent"), "a task with no parent gets one");
    let labels = Case::named("setup-labels").writes();
    assert_eq!(labels.len(), 5);
    assert!(labels.iter().all(|write| write["argv"][1] == "create" && write["argv"].as_array().unwrap().contains(&json!("--force"))));

    let (levers, set) = out("project-set-levers");
    assert_eq!((&set["wip"], &set["priority"], &set["written"], &set["comment"]), (&json!(4), &json!(1), &json!(true), &json!(9401)));
    let writes = levers.writes();
    assert!(writes[0]["stdin"].as_str().unwrap().contains("priorita: 1\nwip: 4\n"), "the keys rewritten in place");
    assert!(writes[1]["stdin"].as_str().unwrap().starts_with("Leve: wip 3 → 4, priorita 2 → 1"));

    let (project, made) = out("project-new-repo");
    assert_eq!((&made["git"], &made["first_push"], &made["objective"]), (&json!("existing"), &Value::Null, &json!(9)));
    assert_eq!(project.dossier_after().unwrap()["origin"], "https://github.com/example/gamma-dossier.git");
    let order = ["gh", "gh", "gh", "gh", "updateProjectV2ItemFieldValue", "updateProjectV2Field", "updateProjectV2ItemFieldValue"];
    assert_eq!(mutations(&project), order, "repo, topic, label, objective, status, option, value");
    let writes = project.writes();
    assert!(writes[3]["stdin"].as_str().unwrap().contains("cartella: {dossier}\n"), "the registry names the dossier");
    assert!(writes[5]["arguments"].as_str().unwrap().contains("{id:\"opt-project-beta\",name:\"beta\",color:GREEN"), "kept");

    let (mirror, mirrored) = out("mirror-writes");
    assert_eq!((&mirrored["written"], &mirrored["already_right"], mirror.writes().len()), (&json!(6), &json!(2), 6));
    let (_, dry) = out("mirror-dry-run");
    assert_eq!((&dry["options_added"], &dry["unlabeled"], &dry["unknown_label"]), (&json!(["beta"]), &json!([5]), &json!([[6, "omega"]])));

    let (_, signed_out) = out("init-unauthenticated");
    assert_eq!((&signed_out["ready"], &signed_out["items"][0]["outcome"]), (&json!(false), &json!("error")), "gh is not signed in");
    assert_eq!(out("init-plan-ready").1["ready"], true);
    let (_, derived) = out("init-derive-written");
    assert_eq!((&derived["outcome"], &derived["exit_code"]), (&json!("written"), &json!(0)));
    let (dry_apply, would) = out("init-apply-dry-run");
    let items: Vec<&Value> = would["writes"].as_array().unwrap().iter().map(|write| &write["item"]).collect();
    assert_eq!(items, ["etichetta:parcheggiato", "etichetta:sessione", "status", "progetto"], "the order of the writes");
    assert_eq!((&would["config"]["outcome"], dry_apply.writes().len()), (&json!("after_the_writes"), 0), "a dry run writes nothing");
    let (failed, refused) = out("init-apply-fails");
    assert_eq!((failed.spec.exit, failed.writes().len(), &refused["config"]), (5, 1, &Value::Null), "one write tried, no configuration");
    let (complete, applied) = out("init-apply-complete");
    assert_eq!((complete.writes().len(), &applied["config"]["outcome"]), (0, &json!("written")));
}

/// The commands whose entries are open: the registry of a project is a free block (ADR 0001 §11).
const OPEN_OUTPUTS: [&str; 1] = ["projects"];

/// Every shape a recording reaches fails closed: a key the table does not know, added to the output
/// of any case, at its top or in its first item, is an error and never passes in Italian, except in an
/// open entry, where it passes as data with its name.
#[test]
fn an_unknown_key_fails_closed_in_every_recorded_output() {
    let mut checked = 0;
    for case in facade_cases() {
        let Recorded::Json(mut value) = case.recorded() else { continue };
        let target = match &mut value {
            Value::Array(items) => items.first_mut(),
            other => Some(other),
        };
        let Some(Value::Object(object)) = target else { continue };
        object.insert("chiave_nuova".to_owned(), json!(1));
        let command = case.command_name();
        match translate_python(&command, &value) {
            Ok(output) => {
                assert!(OPEN_OUTPUTS.contains(&command.as_str()), "{}: an unknown key passed", case.name);
                assert_eq!(output[0]["chiave_nuova"], 1, "{}: it passes as data", case.name);
            }
            Err(error) => assert!(error.to_string().contains("chiave_nuova"), "{}: {error}", case.name),
        }
        checked += 1;
    }
    assert!(checked >= 25, "only {checked} outputs checked");
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
    let arguments = "input:{issueId:$i,stateReason:$r}";
    assert_eq!(lines[3], json!({"mutation": "closeIssue", "variables": {"i": "ISSUE_3", "r": "COMPLETED"}, "arguments": arguments}));
}

/// The real binary on `case`, through the facade to the prototype of `dossier` on its case dossier,
/// against the fake `gh`; `home` gives the folder the binary takes for the dossier.
fn live(label: &str, case: &Case, home: impl FnOnce(&Path) -> PathBuf) -> CliRun {
    run_case_on_dossier(label, case, |state| {
        vec![
            (prototype_env_var("HOME"), home(state).into_os_string()),
            ("PATH".to_owned(), path_with_fake_gh()),
            (responses_var(), case.responses_path().into_os_string()),
            (writes_var(), writes_log(state).into_os_string()),
        ]
    })
}

/// The writes, where the prototype is. The real binary goes through the facade to the real prototype,
/// against the fake `gh` and on each case's dossier, at the fixtures' instant as the recording was, and
/// must end with the recorded exit code and output, make exactly the recorded writes and leave the
/// dossier's repo as the recording left it. A sequence of writes altered by hand must differ, and so
/// must a repo altered by hand and a run on the machine's clock. Local only:
///
///     REGIA_HOME=<dossier> cargo test -p regia-conformance --test facade -- --ignored
#[test]
#[ignore = "needs the prototype's dossier in REGIA_HOME"]
fn the_prototype_through_the_binary_makes_the_recorded_writes() {
    let dossier = PathBuf::from(std::env::var_os(prototype_env_var("HOME")).expect("REGIA_HOME names the dossier"));
    let (mut writes, mut repos) = (0, 0);
    for case in facade_cases() {
        let run = live(&format!("live-{}", case.name), &case, |state| prototype_at_fixture_now(state, &dossier));
        let diff = differences(&case, &run, Writes::Compare);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
        println!("{}: exit {:?}, {} writes, as recorded", case.name, run.exit, run.writes.len());
        if !case.writes().is_empty() {
            let mut altered = case.writes();
            // A subcommand with two of its arguments swapped, or a mutation with its arguments changed.
            match altered[0]["argv"].as_array_mut() {
                Some(argv) => {
                    let len = argv.len();
                    argv.swap(len - 3, len - 1);
                }
                None => altered[0]["arguments"] = json!("ALTERED"),
            }
            assert!(!json_diff(&Value::Array(run.writes.clone()), &Value::Array(altered)).is_empty(), "an altered write went unnoticed");
            writes += run.writes.len();
        }
        if let Some(mut altered) = case.dossier_after() {
            altered["log"].as_array_mut().unwrap().insert(0, json!("0000000 a commit the run did not make"));
            assert!(!json_diff(run.dossier_after.as_ref().unwrap(), &altered).is_empty(), "an altered repo went unnoticed");
            repos += 1;
        }
    }
    assert!(writes > 0 && repos > 0, "no case made a write, or none worked on a repo");
    // The fixtures' instant reaches the prototype through the shim alone: on the machine's clock the
    // lock is taken at another time.
    let case = Case::named("acquire-free");
    let run = live("live-machine-clock", &case, |_| dossier.clone());
    assert!(
        differences(&case, &run, Writes::Compare).iter().any(|line| line.contains("taken_at")),
        "a run on the machine's clock went unnoticed"
    );
}
