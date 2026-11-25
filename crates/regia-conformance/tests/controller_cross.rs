//! Local only: the Rust resident and the prototype's controller on one copy of the state (#468, Acceptance 2 and 3,
//! the X of the plan). CI cannot read the dossier, so these tests are ignored there:
//!
//!     REGIA_HOME=<dossier> cargo test -p regia-conformance --test controller_cross -- --ignored --test-threads=1
//!
//! - **One controller across the languages.** While the Rust resident holds the lock, the prototype's
//!   `controller.py`, from its command line, exits with 3 and names the Rust holder; while the prototype's controller
//!   holds it, the Rust resident exits with 3 and names the Python holder. Neither touches the other's lock.
//! - **The X.** Four runs of the Rust resident with the budget unknown (a restart each, so the count goes on through
//!   the state file) write the alarm into the state file; the prototype's brief reads it and shows it before what
//!   waits for the owner. After two runs, below the threshold, it shows nothing.
//! - **D2 of #417 on the Python side.** The prototype's own tests of the registry loaded after the lock and saved
//!   only with it (`TestRegistroCaricatoDopoIlLock`, `TestSalvataggioRegistroSenzaLock`) pass on the dossier; the
//!   Rust side is `tests/controller_resident.rs`.
//!
//! Every state folder is a fresh one under `target/`, never the live one, and the prototype never reaches its
//! precheck or the tracker: `controller.py` exits before it, and the drivers in `oracle/` give it the hooks of its
//! own tests. The Rust runs on the system's clock, as the Python does, so each reads the other's lock by the wall
//! clock. Python runs with `-B`, so nothing is written in the dossier's `tools/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use regia_agents::{Dry, ProbeConsent};
use regia_controller::engine::{Folders, Liveness, Precheck, PrecheckRun};
use regia_controller::resident::{Config, Console, Exit, HostPorts, Notice, Resident, Sleep};
use regia_exec::harness;
use regia_exec::{Pin, Program};
use regia_identity::prototype_env_var;
use regia_store::{LockStatus, ProjectId, Store, SystemClock};
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
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("controller-cross").join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    fs::create_dir_all(&dir).expect("state folder");
    dir
}

fn reserved() -> ProjectId {
    ProjectId::new("controller").unwrap()
}

fn holder_of_the_lock(store: &Store) -> Option<String> {
    match store.lock_status(&reserved()).unwrap() {
        LockStatus::Held(view) => view.holder().map(str::to_owned),
        _ => None,
    }
}

// ---------- the Rust resident's ports ----------

/// The precheck's line: idle, or idle on the budget unknown, which the tick counts.
struct Answer {
    unknown_budget: bool,
    calls: AtomicUsize,
}

impl Precheck for Answer {
    fn run(&self, _projects: &[String], _timeout_s: f64) -> PrecheckRun {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let line = if self.unknown_budget {
            json!({"v": 1, "outcome": "idle", "exit": 3, "motivo": "budget", "cursore": [],
                   "payload": {"progetti": [{"id": "p468", "esito": "idle", "motivo": "budget"}],
                               "budget": {"claude_5h_pct": null, "motivo": "sonda: troppe richieste (HTTP 429)"}},
                   "t": "2026-01-10T09:00:00Z"})
        } else {
            json!({"v": 1, "outcome": "idle", "exit": 3, "motivo": "nessun-evento", "cursore": [], "payload": {"progetti": []}, "t": "2026-01-10T09:00:00Z"})
        };
        PrecheckRun::from_output(Some(3), &line.to_string())
    }
}

struct NoFolders;

impl Folders for NoFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        Ok(BTreeMap::new())
    }
}

struct NoProcess;

