//! Dev only: one process on a state database, for the tests that need two of them, one per language
//! or several Rust ones. It takes the folder only from `--dir`, and refuses the default folder, where a
//! live controller may hold the real lock.
//!
//!     store-probe --dir <folder> [--wall-offset-s <s>] [--busy-wait-ms <ms>] <command> ...
//!
//! `acquire <project> <holder> <ttl>`, `renew <project> <holder> <ttl>`, `release <project> <holder>`,
//! `status <project>`, `hold-tx` (a write transaction held until stdin closes or the process dies), and
//! `contend <project> <holder> <attempts> --witness <folder> --start-at-ms <epoch ms> --hold-ms <ms>
//! --done <file> --peer-done <file>`.
//!
//! One JSON line on stdout: a test probe's, not the CLI contract. Exit 0 when done, 3 when refused,
//! as the prototype's CLI, and 1 on an error or a broken invariant.

use std::borrow::Cow;
use std::fs;
use std::hash::{BuildHasher, RandomState};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regia_identity::PROTOTYPE_NAME;
use regia_store::{
    Clock, Holder, LockOutcome, LockState, LockStatus, LockView, ProjectId, Store, SystemClock, Ttl, WallTime,
    database_name, is_within,
};
use serde_json::{Value, json};

/// The system clock with the wall clock moved by a fixed offset: someone set the clock.
#[derive(Debug)]
struct ShiftedClock {
    offset_s: f64,
}

impl Clock for ShiftedClock {
    fn now(&self) -> WallTime {
        SystemClock.now().checked_add_secs(self.offset_s).expect("the shifted wall clock stays in range")
    }

    fn monotonic(&self) -> f64 {
        SystemClock.monotonic()
    }

    fn tag(&self) -> Cow<'static, str> {
        SystemClock.tag()
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("store-probe: {message}");
            ExitCode::FAILURE
        }
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|arg| arg == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// The arguments that are not flags, nor values of flags.
fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
        } else if arg.starts_with("--") {
            skip = true;
        } else {
            out.push(arg.as_str());
        }
    }
    out
}

/// The folders a real installation uses, whatever the environment says: never touched from here.
fn live_folders() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let mut out = Vec::new();
    if let Some(local) = var("LOCALAPPDATA") {
        out.push(local.join(PROTOTYPE_NAME));
    }
    if let Some(state) = var("XDG_STATE_HOME") {
        out.push(state.join(PROTOTYPE_NAME));
    }
    for home in [var("USERPROFILE"), var("HOME")].into_iter().flatten() {
        out.push(home.join("AppData").join("Local").join(PROTOTYPE_NAME));
        out.push(home.join(".local").join("state").join(PROTOTYPE_NAME));
    }
    out
}

