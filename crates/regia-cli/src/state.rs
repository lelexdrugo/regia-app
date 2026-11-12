//! The state commands, natively: a task's lease (`lease acquire`, `heartbeat`, `release`, `list`) and the
//! orchestrator's step (`step start`, `renew`, `finish`), with the arguments of the facade table
//! (`table/lease.toml`, `table/step.toml`). They are not subcommands of the CLI's parser: while their group
//! runs on the prototype, the facade takes their argv as it is.
//!
//! The JSON is the contract, with the table's names. A refusal exits with 3, which the precheck and the
//! orchestrator's prompt tell apart from an error, 1. The text is the JSON's keys, one leaf a line, as
//! `checkpoint` prints them: the prototype's sentences would need a catalog (Q6 of the conformance study).

use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use clap::{ArgAction, Parser};
use regia_i18n::Catalog;
use regia_store::{
    EventsCursor, Holder, LeaseEntry, LockOutcome, LockState, LockView, ProjectId, StepClosing, StepOpening, TaskNumber, Ttl, ViewField,
    WallTime, generated_holder, reports_folder, step_commit_message,
};
use serde_json::{Map, Value, json};

use crate::clock::Clock;
use crate::declare::record_activity;
use crate::dossier::{ProjectFolder, sync};
use crate::reads::{parse, python_int};
use crate::{flat, print_json};

/// Claude Code's id of the session a command runs in: the holder a step defaults to, and the session the
/// step records, as the prototype reads them.
const SESSION_VARIABLE: &str = "CLAUDE_CODE_SESSION_ID";
/// The lock of a step, which a long step renews.
const STEP_TTL_S: u64 = 1800;
/// What `step finish` renews the lock by, to check it is still the caller's before it writes the cursor.
const FINISH_TTL_S: u64 = 60;
/// What the checks of `step finish` answer until they are ported: the protocol lets the step close
/// without them.
const CHECKS_NOT_PORTED: &str = "the checks of the step are not ported yet";
/// The exit of a refusal.
const REFUSED: u8 = 3;

/// The fields of a lock or lease document, with the table's names (`lock_view`, `lease_view`).
const VIEW_FIELDS: [(ViewField, &str); 10] = [
    (ViewField::Task, "task"),
    (ViewField::Project, "project"),
    (ViewField::Holder, "holder"),
    (ViewField::TakenAt, "taken_at"),
    (ViewField::HeartbeatAt, "heartbeat_at"),
    (ViewField::TtlS, "ttl_s"),
    (ViewField::ExpiresAt, "expires_at"),
    (ViewField::Replaced, "replaced"),
    (ViewField::StolenAt, "stolen_at"),
    (ViewField::RenewedAt, "renewed_at"),
];

#[derive(Parser)]
#[command(name = "lease acquire", args_override_self = true)]
struct LeaseAcquireArgs {
    /// The task's number.
    task: String,
    #[arg(long)]
    project: String,
    #[arg(long)]
    holder: String,
    /// Seconds.
    #[arg(long)]
    ttl: String,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "lease heartbeat", args_override_self = true)]
struct LeaseHeartbeatArgs {
    /// The task's number.
    task: String,
    #[arg(long)]
    holder: String,
    /// Seconds. Without it: the ttl the lease was taken with.
    #[arg(long)]
    ttl: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "lease release", args_override_self = true)]
struct LeaseReleaseArgs {
    /// The task's number.
    task: String,
    #[arg(long)]
    holder: String,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "lease list", args_override_self = true)]
struct LeaseListArgs {
    /// Only the expired ones.
    #[arg(long)]
    expired: bool,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// `step start` and `step renew`.
#[derive(Parser)]
#[command(name = "step", args_override_self = true)]
struct StepArgs {
    #[arg(long)]
    project: String,
    /// Without it: the session's id; `step start`, with no session either, generates one.
    #[arg(long)]
    holder: Option<String>,
    /// Seconds. Without it: 1800.
    #[arg(long)]
    ttl: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "step finish", args_override_self = true)]
