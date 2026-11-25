//! The tracker's configuration as the prototype reads it (`config_tracker` of `tools/runtime.py`, one copy of
//! the rule), against the cases of `test_config_locale.py` and `test_lettori_config_locale.py` of the dossier
//! that read it: the local `tracker.json` of the state folder wins over the dossier's `config/tracker.json`,
//! and a local file that is there and invalid is an error that names it, never a fallback on the owner's
//! tracker. Where the state folder is (`REGIA_RUNTIME`, `LOCALAPPDATA`, XDG) is `regia-identity`'s rule.

use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{app_config_dir, fixture_for_rust, fixtures_dir, own_folders, prepare_state_dir, read_json, regia_binary, workspace_root};
use regia_core::{TrackerAnchor, TrackerBinding, TrackerConfigV1, V1Error, anchor_file};
use regia_identity::{config_dir_var, env_var, prototype_env_var};
use serde_json::{Value, json};

const KEYS: [&str; 7] = ["owner", "repo", "project_number", "project_id", "project_url", "status_field_id", "status_options"];
const OPTIONS: [&str; 7] = ["Inbox", "Proposta", "Approvato", "In corso", "Decisione", "Fatto", "Scartato"];

struct Folders {
    state: PathBuf,
    dossier: PathBuf,
    /// The app's configuration folder, where the anchor is: empty until a test writes one.
    config: PathBuf,
}