fn run(args: &[String]) -> Result<u8, String> {
    let dir = PathBuf::from(flag(args, "--dir").ok_or("--dir <folder> is required")?);
    if let Some(live) = live_folders().into_iter().find(|live| is_within(&dir, live)) {
        return Err(format!("{} is inside the live state folder {}: refused", dir.display(), live.display()));
    }
    let mut store = Store::at(&dir).map_err(|error| error.to_string())?;
    if let Some(offset) = flag(args, "--wall-offset-s") {
        let offset_s: f64 = offset.parse().map_err(|_| format!("bad --wall-offset-s {offset}"))?;
        store = store.with_clock(ShiftedClock { offset_s });
    }
    if let Some(wait) = flag(args, "--busy-wait-ms") {
        let wait: u64 = wait.parse().map_err(|_| format!("bad --busy-wait-ms {wait}"))?;
        store = store.with_busy_wait(Duration::from_millis(wait));
    }
    let words = positional(args);
    let (command, rest) = words.split_first().ok_or("no command")?;
    let project = |at: usize| -> Result<ProjectId, String> {
        ProjectId::new(*rest.get(at).ok_or("missing project")?).map_err(|error| error.to_string())
    };
    let holder = |at: usize| -> Result<Holder, String> {
        Holder::new(*rest.get(at).ok_or("missing holder")?).map_err(|error| error.to_string())
    };
    let ttl = |at: usize| -> Result<Ttl, String> {
        let text = rest.get(at).ok_or("missing ttl")?;
        let parsed = match text.parse::<u64>() {
            Ok(secs) => Ttl::from_secs(secs),
            Err(_) => Ttl::from_secs_f64(text.parse().map_err(|_| format!("bad ttl {text}"))?),
        };
        parsed.map_err(|error| error.to_string())
    };
    let fail = |error: regia_store::StoreError| error.to_string();
    match *command {
        "acquire" => Ok(print_outcome(&store.acquire_lock(&project(0)?, &holder(1)?, ttl(2)?).map_err(fail)?)),
        "renew" => Ok(print_outcome(&store.renew_lock(&project(0)?, &holder(1)?, ttl(2)?).map_err(fail)?)),
        "release" => Ok(print_outcome(&store.release_lock(&project(0)?, &holder(1)?).map_err(fail)?)),
        "status" => {
            print_line(&status_json(&store.lock_status(&project(0)?).map_err(fail)?));
            Ok(0)
        }
        "hold-tx" => hold_transaction(&store),
        "contend" => contend(&store, &project(0)?, &holder(1)?, rest.get(2).ok_or("missing attempts")?, args),
        other => Err(format!("unknown command {other}")),
    }
}

fn print_line(value: &Value) {
    let mut out = io::stdout().lock();
    writeln!(out, "{value}").expect("stdout");
    out.flush().expect("stdout");
}

fn state_name(state: LockState) -> &'static str {
    match state {
        LockState::Taken => "taken",
        LockState::Renewed => "renewed",
        LockState::Stolen => "stolen",
        LockState::Released => "released",
        LockState::Free => "free",
        LockState::Busy => "busy",
        LockState::Foreign => "foreign",
        LockState::Absent => "absent",
    }
}

fn view_json(view: Option<&LockView>) -> Value {
    view.map_or(Value::Null, |view| {
        json!({
            "holder": view.holder(),
            "expired": view.is_expired(),
            "expires_in_s": view.expires_in_s(),
            "fields": view.fields(),
        })
    })
}

fn outcome_json(outcome: &LockOutcome) -> Value {
    json!({
        "ok": outcome.is_ok(),
        "state": state_name(outcome.state()),
        "contended": matches!(outcome, LockOutcome::Contended { .. }),
        "lock": view_json(outcome.lock()),
    })
}

fn print_outcome(outcome: &LockOutcome) -> u8 {
    print_line(&outcome_json(outcome));
    outcome.exit_code()
}

fn status_json(status: &LockStatus) -> Value {
    let name = match status {
        LockStatus::Free => "free",
        LockStatus::Held(_) => "held",
        LockStatus::Expired(_) => "expired",
        LockStatus::Unreadable => "unreadable",
        LockStatus::Contended => "contended",
    };
    json!({"busy": status.is_busy(), "status": name, "lock": view_json(status.lock())})
}

/// A write transaction on the database, held like a holder stopped halfway through a mutation.
fn hold_transaction(store: &Store) -> Result<u8, String> {
    // Creates the database and its table first, as the prototype's open does.
    store.lock_status(&ProjectId::new("probe").expect("a valid id")).map_err(|error| error.to_string())?;
    let conn = rusqlite::Connection::open(store.folder().join(database_name())).map_err(|error| error.to_string())?;
    conn.busy_timeout(regia_store::BUSY_WAIT).map_err(|error| error.to_string())?;
    conn.execute_batch("BEGIN IMMEDIATE").map_err(|error| error.to_string())?;
    print_line(&json!("ready"));
    let _ = io::stdin().read_to_end(&mut Vec::new());
    let _ = conn.execute_batch("ROLLBACK");
    Ok(0)
}