struct StepFinishArgs {
    #[arg(long)]
    project: String,
    /// Without it: the session's id. The id `step start` printed, when it generated one.
    #[arg(long)]
    holder: Option<String>,
    /// A file to commit besides the named ones, relative to the project's folder. Repeatable.
    #[arg(long)]
    file: Vec<String>,
    /// The project's folder, in place of the one its registry names.
    #[arg(long)]
    folder: Option<String>,
    /// Neither the checks nor the dossier's commit and push.
    #[arg(long)]
    no_push: bool,
    /// Zero or more file names of the project's reports that the step processed: the precheck does not
    /// count them as new until their date changes. Never recorded on their own.
    #[arg(long, num_args = 0.., action = ArgAction::Set)]
    reports_processed: Vec<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// `command` natively, if this module has it: its exit code, or the error to print. A command that writes
/// is recorded in the sessions' log once its arguments pass what the prototype's parser checks, a number
/// that `int()` reads and seconds that `float()` reads, and before it runs, even if it then refuses.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let record = |project: Option<&str>, task: Option<i64>| record_activity(command, project, task, catalog);
    let outcome = match command {
        "lease acquire" => parse::<LeaseAcquireArgs>(command, args).map(|args| {
            if let Some(task) = python_int(&args.task)
                && reads_as_seconds(&args.ttl)
            {
                record(Some(&args.project), Some(task));
            }
            lease_acquire(args)
        }),
        "lease heartbeat" => parse::<LeaseHeartbeatArgs>(command, args).map(|args| {
            if let Some(task) = python_int(&args.task)
                && args.ttl.as_deref().is_none_or(reads_as_seconds)
            {
                record(None, Some(task));
            }
            lease_heartbeat(args)
        }),
        "lease release" => parse::<LeaseReleaseArgs>(command, args).map(|args| {
            if let Some(task) = python_int(&args.task) {
                record(None, Some(task));
            }
            lease_release(args)
        }),
        "lease list" => parse::<LeaseListArgs>(command, args).map(|args| lease_list(&args, catalog)),
        "step start" | "step renew" => parse::<StepArgs>(command, args).map(|args| {
            if args.ttl.as_deref().is_none_or(reads_as_seconds) {
                record(Some(&args.project), None);
            }
            if command == "step start" { step_start(args) } else { step_renew(args) }
        }),
        "step finish" => parse::<StepFinishArgs>(command, args).map(|args| {
            record(Some(&args.project), None);
            step_finish(args)
        }),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
}

fn lease_acquire(args: LeaseAcquireArgs) -> Result<u8, String> {
    let task = task_number(&args.task)?;
    let (project, holder, ttl) = (project(&args.project)?, holder(&args.holder)?, seconds(&args.ttl)?);
    let done = Clock::read()?.store()?.acquire_lease(task, &project, &holder, ttl).map_err(|error| error.to_string())?;
    print(&outcome(&done, Slot::Lease), args.json)?;
    Ok(done.exit_code())
}

fn lease_heartbeat(args: LeaseHeartbeatArgs) -> Result<u8, String> {
    let (task, holder) = (task_number(&args.task)?, holder(&args.holder)?);
    let ttl = args.ttl.as_deref().map(seconds).transpose()?;
    let done = Clock::read()?.store()?.heartbeat_lease(task, &holder, ttl).map_err(|error| error.to_string())?;
    print(&outcome(&done, Slot::Lease), args.json)?;
    Ok(done.exit_code())
}

fn lease_release(args: LeaseReleaseArgs) -> Result<u8, String> {
    let (task, holder) = (task_number(&args.task)?, holder(&args.holder)?);
    let done = Clock::read()?.store()?.release_lease(task, &holder).map_err(|error| error.to_string())?;
    print(&outcome(&done, Slot::Lease), args.json)?;
    Ok(done.exit_code())
}

/// `lease list`: every lease, or the expired ones. In text, a line per lease with the contract's keys.
fn lease_list(args: &LeaseListArgs, catalog: &Catalog) -> Result<u8, String> {
    let entries = Clock::read()?.store()?.leases(args.expired).map_err(|error| error.to_string())?;
    let items: Vec<Value> = entries
        .iter()
        .map(|entry| match entry {
            LeaseEntry::Lease(lease) => Value::Object(view(lease)),
            LeaseEntry::Unreadable { task } => json!({"task": task, "expired": true, "unreadable": true, "expires_in_s": 0}),
        })
        .collect();
    if args.json {
        print_json(&Value::Array(items))?;
        return Ok(0);
    }
    if items.is_empty() {
        println!("{}", catalog.message("brief-nothing"));
    }
    for item in &items {
        let task = format!("#{}", text(item.get("task")));
        let expiry = if item["expired"] == true { "expired".to_owned() } else { format!("expires_in_s={}", item["expires_in_s"]) };
        if item.get("unreadable").is_some() {
            println!("{task:<6} unreadable {expiry}");
        } else {
            println!("{task:<6} project={} holder={} {expiry}", text(item.get("project")), text(item.get("holder")));
        }
    }
    Ok(0)
}

/// `step start`: the project's lock, taken for the step, and the step opened in the transaction that
/// checks the lock is still the caller's. Its instant is taken before the lock: the step observes from
/// when it began.
fn step_start(args: StepArgs) -> Result<u8, String> {
    let (holder, generated) = step_holder(args.holder.as_deref(), true)?;
    let (project, ttl) = (project(&args.project)?, step_ttl(args.ttl.as_deref())?);
    let store = Clock::read()?.store()?;
    let t = store.now();
    let taken = store.acquire_lock(&project, &holder, ttl).map_err(|error| error.to_string())?;
    if !taken.is_ok() {
        print(&outcome(&taken, Slot::Lock), args.json)?;
        return Ok(REFUSED);
    }
    let step = match store.open_step(&project, &holder, session().as_deref(), t).map_err(|error| error.to_string())? {
        StepOpening::Opened(step) => step,
        StepOpening::LockLost { holder: other } => {
            let refusal = match other {
                Some(other) => json!({
                    "ok": false,
                    "status": status(LockState::Foreign),
                    "reason": format!("lock of {}, not yours: it expired before the step's cursor was written", text(Some(&other))),
                }),
                None => json!({
                    "ok": false,
                    "status": status(LockState::Absent),
                    "reason": "no lock: it expired before the step's cursor was written",
                }),
            };
            print(&refusal, args.json)?;
            return Ok(REFUSED);
        }
    };
    let cursor = store.events_cursor(&project).map_err(|error| error.to_string())?;
    let lock = taken.lock();
    let out = json!({
        "ok": true,
        "project": project.as_str(),
        "status": status(taken.state()),
        "holder": holder.as_str(),
        "holder_generated": generated,
        "t": step.t(),
        "resumed": step.resumed(),
        "expires_at": lock.and_then(|lock| lock.field(ViewField::ExpiresAt)),
        "expires_in_s": lock.map(LockView::expires_in_s),
        "cursor": events(&cursor),
    });
    print(&out, args.json)?;
    Ok(0)
}

/// `step renew`: the lock of a long step, extended; it answers as `lock renew` does.
fn step_renew(args: StepArgs) -> Result<u8, String> {
    let (holder, _) = step_holder(args.holder.as_deref(), false)?;
    let (project, ttl) = (project(&args.project)?, step_ttl(args.ttl.as_deref())?);
    let renewed = Clock::read()?.store()?.renew_lock(&project, &holder, ttl).map_err(|error| error.to_string())?;
    print(&outcome(&renewed, Slot::Lock), args.json)?;
    Ok(renewed.exit_code())
}

/// `step finish`: the lock checked as the caller's, the events cursor written and the step emptied in one
/// transaction, the lock released, then the checks and the dossier, which never change the exit code.
fn step_finish(args: StepFinishArgs) -> Result<u8, String> {
    let (holder, _) = step_holder(args.holder.as_deref(), false)?;
    let project = project(&args.project)?;
    let clock = Clock::read()?;
    let store = clock.store()?;
    // The lock must be the caller's, even expired and free: renewing it checks, and refuses another's or none.
    let renewed = store.renew_lock(&project, &holder, whole(FINISH_TTL_S)).map_err(|error| error.to_string())?;
    if !renewed.is_ok() {
        print(&outcome(&renewed, Slot::Lock), args.json)?;
        return Ok(REFUSED);
    }
    let folder = ProjectFolder::new(&args.project, args.folder.as_deref());
    // Only the list given: a report that arrived during the step stays new.
    let reports = processed_reports(&args.reports_processed, &folder);
    let cursor = match store.close_step(&project, &holder, &reports).map_err(|error| error.to_string())? {
        StepClosing::Closed(cursor) => cursor,
        StepClosing::NoStep => {
            return Err(format!(
                "no step of {holder} is open for '{project}' (the lock is yours, but not taken with `step start`): cursor not written, lock not released"
            ));
        }
    };
    let released = store.release_lock(&project, &holder).map_err(|error| error.to_string())?;
    let mut out = json!({
        "ok": true,
        "project": project.as_str(),
        "holder": holder.as_str(),
        "cursor": events(&cursor),
        "lock": status(released.state()),
    });
    if !args.no_push {
        out["checks"] = json!({"unavailable": CHECKS_NOT_PORTED});
        let t = match &cursor {
            EventsCursor::Parts(parts) => parts.t.clone().unwrap_or(Value::Null),
            _ => Value::Null,
        };
        let dossier = sync(&folder, &step_commit_message(&project, &t), &args.file, clock.commit_date());
        out["dossier"] = serde_json::to_value(dossier).map_err(|error| error.to_string())?;
    }
    print(&out, args.json)?;
    Ok(0)
}

/// The dates of the reports `names` lists that exist among the project's reports now (#406), by file name,
/// each name once. A name that does not exist, or a folder that cannot be read, is a warning on stderr:
/// it is not recorded, and the exit code does not change.
fn processed_reports(names: &[String], folder: &ProjectFolder) -> BTreeMap<String, String> {
    let mut wanted: Vec<String> = Vec::new();
    for name in names {
        if let Some(name) = Path::new(name).file_name().map(|name| name.to_string_lossy().into_owned())
            && !wanted.contains(&name)
        {
            wanted.push(name);
        }
    }
    if wanted.is_empty() {
        return BTreeMap::new();
    }
    let folder = match folder.get() {
        Ok(folder) => reports_folder(Path::new(folder)),
        Err(error) => {
            eprintln!("warning: --reports-processed ignored, the project's folder was not read: {error}");
            return BTreeMap::new();
        }
    };
    let mut out = BTreeMap::new();
    for name in wanted {
        match file_date(&folder.join(&name)) {
            Some(date) => {
                out.insert(name, date);
            }
            None => eprintln!("warning: report '{name}' not found in {}: not recorded", folder.display()),
        }
    }
    out
}

/// `datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).isoformat()`: the float Python makes of the
/// file's seconds and nanoseconds, rounded to the microsecond as it rounds them, so the precheck reads back
/// the date it reads from the file.
fn file_date(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = match modified.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as f64 + f64::from(after.subsec_nanos()) * 1e-9,
        Err(before) => -before.duration().as_secs_f64(),
    };
    WallTime::from_unix_secs_f64(secs).map(WallTime::to_python_iso)
}

