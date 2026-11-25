//! Local only: Python and Rust on the same `regia.db` (ADR 0001, section 7, "Not proven"). Each test
//! runs the prototype's real runtime, loaded from the dossier, against the Rust store, one process per
//! language. CI cannot read the dossier, so these tests are ignored there:
//!
//!     REGIA_HOME=<dossier> cargo test -p regia-conformance --test store_cross -- --ignored --test-threads=1
//!
//! Every state folder is a fresh one under `target/`, never the live one.
//!
//! **The verdict rule, fixed before the first run (task #437 of the dossier).** The lock holds across
//! the two languages if all of these pass, on Windows and on Linux:
//!
//! 1. Contention: one process per language makes at least 1,000 attempts each on the same lock, with
//!    0 witness violations (never two holders), 0 anomalies and 0 errors. To count as contention, each
//!    side wins at least 100 times and finds the lock busy at least 100 times; less is inconclusive,
//!    not a verdict, and the run is repeated with other pacing.
//! 2. Cross reads: a lock written by either side reads on the other with the same holder and TTL, by
//!    the wall clock, and while it is valid the other side's acquire, renew and release are refused.
//! 3. The prototype's `controller.py` exits with 3 while the Rust holds the row `(lock, controller)`,
//!    and the Rust is refused while the Python holds it, with the row the controller writes.
//! 4. An expired lock is stolen in both directions, and the new row names the holder it replaced.
//! 5. The known cost is shown, not failed: a forward jump of the wall clock past the TTL lets the other
//!    language steal a live lock, and a backward jump keeps the other language's expired lock alive;
//!    the same language is not affected, because it reads its own monotonic clock.
//!
//! A failure of 1 to 4 means plan B of ADR 0001, section 7: `lock`, `lease` and `step` stay a Python
//! subprocess for as long as the Python controller runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regia_exec::harness::{self, Running};
use regia_exec::{Launch, Limits, Output, Pin, Program};
use regia_identity::prototype_env_var;
use serde_json::Value;

const ATTEMPTS: u64 = 1000;
const MIN_WINS: u64 = 100;
const MIN_BUSY: u64 = 100;

struct Sides {
    dossier: PathBuf,
    /// The interpreter the product would start: an absolute path, found once (threat model T06).
    python: Pin,
    probe: PathBuf,
    oracle: PathBuf,
}

