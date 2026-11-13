//! #456: `init`, `init plan` and `init derive` in the binary, against the prototype (`tools/inizializza.py` of the
//! dossier).
//!
//! - **The cases** (Acceptance 1 and 2): the binary with no prototype, so the native engine answers, on each case of
//!   those commands in the `setup` family, gives the prototype's exit code; its JSON through the table, every key and
//!   none more, or its Italian text; stderr when the prototype wrote some; the calls to `gh` the prototype made, reads
//!   all, as its `chiamate_gh` lists them; no write to the tracker; and the files the case names, byte for byte.
//! - **The controls**: a key the prototype did not print, or one byte of the written file, is a difference.
//! - **What the fixtures cannot hold**, with the prototype's expectations (`test_inizializza.py`,
//!   `test_config_locale.py`): the skills against their copies, a `gh` that is not there, a wrong argument, the
//!   default destination, `init` alone, the plan without any configuration.
//!
//! Against a copy of the prototype, named by `REGIA_HOME` (`git archive` of the dossier's `tools/`, `policies/`,
//! `config/tracker.json` and `skills/`), locally:
//!
//!     REGIA_HOME=<copy> cargo test -p regia-conformance --test init -- --ignored
//!
//! - **The calls**: the prototype, run on each case, calls `gh` as the product does, prints the same, and writes the
//!   same files.
//! - **The skills and the exits**: on the same copies of the same skills both sides print the same plan; without `gh`
//!   both exit 1, on a wrong argument both exit 2.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{
    Case, CliRun, FIXTURE_NOW, Recorded, fixed_now_driver, fixture_for_python, fixture_for_rust, fixtures_dir, json_diff, lf, log_var,
    place_args, prepare_case_state, prepare_dossier, prepare_state_dir, regia_binary, run_case_on_dossier, translate_python,
    unplace_json, unplace_text, written_files,
};
use regia_core::init::{default_project_title, default_repo};
use regia_exec::{Output, Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::{PROTOTYPE_NAME, env_var, prototype_env_var};
use serde_json::{Value, json};

/// The commands of `init` this binary runs natively; `init apply` stays with the prototype.
const PORTED: [&str; 3] = ["init", "init plan", "init derive"];

/// The cases of the ported commands.
fn cases() -> Vec<Case> {
    Case::of_family("setup").into_iter().filter(|case| PORTED.contains(&case.command_name().as_str())).collect()
}

/// A folder in place of the prototype's dossier, with one skill named as the prototype's and no prototype: the facade
/// finds none, so the native engine answers, and the plan has the item of that skill, as the recording has it.
fn skills_source(state: &Path) -> PathBuf {
    let root = state.join("skills-source");
    let skill = root.join("skills").join(PROTOTYPE_NAME);
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::write(skill.join("SKILL.md"), "# The skill\n").unwrap();
    fs::write(skill.join("references").join("a.md"), "a\n").unwrap();
    root
}

/// The variables of a native run: Italian, the prototype's language, and the folder of [`skills_source`].
fn native_env(state: &Path) -> Vec<(String, OsString)> {
    vec![(env_var("LANG"), "it".into()), (prototype_env_var("HOME"), skills_source(state).into())]
}

/// The real binary on `case`, natively.
fn native(label: &str, case: &Case) -> CliRun {
    run_case_on_dossier(label, case, native_env)
}

/// The binary natively on `args`, in a state folder of its own, with the fake `gh` answering `responses`, and the
/// variables of `extra` last.
fn run_with(label: &str, args: &[&str], responses: &Value, extra: impl FnOnce(&Path) -> Vec<(String, OsString)>) -> (Output, PathBuf) {
    let state = prepare_state_dir(label);
    let file = state.join("responses.json");
    fs::write(&file, responses.to_string()).unwrap();
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut launch, _) = fixture_for_rust(regia_exec::harness::launch(&regia_binary(), &args, &state), &file, &state);
    for (name, value) in native_env(&state).into_iter().chain(extra(&state)) {
        launch = launch.env(&name, value);
    }
    (launch.run().expect("run regia"), state)
}

