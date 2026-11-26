//! `path` (#448; the prototype's `percorso` of #530, its oracle `tools/tests/test_percorso.py`): the real binary,
//! natively, in a synthetic dossier, answers what the prototype's tests ask of it: a file and a folder that are there,
//! one that is not, the data's placeholder, a place of the machine, a link, read only; outside a dossier, exit 2 with
//! nothing on stdout. A test that needs the dossier runs the prototype through the facade on the same references and
//! compares the two answers, the machine's paths included.

use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{FAKE_PYTHON_WRONG_CWD, Replay, fixture_for_rust, prepare_state_dir, regia_binary, replay_prototype};
use regia_identity::{env_var, prototype_env_var};
use serde_json::{Value, json};

struct Run {
    exit: Option<i32>,
    stdout: String,
    stderr: String,
}

/// The binary with `args`, from `cwd`, in a state folder of its own; `REGIA_HOME` named when `home` is.
fn run(label: &str, cwd: &Path, args: &[&str], home: Option<&Path>) -> Run {
    let state = prepare_state_dir(label);
    let responses = state.join("no-responses.json");
    fs::write(&responses, "{}").unwrap();
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut launch, _) = fixture_for_rust(regia_exec::harness::launch(&regia_binary(), &args, cwd), &responses, &state);
    if let Some(home) = home {
        launch = launch.env(&prototype_env_var("HOME"), home).env(&env_var("ENGINE"), "python");
    }
    let output = launch.run().expect("regia runs");
    Run {
        exit: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A dossier with `.regia/report/` and `.regia/stato.md`, as the prototype's tests make it.
fn dossier(label: &str) -> PathBuf {
    let dir = regia_conformance::bin_dir().join("conformance-state").join(format!("path-{label}")).join("dossier");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".regia").join("report")).unwrap();
    fs::write(dir.join(".regia").join("stato.md"), "ciao").unwrap();
    dir
}

fn json_of(label: &str, cwd: &Path, reference: &str) -> Value {
    let run = run(label, cwd, &["path", reference, "--json"], None);
    assert_eq!(run.exit, Some(0), "{reference}: {}", run.stderr);
    serde_json::from_str(&run.stdout).unwrap_or_else(|error| panic!("{reference}: {error}: {}", run.stdout))
}

#[test]
fn the_kinds_of_a_reference_as_the_prototype_s_tests_ask() {
    let dossier = dossier("kinds");
    let resolved = |relative: &str| fs::canonicalize(dossier.join(relative)).unwrap();
    let file = json_of("path-file", &dossier, ".regia/stato.md");
    assert_eq!((&file["kind"], &file["exists"], &file["is_folder"], &file["reason"]), (&json!("relative"), &json!(true), &json!(false), &Value::Null));
    assert_eq!(PathBuf::from(file["path"].as_str().unwrap()).file_name(), resolved(".regia/stato.md").file_name());
    let folder = json_of("path-folder", &dossier, ".regia/report");
    assert_eq!((&folder["exists"], &folder["is_folder"]), (&json!(true), &json!(true)));
    assert_eq!(folder["reference"], ".regia/report");
    let absent = json_of("path-absent", &dossier, ".regia/manca.md");
    assert_eq!((&absent["kind"], &absent["exists"], &absent["reason"]), (&json!("relative"), &json!(false), &json!("non presente su questa macchina")));
    let data = json_of("path-data", &dossier, "{dati}/x/y");
    assert_eq!((&data["kind"], &data["path"]), (&json!("data"), &Value::Null));
    assert!(data["reason"].as_str().unwrap().contains("{dati}"));
    let place = json_of("path-place", &dossier, "~/x");
    assert_eq!((&place["kind"], &place["path"], &place["reason"]), (&json!("not_a_path"), &Value::Null, &json!("sede della macchina")));
    let link = json_of("path-link", &dossier, "https://tracker.invalid/y");
    assert_eq!((&link["kind"], &link["exists"], &link["reason"]), (&json!("not_a_path"), &json!(false), &json!("link")));
    let absolute = json_of("path-absolute", &dossier, "/home/user/a.md");
    assert_eq!((&absolute["kind"], &absolute["path"]), (&json!("absolute"), &Value::Null));
    let outside = json_of("path-outside", &dossier, "../x");
    assert_eq!(outside["kind"], "outside");
}

