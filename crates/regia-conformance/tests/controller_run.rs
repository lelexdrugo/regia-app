//! `regia controller run` as a command (#469, F27), the binary against a fixture: the state of the trace `no-work` of
//! #436, with the live controller's lock held as a running Python controller holds it, the tracker's answers on the
//! fake `gh`, and the dailies due at the fixture's instant in a synthetic regia's home.
//!
//! - Acceptance 2: `--once --shadow` runs on a fresh copy and launches nothing. Every file of the live state folder is
//!   the same after the run, byte for byte; the programs a launch would start are the `probe` in its beating mode, and
//!   none beats; the copy has the shadow's line, its chat on `Dry`, and the live lock released.
//! - The prototype's command line: a second controller exits 3 (`test_secondo_controller_da_riga_di_comando_esce_3`),
//!   and a wrong argument exits 1, not clap's 2 (`test_argomenti_errati_escono_1_non_2`, with its fourth case, the
//!   parser's own errors).
//! - The text: a round's line in each language, from the catalogs.

use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use regia_conformance::{StateRow, bin_dir, fixture_for_rust, fixtures_root, prepare_state_dir, read_json, regia_binary, write_state};
use regia_exec::Program;
use regia_exec::harness::launch;
use regia_identity::{env_var, prototype_env_var};
use regia_store::{LockStatus, ProjectId, Store};
use serde_json::{Map, Value, json};

const DOSSIER: &str = "{dossier}";
const LIVE_HOLDER: &str = "controller-4242-live0000";

fn case_file(name: &str) -> Value {
    read_json(&fixtures_root().join("controller").join("cases").join("no-work").join(name))
}

/// `{dossier}` replaced by `dossier` in every string of `value`.
fn placed(value: &Value, dossier: &str) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace(DOSSIER, dossier)),
        Value::Array(items) => Value::Array(items.iter().map(|item| placed(item, dossier)).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, item)| (key.clone(), placed(item, dossier))).collect()),
        other => other.clone(),
    }
}

/// The answers of the fake `gh`, as `tests/controller.rs` makes them: the comments keyed by issue become that issue's.
fn fake_gh_responses(responses: &Value) -> Value {
    let mut out = responses.clone();
    let mut issues = Map::new();
    for (number, data) in responses["comments"].as_object().into_iter().flatten() {
        let mut issue = data["repository"]["issue"].clone();
        if let Some(comments) = issue.get_mut("comments").and_then(Value::as_object_mut) {
            comments.entry("pageInfo").or_insert(json!({"hasNextPage": false, "endCursor": null}));
        }
        if let Some(nodes) = issue.pointer_mut("/comments/nodes").and_then(Value::as_array_mut) {
            for node in nodes {
                let url = format!("https://tracker.invalid/issues/{number}#issuecomment-{}", node["databaseId"]);
                node.as_object_mut().expect("a comment").entry("url").or_insert(json!(url));
            }
        }
        issues.insert(number.clone(), issue);
    }
    out.as_object_mut().expect("the answers").remove("comments");
    out["issues"] = Value::Object(issues);
    out
}