/// The calls the fake `gh` answered in a run, by kind, as it logs them.
fn gh_calls(state: &Path, file: &str) -> Vec<String> {
    fs::read_to_string(state.join(file)).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// The kind the fake `gh` logs a call by, from the line the prototype lists it with in `chiamate_gh`.
fn kind_of(call: &str) -> String {
    if call.starts_with("gh api graphql ") {
        return "owner".to_owned();
    }
    let path = call.strip_prefix("gh api -i ").or_else(|| call.strip_prefix("gh api ")).unwrap_or(call);
    format!("api GET {path}")
}

/// The native output, parsed, with the dossier's folder as the placeholder.
fn native_json(run: &CliRun) -> Result<Value, String> {
    let stdout = lf(&String::from_utf8_lossy(&run.stdout));
    let rust: Value = serde_json::from_str(&stdout).map_err(|error| format!("rust printed no JSON ({error}): {stdout}"))?;
    Ok(run.dossier.as_ref().map_or_else(|| rust.clone(), |dossier| unplace_json(&rust, dossier)))
}

/// Every way the native run differs from what the prototype did on `case`, whose written files are `written`.
fn differences(case: &Case, run: &CliRun, written: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    match case.recorded() {
        Recorded::Json(python) => match (native_json(run), translate_python(&case.command_name(), &python)) {
            (Ok(rust), Ok(translated)) => {
                out.extend(json_diff(&rust, &translated).into_iter().map(|line| format!("stdout {line}")));
                let listed: Vec<String> = python["chiamate_gh"].as_array().unwrap().iter().map(|call| kind_of(call.as_str().unwrap())).collect();
                let answered = gh_calls(&run.state, "fake-gh/calls.log");
                if answered != listed {
                    out.push(format!("gh calls: the fake answered {answered:?}, the prototype listed {listed:?}"));
                }
            }
            (Err(error), _) => out.push(format!("stdout: {error}")),
            (_, Err(error)) => out.push(format!("stdout: the table does not cover the prototype's output: {error}")),
        },
        Recorded::Text(python) => {
            let stdout = lf(&String::from_utf8_lossy(&run.stdout));
            let stdout = run.dossier.as_ref().map_or_else(|| stdout.clone(), |dossier| unplace_text(&stdout, dossier));
            if stdout != python {
                out.push(format!("stdout: rust {stdout:?}, python {python:?}"));
            }
        }
    }
    if run.stderr.trim().is_empty() != case.recorded_stderr().trim().is_empty() {
        out.push(format!("stderr: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
    }
    if !run.writes.is_empty() {
        out.push(format!("writes to the tracker: {:?}", run.writes));
    }
    if let Some(written) = written {
        let dossier = run.dossier.as_ref().expect("a case with written files has a dossier");
        let rust = written_files(dossier, &case.spec.written);
        out.extend(json_diff(&rust, written).into_iter().map(|line| format!("written {line}")));
    }
    out
}

/// Acceptance 1 and 2: each case of `init` alone, `plan` and `derive`, natively, is what the prototype did.
#[test]
fn every_plan_and_derive_case_natively_gives_the_prototype_s_outcome() {
    let cases = cases();
    assert!(cases.len() >= 20, "the cases of the plan and of derive are missing: {}", cases.len());
    for case in &cases {
        let run = native(&format!("native-{}", case.name), case);
        let diff = differences(case, &run, case.written().as_ref());
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

/// The controls: a key the prototype did not print, at the top or in an item, is a difference, and so is one byte of
/// the written file.
#[test]
fn a_key_more_or_a_byte_less_is_a_difference() {
    let case = Case::named("init-plan-ready");
    let run = native("native-control-key", &case);
    assert!(differences(&case, &run, None).is_empty(), "the run itself compares equal");
    let Recorded::Json(python) = case.recorded() else { panic!("a JSON case") };
    let python = translate_python(&case.command_name(), &python).unwrap();
    let mut rust = native_json(&run).unwrap();
    rust["more"] = json!(true);
    rust["items"][0]["more"] = json!(1);
    let diff = json_diff(&rust, &python);
    println!("the control's differences:\n{}", diff.join("\n"));
    assert!(diff.iter().any(|line| line.contains("more") && line.contains("only in rust")), "{diff:?}");
    assert!(diff.iter().any(|line| line.contains("items") && line.contains("only in rust")), "{diff:?}");

    let case = Case::named("init-derive-written");
    let run = native("native-control-byte", &case);
    let mut written = case.written().unwrap();
    let text = written["tracker.json"].as_str().unwrap().to_owned();
    assert!(differences(&case, &run, Some(&written)).is_empty(), "the run itself compares equal");
    written["tracker.json"] = json!(text.replacen("\n}", "}", 1));
    let diff = differences(&case, &run, Some(&written));
    assert!(diff.iter().any(|line| line.starts_with("written")), "a missing line break must show: {diff:?}");
}

/// The plan of `init-plan-ready`, natively, with `prepare` run on the state folder first.
fn ready_plan(label: &str, prepare: impl FnOnce(&Path)) -> Value {
    let case = Case::named("init-plan-ready");
    let run = run_case_on_dossier(label, &case, |state| {
        prepare(state);
        native_env(state)
    });
    assert_eq!(run.exit, Some(0), "{}", run.stderr);
    native_json(&run).unwrap()
}

fn item<'a>(plan: &'a Value, name: &str) -> &'a Value {
    plan["items"].as_array().unwrap().iter().find(|item| item["item"] == name).unwrap_or_else(|| panic!("no item {name}: {plan}"))
}

/// The skill of [`skills_source`] copied into the agents' folders of the home, the state folder, by `which`.
fn copy_skill(state: &Path, which: &[&str]) {
    let source = skills_source(state).join("skills").join(PROTOTYPE_NAME);
    for agent in which {
        let target = state.join(format!(".{agent}")).join("skills").join(PROTOTYPE_NAME);
        fs::create_dir_all(target.join("references")).unwrap();
        for file in ["SKILL.md", "references/a.md"] {
            fs::copy(source.join(file), target.join(file)).unwrap();
        }
    }
}

/// `test_tutto_presente` and `test_cache_python_non_conta`: copies equal to the skill are present, and Python's cache
/// in a copy does not count.
#[test]
fn a_skill_copied_the_same_in_both_homes_is_present() {
    let plan = ready_plan("native-skill-same", |state| {
        copy_skill(state, &["claude", "agents"]);
        let cache = state.join(".claude").join("skills").join(PROTOTYPE_NAME).join("__pycache__");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("x.pyc"), b"\0").unwrap();
    });
    let skill = item(&plan, &format!("skill:{PROTOTYPE_NAME}"));
    assert_eq!((&skill["outcome"], &skill["action"]), (&json!("present"), &json!("none")));
    assert_eq!(skill["detail"], "copie uguali in ~/.claude/skills e ~/.agents/skills");
}

/// `test_skill_non_installata`: no copy anywhere is missing, to install with the prototype's command.
#[test]
fn a_skill_with_no_copy_is_missing_and_to_install() {
    let plan = ready_plan("native-skill-missing", |_| {});
    let skill = item(&plan, &format!("skill:{PROTOTYPE_NAME}"));
    assert_eq!((&skill["outcome"], &skill["action"]), (&json!("missing"), &json!("install")));
    assert!(skill["detail"].as_str().unwrap().contains("python tools/install_skills.py"), "{skill}");
}

/// `test_skill_diversa` and `test_skill_installata_solo_in_una_cartella_e_diversa`: a copy that differs, or a copy in
/// one home only, is different.
#[test]
fn a_skill_whose_copies_differ_is_different() {
    let plan = ready_plan("native-skill-different", |state| {
        copy_skill(state, &["claude", "agents"]);
        fs::write(state.join(".agents").join("skills").join(PROTOTYPE_NAME).join("references").join("a.md"), "old\n").unwrap();
    });
    let skill = item(&plan, &format!("skill:{PROTOTYPE_NAME}"));
    assert_eq!((&skill["outcome"], &skill["action"]), (&json!("different"), &json!("install")));
    let detail = skill["detail"].as_str().unwrap();
    assert!(detail.contains(&format!("~/.agents/skills/{PROTOTYPE_NAME}: diversa")) && detail.contains("~/.claude/skills"), "{detail}");

    let plan = ready_plan("native-skill-one-home", |state| copy_skill(state, &["claude"]));
    assert_eq!(item(&plan, &format!("skill:{PROTOTYPE_NAME}"))["outcome"], "different");
}

/// Without a dossier there is no skill to compare: one item says so, as an error of this machine, and the tracker is
/// still ready.
#[test]
fn without_a_folder_of_skills_one_item_says_so() {
    let case = Case::named("init-plan-ready");
    let run = run_case_on_dossier("native-skill-no-folder", &case, |_| vec![(env_var("LANG"), "it".into())]);
    let plan = native_json(&run).unwrap();
    let skill = item(&plan, "skill");
    assert_eq!((&skill["scope"], &skill["outcome"], &skill["action"]), (&json!("local"), &json!("error"), &json!("manual")));
    assert_eq!(plan["ready"], true);
}

/// The prototype's `nessuna skill in <folder>`: a dossier whose `skills/` holds no folder with a `SKILL.md` gives one
/// item that names the folder.
#[test]
fn a_folder_without_a_skill_is_named_in_its_item() {
    let case = Case::named("init-plan-ready");
    let run = run_case_on_dossier("native-skill-empty-folder", &case, |state| {
        let root = state.join("dossier-without-skills");
        fs::create_dir_all(root.join("skills").join("not-a-skill")).unwrap();
        vec![(env_var("LANG"), "it".into()), (prototype_env_var("HOME"), root.into())]
    });
    let plan = native_json(&run).unwrap();
    let skill = item(&plan, "skill");
    let folder = run.state.join("dossier-without-skills").join("skills");
    assert_eq!(skill["detail"], json!(format!("nessuna skill in {}", folder.display())));
    assert_eq!((&skill["outcome"], &skill["action"]), (&json!("error"), &json!("manual")));
}

/// `test_gh_assente`: without `gh` there is no plan: exit 1, nothing on stdout, for `plan` and for `derive`.
#[test]
fn without_gh_there_is_no_plan() {
    for args in [&["init", "plan", "--json"][..], &["init", "derive", "--destination", "out.json"], &["init"]] {
        let (output, state) =
            run_with("native-no-gh", args, &json!({}), |state| vec![(Program::Gh.env_name(), state.join("no-such-gh").into())]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {stderr}");
        assert!(output.stdout.is_empty() && stderr.contains("cannot be computed"), "{args:?}: {stderr}");
        assert!(!state.join("out.json").exists());
    }
}

/// A wrong argument exits 2, as argparse exits in the prototype's `init`, before any call to `gh`; so does a value
/// that is no owner or repo of GitHub, which the prototype would pass to `gh` (a declared divergence).
#[test]
fn a_wrong_argument_exits_2_before_any_call() {
    let cases: [&[&str]; 6] = [
        &["init", "plan", "--bogus"],
        &["init", "--owner", "x"],
        &["init", "derive", "--overwrite=yes"],
        &["init", "plan", "--owner=-x"],
        &["init", "plan", "--repo", "a b"],
        &["init", "derive", "--destination"],
    ];
    for args in cases {
        let (output, state) = run_with("native-wrong-argument", args, &json!({}), |_| Vec::new());
        assert_eq!(output.status.code(), Some(2), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(gh_calls(&state, "fake-gh/calls.log").is_empty(), "{args:?}");
    }
    let (output, _) = run_with("native-help", &["init", "plan", "--help"], &json!({}), |_| Vec::new());
    assert_eq!(output.status.code(), Some(0));
}

/// The responses of a case, for a run of other arguments.
fn responses_of(name: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(Case::named(name).responses_path()).unwrap()).unwrap()
}

/// `test_destinazione_default_nella_cartella_di_runtime`: without `--destination` the file is the state folder's
/// local configuration. The folder already holds the synthetic one, which says otherwise: exit 3 and the file stays;
/// with `--overwrite` it becomes the derived one, byte for byte.
#[test]
fn without_a_destination_the_file_is_the_state_folder_s() {
    let responses = responses_of("init-derive-written");
    let args = ["init", "derive", "--owner", "example", "--repo", "tracker", "--project-title", "Fixture", "--json"];
    let (output, state) = run_with("native-default-destination", &args, &responses, |_| Vec::new());
    let local = state.join("tracker.json");
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!((output.status.code(), &printed["outcome"], &printed["exit_code"]), (Some(3), &json!("different"), &json!(3)));
    assert_eq!(printed["destination"], json!(local.display().to_string()));
    assert_eq!(fs::read(&local).unwrap(), fs::read(fixtures_dir().join("tracker.json")).unwrap(), "the file stays");

    let overwrite: Vec<&str> = args.iter().copied().chain(["--overwrite"]).collect();
    let (output, state) = run_with("native-default-destination-overwrite", &overwrite, &responses, |_| Vec::new());
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let golden = Case::named("init-derive-written").written().unwrap();
    assert_eq!(fs::read_to_string(state.join("tracker.json")).unwrap(), golden["tracker.json"].as_str().unwrap());
}

/// `test_init_senza_argomenti_e_piano` and `test_json_senza_argomenti_e_piano_json`: `init` alone is the plan with
/// the prototype's defaults, in text and in JSON; `test_json_va_in_coda_agli_argomenti`: `--json` goes anywhere.
#[test]
fn init_alone_is_the_plan() {
    let responses = responses_of("init-unauthenticated");
    let run = |label: &str, args: &[&str]| run_with(label, args, &responses, |_| Vec::new()).0;
    let alone = run("native-alone-json", &["init", "--json"]);
    let plan = run("native-alone-plan-json", &["init", "plan", "--json"]);
    assert_eq!((alone.status.code(), &alone.stdout), (Some(0), &plan.stdout));
    let repo = default_repo();
    let first = run("native-alone-json-first", &["init", "plan", "--json", "--repo", &repo]);
    assert_eq!(first.stdout, plan.stdout);
    let text = run("native-alone-text", &["init"]);
    let plan_text = run("native-alone-plan-text", &["init", "plan"]);
    assert_eq!((text.status.code(), &text.stdout), (Some(0), &plan_text.stdout));
    let header = format!("piano di init: ?/{repo}, Project «{}»\n", default_project_title());
    assert!(String::from_utf8_lossy(&text.stdout).starts_with(&header), "{}", String::from_utf8_lossy(&text.stdout));
}

/// `test_init_parte_senza_configurazione`: the plan needs no configuration, local or of a dossier, and writes none.
#[test]
fn the_plan_runs_without_any_configuration() {
    let responses = responses_of("init-plan-ready");
    let args = ["init", "plan", "--owner", "example", "--repo", "tracker", "--project-title", "Fixture", "--json"];
    let (output, state) = run_with("native-no-config", &args, &responses, |state| {
        fs::remove_file(state.join("tracker.json")).unwrap();
        Vec::new()
    });
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!state.join("tracker.json").exists(), "the plan writes nothing");
}

/// An answer about the user that does not read is an error of the item `gh`, and the tracker is not checked. The
/// prototype's detail ends with the text of its Python exception, the product's with its own (a declared divergence).
#[test]
fn an_unreadable_user_is_an_error_of_gh() {
    let mut responses = responses_of("init-plan-ready");
    responses["rest"]["GET user"] = json!({"headers": {"X-Oauth-Scopes": "project, repo"}, "body": {"name": "no login"}});
    let args = ["init", "plan", "--owner", "example", "--repo", "tracker", "--project-title", "Fixture", "--json"];
    let (output, _) = run_with("native-unreadable-user", &args, &responses, |_| Vec::new());
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    let gh = item(&plan, "gh");
    assert_eq!((&gh["outcome"], &gh["action"], &plan["ready"]), (&json!("error"), &json!("manual"), &json!(false)));
    assert!(gh["detail"].as_str().unwrap().starts_with("risposta di `gh api user` illeggibile: "), "{gh}");
    assert_eq!(item(&plan, "repo")["detail"], "non verificabile: gh non è autenticato");
}

/// The copy of the prototype in `REGIA_HOME`.
fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype", prototype_env_var("HOME")));
    assert!(home.join("tools").join("regia.py").is_file(), "no tools/regia.py in {}", home.display());
    assert!(home.join("skills").is_dir(), "no skills/ in {}: the plan compares them", home.display());
    home
}

/// `regia.py` of the copy at the fixtures' instant on `argv`, as `record` starts it, in `dir` with `responses`; the
/// fake `gh` logs its calls in `python-calls.log`. The variables of `extra` go last.
fn prototype_run(home: &Path, argv: &[String], responses: &Path, dir: &Path, extra: &[(String, OsString)]) -> (Option<i32>, String, String) {
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut full: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    let script = home.join("tools").join("regia.py").display().to_string();
    full.extend([fixed_now_driver().display().to_string(), script, FIXTURE_NOW.to_owned()]);
    full.extend(argv.iter().cloned());
    let launch = regia_exec::harness::program(&interpreter, &full, home).limits(PROTOTYPE_LIMITS);
    let mut launch = fixture_for_python(launch, responses, dir).env(&log_var(), dir.join("python-calls.log"));
    for (name, value) in extra {
        launch = launch.env(name, value);
    }
    let output = launch.run().expect("run the prototype");
    (output.status.code(), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
}

/// The native engine on the copy's own skills: the facade would find the prototype there, so the engine is forced.
fn on_the_copy(home: &Path) -> Vec<(String, OsString)> {
    vec![(env_var("LANG"), "it".into()), (prototype_env_var("HOME"), home.into()), (env_var("ENGINE"), "rust".into())]
}

/// The calls: the prototype, run on each case, calls `gh` as the product does, reads all, in the same order; both
/// print the same, on the copy's skills; and both leave the same files.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_calls_gh_and_writes_as_the_product_does_on_each_case() {
    let home = prototype();
    for case in cases() {
        let dir = prepare_case_state(&format!("python-calls-{}", case.name), &case);
        let argv = case.spec.python.clone().expect("a case of the facade");
        let dossier = case.spec.dossier.as_ref().map(|spec| prepare_dossier(&dir, spec));
        let argv = dossier.as_ref().map_or_else(|| argv.clone(), |dossier| place_args(&argv, dossier));
        let (exit, stdout, stderr) = prototype_run(&home, &argv, &case.responses_path(), &dir, &[]);
        assert_eq!(exit, Some(i32::from(case.spec.exit)), "{}: {stderr}", case.name);
        let rust = run_case_on_dossier(&format!("native-calls-{}", case.name), &case, |_| on_the_copy(&home));
        assert_eq!(gh_calls(&rust.state, "fake-gh/calls.log"), gh_calls(&dir, "python-calls.log"), "{}: the calls", case.name);
        match case.recorded() {
            Recorded::Json(_) => {
                let python: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{}: {error}", case.name));
                let python = dossier.as_ref().map_or_else(|| python.clone(), |dossier| unplace_json(&python, dossier));
                let diff = json_diff(&native_json(&rust).unwrap(), &translate_python(&case.command_name(), &python).unwrap());
                assert_eq!(diff, Vec::<String>::new(), "{}: the output", case.name);
            }
            Recorded::Text(_) => assert_eq!(lf(&String::from_utf8_lossy(&rust.stdout)), lf(&stdout), "{}: the text", case.name),
        }
        if !case.spec.written.is_empty() {
            let python = written_files(dossier.as_ref().unwrap(), &case.spec.written);
            let native = written_files(rust.dossier.as_ref().unwrap(), &case.spec.written);
            assert_eq!(native, python, "{}: the written files", case.name);
        }
    }
}

/// The skills: with the agents' copies the same, different, or in one home only, both sides print the same plan of
/// the copy's own skills.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn both_sides_compare_the_skills_alike() {
    let home = prototype();
    let case = Case::named("init-plan-ready");
    let skills: Vec<PathBuf> = fs::read_dir(home.join("skills")).unwrap().map(|entry| entry.unwrap().path()).filter(|path| path.join("SKILL.md").is_file()).collect();
    let copy = |state: &Path, agents: &[&str], change: bool| {
        for skill in &skills {
            for agent in agents {
                let target = state.join(format!(".{agent}")).join("skills").join(skill.file_name().unwrap());
                copy_folder(skill, &target);
                if change && *agent == "agents" {
                    fs::write(target.join("SKILL.md"), "changed\n").unwrap();
                }
            }
        }
    };
    for (label, agents, change) in [("same", &["claude", "agents"][..], false), ("different", &["claude", "agents"], true), ("one", &["claude"], false)] {
        let dir = prepare_case_state(&format!("python-skills-{label}"), &case);
        copy(&dir, agents, change);
        let (exit, stdout, stderr) = prototype_run(&home, case.spec.python.as_ref().unwrap(), &case.responses_path(), &dir, &[]);
        assert_eq!(exit, Some(0), "{label}: {stderr}");
        let rust = run_case_on_dossier(&format!("native-skills-{label}"), &case, |state| {
            copy(state, agents, change);
            on_the_copy(&home)
        });
        let python = translate_python(&case.command_name(), &serde_json::from_str(&stdout).unwrap()).unwrap();
        assert_eq!(json_diff(&native_json(&rust).unwrap(), &python), Vec::<String>::new(), "{label}");
    }
}

fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_folder(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

/// The exits without a case: without `gh` on its path the prototype exits 1, as the product does without its `gh`;
/// on a wrong argument both exit 2.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn both_sides_exit_1_without_gh_and_2_on_a_wrong_argument() {
    let home = prototype();
    let case = Case::named("init-plan-ready");
    let dir = prepare_case_state("python-no-gh", &case);
    let empty = dir.join("no-gh-here");
    fs::create_dir_all(&empty).unwrap();
    let argv = case.spec.python.clone().unwrap();
    let (exit, stdout, stderr) = prototype_run(&home, &argv, &case.responses_path(), &dir, &[("PATH".to_owned(), empty.into())]);
    assert_eq!((exit, stdout.as_str()), (Some(1), ""), "{stderr}");
    for wrong in [&["init", "piano", "--bogus"][..], &["init", "ricava", "--sovrascrivi=x"]] {
        let argv: Vec<String> = wrong.iter().map(|arg| (*arg).to_owned()).collect();
        let (exit, _, stderr) = prototype_run(&home, &argv, &case.responses_path(), &dir, &[]);
        assert_eq!(exit, Some(2), "{wrong:?}: {stderr}");
    }
}