fn sides() -> Sides {
    let dossier = std::env::var_os(prototype_env_var("HOME"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {} to the dossier: these tests run its runtime", prototype_env_var("HOME")));
    assert!(dossier.join("tools").join("runtime.py").is_file(), "no tools/runtime.py in {}", dossier.display());
    Sides {
        dossier,
        python: Pin::resolve(Program::Python).unwrap_or_else(|error| panic!("{error}")),
        probe: PathBuf::from(env!("CARGO_BIN_EXE_store-probe")),
        oracle: Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle").join("store_oracle.py"),
    }
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// A fresh, empty state folder under `target/`.
fn folder(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("store-cross").join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    fs::create_dir_all(&dir).expect("state folder");
    dir
}

#[derive(Debug)]
struct Ran {
    code: i32,
    json: Value,
    stderr: String,
}

fn finish(output: Output) -> Ran {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("null");
    Ran {
        code: output.status.code().unwrap_or(-1),
        json: serde_json::from_str(last).unwrap_or_else(|error| panic!("not JSON ({error}): {stdout}")),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

impl Sides {
    /// `store-probe`, a binary of this workspace.
    fn rust_launch(&self, dir: &Path, args: &[&str]) -> Launch {
        let argv = [words(&["--dir"]), vec![text(dir)], words(args)].concat();
        harness::launch(&self.probe, &argv, dir).without(&prototype_env_var("RUNTIME"))
    }

    /// The oracle on the prototype's runtime, through `regia-exec` as the product starts Python. `-I`
    /// already ignores every `PYTHON*` variable, so the allow-list changes nothing it reads.
    fn python_launch(&self, dir: &Path, args: &[&str]) -> Launch {
        let argv = [
            words(&["-I"]),
            vec![text(&self.oracle)],
            words(&["--dossier"]),
            vec![text(&self.dossier)],
            words(&["--dir"]),
            vec![text(dir)],
            words(args),
        ]
        .concat();
        harness::program(&self.python, &argv, dir).without(&prototype_env_var("RUNTIME"))
    }

    fn rust(&self, dir: &Path, args: &[&str]) -> Ran {
        finish(self.rust_launch(dir, args).run().expect("run store-probe"))
    }

    fn python(&self, dir: &Path, args: &[&str]) -> Ran {
        finish(self.python_launch(dir, args).run().expect("run the prototype's runtime"))
    }
}

/// A process holding a write transaction until it is killed or its stdin closes.
struct Holding(Running);

impl Holding {
    fn start(launch: Launch) -> Self {
        let mut running = harness::start(launch).expect("start the holder");
        let line = running.read_line().expect("read");
        assert_eq!(line.trim(), "\"ready\"", "the holder did not start");
        Self(running)
    }

    fn kill(self) {
        self.0.kill().expect("kill");
    }

    fn release(self) {
        self.0.finish().expect("wait");
    }
}

fn s(value: &Value) -> &str {
    value.as_str().unwrap_or_else(|| panic!("not a string: {value}"))
}

#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn python_writes_rust_reads() {
    let sides = sides();
    let dir = folder("python-writes");
    let taken = sides.python(&dir, &["acquire", "p437", "py-holder", "60"]);
    assert_eq!((taken.code, s(&taken.json["stato"])), (0, "preso"), "{taken:?}");

    let status = sides.rust(&dir, &["status", "p437"]);
    assert_eq!(status.json["status"], "held", "{status:?}");
    assert_eq!(status.json["lock"]["holder"], "py-holder");
    assert_eq!(status.json["lock"]["fields"]["ttl_s"], 60);
    let left = status.json["lock"]["expires_in_s"].as_i64().expect("seconds");
    assert!((58..=60).contains(&left), "read by the wall clock: {left}");

    for args in [&["acquire", "p437", "rs-holder", "60"][..], &["renew", "p437", "rs-holder", "60"], &["release", "p437", "rs-holder"]] {
        let refused = sides.rust(&dir, args);
        assert_eq!(refused.code, 3, "{args:?}: {refused:?}");
        assert_eq!(refused.json["lock"]["holder"], "py-holder", "{args:?}");
    }
    let released = sides.python(&dir, &["release", "p437", "py-holder"]);
    assert_eq!(s(&released.json["stato"]), "rilasciato");
    assert_eq!(sides.rust(&dir, &["status", "p437"]).json["status"], "free");
}

#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn rust_writes_python_reads() {
    let sides = sides();
    let dir = folder("rust-writes");
    let taken = sides.rust(&dir, &["acquire", "p437", "rs-holder", "60"]);
    assert_eq!((taken.code, &taken.json["state"]), (0, &Value::from("taken")), "{taken:?}");

    let status = sides.python(&dir, &["status", "p437"]);
    assert_eq!((status.json["occupato"].as_bool(), s(&status.json["stato"])), (Some(true), "occupato"), "{status:?}");
    assert_eq!(s(&status.json["proprietario"]), "rs-holder");
    assert_eq!(status.json["ttl_s"], 60);
    let left = status.json["scade_tra_s"].as_i64().expect("seconds");
    assert!((58..=60).contains(&left), "read by the wall clock: {left}");

    for (args, state) in [
        (&["acquire", "p437", "py-holder", "60"][..], "occupato"),
        (&["renew", "p437", "py-holder", "60"], "altrui"),
        (&["release", "p437", "py-holder"], "altrui"),
    ] {
        let refused = sides.python(&dir, args);
        assert_eq!((refused.code, s(&refused.json["stato"])), (3, state), "{args:?}: {refused:?}");
    }
    let released = sides.rust(&dir, &["release", "p437", "rs-holder"]);
    assert_eq!(released.json["state"], "released");
    assert_eq!(s(&sides.python(&dir, &["status", "p437"]).json["stato"]), "libero");
}

#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn each_side_renews_the_others_row_and_the_generation_counts_both() {
    let sides = sides();
    let dir = folder("renew-across");
    assert_eq!(sides.python(&dir, &["acquire", "p437", "same", "60"]).code, 0);
    let renewed = sides.rust(&dir, &["renew", "p437", "same", "120"]);
    assert_eq!(renewed.json["state"], "renewed", "{renewed:?}");
    assert_eq!(renewed.json["lock"]["fields"]["ttl_s"], 120);
    let again = sides.python(&dir, &["renew", "p437", "same", "90"]);
    assert_eq!((s(&again.json["stato"]), again.json["ttl_s"].as_f64()), ("rinnovato", Some(90.0)), "{again:?}");
    assert!(again.json["rinnovato"].is_string(), "{again:?}");

    let conn = rusqlite::Connection::open(dir.join("regia.db")).expect("open");
    let generation: i64 = conn.query_row("SELECT gen FROM documenti WHERE tipo = 'lock'", [], |row| row.get(0)).expect("gen");
    assert_eq!(generation, 3, "one per write, whoever wrote");
}

#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn both_create_the_same_schema() {
    let sides = sides();
    let (by_python, by_rust) = (folder("schema-python"), folder("schema-rust"));
    sides.python(&by_python, &["status", "p437"]);
    sides.rust(&by_rust, &["status", "p437"]);
    let schema = |dir: &Path| -> (String, String) {
        let conn = rusqlite::Connection::open(dir.join("regia.db")).expect("open");
        let sql = conn.query_row("SELECT sql FROM sqlite_master WHERE type = 'table'", [], |row| row.get(0)).expect("schema");
        let journal = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).expect("journal");
        (sql, journal)
    };
    assert_eq!(schema(&by_python), schema(&by_rust));
    assert_eq!(schema(&by_python).1, "delete", "the prototype never sets the journal mode: a database it creates is in the rollback journal");
    assert_eq!(schema(&by_rust).1, "delete", "rollback journal, never WAL");
}

