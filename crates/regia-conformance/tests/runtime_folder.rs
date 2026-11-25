//! The refusal of the temporary folder, from every rule that names the state folder, against
//! `test_rifiuta_la_cartella_temporanea_da_qualunque_regola` of `tools/tests/test_lettori_config_locale.py`
//! of the dossier. The rule that names the folder is in `regia-identity`; the refusal is in `regia-store`
//! (Storage Sense empties `%TEMP%`), so this is where the two are tried together.
//!
//! The prototype's `MOVIOLA_ATTIVITA` case has no counterpart: that variable is not ported. A case needs
//! the process's variables, which tests share, so each runs this test binary again as a child with them set.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use regia_exec::harness;
use regia_identity::PROTOTYPE_NAME;
use regia_store::Store;

const CHILD_CASE: &str = "REGIA_RUNTIME_FOLDER_TEST_CHILD";

/// The child half: opens the store from the environment and prints what came of it. A plain run of the
/// ignored tests does not set the marker and does nothing.
#[test]
#[ignore = "run by the tests below as a child process"]
fn child_opens_the_store() {
    if env::var_os(CHILD_CASE).is_none() {
        return;
    }
    let outcome = match Store::open() {
        Ok(store) => format!("opened {}", store.folder().display()),
        Err(error) => format!("refused {}", format!("{error:?}").split([' ', '{']).next().unwrap_or("")),
    };
    println!("STORE_OUTCOME={outcome}");
}

/// The outcome of `Store::open()` in a child where the three variables of the rule are as given (`None`
/// removes one). The temporary-folder variables are the child's own, so it and this process agree on them.
fn open_with(runtime: Option<&Path>, localappdata: Option<&Path>, xdg: Option<&Path>) -> String {
    let args: Vec<String> =
        ["--exact", "child_opens_the_store", "--ignored", "--nocapture", "--test-threads=1"].map(str::to_owned).to_vec();
    let mut launch = harness::launch(&env::current_exe().unwrap(), &args, Path::new(env!("CARGO_MANIFEST_DIR"))).env(CHILD_CASE, "1");
    for (name, value) in [("REGIA_RUNTIME", runtime), ("LOCALAPPDATA", localappdata), ("XDG_STATE_HOME", xdg)] {
        launch = match value {
            Some(value) => launch.env(name, value),
            None => launch.without(name),
        };
    }
    let output = launch.run().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    // With `--nocapture` libtest prints its own `test <name> ... ` before the child's line.
    stdout.lines().find_map(|line| line.split_once("STORE_OUTCOME=")).expect("the child prints its outcome").1.to_owned()
}

fn inside_temporary(name: &str) -> PathBuf {
    env::temp_dir().join(format!("regia-runtime-folder-{}", std::process::id())).join(name)
}

/// `REGIA_RUNTIME` under the temporary folder: refused, and nothing is created.
#[test]
fn regia_runtime_under_the_temporary_folder_is_refused() {
    let folder = inside_temporary("r");
    assert_eq!(open_with(Some(&folder), None, None), "refused TemporaryFolder");
    assert!(!folder.exists());
}

/// `REGIA_RUNTIME` empty and the platform's own variable under the temporary folder: refused. On Windows
/// that is `LOCALAPPDATA` (`<it>\regia`), elsewhere `XDG_STATE_HOME`.
#[test]
fn the_platform_default_under_the_temporary_folder_is_refused() {
    let folder = inside_temporary("l");
    let outcome = if cfg!(windows) {
        open_with(Some(Path::new("")), Some(&folder), None)
    } else {
        open_with(Some(Path::new("")), None, Some(&folder))
    };
    assert_eq!(outcome, "refused TemporaryFolder");
    assert!(!folder.join(PROTOTYPE_NAME).exists());
}

/// The control: a folder outside the temporary one is opened, and created.
#[test]
fn a_folder_outside_the_temporary_one_is_opened() {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("runtime-folder").join("outside");
    let _ = fs::remove_dir_all(&folder);
    assert!(open_with(Some(&folder), None, None).starts_with("opened "));
    assert!(folder.is_dir());
}
