//! The tracker's configuration as the prototype reads it (`config_tracker` of `tools/runtime.py`, one copy of
//! the rule), against the cases of `test_config_locale.py` and `test_lettori_config_locale.py` of the dossier
//! that read it: the local `tracker.json` of the state folder wins over the dossier's `config/tracker.json`,
//! and a local file that is there and invalid is an error that names it, never a fallback on the owner's
//! tracker. Where the state folder is (`REGIA_RUNTIME`, `LOCALAPPDATA`, XDG) is `regia-identity`'s rule.

use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{fixtures_dir, read_json, regia_binary, workspace_root};
use regia_core::{TrackerBinding, TrackerConfigV1, V1Error};
use regia_identity::prototype_env_var;
use serde_json::{Value, json};

const KEYS: [&str; 7] = ["owner", "repo", "project_number", "project_id", "project_url", "status_field_id", "status_options"];
const OPTIONS: [&str; 7] = ["Inbox", "Proposta", "Approvato", "In corso", "Decisione", "Fatto", "Scartato"];

struct Folders {
    state: PathBuf,
    dossier: PathBuf,
}

/// A state folder with nothing in it, and a dossier whose `config/tracker.json` is the synthetic one.
fn folders(name: &str) -> Folders {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("config").join(name);
    let _ = fs::remove_dir_all(&root);
    let (state, dossier) = (root.join("state"), root.join("dossier"));
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(dossier.join("config")).unwrap();
    fs::copy(fixtures_dir().join("tracker.json"), dossier.join("config").join("tracker.json")).unwrap();
    Folders { state, dossier }
}

fn repo_config() -> Value {
    read_json(&fixtures_dir().join("tracker.json"))
}

/// The repo's configuration with `changes`, as a valid local file.
fn local_valid(changes: Value) -> Value {
    let mut config = repo_config();
    config.as_object_mut().unwrap().remove("_note");
    for (key, value) in changes.as_object().unwrap() {
        config[key] = value.clone();
    }
    config
}

fn write_local(folders: &Folders, text: &str) -> PathBuf {
    let path = folders.state.join("tracker.json");
    fs::write(&path, text).unwrap();
    path
}

fn load(folders: &Folders) -> Result<TrackerConfigV1, V1Error> {
    TrackerConfigV1::load(Some(&folders.state), Some(&folders.dossier))
}

/// An error that names the local file and says each of `parts`.
fn assert_error(folders: &Folders, parts: &[&str]) {
    let error = load(folders).expect_err("an invalid local file is an error").to_string();
    assert!(error.contains(&folders.state.join("tracker.json").display().to_string()), "{error}");
    for part in parts {
        assert!(error.contains(part), "{error} does not say {part}");
    }
}

#[test]
fn without_a_local_file_the_config_is_the_dossier_s() {
    let folders = folders("from-repo");
    assert_eq!(load(&folders).unwrap().project_id, repo_config()["project_id"].as_str().unwrap());
}

#[test]
fn a_valid_local_file_wins_over_the_dossier_s_and_names_the_tracker() {
    let folders = folders("local-wins");
    let local = local_valid(json!({"project_id": "PVT_other", "repo": "other-repo", "owner": "who"}));
    write_local(&folders, &local.to_string());
    let config = load(&folders).unwrap();
    assert_eq!((config.project_id.as_str(), config.repo.as_str(), config.owner.as_str()), ("PVT_other", "other-repo", "who"));
    assert_ne!(config.project_id, repo_config()["project_id"].as_str().unwrap());
    // The statuses follow it too: every option of the local file binds.
    assert!(TrackerBinding::new(&config).is_ok());
}

#[test]
fn a_broken_local_file_is_an_error_that_names_it_and_never_a_fallback() {
    let folders = folders("broken");
    write_local(&folders, "{\"owner\": ");
    assert_error(&folders, &[]);
    write_local(&folders, "");
    assert!(load(&folders).is_err(), "an empty local file is not the dossier's config");
    write_local(&folders, "[1, 2]");
    assert_error(&folders, &["not a JSON object"]);
}

#[test]
fn a_local_file_without_a_key_is_an_error_that_names_the_key() {
    for key in KEYS {
        let folders = folders(&format!("missing-{key}"));
        let mut local = local_valid(json!({}));
        local.as_object_mut().unwrap().remove(key);
        write_local(&folders, &local.to_string());
        assert_error(&folders, &["missing the keys", key]);
    }
}