#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn expired_locks_are_stolen_both_ways() {
    let sides = sides();
    let dir = folder("steal");
    assert_eq!(sides.python(&dir, &["acquire", "p437", "py-holder", "0.2"]).code, 0);
    thread::sleep(Duration::from_millis(400));
    let stolen = sides.rust(&dir, &["acquire", "p437", "rs-holder", "0.2"]);
    assert_eq!(stolen.json["state"], "stolen", "{stolen:?}");
    assert_eq!(stolen.json["lock"]["fields"]["al_posto_di"], "py-holder");
    assert!(stolen.json["lock"]["fields"]["rubato_il"].is_string());

    thread::sleep(Duration::from_millis(400));
    let back = sides.python(&dir, &["acquire", "p437", "py-holder", "60"]);
    assert_eq!((s(&back.json["stato"]), s(&back.json["al_posto_di"])), ("rubato", "rs-holder"), "{back:?}");
}

#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn a_forward_wall_jump_past_the_ttl_steals_across_languages_only() {
    let sides = sides();
    // The known cost of ADR 0001, section 7: across languages the reader has only the wall clock.
    let dir = folder("jump-forward-python-holds");
    assert_eq!(sides.python(&dir, &["acquire", "p437", "py-holder", "600"]).code, 0);
    let same = sides.python(&dir, &["--wall-offset-s", "700", "acquire", "p437", "py-other", "600"]);
    assert_eq!(s(&same.json["stato"]), "occupato", "the Python reads its own monotonic clock: {same:?}");
    let cross = sides.rust(&dir, &["--wall-offset-s", "700", "acquire", "p437", "rs-holder", "600"]);
    assert_eq!(cross.json["state"], "stolen", "known cost, Rust over Python: {cross:?}");

    let dir = folder("jump-forward-rust-holds");
    assert_eq!(sides.rust(&dir, &["acquire", "p437", "rs-holder", "600"]).code, 0);
    let same = sides.rust(&dir, &["--wall-offset-s", "700", "acquire", "p437", "rs-other", "600"]);
    assert_eq!(same.json["state"], "busy", "the Rust reads its own monotonic clock: {same:?}");
    let cross = sides.python(&dir, &["--wall-offset-s", "700", "acquire", "p437", "py-holder", "600"]);
    assert_eq!(s(&cross.json["stato"]), "rubato", "known cost, Python over Rust: {cross:?}");
}

