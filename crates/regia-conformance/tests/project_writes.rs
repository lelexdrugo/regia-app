//! #452: `setup`, `project new` and `project set` in the binary, against the prototype.
//!
//! - **The cases** (Acceptance 1): the binary with no prototype, so the native engine answers, on each case of the
//!   `project-writes` family and on the four recorded before it (`setup-labels`, `project-new-repo`,
//!   `project-set-levers`, `project-set-dry-text`), gives the prototype's exit code; its JSON through the table, or
//!   its Italian text; its writes to the tracker, in order; the dossier's repo after the run; and the files of the
//!   scaffold, byte for byte after the declared `lf`. stderr is each side's own words: both empty or both not.
//! - **What differs, and why** (each declared here and nowhere else):
//!   - the Project's field «Progetto», the prototype's last step of `project new`, is #454's: its calls and writes
//!     are a suffix of the prototype's, which the product does not make, and it warns instead. Where the prototype's
//!     mirror succeeded (`project-new-repo`) its stderr is empty and the product's is that warning;
//!   - the scaffold has two additions of #452, the moviola's folder and its rule: one more file written and listed,
//!     and one more line in `git status`;
//!   - the paths of the scaffold are compared with `/`: Python's `pathlib` writes `\` on Windows;
//!   - a body on stdin is compared as the product sent it: the recording's `lf` undoes the prototype's text mode on
//!     Windows, and the product writes the body's bytes, CRLF included.
//! - **The calls**: the reads too, in order, for the cases of [`CALLS`]; for every case, where the prototype is, by the
//!   cross test. The walk of the cursors before a read of the list is the product's own (#572), checked apart.
//! - **The activity log** (#445/V5): `project new` records its line with the id as given, before its own checks;
//!   `project set` and `setup` record none.
//! - **V20**: the repo is created private, and no other visibility can be asked for; the control with `--public` fails.
//! - **The first push**, which no case can record (it would reach GitHub): on a folder that is no repo, with a global
//!   configuration that sends the GitHub URL to a bare repo beside it.
//!
//! Against a copy of the prototype, named by `REGIA_HOME` (`git archive` of the dossier's `tools/`, `policies/`,
//! `config/tracker.json` and `templates/`), locally:
//!
//!     REGIA_HOME=<copy> cargo test -p regia-conformance --test project_writes -- --ignored

use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{
    Case, CliRun, FIXTURE_NOW, Recorded, fixed_now_driver, fixture_for_python, fixture_for_rust, json_diff, lf, log_var, place_args,
    prepare_case_state, prepare_dossier, prepare_state_dir, regia_binary, run_case_on_dossier, translate_python, unplace_json, unplace_text,
    writes_log, writes_of_run, written_files,
};
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::{env_var, prototype_env_var};
use serde_json::{Value, json};

/// The cases recorded before #452, by #441: through the facade then, natively now.
const RECORDED_BEFORE: [&str; 4] = ["setup-labels", "project-new-repo", "project-set-levers", "project-set-dry-text"];

/// The cases whose recording has the prototype's mirror succeed: its stderr is empty.
const MIRRORED: [&str; 1] = ["project-new-repo"];

/// The id the recorder gives the Project's field «Progetto».
const PROJECT_FIELD: &str = "FIELD_PROJECT_FIXTURE";

/// The rule of the moviola, the scaffold's last file, under the dossier.
const MOVIOLA_RULE: &str = ".regia/moviola/.gitignore";

/// The calls to `gh` of some cases, reads included, by kind as the fake logs them: what the prototype made, up to
/// its mirror. The cross test checks every case against the prototype's own.
const CALLS: [(&str, &[&str]); 8] = [
    ("project-new-bad-id", &[]),
    ("project-new-repo-no-folder", &["issues 0"]),
    ("project-new-scaffold", &["issues 0", "label create", "issue create", "issue 10", "node ISSUE_10", "mutation updateProjectV2ItemFieldValue"]),
    (
        "project-new-repo-same-origin-text",
        &["issues 0", "repo create", "repo edit", "label create", "issue create", "issue 10", "node ISSUE_10", "mutation updateProjectV2ItemFieldValue"],
    ),
    ("project-set-nothing", &[]),
    ("project-set-crlf", &["issues 0", "issue edit", "issue view", "issue comment"]),
    ("project-set-same-text", &["issues 0"]),
    ("setup-labels-json", &["label create", "label create", "label create", "label create", "label create"]),
];