#[test]
fn outside_a_dossier_it_exits_2_and_says_why_and_the_text_is_the_catalog_s() {
    let dossier = dossier("outside");
    let elsewhere = dossier.parent().unwrap().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let refused = run("path-no-dossier", &elsewhere, &["path", ".regia/stato.md"], None);
    assert_eq!((refused.exit, refused.stdout.as_str()), (Some(2), ""));
    assert!(refused.stderr.contains(".regia"), "{}", refused.stderr);
    let text = run("path-text", &dossier, &["path", ".regia/manca.md"], None);
    assert_eq!(text.exit, Some(0));
    let lines: Vec<&str> = text.stdout.lines().collect();
    assert_eq!(lines.len(), 6, "{}", text.stdout);
    assert!(lines[0].ends_with(": .regia/manca.md") && lines[1].ends_with(": relative"), "{}", text.stdout);
    assert!(lines[4].ends_with(": non presente su questa macchina"), "the reason is the prototype's sentence: {}", text.stdout);
    assert!(lines[5].contains("`.regia/manca.md`: "), "how a document cites it, with the catalog's sentence: {}", text.stdout);
    let before: Vec<PathBuf> = walk(&dossier);
    for reference in [".regia/stato.md", "nuovo/file.md", "{dati}/x", "~/x"] {
        run("path-read-only", &dossier, &["path", reference], None);
    }
    assert_eq!(walk(&dossier), before, "read only");
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        }
        out.push(path);
    }
    out.sort();
    out
}

/// The prototype through the facade and the binary natively, on the same references: the same answer, its machine
/// path included. Both resolve against the prototype's own dossier: the facade starts the prototype from there, so
/// through it a `path` with no `--project` reads that folder, not the caller's (a limit of the facade, not of `path`).
#[test]
#[ignore = "needs the dossier (REGIA_HOME) and Python"]
fn the_native_path_answers_as_the_prototype_through_the_facade() {
    let home = std::env::var_os("REGIA_HOME").map(PathBuf::from).expect("REGIA_HOME names the dossier");
    let dossier = home.clone();
    for reference in [".regia/stato.md", ".regia/report", ".regia/manca.md", "{dati}/x", "~/x", "https://tracker.invalid/y", "/home/user/a.md", "../x", "a b", "<nome>/x"] {
        let native = json_of("path-live-native", &dossier, reference);
        let prototype = run("path-live-python", &dossier, &["path", reference, "--json"], Some(&home));
        assert_eq!(prototype.exit, Some(0), "{reference}: {}", prototype.stderr);
        let prototype: Value = serde_json::from_str(&prototype.stdout).unwrap();
        assert_eq!(native, prototype, "{reference}");
    }
}

/// #448/V3, #701: through the facade, `path <reference>` with no `--project` resolves where the caller is, as the native
/// `path` does, and not in the prototype's own dossier, the folder the facade starts it from. The caller is in a
/// dossier of its own (a synthetic one), not `REGIA_HOME`: the prototype answers for the first only if the facade
/// passes the caller's folder on.
#[test]
#[ignore = "needs the dossier (REGIA_HOME) and Python"]
fn through_the_facade_path_resolves_in_the_callers_folder() {
    let home = std::env::var_os("REGIA_HOME").map(PathBuf::from).expect("REGIA_HOME names the dossier");
    let caller = dossier("facade-caller");
    assert_ne!(fs::canonicalize(&caller).unwrap(), fs::canonicalize(&home).unwrap());
    for reference in [".regia/stato.md", ".regia/report", ".regia/manca.md", "nuovo/file.md"] {
        let native = json_of("path-caller-native", &caller, reference);
        let prototype = run("path-caller-python", &caller, &["path", reference, "--json"], Some(&home));
        assert_eq!(prototype.exit, Some(0), "{reference}: {}", prototype.stderr);
        let prototype: Value = serde_json::from_str(&prototype.stdout).unwrap();
        assert_eq!(native, prototype, "{reference}: the facade answered for another folder than the caller's");
        let inside = PathBuf::from(prototype["path"].as_str().expect("a path"));
        assert!(inside.starts_with(fs::canonicalize(&caller).unwrap()) || inside.starts_with(&caller), "{reference}: {}", inside.display());
    }
    // Outside a dossier the facade says so, as the native `path` does: exit 2 and nothing on stdout.
    let elsewhere = caller.parent().unwrap().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let native = run("path-caller-none-native", &elsewhere, &["path", ".regia/stato.md"], None);
    let prototype = run("path-caller-none-python", &elsewhere, &["path", ".regia/stato.md"], Some(&home));
    assert_eq!((prototype.exit, prototype.stdout.as_str()), (native.exit, native.stdout.as_str()), "{}", prototype.stderr);
}