#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn a_backward_wall_jump_keeps_an_expired_lock_alive_across_languages_only() {
    let sides = sides();
    let dir = folder("jump-back-python-holds");
    assert_eq!(sides.python(&dir, &["acquire", "p437", "py-holder", "1"]).code, 0);
    thread::sleep(Duration::from_millis(1500));
    let cross = sides.rust(&dir, &["--wall-offset-s", "-1000", "acquire", "p437", "rs-holder", "60"]);
    assert_eq!(cross.json["state"], "busy", "known cost, the Python's expired lock lives on for the Rust: {cross:?}");
    let same = sides.python(&dir, &["--wall-offset-s", "-1000", "acquire", "p437", "py-other", "60"]);
    assert_eq!(s(&same.json["stato"]), "rubato", "the Python reads its own monotonic clock: {same:?}");

    let dir = folder("jump-back-rust-holds");
    assert_eq!(sides.rust(&dir, &["acquire", "p437", "rs-holder", "1"]).code, 0);
    thread::sleep(Duration::from_millis(1500));
    let cross = sides.python(&dir, &["--wall-offset-s", "-1000", "acquire", "p437", "py-holder", "60"]);
    assert_eq!(s(&cross.json["stato"]), "occupato", "known cost, the Rust's expired lock lives on for the Python: {cross:?}");
    let same = sides.rust(&dir, &["--wall-offset-s", "-1000", "acquire", "p437", "rs-other", "60"]);
    assert_eq!(same.json["state"], "stolen", "the Rust reads its own monotonic clock: {same:?}");
}

#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn a_transaction_held_by_one_language_makes_the_other_busy_and_dying_frees_it() {
    let sides = sides();
    let dir = folder("held-by-python");
    let held = Holding::start(sides.python_launch(&dir, &["hold-tx"]));
    let refused = sides.rust(&dir, &["--busy-wait-ms", "300", "acquire", "p437", "rs-holder", "60"]);
    assert_eq!((refused.code, &refused.json["contended"]), (3, &Value::Bool(true)), "{refused:?}");
    let status = sides.rust(&dir, &["--busy-wait-ms", "300", "status", "p437"]);
    assert_eq!((&status.json["status"], &status.json["busy"]), (&Value::from("contended"), &Value::Bool(true)));
    held.kill();
    assert_eq!(sides.rust(&dir, &["acquire", "p437", "rs-holder", "60"]).json["state"], "taken", "a dead holder frees it");

    let dir = folder("held-by-rust");
    let held = Holding::start(sides.rust_launch(&dir, &["hold-tx"]));
    let refused = sides.python(&dir, &["--busy-wait", "0.3", "acquire", "p437", "py-holder", "60"]);
    assert_eq!((refused.code, s(&refused.json["stato"])), (3, "occupato"), "{refused:?}");
    assert!(s(&refused.json["motivo"]).contains("contesa"), "{refused:?}");
    let status = sides.python(&dir, &["--busy-wait", "0.3", "status", "p437"]);
    assert_eq!((status.json["occupato"].as_bool(), s(&status.json["stato"])), (Some(true), "illeggibile"));
    held.release();
    let held = Holding::start(sides.rust_launch(&dir, &["hold-tx"]));
    held.kill();
    assert_eq!(s(&sides.python(&dir, &["acquire", "p437", "py-holder", "60"]).json["stato"]), "preso");
}

