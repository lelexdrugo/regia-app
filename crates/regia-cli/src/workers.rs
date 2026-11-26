//! `workers`, natively (#744): the prototype's `tools/worker_orca.py --task N [N…] [--json] [--dispatch]` (#543,
//! #641, #644, #698; F277 of the dossier's inventory). For each task asked, every dispatch in progress of Orca whose
//! task's title names it, whatever the tracker says of the task: minutes since its worker's last line, whether it
//! waits for a person, whether it is past the silence's threshold, why it is blocked past the block's, and why its
//! session stopped it. With more than one dispatch the task's own line is the one of the dispatch that wrote last, with
//! the count, and a line follows for each. Read only: Orca's list, the tasks of its runs, transcripts and
//! `worker-show`. Native only, as `policy`: the prototype has it in a script of its own, which the facade does not
//! start. The JSON is the contract, with provisional names (#223); the text is the catalog's, a line for each of the
//! prototype's, with a block's case in the contract's word. Without `--dispatch` the output holds no id of Orca. The
//! exits are the prototype's: 0 when Orca answered, also for a task with no worker; 1, with one line on stderr, when
//! it did not; 2 for wrong arguments.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use regia_agents::{
    Blocked, DispatchState, OrcaWorkers, SessionCause, TaskDispatches, WAITING_ACTIVITIES,
    WorkerThresholds,
};
use regia_i18n::Catalog;
use regia_runtime::dossier_dir;
use regia_store::WallTime;
use serde_json::{Value, json};

use crate::clock::Clock;
use crate::print_json;

/// How long the command reads at most: wider than the precheck's, for it may page long transcripts (the prototype's
/// `SCADENZA_CLI_S`).
const DEADLINE: Duration = Duration::from_secs(120);

/// The exit when Orca does not answer, or cannot be found: the prototype's.
const UNREACHABLE: u8 = 1;

#[derive(clap::Args)]
pub(crate) struct WorkersArgs {
    /// The numbers of the tasks, #N of the tracker. A number given twice is read once; given twice, the option keeps its
    /// last numbers, as the prototype's.
    #[arg(long, required = true, num_args = 1.., value_name = "N", action = clap::ArgAction::Set)]
    task: Vec<u64>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
    /// Adds the ids of Orca's dispatch and run of each worker, which `worker-stop --dispatch` and `worker-show` ask
    /// for. Without it the output holds no id of Orca: they do not belong in the documents of the state.
    #[arg(long)]
    dispatch: bool,
}