fn cases() -> Vec<Case> {
    let mut cases = Case::of_family("project-writes");
    cases.extend(RECORDED_BEFORE.iter().map(|name| Case::named(name)));
    cases
}

/// The real binary on `case`, natively, in Italian, the prototype's language.
fn native(label: &str, case: &Case) -> CliRun {
    run_case_on_dossier(label, case, |_| vec![(env_var("LANG"), "it".into())])
}

/// The writes the fake `gh` saw in a run, in order, each body as it was sent, with the placeholder in place of the
/// dossier's folder.
fn raw_writes(run: &CliRun) -> Vec<Value> {
    let text = fs::read_to_string(writes_log(&run.state)).unwrap_or_default();
    let writes = text.lines().map(|line| serde_json::from_str::<Value>(line).expect("a JSON line of the writes log"));
    writes.map(|write| run.dossier.as_ref().map_or_else(|| write.clone(), |dossier| unplace_json(&write, dossier))).collect()
}

/// Whether a write is the mirror's: the field «Progetto» created or given an option, or an item given its value.
fn is_mirror_write(write: &Value) -> bool {
    match write["mutation"].as_str() {
        Some("createProjectV2Field" | "updateProjectV2Field") => true,
        Some("updateProjectV2ItemFieldValue") => write["variables"]["f"] == PROJECT_FIELD,
        _ => false,
    }
}

/// The prototype's writes up to its mirror, and an error if a write after the mirror's first is not the mirror's.
fn without_mirror(writes: &[Value]) -> Result<Vec<Value>, String> {
    let at = writes.iter().position(is_mirror_write).unwrap_or(writes.len());
    match writes[at..].iter().find(|write| !is_mirror_write(write)) {
        Some(other) => Err(format!("a write after the mirror is not the mirror's: {other}")),
        None => Ok(writes[..at].to_vec()),
    }
}

/// A path of the scaffold with `/` for its separators.
fn slashes(text: &str) -> String {
    text.replace('\\', "/")
}

/// The product's JSON of `project new` with the scaffold's paths in `/`, and its last file, the moviola's rule, taken
/// out when the prototype's list does not have it.
fn comparable_scaffold(value: &mut Value) {
    if let Some(Value::Array(paths)) = value.get_mut("scaffold") {
        for path in paths.iter_mut() {
            *path = json!(slashes(path.as_str().unwrap_or_default()));
        }
    }
}