/// The state as the table names it (`enums.lock_status`).
fn status(state: LockState) -> &'static str {
    match state {
        LockState::Taken => "taken",
        LockState::Renewed => "renewed",
        LockState::Stolen => "stolen",
        LockState::Heartbeat => "heartbeat",
        LockState::Released => "released",
        LockState::Free => "free",
        LockState::Busy => "busy",
        LockState::Foreign => "foreign",
        LockState::Absent => "absent",
    }
}

/// What a refusal's reason names.
#[derive(Debug, Clone, Copy)]
enum Slot {
    Lock,
    Lease,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Slot::Lock => "lock",
            Slot::Lease => "lease",
        }
    }
}

/// The outcome of an operation on a lock or a lease (`lock_outcome`, `lease_outcome`): the view of what it
/// wrote, or the refusal with its reason and the document as it is.
fn outcome(outcome: &LockOutcome, slot: Slot) -> Value {
    let mut out = Map::new();
    out.insert("ok".to_owned(), outcome.is_ok().into());
    out.insert("status".to_owned(), status(outcome.state()).into());
    let name = slot.name();
    let mut refused = |current: Option<&LockView>, reason: Option<String>| {
        if let Some(current) = current {
            out.insert("current".to_owned(), Value::Object(view(current)));
        }
        if let Some(reason) = reason {
            out.insert("reason".to_owned(), reason.into());
        }
    };
    match outcome {
        LockOutcome::Taken(written) | LockOutcome::Renewed(written) | LockOutcome::Stolen(written) | LockOutcome::Heartbeat(written) => {
            out.extend(view(written));
        }
        LockOutcome::Released | LockOutcome::Free | LockOutcome::Foreign(None) => {}
        LockOutcome::Busy(current) => {
            let (holder, until) = (text(current.field(ViewField::Holder)), text(current.field(ViewField::ExpiresAt)));
            refused(Some(current), Some(format!("{name} of {holder}, valid until {until}")));
        }
        LockOutcome::Contended { current } => refused(current.as_ref(), Some(format!("{name} contended, try again"))),
        LockOutcome::Foreign(Some(current)) => {
            refused(Some(current), Some(format!("{name} of {}, not yours", text(current.field(ViewField::Holder)))));
        }
        LockOutcome::Absent => refused(None, Some(format!("no {name}"))),
    }
    Value::Object(out)
}