/// A run's folders: the harness's (home, settings, fake `gh`), the live state folder inside it, its shadow copy beside
/// it, the regia's home and the projects' dossier.
struct Fixture {
    run: PathBuf,
    live: PathBuf,
    copy: PathBuf,
    home: PathBuf,
    responses: PathBuf,
    beat: PathBuf,
    probes: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Fixture {
        let run = prepare_state_dir(&format!("controller-run-{label}"));
        let (live, copy, home, dossier) = (run.join("live"), run.join("live-shadow"), run.join("home"), run.join("dossier"));
        for folder in [&live, &home.join("config"), &dossier.join("alpha"), &dossier.join("beta")] {
            fs::create_dir_all(folder).unwrap();
        }
        let slashed = dossier.display().to_string().replace('\\', "/");
        let state = placed(&case_file("state.json"), &slashed);
        let rows: Vec<StateRow> = state["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| StateRow { kind: row["tipo"].as_str().unwrap().to_owned(), key: row["chiave"].as_str().unwrap().to_owned(), doc: row["doc"].clone() })
            .collect();
        write_state(&live, &rows);
        for (name, content) in state["files"].as_object().unwrap() {
            let text = content.as_str().map_or_else(|| serde_json::to_string_pretty(content).unwrap(), str::to_owned);
            fs::write(live.join(name), text).unwrap();
        }
        // A trace of the controller before: what the shadow reads back, and where it adds its own line.
        fs::write(live.join("controller.log"), "{\"v\":1,\"giro\":7,\"decisione\":\"attesa-idle\"}\n").unwrap();
        let config = json!({"operazioni": [{"nome": "giornalieri", "ora": "00:00", "giorni": "tutti", "azione": "chat", "prompt": "regia: giornalieri"}]});
        fs::write(home.join("config").join("controller.json"), config.to_string()).unwrap();
        let responses = run.join("responses.json");
        fs::write(&responses, fake_gh_responses(&placed(&case_file("responses.json"), &slashed)).to_string()).unwrap();
        let probes = run.join("probes");
        fs::create_dir_all(&probes).unwrap();
        let probe = bin_dir().join(format!("probe{}", std::env::consts::EXE_SUFFIX));
        for program in ["claude", "codex", "orca"] {
            fs::copy(&probe, probes.join(format!("{program}{}", std::env::consts::EXE_SUFFIX))).unwrap();
        }
        Fixture { beat: run.join("a-launch-ran.txt"), run, live, copy, home, responses, probes }
    }

    /// The command against the fixture: the live state folder, the regia's home, and every program a launch would
    /// start a beating probe, by its variable and first on the path.
    fn command(&self, args: &[&str], language: &str) -> Ran {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (launch, _) = fixture_for_rust(launch(&regia_binary(), &args, &self.run), &self.responses, &self.run);
        let path = std::env::join_paths(std::iter::once(self.probes.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
        let mut launch = launch
            .env(&prototype_env_var("RUNTIME"), &self.live)
            .env(&prototype_env_var("HOME"), &self.home)
            .env(&env_var("LANG"), language)
            .env("PATH", path)
            .env("REGIA_PROBE_MODE", "beat")
            .env("REGIA_PROBE_BEAT", &self.beat);
        for program in [Program::Claude, Program::Codex, Program::Orca] {
            let name = program.name().to_owned();
            launch = launch.env(&program.env_name(), self.probes.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)));
        }
        let output = launch.run().expect("the command runs");
        Ran { code: output.status.code(), stdout: output.stdout, stderr: output.stderr }
    }
}

/// What a run of the command left: its exit code and what it printed.
struct Ran {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Every file under `folder`, by its path from there, with its bytes.
fn files(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, folder: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path.strip_prefix(root).unwrap().display().to_string().replace('\\', "/");
                out.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(folder, folder, &mut out);
    out
}

fn digest(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hasher = DefaultHasher::new();
    files.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn stdout(run: &Ran) -> String {
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn stderr(run: &Ran) -> String {
    String::from_utf8_lossy(&run.stderr).into_owned()
}

fn controller_lock(folder: &Path) -> LockStatus {
    Store::at(folder).unwrap().lock_status(&ProjectId::new("controller").unwrap()).unwrap()
}

/// Acceptance 2.
#[test]
fn a_shadow_round_runs_on_a_fresh_copy_and_launches_nothing() {
    let fixture = Fixture::new("shadow");
    let before = files(&fixture.live);
    assert!(before.contains_key(regia_store::database_name().as_str()) && before.contains_key("tracker.json"));
    let run = fixture.command(&["controller", "run", "--once", "--shadow"], "en");
    assert_eq!(run.code, Some(0), "{}\n{}", stdout(&run), stderr(&run));
    let after = files(&fixture.live);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>(), "no file added to the live folder, none removed");
    assert!(after == before, "the live folder is the same, byte for byte");
    println!("live folder: {} files, digest {} before and {} after", before.len(), digest(&before), digest(&after));
    assert!(!fixture.beat.exists(), "no program a launch starts was started");
    match controller_lock(&fixture.live) {
        LockStatus::Held(view) => assert_eq!(view.holder(), Some(LIVE_HOLDER), "the live controller keeps its lock"),
        other => panic!("{other:?}"),
    }

    let out = stdout(&run);
    assert!(out.contains(&fixture.copy.display().to_string()), "{out}");
    assert!(out.contains(LIVE_HOLDER), "the copy's lock of the live controller is released, and the run says so: {out}");
    assert!(out.contains("round 1 2026-01-10T09:00:00Z: precheck exit 3 idle/"), "{out}");
    assert!(out.contains("[operations: giornalieri lanciata]"), "{out}");
    assert!(fixture.copy.join(regia_controller::shadow::MARKER).is_file());
    let log: Vec<Value> = fs::read_to_string(fixture.copy.join("controller.log")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(log.len(), 2, "the trace before, then the shadow's line beside it");
    let shadow = &log[1];
    assert_eq!((shadow["giro"].clone(), shadow["decisione"].clone(), shadow["lanciati"].clone()), (json!(1), json!("attesa-idle"), json!([])));
    assert_eq!((shadow["operazioni"][0]["azione"].clone(), shadow["operazioni"][0]["adapter"].clone()), (json!("lanciata"), json!("finto")), "the chat on Dry");
    let status: Value = read_json(&fixture.copy.join("controller-stato.json"));
    assert_eq!(status["operazioni"]["giornalieri"]["esito"], "lanciata");
    assert_eq!(controller_lock(&fixture.copy), LockStatus::Free, "the shadow's own lock is released at its exit");
    let cursor = Store::at(&fixture.copy).unwrap().cursor(&ProjectId::new("controller").unwrap(), &regia_store::CursorName::new("operazioni").unwrap()).unwrap();
    assert_eq!(cursor["ops"]["giornalieri"]["esito"], "lanciata", "the cursor of the operations is the copy's");

    // A second run replaces the copy, which carries its marker; a folder without it is left alone, and the run stops.
    let again = fixture.command(&["controller", "run", "--once", "--shadow"], "en");
    assert_eq!(again.code, Some(0), "{}", stderr(&again));
    assert_eq!(fs::read_to_string(fixture.copy.join("controller.log")).unwrap().lines().count(), 2, "a fresh copy, not the old one");
    fs::remove_file(fixture.copy.join(regia_controller::shadow::MARKER)).unwrap();
    let refused = fixture.command(&["controller", "run", "--once", "--shadow"], "en");
    assert_eq!(refused.code, Some(1), "{}", stdout(&refused));
    assert!(stderr(&refused).contains("is not a shadow copy"), "{}", stderr(&refused));
    assert!(fixture.copy.join("controller.log").is_file(), "the folder is left as it is");
    assert!(files(&fixture.live) == before);
}

/// `test_secondo_controller_da_riga_di_comando_esce_3`: the live controller's lock, on the live folder, refuses the run.
#[test]
fn a_second_controller_from_the_command_line_exits_3() {
    let fixture = Fixture::new("second");
    let run = fixture.command(&["controller", "run", "--once", "--launcher", "fake"], "en");
    assert_eq!(run.code, Some(3), "{}", stderr(&run));
    assert!(stderr(&run).contains(&format!("controller: another controller is active ({LIVE_HOLDER})")), "{}", stderr(&run));
    assert!(!fixture.live.join("controller-stato.json").exists() && fs::read_to_string(fixture.live.join("controller.log")).unwrap().lines().count() == 1, "no round ran");
    let italian = fixture.command(&["controller", "run", "--once", "--launcher", "fake"], "it");
    assert_eq!(italian.code, Some(3));
    assert!(stderr(&italian).contains("un altro controller è attivo"), "{}", stderr(&italian));
    assert!(!fixture.beat.exists());
}

/// `test_argomenti_errati_escono_1_non_2`, with its fourth case: the parser's own errors exit 1 too, and the fixed clock
/// of more than one round.
#[test]
fn wrong_arguments_exit_1_not_2() {
    let fixture = Fixture::new("arguments");
    for args in [
        &["--interval", "x"][..],
        &["--launcher", "boh"],
        &["--interval=-3"],
        &["--interval", "-3"],
        &["--max-steps", "0"],
        &["--rounds", "-1"],
        &["--nope"],
        &["--rounds", "2"],
    ] {
        let argv: Vec<&str> = ["controller", "run"].into_iter().chain(args.iter().copied()).collect();
        let run = fixture.command(&argv, "en");
        assert_eq!(run.code, Some(1), "{args:?}: {}", stderr(&run));
    }
    let fixed = fixture.command(&["controller", "run", "--rounds", "2"], "en");
    assert!(stderr(&fixed).contains("fixes the clock"), "{}", stderr(&fixed));
    assert_eq!(fixture.command(&["controller"], "en").code, Some(1), "no subcommand");
    let help = fixture.command(&["controller", "run", "--help"], "en");
    assert_eq!(help.code, Some(0));
    assert!(stdout(&help).contains("--shadow") && stdout(&help).contains("--launcher"), "{}", stdout(&help));
    assert!(files(&fixture.live).contains_key("tracker.json"));
    assert!(!fixture.live.join("controller-stato.json").exists(), "no round ran");
}

/// A round's line in Italian, with the prototype's words: the catalog's `giro`, `operazioni` and `precheck exit`.
#[test]
fn a_round_s_line_in_italian() {
    let fixture = Fixture::new("italian");
    let run = fixture.command(&["controller", "run", "--once", "--shadow"], "it");
    assert_eq!(run.code, Some(0), "{}", stderr(&run));
    let out = stdout(&run);
    assert!(out.contains("giro in ombra") && out.contains("giro 1 2026-01-10T09:00:00Z: precheck exit 3 idle/"), "{out}");
    assert!(out.contains("[operazioni: giornalieri lanciata]"), "{out}");
}
