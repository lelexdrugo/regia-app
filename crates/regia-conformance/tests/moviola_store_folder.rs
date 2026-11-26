//! The moviola store's folder (#491, from #573/V2 of the dossier) against `CartellaDelloStore` of
//! `tools/tests/test_lettori_config_locale.py` of the dossier: `MOVIOLA_ATTIVITA` × `REGIA_RUNTIME` × `LOCALAPPDATA`
//! on Windows and `XDG_STATE_HOME` elsewhere, the refusal of the temporary folder from every rule, and a rule that
//! creates nothing. The product's rule is three pieces, tried here together: `regia_core::moviola::folder_rule` (the
//! variable, else the state folder's subfolder), `regia_identity::state_dir` (the state folder) and `regia_store`'s
//! refusal of the temporary folder (`is_within`, `temporary_folder`), where that rule lives.
//!
//! A case needs the process's variables, which tests share, so each runs this test binary again as a child with them
//! set, as `runtime_folder.rs` does. The expected folders are written by hand, as the prototype's are. The platform
//! is the machine's: the prototype's Windows cases run on Windows, the others elsewhere. One difference is pinned, the
//! state folder's (P4 of `crates/regia-identity/PROTOTYPE_CASES.md`): on Windows without `LOCALAPPDATA` the product
//! names no folder, where the prototype falls back to the profile's.

use std::env;
use std::path::{Path, PathBuf};

use regia_core::moviola;
use regia_exec::harness;
use regia_identity::{PROTOTYPE_NAME, prototype_env_var, state_dir};
use regia_store::{is_within, temporary_folder};

const CHILD_CASE: &str = "REGIA_MOVIOLA_STORE_FOLDER_TEST_CHILD";

/// The store's subfolder under the state folder, in the prototype's word.
const SUBFOLDER: &str = "attivita";

/// The child half: names the store's folder from the environment and prints what came of it. A plain run of the
/// ignored tests does not set the marker and does nothing.
#[test]
#[ignore = "run by the tests below as a child process"]
fn child_names_the_store_folder() {
    if env::var_os(CHILD_CASE).is_none() {
        return;
    }
    let variable = env::var_os(moviola::folder_variable());
    let outcome = match moviola::folder_rule(variable.as_deref(), state_dir().as_deref()) {
        None => "none".to_owned(),
        Some(folder) if is_within(&folder, &temporary_folder()) => "refused".to_owned(),
        Some(folder) => format!("folder {}", folder.display()),
    };
    println!("STORE_FOLDER_OUTCOME={outcome}");
}

/// A variable of a case: set to a value (the empty one included) or removed.
type Var<'a> = (&'a str, Option<&'a str>);