/// A view's fields with the table's names, and its expiry at the time of reading.
fn view(view: &LockView) -> Map<String, Value> {
    let mut out: Map<String, Value> =
        VIEW_FIELDS.iter().filter_map(|(field, name)| view.field(*field).map(|value| ((*name).to_owned(), value.clone()))).collect();
    out.insert("expired".to_owned(), view.is_expired().into());
    out.insert("expires_in_s".to_owned(), view.expires_in_s().into());
    out
}

/// The events cursor with the table's names (`events_cursor`). The old form, the instant alone, reads as
/// the precheck reads it: an instant, with no comments or reports of its own.
fn events(cursor: &EventsCursor) -> Value {
    match cursor {
        EventsCursor::Absent => Value::Null,
        EventsCursor::Instant(t) => json!({"t": t}),
        EventsCursor::Parts(parts) => {
            let mut out = Map::new();
            for (name, part) in [("v", &parts.version), ("t", &parts.t)] {
                if let Some(part) = part {
                    out.insert(name.to_owned(), part.clone());
                }
            }
            if let Some(own) = &parts.own {
                let mut fields = Map::new();
                for (name, part) in [("comments", &own.comments), ("reports", &own.reports)] {
                    if let Some(part) = part {
                        fields.insert(name.to_owned(), part.clone());
                    }
                }
                out.insert("own".to_owned(), Value::Object(fields));
            }
            Value::Object(out)
        }
        EventsCursor::Other(value) => value.clone(),
    }
}