/// The binary through the facade, from `cwd`, with the stand-in interpreter replaying what `replay_of` makes of the
/// stand-in's own dossier (the one the binary names as the prototype's).
fn run_through_the_stand_in(label: &str, cwd: &Path, args: &[&str], replay_of: impl FnOnce(&Path) -> Replay) -> Run {
    let state = prepare_state_dir(label);
    let responses = state.join("no-responses.json");
    fs::write(&responses, "{}").unwrap();
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut launch, _) = fixture_for_rust(regia_exec::harness::launch(&regia_binary(), &args, cwd), &responses, &state);
    for (name, value) in replay_prototype(&state, &replay_of(&state.join("dossier"))) {
        launch = launch.env(&name, value);
    }
    let output = launch.env(&env_var("ENGINE"), "python").run().expect("regia runs");
    Run {
        exit: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// #448/V3, #701, in CI: the prototype is a stand-in that checks the folder it is started in and the argv it is
/// given, the script included. Through the facade, `path <reference>` with no `--project` starts it in the caller's
/// folder with the script in full, so it answers for the caller's dossier as the native `path` does. With `--project`
/// it is started from the dossier's root with the script relative, as every command is, and a stand-in that expects
/// the caller's folder refuses that: the stand-in does notice a wrong folder.
#[test]
fn through_the_facade_path_starts_the_prototype_in_the_callers_folder() {
    let caller = fs::canonicalize(dossier("stand-in-caller")).unwrap();
    let answer = r#"{"riferimento": ".regia/stato.md", "tipo": "relativo", "percorso": "somewhere", "esiste": true, "cartella": false, "motivo": null}"#;
    let replay = |argv: Vec<String>, cwd: &Path| Replay {
        argv: regia_exec::python::FLAGS.iter().map(|flag| (*flag).to_owned()).chain(argv).collect(),
        stdout: format!("{answer}
"),
        stderr: String::new(),
        exit: 0,
        json: false,
        cwd: Some(cwd.display().to_string()),
    };
    let words = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<_>>();

    let no_project = run_through_the_stand_in("stand-in-no-project", &caller, &["path", ".regia/stato.md", "--json"], |dossier| {
        let script = dossier.join("tools").join("regia.py").to_str().unwrap().replace('\\', "/");
        replay([vec![script], words(&["--json", "percorso", ".regia/stato.md"])].concat(), &caller)
    });
    assert_eq!(no_project.exit, Some(0), "{}", no_project.stderr);
    let value: Value = serde_json::from_str(&no_project.stdout).expect("JSON");
    assert_eq!((&value["kind"], &value["exists"], &value["is_folder"]), (&json!("relative"), &json!(true), &json!(false)));

    // The same argv with the caller's folder named by a stand-in that expects the dossier's root: refused.
    let wrong = run_through_the_stand_in("stand-in-wrong-folder", &caller, &["path", ".regia/stato.md", "--json"], |dossier| {
        let script = dossier.join("tools").join("regia.py").to_str().unwrap().replace('\\', "/");
        replay([vec![script], words(&["--json", "percorso", ".regia/stato.md"])].concat(), dossier)
    });
    assert_eq!(wrong.exit, Some(i32::from(FAKE_PYTHON_WRONG_CWD)), "{}", wrong.stderr);

    // With `--project`: from the dossier's root, the script relative.
    let project = run_through_the_stand_in("stand-in-project", &caller, &["path", ".regia/stato.md", "--project", "alpha", "--json"], |dossier| {
        replay(words(&["tools/regia.py", "--json", "percorso", ".regia/stato.md", "--progetto", "alpha"]), dossier)
    });
    assert_eq!(project.exit, Some(0), "{}", project.stderr);
}