/// Every way the native run differs from what the prototype did on `case`, with the differences of the module's doc
/// taken out, each where it is declared.
fn differences(case: &Case, run: &CliRun) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    let added = run.exit == Some(0) && case.command_name() == "project new" && run.dossier.is_some();
    let raw = lf(&String::from_utf8_lossy(&run.stdout));
    let stdout = run.dossier.as_ref().map_or_else(|| raw.clone(), |dossier| unplace_text(&raw, dossier));
    match case.recorded() {
        Recorded::Json(python) => match (serde_json::from_str::<Value>(&raw), translate_python(&case.command_name(), &python)) {
            (Ok(rust), Ok(mut python)) => {
                let mut rust = run.dossier.as_ref().map_or_else(|| rust.clone(), |dossier| unplace_json(&rust, dossier));
                comparable_scaffold(&mut rust);
                comparable_scaffold(&mut python);
                if added {
                    let last = rust["scaffold"].as_array_mut().and_then(Vec::pop);
                    if !last.as_ref().and_then(Value::as_str).is_some_and(|last| last.ends_with(MOVIOLA_RULE)) {
                        out.push(format!("stdout: the scaffold's last file is not the moviola's rule: {last:?}"));
                    }
                }
                out.extend(json_diff(&rust, &python).into_iter().map(|line| format!("stdout {line}")));
            }
            (Err(error), _) => out.push(format!("stdout: rust printed no JSON ({error}): {stdout}")),
            (_, Err(error)) => out.push(format!("stdout: the table does not cover the prototype's output: {error}")),
        },
        Recorded::Text(python) => {
            let created = |text: &str| -> Vec<String> {
                text.lines().map(|line| if line.starts_with("  creato ") { slashes(line) } else { line.to_owned() }).collect()
            };
            let mut rust = created(&stdout);
            if added {
                let at = rust.iter().position(|line| line.starts_with("  creato ") && line.ends_with(MOVIOLA_RULE));
                match at {
                    Some(at) => drop(rust.remove(at)),
                    None => out.push("stdout: no line for the moviola's rule".to_owned()),
                }
            }
            if rust != created(&python) {
                out.push(format!("stdout: rust {rust:?}, python {:?}", created(&python)));
            }
        }
    }
    let python_stderr = !case.recorded_stderr().trim().is_empty();
    let rust_stderr = !run.stderr.trim().is_empty();
    let expected_stderr = python_stderr || MIRRORED.contains(&case.name.as_str());
    if rust_stderr != expected_stderr {
        out.push(format!("stderr: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
    }
    match without_mirror(&case.writes()) {
        Ok(python) => out.extend(json_diff(&Value::Array(raw_writes(run)), &Value::Array(python)).into_iter().map(|line| format!("writes {line}"))),
        Err(error) => out.push(error),
    }
    if let (Some(python), Some(mut rust)) = (case.dossier_after(), run.dossier_after.clone()) {
        if added && let Some(Value::Array(status)) = rust.get_mut("status") {
            status.retain(|line| line.as_str() != Some(&format!("?? {MOVIOLA_RULE}")));
        }
        out.extend(json_diff(&rust, &python).into_iter().map(|line| format!("dossier {line}")));
    }
    if let (Some(python), Some(dossier)) = (case.written(), run.dossier.as_ref()) {
        let rust = written_files(dossier, &case.spec.written);
        for path in &case.spec.written {
            let (rust, python) = (&rust[path], &python[path]);
            if path == MOVIOLA_RULE {
                if !python.is_null() || !rust.as_str().is_some_and(|text| text.ends_with("snapshot/*.html\nsnapshot/*.svg\n")) {
                    out.push(format!("{path}: the prototype has {python}, the product {rust}"));
                }
            } else if rust.as_str().map(lf) != python.as_str().map(lf) {
                out.push(format!("{path}: rust {rust}, python {python}"));
            }
        }
    }
    if let Some((_, calls)) = CALLS.iter().find(|(name, _)| *name == case.name) {
        let (walk, rust) = walk_and_reads(&gh_calls(&run.state, "fake-gh/calls.log"));
        if rust != *calls {
            out.push(format!("gh calls: rust {rust:?}, expected {calls:?}"));
        }
        let pages = calls.iter().filter(|call| call.starts_with("issues ")).count();
        if walk.len() != pages {
            out.push(format!("gh calls: a walk of {} calls for {pages} pages of the list", walk.len()));
        }
    }
    out
}

/// The calls of a run apart: the walk of the cursors, the product's own light read of the list before its pages
/// (#572), which the prototype never made; and the others, the prototype's.
fn walk_and_reads(calls: &[String]) -> (Vec<String>, Vec<String>) {
    calls.iter().cloned().partition(|call| call.contains("-walk "))
}

/// The calls the fake `gh` answered in a run, reads included, by kind.
fn gh_calls(state: &Path, file: &str) -> Vec<String> {
    fs::read_to_string(state.join(file)).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// Acceptance 1: each case of the three commands, natively, is what the prototype did, with the declared
/// differences alone.
#[test]
fn every_case_natively_gives_the_prototype_s_outcome() {
    let cases = cases();
    assert!(cases.len() >= 28, "the cases are missing: {}", cases.len());
    for case in &cases {
        let run = native(&format!("native-{}", case.name), case);
        let diff = differences(case, &run);
        assert!(diff.is_empty(), "case {}:\n{}", case.name, diff.join("\n"));
    }
}

/// The controls: one byte of a model, or two writes swapped in the recording, is a difference; and a write after
/// the mirror's first that is not the mirror's is an error, never a part of the suffix.
#[test]
fn a_byte_of_the_scaffold_or_a_write_out_of_order_fails_the_comparison() {
    let case = Case::named("project-new-scaffold");
    let run = native("native-control-scaffold", &case);
    assert!(differences(&case, &run).is_empty(), "the run itself compares equal");
    let model = run.dossier.clone().expect("the case's dossier").join(".regia").join("mappa.md");
    let text = fs::read_to_string(&model).unwrap();
    fs::write(&model, text.replacen('(', "[", 1)).unwrap();
    let diff = differences(&case, &run);
    println!("the control's differences:
{}", diff.join("
"));
    assert!(diff.iter().any(|line| line.contains("mappa.md")), "one byte must show: {diff:?}");
    let mut swapped = case.writes();
    swapped.swap(0, 1);
    let shown = json_diff(&Value::Array(raw_writes(&run)), &Value::Array(without_mirror(&swapped).unwrap()));
    assert!(!shown.is_empty(), "a swap must show");
    let mirror = json!({"mutation": "updateProjectV2Field", "variables": {}, "arguments": ""});
    assert!(without_mirror(&[mirror, case.writes()[0].clone()]).is_err(), "nothing but the mirror after it");
}

/// V20: every repo is created private, with no other visibility; the control with `--public` fails.
#[test]
fn a_dossier_s_repo_is_only_ever_private() {
    fn private_only(writes: &[Value]) -> Result<usize, String> {
        let creates: Vec<&Value> = writes.iter().filter(|write| write["argv"][0] == "repo" && write["argv"][1] == "create").collect();
        for write in &creates {
            let argv: Vec<&str> = write["argv"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
            let other = argv.iter().any(|arg| matches!(*arg, "--public" | "--internal") || arg.starts_with("--visibility"));
            if !argv.contains(&"--private") || other {
                return Err(format!("not private: {argv:?}"));
            }
        }
        Ok(creates.len())
    }
    let mut created = 0;
    for name in ["project-new-repo", "project-new-repo-same-origin-text"] {
        let run = native(&format!("native-private-{name}"), &Case::named(name));
        assert_eq!(run.exit, Some(0), "{name}: {}", run.stderr);
        created += private_only(&raw_writes(&run)).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
    assert_eq!(created, 2);
    let mut public = Case::named("project-new-repo").writes();
    public[0]["argv"].as_array_mut().unwrap().push(json!("--public"));
    assert!(private_only(&public).is_err(), "a public repo must fail the check");
}

/// #445/V5: `project new` records its line with the id as given, even when it then refuses the id; `project set`
/// and `setup` record none, as in the prototype.
#[test]
fn only_project_new_records_its_line_in_the_sessions_log() {
    let dir = prepare_state_dir("native-project-activity");
    for name in ["project-new-no-folder", "project-new-bad-id", "project-set-same-text", "setup-labels-json"] {
        let case = Case::named(name);
        let responses = if case.responses_path().is_file() { case.responses_path() } else { Case::named("project-new-exists").responses_path() };
        let (launch, _) = fixture_for_rust(regia_exec::harness::launch(&regia_binary(), &case.spec.args, &dir), &responses, &dir);
        let output = launch.env("CLAUDE_CODE_SESSION_ID", "claude-1").run().expect("run regia");
        assert_eq!(output.status.code(), Some(i32::from(case.spec.exit)), "{name}: {}", String::from_utf8_lossy(&output.stderr));
    }
    let mut lines: Vec<Value> = Vec::new();
    for entry in fs::read_dir(dir.join("dichiarazioni")).expect("the log's folder") {
        let text = fs::read_to_string(entry.expect("an entry").path()).expect("a log file");
        lines.extend(text.lines().map(|line| serde_json::from_str::<Value>(line).expect("a JSON line")));
    }
    let mut summary: Vec<Value> = lines.iter().map(|event| json!([event["comando"], event["progetto"], event.get("task")])).collect();
    summary.sort_by_key(Value::to_string);
    assert_eq!(summary, [json!(["progetto new", "Not Valid", null]), json!(["progetto new", "zeta", null])]);
}

/// A state folder whose global git configuration, the home's, gives commits an identity and sends `url` to `bare`.
fn git_home(state: &Path, url: &str, bare: &Path) {
    let bare = bare.display().to_string().replace('\\', "/");
    let config = format!(
        "[user]\n\tname = Fixture\n\temail = fixture@example.invalid\n[commit]\n\tgpgsign = false\n[url \"{bare}\"]\n\tinsteadOf = {url}\n"
    );
    fs::write(state.join(".gitconfig"), config).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let pin = Pin::resolve(Program::Git).expect("git");
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let output = regia_exec::harness::program(&pin, &args, dir).run().expect("git runs");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// `project new --repo` on a folder that is no repo: `main`, `origin`, one commit of the scaffold alone, pushed with
/// its upstream, as the prototype's own test proves on its side (`test_dossier.py`); and when the push fails, the
/// commit stays local and the project is made all the same.
#[test]
fn a_folder_made_a_repo_gets_the_scaffold_pushed_with_its_upstream() {
    let url = "https://github.com/example/fp-dossier.git";
    for (label, reachable) in [("pushed", true), ("unreachable", false)] {
        let case = Case::named("project-new-scaffold");
        let state = prepare_case_state(&format!("native-first-push-{label}"), &case);
        let folder = state.join("fresh-dossier");
        fs::create_dir_all(&folder).unwrap();
        let bare = state.join("fp-dossier.git");
        if reachable {
            git(&state, &["init", "-q", "--bare", "-b", "main", &bare.display().to_string()]);
        }
        git_home(&state, url, &bare);
        let args: Vec<String> = ["project", "new", "--id", "delta", "--title", "pushed", "--folder", &folder.display().to_string(), "--objective", "O.",
            "--definition-of-done", "D.", "--repo", "fp-dossier", "--json"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect();
        let (launch, _) = fixture_for_rust(regia_exec::harness::launch(&regia_binary(), &args, &state), &case.responses_path(), &state);
        let output = launch.env(&env_var("LANG"), "it").run().expect("run regia");
        assert_eq!(output.status.code(), Some(0), "{label}: {}", String::from_utf8_lossy(&output.stderr));
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!((&json["git"], &json["repo"]), (&json!("init"), &json!("example/fp-dossier")), "{label}");
        let first = json["first_push"].as_str().unwrap();
        assert_eq!(git(&folder, &["symbolic-ref", "--short", "HEAD"]), "main");
        assert_eq!(git(&folder, &["remote", "get-url", "origin"]), url);
        let committed = git(&folder, &["ls-files"]);
        assert_eq!(committed.lines().count(), 5, "the scaffold alone, the moviola's rule with it: {committed}");
        assert_eq!(git(&folder, &["status", "--porcelain"]), "", "{label}");
        if reachable {
            let sha = first.strip_prefix("pushato ").unwrap_or_else(|| panic!("{first}"));
            assert!(!sha.is_empty() && sha.chars().all(|c| c.is_ascii_hexdigit()), "{first}");
            assert_eq!(git(&folder, &["rev-parse", "HEAD"]), git(&bare, &["rev-parse", "main"]));
            assert_eq!(git(&folder, &["rev-parse", "--abbrev-ref", "@{u}"]), "origin/main");
            assert_eq!(git(&folder, &["log", "-1", "--format=%s"]), "delta: avvio del progetto");
        } else {
            assert!(first.starts_with("commit locale fatto, git push non riuscito: "), "{first}");
        }
    }
}

/// The copy of the prototype in `REGIA_HOME`.
fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype", prototype_env_var("HOME")));
    assert!(home.join("tools").join("regia.py").is_file(), "no tools/regia.py in {}", home.display());
    home
}

/// `regia.py` of the copy at the fixtures' instant, as `record` starts it, on the case's state, responses and dossier,
/// with its session; the fake `gh` logs its calls in `python-calls.log`.
fn prototype_run(home: &Path, case: &Case, dir: &Path) -> (Option<i32>, String) {
    let argv = case.spec.python.clone().expect("a case of the facade");
    let dossier = case.spec.dossier.as_ref().map(|spec| prepare_dossier(dir, spec));
    let argv = dossier.as_ref().map_or_else(|| argv.clone(), |dossier| place_args(&argv, dossier));
    let responses = dossier.as_ref().map_or_else(|| case.responses_path(), |dossier| regia_conformance::placed_responses(&case.responses_path(), dir, dossier));
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut full: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    let script = home.join("tools").join("regia.py").display().to_string();
    full.extend([fixed_now_driver().display().to_string(), script, FIXTURE_NOW.to_owned()]);
    full.extend(argv);
    let launch = regia_exec::harness::program(&interpreter, &full, home).limits(PROTOTYPE_LIMITS);
    let mut launch = fixture_for_python(launch, &responses, dir).env(&log_var(), dir.join("python-calls.log"));
    for (name, value) in case.spec.session_env() {
        launch = launch.env(&name, value);
    }
    let output = launch.run().expect("run the prototype");
    (output.status.code(), String::from_utf8_lossy(&output.stderr).into_owned())
}

/// The calls of the mirror, which the prototype makes last: the read of the Project's fields, then its writes and
/// the reads before them.
fn is_mirror_call(call: &str) -> bool {
    call == "node PROJECT_FIXTURE" || call.starts_with("mutation updateProjectV2Field") || call.starts_with("mutation createProjectV2Field")
        || call.starts_with("issue ") || call.starts_with("node ISSUE_") || call == "mutation updateProjectV2ItemFieldValue"
}

/// The calls: the prototype, run on each case, calls `gh` as the product does, reads included, in the same order, up
/// to its mirror, which starts with the read of the Project's fields; and writes the same up to it.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_calls_gh_as_the_product_does_on_each_case() {
    let home = prototype();
    let mut report = Vec::new();
    for case in cases() {
        let dir = prepare_case_state(&format!("python-calls-{}", case.name), &case);
        let (exit, stderr) = prototype_run(&home, &case, &dir);
        assert_eq!(exit, Some(i32::from(case.spec.exit)), "{}: {stderr}", case.name);
        let rust = native(&format!("native-calls-{}", case.name), &case);
        let (_, rust_calls) = walk_and_reads(&gh_calls(&rust.state, "fake-gh/calls.log"));
        let python_calls = gh_calls(&dir, "python-calls.log");
        assert!(python_calls.starts_with(&rust_calls), "{}: the calls\nrust   {rust_calls:?}\npython {python_calls:?}", case.name);
        let rest = &python_calls[rust_calls.len()..];
        assert!(rest.first().is_none_or(|first| first == "node PROJECT_FIXTURE"), "{}: the mirror starts with the fields: {rest:?}", case.name);
        assert!(rest.iter().all(|call| is_mirror_call(call)), "{}: only the mirror after: {rest:?}", case.name);
        let python_writes = without_mirror(&writes_of_run(&dir)).unwrap_or_else(|error| panic!("{}: {error}", case.name));
        let python_writes: Vec<Value> = match case.spec.dossier.as_ref() {
            Some(_) => python_writes.iter().map(|write| unplace_json(write, &regia_conformance::dossier_dir(&dir))).collect(),
            None => python_writes,
        };
        let rust_writes = raw_writes(&rust);
        assert_eq!(json_diff(&Value::Array(rust_writes), &Value::Array(python_writes)), Vec::<String>::new(), "{}: the writes", case.name);
        report.push(format!("{}: exit {:?}, {} calls of the product, {} of the mirror after them", case.name, exit, rust_calls.len(), rest.len()));
    }
    println!("{}", report.join("\n"));
}
