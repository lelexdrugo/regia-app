//! Local only: the scheduled operations of the Rust resident read by the prototype on one copy of the state (#469,
//! the test of conformance of the slice). CI cannot read the dossier, so these tests are ignored there:
//!
//!     REGIA_HOME=<dossier> cargo test -p regia-conformance --test controller_schedule_cross -- --ignored --test-threads=1
//!
//! - **The cursor `operazioni`.** The Rust resident opens today's chat of the dailies; the prototype's own
//!   `_operazioni`, on the same folder at the same day, reads the cursor the Rust wrote as its own: done today, and no
//!   second chat.
//! - **The state file.** Without a surface for the chat the Rust resident leaves it to open; the prototype's brief
//!   reads today's chat to open from the Rust's `controller-stato.json`.
//!
//! Every state folder is a fresh one under `target/`, never the live one. Both sides run on the system's clock and the
//! machine's local day, as the prototype counts it. Python runs with `-B`, so nothing is written in the dossier's
//! `tools/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use regia_agents::{Dry, LaunchError, LaunchRequest, LaunchSurface, ProbeConsent, Receipt, SurfaceKind, WakeOutcome};
use regia_controller::engine::{Folders, Liveness, Precheck, PrecheckRun};
use regia_controller::resident::{Config, Console, Exit, HostPorts, Notice, Resident, Sleep};
use regia_controller::schedule::{ChatSurface, Operations, Source, StoreSchedule};
use regia_exec::harness;
use regia_exec::{Pin, Program};
use regia_identity::prototype_env_var;
use regia_store::{Store, SystemClock};
use serde_json::{Value, json};

fn dossier() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to the dossier: these tests run its controller and its brief", prototype_env_var("HOME")));
    assert!(home.join("tools").join("controller.py").is_file(), "no tools/controller.py in {}", home.display());
    home
}

fn python() -> Pin {
    Pin::resolve(Program::Python).unwrap_or_else(|error| panic!("{error}"))
}

fn oracle(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle").join(name)
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

/// A fresh, empty state folder under `target/`.
fn folder(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("controller-schedule-cross").join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    fs::create_dir_all(&dir).expect("state folder");
    dir
}

/// The dailies at midnight every day: due at any hour of the machine's day.
fn dailies() -> Value {
    json!({"operazioni": [{"nome": "giornalieri", "ora": "00:00", "giorni": "tutti", "azione": "chat", "prompt": "regia: giornalieri"}]})
}

struct Idle;

impl Precheck for Idle {
    fn run(&self, _projects: &[String], _timeout_s: f64) -> PrecheckRun {
        PrecheckRun::from_output(Some(3), &json!({"v": 1, "outcome": "idle", "exit": 3, "motivo": "nessun-evento", "payload": {"progetti": []}}).to_string())
    }
}

struct NoFolders;

impl Folders for NoFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        Ok(BTreeMap::new())
    }
}

struct Nobody;

impl Liveness for Nobody {
    fn alive(&self, _pid: u32) -> bool {
        false
    }
}

struct Quiet;

impl Console for Quiet {
    fn notice(&self, _notice: &Notice) {}
}

struct NoSleep;

impl Sleep for NoSleep {
    fn sleep(&self, _secs: f64) {}
}

/// A chat that opens on Orca, as far as its receipt says.
struct Opens;

impl LaunchSurface for Opens {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Orca
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        let mut receipt = Dry::new().launch(request)?;
        receipt.surface = SurfaceKind::Orca;
        receipt.session = Some("rust-chat".to_owned());
        Ok(receipt)
    }

    fn wake(&self) -> WakeOutcome {
        WakeOutcome::fake()
    }
}