/// The row `prendi_lock` of `controller.py` writes: lock `controller`, 600 s, holder `controller-<pid>-<hex>`.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_controller_lock_excludes_both_ways() {
    let sides = sides();
    let dir = folder("controller");
    let holder = format!("rust-437-{}", std::process::id());
    let taken = sides.rust(&dir, &["acquire", "controller", &holder, "600"]);
    assert_eq!(taken.json["state"], "taken", "{taken:?}");

    let missing_config = dir.join("no-operations.json");
    // `-X utf8`: the allow-list drops the `PYTHONUTF8` this used to inherit, which the prototype's own
    // launcher sets for every Python it starts. Not `-I`: `import runtime` needs the script's folder.
    let argv = [
        words(&["-X", "utf8"]),
        vec![text(&sides.dossier.join("tools").join("controller.py"))],
        words(&["--una-volta", "--lancio", "finto", "--no-ripresa", "--no-sveglia", "--progetto", "p437", "--config"]),
        vec![text(&missing_config)],
    ]
    .concat();
    let output = harness::program(&sides.python, &argv, &dir)
        .env(&prototype_env_var("RUNTIME"), &dir)
        .run()
        .expect("run controller.py");
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("controller.py exit {:?}\nstdout: {}\nstderr: {stderr}", output.status.code(), String::from_utf8_lossy(&output.stdout));
    assert_eq!(output.status.code(), Some(3), "controller.py must stop: {stderr}");
    assert!(stderr.contains(&holder), "it must have seen this lock, not another: {stderr}");
    let still = sides.rust(&dir, &["status", "controller"]);
    assert_eq!(still.json["lock"]["holder"], holder.as_str(), "its own release must not touch the Rust lock");
    assert_eq!(sides.rust(&dir, &["release", "controller", &holder]).json["state"], "released");

    let python_holder = format!("controller-{}-0123abcd", std::process::id());
    let taken = sides.python(&dir, &["acquire", "controller", &python_holder, "600"]);
    assert_eq!(s(&taken.json["stato"]), "preso");
    let refused = sides.rust(&dir, &["acquire", "controller", "rust-controller", "600"]);
    println!("store-probe while the Python holds it: exit {} {}", refused.code, refused.json);
    assert_eq!((refused.code, &refused.json["state"]), (3, &Value::from("busy")), "{refused:?}");
    assert_eq!(refused.json["lock"]["holder"], python_holder.as_str());
}

/// `test_a_otto_processi_un_solo_vincitore` of `test_runtime.py`, with eight real processes, four of each
/// language, in four rounds: each round starts all eight on the same free lock at the same instant, and ends
/// with one winner, seven refusals and the winner's row. The prototype's case starts eight Python processes;
/// the one-language cases on the Rust side are in `crates/regia-store/tests/state.rs`.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn eight_processes_four_of_each_language_give_one_winner_in_each_of_four_rounds() {
    let sides = sides();
    let dir = folder("eight-processes");
    let limits = Limits { timeout: Duration::from_secs(120), ..Limits::default() };
    for round in 0..4 {
        let project = format!("p{round}");
        // The interpreters need a moment to start; every process then waits for this instant.
        let start = (epoch_ms() + 3000).to_string();
        let ran: Vec<(String, bool, Ran)> = thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let (sides, dir, project, start) = (&sides, &dir, &project, &start);
                    scope.spawn(move || {
                        let rust = i % 2 == 0;
                        let holder = format!("{}{i}", if rust { "rs-" } else { "py-" });
                        let args = ["acquire", project.as_str(), holder.as_str(), "60", "--start-at-ms", start.as_str()];
                        let launch = if rust { sides.rust_launch(dir, &args) } else { sides.python_launch(dir, &args) };
                        let output = launch.limits(limits).run().expect("run one of the eight");
                        (holder, rust, finish(output))
                    })
                })
                .collect();
            handles.into_iter().map(|handle| handle.join().expect("one of the eight")).collect()
        });
        let winners: Vec<&(String, bool, Ran)> = ran.iter().filter(|(_, _, ran)| ran.code == 0).collect();
        assert_eq!(winners.len(), 1, "round {round}: {ran:?}");
        for (holder, rust, ran) in &ran {
            let refused = if *rust { s(&ran.json["state"]) == "busy" } else { s(&ran.json["stato"]) == "occupato" };
            assert!(ran.code == 0 || (ran.code == 3 && refused), "round {round}, {holder}: {ran:?}");
        }
        let held = sides.rust(&dir, &["status", &project]);
        assert_eq!(held.json["lock"]["holder"], winners[0].0.as_str(), "round {round}: the row is the winner's: {held:?}");
    }
}

fn refs(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

fn epoch_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("clock after 1970").as_millis()
}