/// `workers`, at the clock of this process, with the thresholds of the prototype's `policies/worker.yaml` while the
/// prototype exists, as the precheck reads them, and Orca as setup finds it.
pub(crate) fn run(args: &WorkersArgs, catalog: &Catalog) -> Result<ExitCode, String> {
    let now = Clock::read()?.wall()?;
    let text = dossier_dir()
        .and_then(|dossier| std::fs::read_to_string(dossier.join(WorkerThresholds::FILE)).ok());
    let thresholds = WorkerThresholds::from_policy(text.as_deref());
    let mut tasks: Vec<u64> = Vec::new();
    for task in &args.task {
        if !tasks.contains(task) {
            tasks.push(*task);
        }
    }
    let deadline = Instant::now() + DEADLINE;
    let found = match OrcaWorkers::new().read_dispatches(&tasks, thresholds, now, deadline) {
        Ok(found) => found,
        Err(reason) => {
            eprintln!(
                "{}",
                catalog.message_with("workers-unreachable", "reason", &reason)
            );
            return Ok(ExitCode::from(UNREACHABLE));
        }
    };
    let view = View {
        minutes: Minutes::of(thresholds),
        ids: args.dispatch,
    };
    let entries: Vec<Entry<'_>> = found
        .iter()
        .map(|task| Entry::of(task, now, thresholds))
        .collect();
    if args.json {
        print_json(&view.json(&entries))?;
    } else {
        for line in entries.iter().flat_map(|entry| view.lines(entry, catalog)) {
            println!("{line}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The thresholds in whole minutes, as the output names them.
#[derive(Clone, Copy)]
struct Minutes {
    silence: u64,
    block: u64,
}

impl Minutes {
    fn of(thresholds: WorkerThresholds) -> Minutes {
        Minutes {
            silence: (thresholds.silence_s / 60.0) as u64,
            block: (thresholds.block_s / 60.0) as u64,
        }
    }
}

/// What a dispatch that reads says at the clock: when it last wrote, the whole minutes since then, and whether that is
/// past the silence's threshold, with what it read.
#[derive(Clone, Copy)]
struct Facts {
    last: WallTime,
    minutes: u64,
    waiting: bool,
    silent: bool,
    blocked: Option<Blocked>,
    cause: Option<SessionCause>,
}

/// A dispatch as the command shows it: its place in its task's list, and its facts when it reads.
struct Shown<'a> {
    n: usize,
    state: &'a DispatchState,
    facts: Option<Facts>,
}

/// A task asked, with its dispatches.
struct Entry<'a> {
    task: u64,
    dispatches: Vec<Shown<'a>>,
}

impl<'a> Entry<'a> {
    fn of(task: &'a TaskDispatches, now: WallTime, thresholds: WorkerThresholds) -> Entry<'a> {
        let dispatches = task
            .dispatches
            .iter()
            .enumerate()
            .map(|(at, state)| {
                let facts = state.read.as_ref().map(|read| {
                    // A last line after the clock is a last line now, as the prototype's `max(0.0, …)`.
                    let micros =
                        u64::try_from(now.unix_micros() - read.last.unix_micros()).unwrap_or(0);
                    Facts {
                        last: read.last,
                        minutes: micros / 60_000_000,
                        waiting: read.waiting,
                        silent: micros as f64 / 1e6 > thresholds.silence_s,
                        blocked: read.blocked,
                        cause: read.cause,
                    }
                });
                Shown {
                    n: at + 1,
                    state,
                    facts,
                }
            })
            .collect();
        Entry {
            task: task.task,
            dispatches,
        }
    }

    /// The facts of the task's own line: those of the dispatch that wrote last, the first of the list among those that
    /// wrote at the same instant, as the prototype's `max` keeps it. `None` when no dispatch reads.
    fn head(&self) -> Option<&Facts> {
        let mut head: Option<&Facts> = None;
        for facts in self
            .dispatches
            .iter()
            .filter_map(|shown| shown.facts.as_ref())
        {
            if head.is_none_or(|head| facts.last > head.last) {
                head = Some(facts);
            }
        }
        head
    }

    /// `read` when a dispatch reads, `unreadable` when there are some and none reads, `none` without one.
    fn worker(&self) -> &'static str {
        match (self.dispatches.is_empty(), self.head()) {
            (true, _) => "none",
            (false, None) => "unreadable",
            (false, Some(_)) => "read",
        }
    }
}

/// A block's case in the contract's word, in the JSON and in the text of every language.
fn blocked_word(blocked: Blocked) -> &'static str {
    match blocked {
        Blocked::OpenCall => "open-call",
        Blocked::WaitingForPerson => "waiting-for-person",
    }
}

/// A session's cause in the contract's word.
fn cause_word(cause: SessionCause) -> &'static str {
    match cause {
        SessionCause::Limit => "session-limit",
        SessionCause::Billing => "billing",
    }
}

/// How the command prints: the thresholds' minutes, and whether Orca's ids are shown.
struct View {
    minutes: Minutes,
    ids: bool,
}