/// The outcome in a child where the rule's variables are as given; every one not given is removed.
fn name_with(vars: &[Var<'_>]) -> String {
    let args: Vec<String> =
        ["--exact", "child_names_the_store_folder", "--ignored", "--nocapture", "--test-threads=1"].map(str::to_owned).to_vec();
    let mut launch = harness::launch(&env::current_exe().unwrap(), &args, Path::new(env!("CARGO_MANIFEST_DIR"))).env(CHILD_CASE, "1");
    let variable = moviola::folder_variable().to_owned();
    let runtime = prototype_env_var("RUNTIME");
    for name in [variable.as_str(), runtime.as_str(), "LOCALAPPDATA", "XDG_STATE_HOME"] {
        launch = match vars.iter().find(|(given, _)| *given == name).and_then(|(_, value)| *value) {
            Some(value) => launch.env(name, value),
            None => launch.without(name),
        };
    }
    for (name, value) in vars.iter().filter(|(name, _)| *name == "HOME") {
        launch = match value {
            Some(value) => launch.env(name, value),
            None => launch.without(name),
        };
    }
    let output = launch.run().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    stdout.lines().find_map(|line| line.split_once("STORE_FOLDER_OUTCOME=")).expect("the child prints its outcome").1.to_owned()
}

/// The folders of the cases, in the build's own folder: none of them is made.
struct Folders {
    m: PathBuf,
    r: PathBuf,
    l: PathBuf,
    x: PathBuf,
    home: PathBuf,
}

fn folders(case: &str) -> Folders {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("moviola-store-folder").join(case);
    Folders { m: base.join("m"), r: base.join("r"), l: base.join("l"), x: base.join("x"), home: base.join("home") }
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

fn folder(path: PathBuf) -> String {
    format!("folder {}", path.display())
}

#[test]
fn on_windows_the_variable_then_the_runtime_folder_then_localappdata() {
    if !cfg!(windows) {
        return;
    }
    let f = folders("windows");
    let (m, r, l, x) = (text(&f.m), text(&f.r), text(&f.l), text(&f.x));
    let (m, r, l, x) = (m.as_str(), r.as_str(), l.as_str(), x.as_str());
    let variable = moviola::folder_variable();
    let runtime = prototype_env_var("RUNTIME");
    let runtime = runtime.as_str();
    let cases: Vec<(&str, Vec<Var<'_>>, String)> = vec![
        ("the variable wins over everything", vec![(variable, Some(m)), (runtime, Some(r)), ("LOCALAPPDATA", Some(l))], folder(f.m.clone())),
        ("the variable alone", vec![(variable, Some(m)), ("LOCALAPPDATA", Some(l))], folder(f.m.clone())),
        ("REGIA_RUNTIME without the variable: under REGIA_RUNTIME", vec![(runtime, Some(r)), ("LOCALAPPDATA", Some(l))], folder(f.r.join(SUBFOLDER))),
        ("the variable empty counts as unset", vec![(variable, Some("")), (runtime, Some(r)), ("LOCALAPPDATA", Some(l))], folder(f.r.join(SUBFOLDER))),
        ("only LOCALAPPDATA", vec![("LOCALAPPDATA", Some(l))], folder(f.l.join(PROTOTYPE_NAME).join(SUBFOLDER))),
        ("REGIA_RUNTIME empty counts as unset", vec![(runtime, Some("")), ("LOCALAPPDATA", Some(l))], folder(f.l.join(PROTOTYPE_NAME).join(SUBFOLDER))),
        // The prototype falls back to `<profile>/AppData/Local/regia`: the difference P4, pinned.
        ("without LOCALAPPDATA: no folder", vec![], "none".to_owned()),
        ("LOCALAPPDATA empty: no folder", vec![("LOCALAPPDATA", Some(""))], "none".to_owned()),
        ("XDG_STATE_HOME does not count on Windows", vec![("LOCALAPPDATA", Some(l)), ("XDG_STATE_HOME", Some(x))], folder(f.l.join(PROTOTYPE_NAME).join(SUBFOLDER))),
    ];
    for (note, vars, expected) in cases {
        assert_eq!(name_with(&vars), expected, "{note}");
    }
}

#[test]
fn off_windows_the_variable_then_the_runtime_folder_then_xdg_state_home() {
    if cfg!(windows) {
        return;
    }
    let f = folders("posix");
    let (m, r, l, x, home) = (text(&f.m), text(&f.r), text(&f.l), text(&f.x), text(&f.home));
    let (m, r, l, x, home) = (m.as_str(), r.as_str(), l.as_str(), x.as_str(), home.as_str());
    let variable = moviola::folder_variable();
    let runtime = prototype_env_var("RUNTIME");
    let runtime = runtime.as_str();
    let under_home = folder(f.home.join(".local").join("state").join(PROTOTYPE_NAME).join(SUBFOLDER));
    let cases: Vec<(&str, Vec<Var<'_>>, String)> = vec![
        ("the variable wins over everything", vec![(variable, Some(m)), (runtime, Some(r)), ("XDG_STATE_HOME", Some(x)), ("HOME", Some(home))], folder(f.m.clone())),
        ("REGIA_RUNTIME without the variable", vec![(runtime, Some(r)), ("XDG_STATE_HOME", Some(x)), ("HOME", Some(home))], folder(f.r.join(SUBFOLDER))),
        ("only XDG_STATE_HOME", vec![("XDG_STATE_HOME", Some(x)), ("HOME", Some(home))], folder(f.x.join(PROTOTYPE_NAME).join(SUBFOLDER))),
        ("XDG_STATE_HOME empty: ~/.local/state", vec![("XDG_STATE_HOME", Some("")), ("HOME", Some(home))], under_home.clone()),
        ("no variable: ~/.local/state", vec![("HOME", Some(home))], under_home.clone()),
        ("LOCALAPPDATA does not count off Windows", vec![("LOCALAPPDATA", Some(l)), ("HOME", Some(home))], under_home),
    ];
    for (note, vars, expected) in cases {
        assert_eq!(name_with(&vars), expected, "{note}");
    }
}

/// The refusal of the temporary folder holds for the variable and for the store that comes from the state folder,
/// and naming the folder creates nothing.
#[test]
fn every_rule_refuses_the_temporary_folder_and_none_creates_the_folder() {
    let temporary = env::temp_dir().join(format!("regia-moviola-store-{}", std::process::id()));
    let (a, r, platform) = (text(&temporary.join("a")), text(&temporary.join("r")), text(&temporary.join("p")));
    let variable = moviola::folder_variable();
    let runtime = prototype_env_var("RUNTIME");
    let runtime = runtime.as_str();
    let own = if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_STATE_HOME" };
    for vars in [vec![(variable, Some(a.as_str()))], vec![(runtime, Some(r.as_str()))], vec![(own, Some(platform.as_str()))]] {
        assert_eq!(name_with(&vars), "refused", "{vars:?}");
    }
    assert!(!temporary.exists());
    let f = folders("not-created");
    assert_eq!(name_with(&[(runtime, Some(text(&f.r).as_str()))]), folder(f.r.join(SUBFOLDER)));
    assert!(!f.r.join(SUBFOLDER).exists() && !f.r.exists());
}