/// The contention run of the verdict: its log goes to `target/` and, by hand, to the dossier.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn contention_never_gives_two_holders() {
    let sides = sides();
    let dir = folder("contention");
    let witness = dir.join("witness");
    fs::create_dir_all(&witness).expect("witness folder");
    let (rust_done, python_done) = (dir.join("rust.done"), dir.join("python.done"));
    // Both start together, once both interpreters are up.
    let start = (epoch_ms() + 3000).to_string();
    let attempts = ATTEMPTS.to_string();
    let witness = witness.display().to_string();
    let args = |holder: &str, done: &Path, peer: &Path| -> Vec<String> {
        let (done, peer) = (done.display().to_string(), peer.display().to_string());
        [
            "contend", "p437", holder, &attempts, "--witness", &witness, "--start-at-ms", &start, "--hold-ms", "2", "--done",
            &done, "--peer-done", &peer,
        ]
        .map(str::to_owned)
        .to_vec()
    };
    let rust_args = args("rs-holder", &rust_done, &python_done);
    let python_args = args("py-holder", &python_done, &rust_done);
    // One supervised run per side, at once. A run takes about 20 s on Windows; the default deadline
    // would end a slow one as a "not JSON" panic instead of a timeout.
    let limits = Limits { timeout: Duration::from_secs(600), ..Limits::default() };
    let (rust, python) = thread::scope(|scope| {
        let rust = scope.spawn(|| sides.rust_launch(&dir, &refs(&rust_args)).limits(limits).run());
        let python = scope.spawn(|| sides.python_launch(&dir, &refs(&python_args)).limits(limits).run());
        (rust.join().expect("the Rust side"), python.join().expect("the Python side"))
    });
    let rust = finish(rust.expect("the Rust side"));
    let python = finish(python.expect("the Python side"));

    let violations = [&rust, &python].iter().map(|ran| ran.json["violations"].as_u64().unwrap_or(u64::MAX)).sum::<u64>();
    let clean = [&rust, &python].iter().all(|ran| ran.code == 0 && ran.json["anomalies"] == Value::Array(Vec::new()));
    let contended = [&rust, &python].iter().all(|ran| {
        ran.json["attempts"].as_u64().unwrap_or(0) >= ATTEMPTS
            && ran.json["wins"].as_u64().unwrap_or(0) >= MIN_WINS
            && ran.json["busy"].as_u64().unwrap_or(0) >= MIN_BUSY
    });
    let verdict = match (violations == 0 && clean, contended) {
        (false, _) => "broken: two holders at once, or an anomaly",
        (true, false) => "inconclusive: too little contention, repeat with other pacing",
        (true, true) => "holds",
    };
    let mut log = String::new();
    log.push_str("# Contention on one lock: Python and Rust on the same regia.db (task #437)
");
    log.push_str(&format!("when: {}
", regia_store::WallTime::now().to_iso()));
    log.push_str(&format!("os: {} {}
", std::env::consts::OS, std::env::consts::ARCH));
    log.push_str(&format!(
        "rule: each side at least {ATTEMPTS} attempts, at least {MIN_WINS} wins and {MIN_BUSY} busy; 0 violations, 0 anomalies
"
    ));
    log.push_str("each attempt: acquire with a 60 s ttl; on a win, a witness folder created, up to 2 ms held, removed, then release
");
    log.push_str(&format!("rust: exit {} {}
", rust.code, rust.json));
    log.push_str(&format!("python: exit {} {}
", python.code, python.json));
    log.push_str(&format!("two holders at once: {violations}
"));
    log.push_str(&format!("verdict of this run: {verdict}
"));
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("store-cross").join(format!("contention-{}.log", std::env::consts::OS));
    fs::write(&path, &log).expect("write the log");
    println!("{log}log: {}", path.display());
    if !rust.stderr.is_empty() || !python.stderr.is_empty() {
        println!("stderr, rust: {}
stderr, python: {}", rust.stderr, python.stderr);
    }
    assert_eq!(violations, 0, "two holders at once");
    assert!(clean, "anomalies: {rust:?} {python:?}");
    assert!(contended, "inconclusive, not a verdict: too little contention to count");
}