impl View {
    /// The contract: the thresholds, and an entry for each task asked, with its dispatches.
    fn json(&self, entries: &[Entry<'_>]) -> Value {
        let tasks: Vec<Value> = entries
            .iter()
            .map(|entry| {
                let head = entry.head();
                let dispatches: Vec<Value> = entry
                    .dispatches
                    .iter()
                    .map(|shown| self.dispatch_json(shown))
                    .collect();
                json!({
                    "task": entry.task,
                    "worker": entry.worker(),
                    "in_progress": entry.dispatches.len(),
                    "minutes": head.map(|facts| facts.minutes),
                    "waiting": head.map(|facts| facts.waiting),
                    "silent": head.map(|facts| facts.silent),
                    "blocked": head.and_then(|facts| facts.blocked).map(blocked_word),
                    "cause": head.and_then(|facts| facts.cause).map(cause_word),
                    "dispatches": dispatches,
                })
            })
            .collect();
        json!({"silence_minutes": self.minutes.silence, "block_minutes": self.minutes.block, "tasks": tasks})
    }

    fn dispatch_json(&self, shown: &Shown<'_>) -> Value {
        let facts = shown.facts.as_ref();
        let mut value = json!({
            "n": shown.n,
            "readable": facts.is_some(),
            "minutes": facts.map(|facts| facts.minutes),
            "waiting": facts.map(|facts| facts.waiting),
            "activity": shown.state.activity,
            "silent": facts.map(|facts| facts.silent),
            "blocked": facts.and_then(|facts| facts.blocked).map(blocked_word),
            "cause": facts.and_then(|facts| facts.cause).map(cause_word),
        });
        if self.ids {
            value["dispatch_id"] = json!(shown.state.dispatch_id);
            value["run_id"] = json!(shown.state.run_id);
        }
        value
    }

    /// The text of a task, as the prototype's `_righe_task`: its line; with more than one dispatch, how many and how
    /// many are blocked, then a line for each.
    fn lines(&self, entry: &Entry<'_>, catalog: &Catalog) -> Vec<String> {
        let head = match entry.head() {
            Some(facts) => self.body(facts, catalog),
            None if entry.dispatches.is_empty() => catalog.message("workers-none"),
            None => catalog.message("workers-unreadable"),
        };
        let mut first = format!("#{}: {head}", entry.task);
        match entry.dispatches.as_slice() {
            [] => vec![first],
            [only] => vec![format!(
                "{first}{}{}",
                activity(only.state, catalog),
                self.ids_of(only.state)
            )],
            all => {
                let total = all.len().to_string();
                first.push_str("; ");
                first.push_str(&catalog.message_with("workers-in-progress", "count", &total));
                let blocked = all
                    .iter()
                    .filter(|shown| shown.facts.is_some_and(|facts| facts.blocked.is_some()))
                    .count();
                if blocked > 0 {
                    let count = catalog.message_with(
                        "workers-blocked-count",
                        "count",
                        &blocked.to_string(),
                    );
                    first.push_str(&format!(" ({count})"));
                }
                let mut lines = vec![first];
                for shown in all {
                    let text = match &shown.facts {
                        Some(facts) => format!(
                            "{}{}",
                            self.body(facts, catalog),
                            activity(shown.state, catalog)
                        ),
                        None => catalog.message("workers-dispatch-unreadable"),
                    };
                    let place = catalog.message_with(
                        "workers-dispatch",
                        "position",
                        &format!("{}/{total}", shown.n),
                    );
                    lines.push(format!("    {place}: {text}{}", self.ids_of(shown.state)));
                }
                lines
            }
        }
    }

    /// What a line says of a dispatch that reads, after its task: the prototype's `_riga`. A cause of the session
    /// follows the block when there is one, and takes the place of «only silent» when there is not.
    fn body(&self, facts: &Facts, catalog: &Catalog) -> String {
        let (silence, block) = (
            self.minutes.silence.to_string(),
            self.minutes.block.to_string(),
        );
        let last = catalog.message_with("workers-last-line", "minutes", &facts.minutes.to_string());
        let waiting = catalog.message(if facts.waiting {
            "workers-waiting"
        } else {
            "workers-not-waiting"
        });
        let threshold = if facts.silent {
            "workers-past-threshold"
        } else {
            "workers-within-threshold"
        };
        let threshold = catalog.message_with(threshold, "minutes", &silence);
        let cause = facts
            .cause
            .map(|cause| match cause {
                SessionCause::Limit => "workers-cause-limit",
                SessionCause::Billing => "workers-cause-billing",
            })
            .map(|id| format!("; {}", catalog.message(id)))
            .unwrap_or_default();
        let tail = match facts.blocked {
            Some(blocked) => {
                let past = catalog.message_with("workers-past", "minutes", &block);
                let detail = format!("{}, {past}", blocked_word(blocked));
                format!(
                    "; {}{cause}",
                    catalog.message_with("workers-blocked", "detail", &detail)
                )
            }
            None if !cause.is_empty() => cause,
            None if facts.minutes >= self.minutes.block => {
                format!(
                    "; {}",
                    catalog.message_with("workers-only-silent", "minutes", &block)
                )
            }
            None => String::new(),
        };
        format!("{last}; {waiting}; {threshold}{tail}")
    }

    /// Orca's ids at the end of a dispatch's line, with `--dispatch` only.
    fn ids_of(&self, state: &DispatchState) -> String {
        if !self.ids {
            return String::new();
        }
        format!(
            "  [dispatch={} run={}]",
            state.dispatch_id,
            state.run_id.as_deref().unwrap_or("-")
        )
    }
}

/// The note of Orca's activity, when it is a wait for a person.
fn activity(state: &DispatchState, catalog: &Catalog) -> String {
    match state.activity.as_deref() {
        Some(activity) if WAITING_ACTIVITIES.contains(&activity) => {
            format!(
                " {}",
                catalog.message_with("workers-activity", "activity", activity)
            )
        }
        _ => String::new(),
    }
}