#[test]
fn a_local_file_without_an_option_of_a_status_is_an_error_that_names_it() {
    for option in OPTIONS {
        let folders = folders(&format!("option-{}", option.replace(' ', "-")));
        let mut local = local_valid(json!({}));
        local["status_options"].as_object_mut().unwrap().remove(option);
        write_local(&folders, &local.to_string());
        assert_error(&folders, &["status_options without", option]);
    }
    let folders = folders("options-not-an-object");
    write_local(&folders, &local_valid(json!({"status_options": ["Inbox"]})).to_string());
    assert_error(&folders, &["status_options"]);
}

/// `test_file_locale_rotto_con_repo_illeggibile_non_e_un_ripiego` and `…_senza_file_locale_e_repo_illeggibile`:
/// with a broken local file the dossier's is never read, and without one the dossier's own error stands.
#[test]
fn the_dossier_s_file_is_read_only_without_a_local_one() {
    let folders = folders("repo-unreadable");
    let repo = folders.dossier.join("config").join("tracker.json");
    fs::write(&repo, "{ not json").unwrap();
    write_local(&folders, "{");
    let error = load(&folders).unwrap_err().to_string();
    assert!(error.contains(&folders.state.join("tracker.json").display().to_string()) && !error.contains(&repo.display().to_string()), "{error}");
    fs::remove_file(folders.state.join("tracker.json")).unwrap();
    assert!(load(&folders).unwrap_err().to_string().contains(&repo.display().to_string()));
}

/// `test_senza_file_locale_la_cartella_di_runtime_non_viene_creata` and `test_regia_home_senza_file_locale`:
/// reading creates nothing, and the dossier named in place of the state folder's file is the one read.
#[test]
fn reading_creates_no_folder_and_the_dossier_named_is_read() {
    let folders = folders("no-folder");
    let absent = folders.state.join("never-created");
    let other = folders.state.join("another-root");
    fs::create_dir_all(other.join("config")).unwrap();
    fs::write(other.join("config").join("tracker.json"), local_valid(json!({"project_id": "PVT_home"})).to_string()).unwrap();
    assert_eq!(TrackerConfigV1::load(Some(&absent), Some(&other)).unwrap().project_id, "PVT_home");
    assert!(!absent.exists());
}

/// `test_il_comando_esce_con_1_e_senza_traceback`: the command itself, `projects`, stops on a broken local file
/// with exit 1 and a line that names it, before any call to `gh`; and the file is looked up in the state
/// folder the variable names (`test_il_file_locale_si_cerca_nella_cartella_di_runtime`).
#[test]
fn the_command_exits_1_on_a_broken_local_file_before_any_call() {
    let folders = folders("command");
    let path = write_local(&folders, "{");
    let args = ["projects".to_owned(), "--json".to_owned()];
    let output = regia_exec::harness::launch(&regia_binary(), &args, &folders.state)
        .env(&prototype_env_var("RUNTIME"), &folders.state)
        .env("HOME", &folders.state)
        .env("USERPROFILE", &folders.state)
        .without(&prototype_env_var("HOME"))
        .env(&regia_exec::Program::Gh.env_name(), folders.state.join("no-such-gh"))
        .run()
        .expect("run regia");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(&path.display().to_string()), "{stderr}");
    assert!(output.stdout.is_empty() && !stderr.contains("panicked"), "{stderr}");
}

/// `test_nessuna_copia_della_regola_fuori_da_runtime`: no source but the v1 reader builds the path of a
/// `tracker.json`; the conformance crate writes the synthetic one for its runs.
#[test]
fn the_rule_has_one_copy() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "target") {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let root = workspace_root();
    let mut sources = Vec::new();
    walk(&root.join("crates"), &mut sources);
    walk(&root.join("app").join("src-tauri"), &mut sources);
    let allowed = [root.join("crates/regia-core/src/v1.rs"), root.join("crates/regia-conformance")];
    for path in sources.iter().filter(|path| !allowed.iter().any(|allowed| path.starts_with(allowed))) {
        let text = fs::read_to_string(path).unwrap();
        assert!(!text.contains("join(\"tracker.json\")"), "{} builds the path of tracker.json", path.display());
    }
}
