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
//! - **The limits of the days skipped** (#732). The prototype's constants are the format's `lookback_days` and
//!   `skipped_kept`, and on the scenarios of the two cases of `controller_schedule.rs` that read them, the prototype
//!   records and keeps the days those cases expect of the Rust. So the two agree, and a format changed alone fails here.
//!
//! Every state folder is a fresh one under `target/`, never the live one. The first two run both sides on the system's
//! clock and the machine's local day, as the prototype counts it; the limits' cases tick the prototype at fixed
//! instants, at noon UTC, the same day in the machine's zone. Python runs with `-B`, so nothing is written in the
//! dossier's `tools/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Datelike, Days, NaiveDate};
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

// ---------- the limits of the days skipped (#732) ----------

/// The days of the week of the two operations, Monday 0.
const WEEKDAYS: [u32; 5] = [0, 1, 2, 3, 4];
const EVERY_DAY: [u32; 7] = [0, 1, 2, 3, 4, 5, 6];

/// The days of `controller_schedule.rs`'s cases: October 2026's, and past 31 the month after's.
const NOVEMBER: u32 = 31;

fn date(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 1).unwrap() + Days::new(u64::from(day) - 1)
}

/// Noon UTC of a day: the same day in the machine's zone, as the prototype counts it.
fn noon(day: u32) -> String {
    format!("{}T12:00:00Z", date(day))
}

/// The days from `first` to `last`, both in, that fall on `days` (Monday 0), in order, as the state writes them.
fn dates(first: NaiveDate, last: NaiveDate, days: &[u32]) -> Vec<String> {
    first.iter_days().take_while(|day| *day <= last).filter(|day| days.contains(&day.weekday().num_days_from_monday())).map(|day| day.to_string()).collect()
}

/// The limits of format v1 on the days skipped: `lookback_days` and `skipped_kept` of `operations_v1.toml`.
fn skipped_limits() -> (u64, usize) {
    let file = regia_conformance::workspace_root().join("crates").join("regia-controller").join("formats").join("operations_v1.toml");
    let format: toml::Table = toml::from_str(&fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))).expect("TOML");
    let limit = |key: &str| format[key].as_integer().unwrap_or_else(|| panic!("no {key} in {}", file.display()));
    (u64::try_from(limit("lookback_days")).unwrap(), usize::try_from(limit("skipped_kept")).unwrap())
}

/// The operations of the two cases, at midnight: due at any hour of the machine's day.
fn weekdays() -> Value {
    json!({"operazioni": [{"nome": "review", "ora": "00:00", "giorni": "lun-ven", "azione": "chat", "prompt": "review the week"}]})
}

fn every_day() -> Value {
    json!({"operazioni": [{"nome": "sera", "ora": "00:00", "giorni": "tutti", "azione": "chat", "prompt": "the evening's brief"}]})
}

/// The prototype's operations on the fresh folder `dir`, one tick at noon of each day, in order.
fn prototype_ticks(dossier: &Path, dir: &Path, config: &Value, days: &[u32]) -> Value {
    let file = dir.join("operations.json");
    fs::write(&file, config.to_string()).unwrap();
    let mut argv = [
        words(&["-X", "utf8", "-I", "-B"]),
        vec![text(&oracle("schedule_oracle.py"))],
        words(&["--dossier"]),
        vec![text(dossier)],
        words(&["--dir"]),
        vec![text(dir)],
        words(&["--config"]),
        vec![text(&file)],
        words(&["--closings"]),
        vec![text(&dir.join("giornalieri"))],
    ]
    .concat();
    for day in days {
        argv.extend(["--at".to_owned(), noon(*day)]);
    }
    let python = python_json(&argv, dir);
    assert_eq!(python["error"], Value::Null, "{python}");
    python
}

/// The scenario of `lookback_days_bounds_the_days_skipped_after_a_long_absence`: the prototype's
/// `SALTATE_LOOKBACK_GIORNI` is the format's, and after a run on Thursday 22 October and twenty days away it records
/// the lookback's working days only, and asks for today's chat only.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_prototype_s_lookback_is_the_format_s() {
    let (dossier, dir) = (dossier(), folder("lookback"));
    let (lookback, _) = skipped_limits();
    let python = prototype_ticks(&dossier, &dir, &weekdays(), &[22, NOVEMBER + 11]);
    assert_eq!(python["limits"]["lookback_days"], json!(lookback), "the prototype's lookback is the format's: {python}");
    assert_eq!(python["voices"], json!([["review", "lanciata"], ["review", "saltata"], ["review", "lanciata"]]), "{python}");
    let today = date(NOVEMBER + 11);
    assert_eq!(python["records"][1][0]["giorni"], json!(dates(today - Days::new(lookback), today - Days::new(1), &WEEKDAYS)), "{python}");
    assert_eq!(python["calls"], json!(["review", "review"]), "no chat for the days skipped");
}

/// The scenario of `skipped_kept_bounds_the_days_an_entry_keeps`: the prototype's `SALTATE_TENUTE` is the format's,
/// and after two absences of a week its entry keeps the most recent days skipped, the older first.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_prototype_keeps_the_format_s_skipped_kept() {
    let (dossier, dir) = (dossier(), folder("kept"));
    let (_, kept) = skipped_limits();
    let python = prototype_ticks(&dossier, &dir, &every_day(), &[22, 29, NOVEMBER + 5]);
    assert_eq!(python["limits"]["skipped_kept"], json!(kept), "the prototype keeps what the format keeps: {python}");
    let (away, again) = (dates(date(23), date(28), &EVERY_DAY), dates(date(30), date(NOVEMBER + 4), &EVERY_DAY));
    assert_eq!((python["records"][1][0]["giorni"].clone(), python["records"][2][0]["giorni"].clone()), (json!(away), json!(again)), "{python}");
    let all = [away, again].concat();
    assert_eq!(python["ops"]["sera"]["saltate"], json!(all[all.len() - kept..]), "{python}");
    assert_eq!(python["calls"], json!(["sera", "sera", "sera"]));
}