/// One round of the Rust resident on `dir`, on the system's clock, with the dailies and the chat on `chat`.
fn rust_round(dir: &Path, chat: ChatSurface) {
    let store = Store::at(dir).unwrap();
    let mut config = Config::new(dir);
    config.rounds = 1;
    let ports = HostPorts {
        store: store.clone(),
        clock: Arc::new(SystemClock),
        sleep: Arc::new(NoSleep),
        precheck: Arc::new(Idle),
        folders: Arc::new(NoFolders),
        liveness: Arc::new(Nobody),
        surface: Arc::new(Dry::new()),
        ids: Arc::new(|| "id-cross".to_owned()),
        consent: ProbeConsent::Refused,
        token_expiry: None,
        console: Arc::new(Quiet),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let resident = Resident::new(config, ports);
    let state = Arc::new(StoreSchedule::new(store, resident.holder().clone()));
    let operations = Operations::new(Source::Document(dailies()), state, chat).with_home(Some(dir.to_path_buf())).with_closings(dir.join("giornalieri"));
    assert_eq!(resident.with_before_precheck(Box::new(operations)).run(), Exit::Done);
}

fn python_json(argv: &[String], dir: &Path) -> Value {
    let output = harness::program(&python(), argv, dir).run().expect("run the prototype");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the prototype failed: {}\n{stdout}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_str(stdout.lines().last().unwrap_or("null")).unwrap_or_else(|error| panic!("not JSON ({error}): {stdout}"))
}

/// The test of conformance of the slice: the cursor `operazioni` the Rust wrote, read by the prototype on the copy.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_prototype_reads_the_cursor_of_the_operations_the_rust_wrote() {
    let (dossier, dir) = (dossier(), folder("cursor"));
    rust_round(&dir, ChatSurface::Open(Arc::new(Opens)));
    let config = dir.join("operations.json");
    fs::write(&config, dailies().to_string()).unwrap();
    let argv = [
        words(&["-X", "utf8", "-I", "-B"]),
        vec![text(&oracle("schedule_oracle.py"))],
        words(&["--dossier"]),
        vec![text(&dossier)],
        words(&["--dir"]),
        vec![text(&dir)],
        words(&["--config"]),
        vec![text(&config)],
        words(&["--closings"]),
        vec![text(&dir.join("giornalieri"))],
    ]
    .concat();
    let python = python_json(&argv, &dir);
    assert_eq!(python["error"], Value::Null, "{python}");
    assert_eq!(python["voices"], json!([["giornalieri", "gia-eseguita-oggi"]]), "the Rust's chat of today is the prototype's: {python}");
    assert_eq!(python["calls"], json!([]), "no second chat");
    let entry = &python["ops"]["giornalieri"];
    assert_eq!((entry["esito"].clone(), entry["ultima_lanciata"]["origine"].clone(), entry["ultima_lanciata"]["sessione"].clone()), (json!("lanciata"), json!("orario"), json!("rust-chat")));
    assert_eq!(entry["tentativi"], 1);
}

/// The chat to open of today, in the Rust's state file, read by the prototype's brief.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_prototype_s_brief_shows_the_chat_to_open_the_rust_wrote() {
    let (dossier, dir) = (dossier(), folder("to-open"));
    rust_round(&dir, ChatSurface::None);
    let argv = [words(&["-X", "utf8", "-I", "-B"]), vec![text(&oracle("brief_oracle.py"))], words(&["--dossier"]), vec![text(&dossier)], words(&["--dir"]), vec![text(&dir)]].concat();
    let brief = python_json(&argv, &dir);
    let to_open = &brief["warnings"]["da_aprire"];
    assert_eq!(to_open.as_array().map(Vec::len), Some(1), "{brief}");
    assert_eq!((to_open[0]["nome"].clone(), to_open[0]["esito"].clone(), to_open[0]["prompt"].clone()), (json!("giornalieri"), json!("da-aprire"), json!("regia: giornalieri")));
    assert!(brief["text"].as_str().unwrap_or_default().contains("giornalieri"), "the brief's text names it: {}", brief["text"]);
}