fn random_below(bound: u64) -> u64 {
    if bound == 0 {
        return 0;
    }
    RandomState::new().hash_one(Instant::now()) % bound
}

fn epoch_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("clock after 1970").as_millis()
}

/// Inside the critical section: a folder only the holder of the lock may create, by either language.
/// Finding it there, or not finding it when leaving, means two holders at once. A folder, not a file,
/// because an antivirus that opens a new file can make deleting it fail.
struct Witness {
    marker: PathBuf,
}

impl Witness {
    fn enter(&self) -> Result<(), String> {
        match fs::create_dir(&self.marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err("violation".to_owned()),
            Err(error) => Err(format!("witness error: {error}")),
        }
    }

    fn leave(&self) -> Result<(), String> {
        match fs::remove_dir(&self.marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err("violation".to_owned()),
            Err(error) => Err(format!("witness error: {error}")),
        }
    }
}

/// Takes and releases the lock in a loop against a peer doing the same, until both made `attempts`.
fn contend(store: &Store, project: &ProjectId, holder: &Holder, attempts: &str, args: &[String]) -> Result<u8, String> {
    let attempts: u64 = attempts.parse().map_err(|_| format!("bad attempts {attempts}"))?;
    let witness = Witness { marker: Path::new(flag(args, "--witness").ok_or("--witness <folder>")?).join("inside") };
    let start_at: u128 = flag(args, "--start-at-ms").ok_or("--start-at-ms")?.parse().map_err(|_| "bad --start-at-ms")?;
    let hold_ms: u64 = flag(args, "--hold-ms").unwrap_or("2").parse().map_err(|_| "bad --hold-ms")?;
    let done = PathBuf::from(flag(args, "--done").ok_or("--done <file>")?);
    let peer_done = PathBuf::from(flag(args, "--peer-done").ok_or("--peer-done <file>")?);
    let ttl = Ttl::from_secs(60).expect("a valid ttl");
    while epoch_ms() < start_at {
        std::hint::spin_loop();
    }
    let started = Instant::now();
    let (mut made, mut wins, mut busy, mut contended, mut violations) = (0_u64, 0_u64, 0_u64, 0_u64, 0_u64);
    let mut anomalies: Vec<String> = Vec::new();
    loop {
        made += 1;
        match store.acquire_lock(project, holder, ttl) {
            Ok(LockOutcome::Taken(_) | LockOutcome::Stolen(_)) => {
                wins += 1;
                if let Err(problem) = witness.enter() {
                    if problem == "violation" { violations += 1 } else { anomalies.push(problem) }
                }
                thread::sleep(Duration::from_micros(random_below(hold_ms * 1000 + 1)));
                if let Err(problem) = witness.leave() {
                    if problem == "violation" { violations += 1 } else { anomalies.push(problem) }
                }
                match store.release_lock(project, holder) {
                    Ok(LockOutcome::Released) => {}
                    other => anomalies.push(format!("release at attempt {made}: {other:?}")),
                }
            }
            Ok(LockOutcome::Busy(_)) => busy += 1,
            Ok(LockOutcome::Contended { .. }) => contended += 1,
            other => anomalies.push(format!("acquire at attempt {made}: {other:?}")),
        }
        if made == attempts {
            fs::write(&done, b"").map_err(|error| format!("{}: {error}", done.display()))?;
        }
        if made >= attempts && peer_done.exists() {
            break;
        }
        thread::sleep(Duration::from_micros(random_below(1000)));
    }
    anomalies.truncate(20);
    let clean = violations == 0 && anomalies.is_empty();
    print_line(&json!({
        "side": "rust",
        "sqlite": rusqlite::version(),
        "clock": SystemClock.tag(),
        "attempts": made,
        "wins": wins,
        "busy": busy,
        "contended": contended,
        "violations": violations,
        "anomalies": anomalies,
        "seconds": started.elapsed().as_secs_f64(),
    }));
    Ok(if clean { 0 } else { 1 })
}