/// A value as a reason names it: a text as it is, nothing as `-`.
fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => "-".to_owned(),
        Some(other) => other.to_string(),
    }
}

/// `value`: JSON with `--json`, else its keys, one leaf a line.
fn print(value: &Value, json: bool) -> Result<(), String> {
    if json {
        return print_json(value);
    }
    for line in flat(value) {
        println!("{line}");
    }
    Ok(())
}

fn project(id: &str) -> Result<ProjectId, String> {
    ProjectId::new(id).map_err(|error| error.to_string())
}

fn holder(id: &str) -> Result<Holder, String> {
    Holder::new(id).map_err(|error| error.to_string())
}

/// The prototype's parser reads a task's number as `int()` does; a negative one is refused after.
fn task_number(text: &str) -> Result<TaskNumber, String> {
    let number = python_int(text).ok_or_else(|| format!("argument task: invalid int value: '{text}'"))?;
    u64::try_from(number).map(TaskNumber::new).map_err(|_| format!("invalid task number: {number}"))
}

/// Whether `float()` reads `text`, with blanks around: what the prototype's parser checks of seconds.
fn reads_as_seconds(text: &str) -> bool {
    regia_core::pystr::strip(text).parse::<f64>().is_ok()
}

/// Seconds as the prototype's parser reads them, `float()` with blanks around: a float, as it writes it.
fn seconds(text: &str) -> Result<Ttl, String> {
    let secs: f64 = regia_core::pystr::strip(text).parse().map_err(|_| format!("argument --ttl: invalid float value: '{text}'"))?;
    Ttl::from_secs_f64(secs).map_err(|error| error.to_string())
}