impl Liveness for NoProcess {
    fn alive(&self, _pid: u32) -> bool {
        false
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<Notice>>);

impl Console for Recorder {
    fn notice(&self, notice: &Notice) {
        self.0.lock().unwrap().push(notice.clone());
    }
}

/// The system's sleep, cut short when a stop is asked.
struct UntilStopped(Arc<AtomicBool>);

impl Sleep for UntilStopped {
    fn sleep(&self, secs: f64) {
        let end = Instant::now() + Duration::from_secs_f64(secs.max(0.0));
        while Instant::now() < end && !self.0.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn resident(dir: &Path, answer: Arc<Answer>, rounds: u64, stop: Arc<AtomicBool>) -> Resident {
    let mut config = Config::new(dir);
    config.rounds = rounds;
    config.options.no_wake = true;
    let ports = HostPorts {
        store: Store::at(dir).expect("the store"),
        clock: Arc::new(SystemClock),
        sleep: Arc::new(UntilStopped(stop.clone())),
        precheck: answer,
        folders: Arc::new(NoFolders),
        liveness: Arc::new(NoProcess),
        surface: Arc::new(Dry::new()),
        ids: Arc::new(|| "id-cross".to_owned()),
        consent: ProbeConsent::Granted,
        console: Arc::new(Recorder::default()),
        stop,
    };
    Resident::new(config, ports)
}

fn answer(unknown_budget: bool) -> Arc<Answer> {
    Arc::new(Answer { unknown_budget, calls: AtomicUsize::new(0) })
}

// ---------- Acceptance 2 ----------

/// The Rust resident holds the lock: the prototype's controller, from its command line, exits with 3 before its
/// precheck, names the Rust holder, and leaves the lock as it was.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_python_controller_exits_3_while_the_rust_resident_holds_the_lock() {
    let (dossier, dir) = (dossier(), folder("rust-holds"));
    let stop = Arc::new(AtomicBool::new(false));
    let mut rust = resident(&dir, answer(false), 0, stop.clone());
    let holder = rust.holder().as_str().to_owned();
    let running = std::thread::spawn(move || rust.run());
    let store = Store::at(&dir).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while holder_of_the_lock(&store).as_deref() != Some(holder.as_str()) {
        assert!(Instant::now() < deadline, "the Rust resident did not take the lock");
        std::thread::sleep(Duration::from_millis(50));
    }

    // Not `-I`: `controller.py` imports its siblings from the script's folder. `-B`: no bytecode in the dossier.
    let argv = [
        words(&["-X", "utf8", "-B"]),
        vec![text(&dossier.join("tools").join("controller.py"))],
        words(&["--una-volta", "--lancio", "finto", "--no-ripresa", "--no-sveglia", "--progetto", "p468", "--config"]),
        vec![text(&dir.join("no-operations.json"))],
    ]
    .concat();
    let output = harness::program(&python(), &argv, &dir).env(&prototype_env_var("RUNTIME"), &dir).run().expect("run controller.py");
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("controller.py exit {:?}\nstderr: {stderr}", output.status.code());
    assert_eq!(output.status.code(), Some(3), "controller.py must stop: {stderr}");
    assert!(stderr.contains("un altro controller è attivo") && stderr.contains(&holder), "it saw this lock, not another: {stderr}");
    assert_eq!(holder_of_the_lock(&store).as_deref(), Some(holder.as_str()), "its release did not touch the Rust lock");

    stop.store(true, Ordering::SeqCst);
    assert_eq!(running.join().expect("the resident's thread"), Exit::Stopped);
    assert_eq!(store.lock_status(&reserved()).unwrap(), LockStatus::Free, "the Rust resident released its lock");
}

/// The prototype's controller holds the lock, past its first tick: the Rust resident exits with 3 before its
/// precheck, names the Python holder, and leaves the lock; the prototype then stops as Ctrl+C stops it, and lets it
/// go.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_rust_resident_exits_3_while_the_python_controller_holds_the_lock() {
    let (dossier, dir) = (dossier(), folder("python-holds"));
    let argv = [
        words(&["-X", "utf8", "-I", "-B"]),
        vec![text(&oracle("controller_oracle.py"))],
        words(&["--dossier"]),
        vec![text(&dossier)],
        words(&["--dir"]),
        vec![text(&dir)],
    ]
    .concat();
    let mut python_controller = harness::start(harness::program(&python(), &argv, &dir)).expect("start the prototype's controller");
    let ready: Value = serde_json::from_str(&python_controller.read_line().expect("its line")).expect("a JSON line");
    let python_holder = ready["ready"].as_str().unwrap_or_else(|| panic!("not ready: {ready}")).to_owned();
    assert!(python_holder.starts_with("controller-"), "the prototype's controller's holder: {python_holder}");
    let python_status: Value = serde_json::from_slice(&fs::read(dir.join("controller-stato.json")).expect("its state file")).unwrap();
    assert_eq!(python_status["proprietario"], python_holder.as_str());