/// A state folder with nothing in it, and a dossier whose `config/tracker.json` is the synthetic one.
fn folders(name: &str) -> Folders {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("config").join(name);
    let _ = fs::remove_dir_all(&root);
    let (state, dossier) = (root.join("state"), root.join("dossier"));
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(dossier.join("config")).unwrap();
    fs::copy(fixtures_dir().join("tracker.json"), dossier.join("config").join("tracker.json")).unwrap();
    Folders { config: app_config_dir(&state), state, dossier }
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
    TrackerConfigV1::load(Some(&folders.state), Some(&folders.dossier), Some(&folders.config))
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
    assert_eq!(TrackerConfigV1::load(Some(&absent), Some(&other), None).unwrap().project_id, "PVT_home");
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
    let output = own_folders(regia_exec::harness::launch(&regia_binary(), &args, &folders.state), &folders.state)
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

/// The tracker of the repo's configuration, as an anchor names it.
fn repo_anchor() -> TrackerAnchor {
    let config = repo_config();
    let text = |key: &str| config[key].as_str().unwrap().to_owned();
    TrackerAnchor { owner: text("owner"), repo: text("repo"), project_number: config["project_number"].as_u64().unwrap(), project_id: text("project_id") }
}

/// A tracker as the error shows it.
fn shown(anchor: &TrackerAnchor) -> String {
    format!("{}/{} #{} {}", anchor.owner, anchor.repo, anchor.project_number, anchor.project_id)
}

fn write_anchor(config: &Path, anchor: &TrackerAnchor) -> PathBuf {
    fs::create_dir_all(config).unwrap();
    let path = anchor_file(config);
    fs::write(&path, anchor.file_text()).unwrap();
    path
}

/// The ways an anchor names another tracker than the repo's: another owner, repo, Project number or Project id.
fn other_anchors() -> [(&'static str, TrackerAnchor); 4] {
    let same = repo_anchor();
    [
        ("owner", TrackerAnchor { owner: format!("{}-other", same.owner), ..same.clone() }),
        ("repo", TrackerAnchor { repo: format!("{}-other", same.repo), ..same.clone() }),
        ("project_number", TrackerAnchor { project_number: same.project_number + 1, ..same.clone() }),
        ("project_id", TrackerAnchor { project_id: format!("{}_other", same.project_id), ..same.clone() }),
    ]
}

/// #456 round 2, Acceptance 1 (threat model T08, V17): the anchor of the app's configuration folder against the
/// `tracker.json` read, the state folder's or the dossier's. The same tracker loads, owner and repo in any case;
/// another owner, repo, Project number or Project id is an error that names both files and shows both trackers;
/// with no anchor the configuration loads as before.
#[test]
fn the_anchor_stops_a_tracker_json_that_names_another_tracker() {
    for local in [false, true] {
        let folders = folders(if local { "anchor-local" } else { "anchor-dossier" });
        let file = if local { write_local(&folders, &local_valid(json!({})).to_string()) } else { folders.dossier.join("config").join("tracker.json") };
        let id = repo_config()["project_id"].as_str().unwrap().to_owned();
        assert_eq!(load(&folders).unwrap().project_id, id, "no anchor: as before");
        let same = repo_anchor();
        write_anchor(&folders.config, &TrackerAnchor { owner: same.owner.to_uppercase(), ..same.clone() });
        assert_eq!(load(&folders).unwrap().project_id, id, "the same tracker");
        for (what, other) in other_anchors() {
            let anchor = write_anchor(&folders.config, &other);
            let error = load(&folders).expect_err(what);
            let V1Error::AnchorMismatch { file: read, anchor: named, found, anchored } = &error else { panic!("{what}: {error}") };
            assert_eq!((read, named), (&file, &anchor), "{what}");
            assert_eq!((found.to_string(), anchored.to_string()), (shown(&same), shown(&other)), "{what}");
            println!("{what}, local {local}: {error}");
        }
    }
}

/// The command stops on it before any call to `gh`, with exit 1 and nothing on stdout, and says it in the user's
/// language: the file read, the anchor, both trackers and what to do. 1, as for any invalid configuration
/// (`conformita.md` §7.1): 3 would read as a lock held.
#[test]
fn the_command_exits_1_on_another_tracker_and_shows_both() {
    let state = prepare_state_dir("config-anchor-command");
    let (_, other) = other_anchors().into_iter().next().unwrap();
    let anchor = write_anchor(&app_config_dir(&state), &other);
    let args = ["projects".to_owned(), "--json".to_owned()];
    let languages = [("en", "names another tracker than the one this app is anchored to"), ("it", "indica un tracker diverso da quello a cui è ancorata l'app")];
    for (language, sentence) in languages {
        let output = own_folders(regia_exec::harness::launch(&regia_binary(), &args, &state), &state)
            .without(&prototype_env_var("HOME"))
            .without(&env_var("ENGINE"))
            .env(&env_var("LANG"), language)
            .env(&regia_exec::Program::Gh.env_name(), state.join("no-such-gh"))
            .run()
            .expect("run regia");
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("{language}, exit {:?}:\n{stderr}", output.status.code());
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(output.stdout.is_empty() && !stderr.contains("panicked"), "{stderr}");
        let parts = [
            sentence.to_owned(),
            state.join("tracker.json").display().to_string(),
            anchor.display().to_string(),
            shown(&other),
            shown(&repo_anchor()),
            "init derive".to_owned(),
        ];
        for part in parts {
            assert!(stderr.contains(part.as_str()), "{language}: no {part} in {stderr}");
        }
    }
    let _ = fs::remove_dir_all(&state);
}

/// #456 round 2, Acceptance 3: no test reads or writes the machine's configuration folder. At run time, the probe
/// started as the product is, through `fixture_for_rust` and through `own_folders`, has the variable naming the
/// folder of its own case.
#[test]
fn every_run_has_a_configuration_folder_of_its_own() {
    let probe = Path::new(env!("CARGO_BIN_EXE_probe"));
    let state = prepare_state_dir("config-dir-guard");
    let (fixture, _) = fixture_for_rust(regia_exec::harness::launch(probe, &[], &state), &state.join("responses.json"), &state);
    let direct = own_folders(regia_exec::harness::launch(probe, &[], &state), &state);
    for launch in [fixture, direct] {
        let output = launch.run().expect("the probe runs");
        let report: Value = serde_json::from_slice(&output.stdout).expect("the probe's JSON");
        assert_eq!(report["env"][config_dir_var()], json!(app_config_dir(&state).display().to_string()), "{report}");
    }
    let _ = fs::remove_dir_all(&state);
}

/// Whether a run of the product at byte `at` of `text` gets its folders: in its statement or in the next one.
fn moves_its_folders(text: &str, at: usize) -> bool {
    let start = text[..at].rfind([';', '{', '}']).map_or(0, |found| found + 1);
    let mut end = at;
    for _ in 0..2 {
        end = text[end..].find(';').map_or(text.len(), |found| end + found + 1);
    }
    let window = &text[start..end];
    window.contains("fixture_for_rust(") || window.contains("own_folders(")
}

/// The other half, on the sources of this crate: every run of the product (the CLI, the precheck, the app) gets its
/// folders from `fixture_for_rust` or `own_folders`, and nothing asks `regia-identity` for the machine's
/// configuration folder. The needles are built, so this file is scanned like the others.
#[test]
fn no_run_of_the_product_skips_its_own_folders() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let crate_dir = workspace_root().join("crates").join("regia-conformance");
    let mut sources = Vec::new();
    walk(&crate_dir.join("src"), &mut sources);
    walk(&crate_dir.join("tests"), &mut sources);
    let launches = ["regia_binary", "precheck_binary", "app"].map(|binary| format!("launch(&{binary}"));
    let machine = format!("{}()", "config_dir");
    let mut found = Vec::new();
    let mut runs = 0;
    for path in &sources {
        let text = fs::read_to_string(path).unwrap();
        for launch in &launches {
            for (at, _) in text.match_indices(launch.as_str()) {
                // `live-check` runs the product as this machine runs it, on the real tracker with the real configuration
                // and its anchor: a check of the machine that `cargo test` never starts, not a test.
                if text[..at].ends_with("live_rust_") {
                    continue;
                }
                runs += 1;
                if !moves_its_folders(&text, at) {
                    found.push(format!("{}: a run of the product without its folders", path.display()));
                }
            }
        }
        for (at, _) in text.match_indices(machine.as_str()) {
            if !text[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                found.push(format!("{}: asks for the machine's configuration folder", path.display()));
            }
        }
    }
    assert!(runs >= 10, "the scan sees the runs of the product: {runs}");
    assert!(found.is_empty(), "{}", found.join("\n"));
    // The control: a run that does not move its folders is seen.
    let bare = format!("let output = regia_exec::harness::{}(), &args, &state).run();\nlet next = 1;", launches[0]);
    assert!(!moves_its_folders(&bare, bare.find(launches[0].as_str()).unwrap()));
}