/// A whole number of seconds, as the prototype writes its own constants.
fn whole(secs: u64) -> Ttl {
    Ttl::from_secs(secs).expect("a constant ttl is more than 0")
}

/// A step's ttl: as given, or the default, a whole number.
fn step_ttl(given: Option<&str>) -> Result<Ttl, String> {
    given.map_or_else(|| Ok(whole(STEP_TTL_S)), seconds)
}

/// The id of the session the command runs in, if it has one.
fn session() -> Option<String> {
    std::env::var(SESSION_VARIABLE).ok().filter(|id| !id.is_empty())
}

/// Who a step is for: `--holder`, else the session's id. A step that starts with neither generates one,
/// which it prints for the end to be given back; `true` then.
fn step_holder(given: Option<&str>, generate: bool) -> Result<(Holder, bool), String> {
    if let Some(given) = given.filter(|given| !given.is_empty()) {
        return Ok((holder(given)?, false));
    }
    if let Some(session) = session() {
        return Ok((holder(&session)?, false));
    }
    if generate {
        return Ok((generated_holder(), true));
    }
    Err(format!("unknown holder: {SESSION_VARIABLE} is not set; pass --holder, the id `step start` printed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args<T: Parser>(name: &str, argv: &[&str]) -> Result<T, u8> {
        parse::<T>(name, &argv.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    /// The prototype's parser: `--file` adds up, the last `--reports-processed` wins, and a list can be empty.
    #[test]
    fn the_arguments_are_the_prototype_s() {
        let finish: StepFinishArgs = args("step finish", &["--project", "a", "--file", "x", "--file", "y", "--reports-processed", "r1", "r2", "--json"]).unwrap();
        assert_eq!((finish.file.as_slice(), finish.reports_processed.as_slice(), finish.json), (&["x".to_owned(), "y".to_owned()][..], &["r1".to_owned(), "r2".to_owned()][..], true));
        let twice: StepFinishArgs = args("step finish", &["--project", "a", "--reports-processed", "r1", "--reports-processed", "r2"]).unwrap();
        assert_eq!(twice.reports_processed, ["r2"]);
        let empty: StepFinishArgs = args("step finish", &["--project", "a", "--reports-processed"]).unwrap();
        assert!(empty.reports_processed.is_empty());
        assert!(args::<StepArgs>("step start", &["--holder", "w"]).is_err(), "--project is required");
        assert!(args::<LeaseAcquireArgs>("lease acquire", &["7", "--project", "a", "--holder", "w"]).is_err(), "--ttl is required");
        assert_eq!(args::<LeaseListArgs>("lease list", &["--help"]).err(), Some(0));
    }

    /// Numbers and seconds as the prototype's parser reads them: `int()` and `float()`, with a float
    /// written as a float and the step's default as a whole number.
    #[test]
    fn numbers_and_seconds_read_as_python_reads_them() {
        assert_eq!(task_number(" 7 ").map(TaskNumber::get), Ok(7));
        assert_eq!(task_number("1_000").map(TaskNumber::get), Ok(1000));
        assert!(task_number("-7").is_err() && task_number("x").is_err() && task_number("7.0").is_err());
        assert_eq!(seconds(" 900 ").map(Ttl::as_secs_f64), Ok(900.0));
        assert!(seconds("0").is_err() && seconds("-1").is_err() && seconds("inf").is_err() && seconds("nan").is_err() && seconds("x").is_err());
        assert_eq!(step_ttl(None).map(Ttl::as_secs_f64), Ok(1800.0));
    }

    #[test]
    fn the_events_cursor_has_the_table_s_names() {
        assert_eq!(events(&EventsCursor::Absent), Value::Null);
        assert_eq!(events(&EventsCursor::Instant("2026-01-09T18:00:00Z".to_owned())), json!({"t": "2026-01-09T18:00:00Z"}));
        let parts = regia_store::EventsParts {
            version: Some(json!(1)),
            t: Some(json!("x")),
            own: Some(regia_store::OwnParts { comments: Some(json!([1])), reports: None }),
        };
        assert_eq!(events(&EventsCursor::Parts(parts)), json!({"v": 1, "t": "x", "own": {"comments": [1]}}));
    }
}