    let precheck = answer(false);
    let exit = resident(&dir, precheck.clone(), 1, Arc::new(AtomicBool::new(false))).run();
    println!("the Rust resident while the Python holds the lock: {exit:?}");
    assert_eq!((exit.clone(), exit.code()), (Exit::AnotherController { holder: Some(python_holder.clone()) }, 3));
    assert_eq!(precheck.calls.load(Ordering::SeqCst), 0);
    let store = Store::at(&dir).unwrap();
    assert_eq!(holder_of_the_lock(&store).as_deref(), Some(python_holder.as_str()), "the Rust's release did not touch it");

    let status = python_controller.finish().expect("the prototype's controller ends");
    assert!(status.success(), "{status:?}");
    assert_eq!(store.lock_status(&reserved()).unwrap(), LockStatus::Free, "the prototype's controller let its lock go");
}

// ---------- Acceptance 3, the X ----------

fn python_brief(dossier: &Path, dir: &Path) -> Value {
    let argv = [
        words(&["-X", "utf8", "-I", "-B"]),
        vec![text(&oracle("brief_oracle.py"))],
        words(&["--dossier"]),
        vec![text(dossier)],
        words(&["--dir"]),
        vec![text(dir)],
    ]
    .concat();
    let output = harness::program(&python(), &argv, dir).run().expect("run the prototype's brief");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the brief failed: {}\n{stdout}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_str(stdout.lines().last().unwrap_or("null")).unwrap_or_else(|error| panic!("not JSON ({error}): {stdout}"))
}

/// The state file the Rust resident wrote, read by the Python brief: its alarm, before what waits for the owner.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_python_brief_shows_the_alarm_the_rust_resident_wrote() {
    let (dossier, dir) = (dossier(), folder("brief"));
    for run in 1..=4 {
        let exit = resident(&dir, answer(true), 1, Arc::new(AtomicBool::new(false))).run();
        assert_eq!(exit, Exit::Done, "run {run}");
        if run == 2 {
            assert_eq!(python_brief(&dossier, &dir)["warnings"], Value::Null, "two ticks: below the threshold, no warning");
        }
    }
    let status: Value = serde_json::from_slice(&fs::read(dir.join("controller-stato.json")).unwrap()).unwrap();
    assert_eq!((status["budget_sconosciuto"]["giri"].clone(), status["budget_sconosciuto"]["allarme"].clone()), (json!(4), json!(true)));

    let brief = python_brief(&dossier, &dir);
    println!("the Python brief on the Rust's state file:\n{}", brief["text"].as_str().unwrap_or_default());
    let warnings = &brief["warnings"];
    assert_eq!((warnings["budget"]["giri"].clone(), warnings["budget"]["allarme"].clone()), (json!(4), json!(true)), "{warnings}");
    assert_eq!(warnings["budget"]["motivo"], "sonda: troppe richieste (HTTP 429)");
    assert_eq!((warnings["vecchio"].clone(), warnings["ultimo_giro"].clone()), (json!(false), status["ultimo_giro"].clone()));
    let text = brief["text"].as_str().unwrap();
    let alarm = text.find("BUDGET DI CLAUDE SCONOSCIUTO da 4 giri").unwrap_or_else(|| panic!("no alarm in the brief:\n{text}"));
    assert!(text.contains("troppe richieste (HTTP 429)"));
    assert!(text.find("== Aspetta te").is_some_and(|waiting| alarm < waiting), "the alarm comes first:\n{text}");
    assert_eq!(brief["json"]["controller"]["budget"]["giri"], 4);
}

// ---------- D2 of #417, the Python side ----------

/// The prototype's tests of D2 (#417) and of the registry saved only with the lock (#507), on the dossier's code.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_prototype_s_tests_of_the_registry_pass_on_the_dossier() {
    let dossier = dossier();
    let tests = dossier.join("tools").join("tests");
    let classes = ["test_controller.TestRegistroCaricatoDopoIlLock", "test_controller_lock_lanci.TestSalvataggioRegistroSenzaLock"];
    let argv = [words(&["-X", "utf8", "-B", "-m", "unittest", "-v"]), words(&classes)].concat();
    let output = harness::program(&python(), &argv, &tests).run().expect("run the prototype's tests");
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("{stderr}");
    assert!(output.status.success() && stderr.contains("Ran 6 tests") && stderr.trim_end().ends_with("OK"), "{stderr}");
}
