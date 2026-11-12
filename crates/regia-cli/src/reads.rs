//! The reads of the tracker, natively: `projects`, `task show` and `task list`, with the arguments of the
//! facade table (`table/project.toml`, `table/task.toml`). They are not subcommands of the CLI's parser: while
//! their group runs on the prototype, the facade takes their argv as it is. The JSON is the contract; the
//! text is not (ADR 0001 §11), and holds only the contract's keys and the catalog's words.

use clap::Parser;
use regia_core::{
    GraphOrder, IssueState, ListOptions, ListedTask, Registry, ShownTask, Status, Task, Tracker, TrackerBinding, TrackerConfigV1,
    UNSET_STATUS, comments, parse_instant, project_entry, task_list,
};
use regia_github::GhTracker;
use regia_i18n::Catalog;
use regia_identity::state_dir;
use regia_runtime::dossier_dir;
use serde_json::Value;

use crate::print_json;

#[derive(Parser)]
#[command(name = "projects", args_override_self = true)]
struct ProjectsArgs {
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "task show", args_override_self = true)]
struct TaskShowArgs {
    /// The issue's number.
    number: String,
    /// Without the body, in text.
    #[arg(long)]
    short: bool,
    /// With the issue's comments, oldest first.
    #[arg(long)]
    comments: bool,
    /// Only the comments from this ISO 8601 instant on; alone it implies `--comments`.
    #[arg(long)]
    since: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "task list", args_override_self = true)]
struct TaskListArgs {
    #[arg(long)]
    project: Option<String>,
    /// Statuses separated by commas: `approved,in_progress`.
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    ready: bool,
    #[arg(long)]
    waiting_on_owner: bool,
    #[arg(long)]
    objectives: bool,
    #[arg(long)]
    include_closed: bool,
    #[arg(long, value_parser = ["graph"])]
    order: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// The arguments, or the exit of a wrong one: 1, as the prototype's parser exits, and 0 for help.
fn parse<T: Parser>(name: &str, args: &[String]) -> Result<T, u8> {
    T::try_parse_from(std::iter::once(name).chain(args.iter().map(String::as_str))).map_err(|error| {
        let _ = error.print();
        let shown = matches!(error.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion);
        if shown { 0 } else { 1 }
    })
}

/// `command` natively, if this module has it: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let outcome = match command {
        "projects" => parse::<ProjectsArgs>(command, args).map(projects),
        "task show" => parse::<TaskShowArgs>(command, args).map(task_show),
        "task list" => parse::<TaskListArgs>(command, args).map(|args| task_list_command(&args, catalog)),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
}

/// The config, its binding and the tracker it names.
fn tracker() -> Result<(TrackerConfigV1, TrackerBinding, GhTracker), String> {
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref()).map_err(|error| error.to_string())?;
    let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
    let tracker = GhTracker::new(&config);
    Ok((config, binding, tracker))
}

/// `projects`: one entry per open Objective, with its registry. Only the Objectives' statuses are read, so
/// an option the config does not bind on another issue does not stop the list.
fn projects(args: ProjectsArgs) -> Result<u8, String> {
    let (config, binding, tracker) = tracker()?;
    let mut entries = Vec::new();
    for mut issue in tracker.open_issues().map_err(|error| error.to_string())? {
        let body = std::mem::take(&mut issue.body);
        let option = issue.status_option_id.take();
        let mut task = binding.resolve(issue).map_err(|error| error.to_string())?;
        if !task.objective {
            continue;
        }
        task.status = binding.status(task.number, option.as_deref()).map_err(|error| error.to_string())?;
        let entry = project_entry(&task, &Registry::parse(&body), &config.owner).map_err(|error| error.to_string())?;
        entries.push(Value::Object(entry));
    }
    if args.json {
        print_json(&Value::Array(entries))?;
        return Ok(0);
    }
    let text = |value: &Value| match value {
        Value::Null => "-".to_owned(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    for entry in &entries {
        let products = entry["products"].as_array().map_or(0, Vec::len);
        println!(
            "{:<20} #{:<4} {}  [{}]  folder={}  repo={}  products={products}",
            text(&entry["id"]),
            text(&entry["objective"]),
            text(&entry["title"]),
            text(&entry["status"]),
            entry.get("folder").map_or_else(|| "?".to_owned(), text),
            text(&entry["repo"]),
        );
    }
    Ok(0)
}

/// An argument read as Python's `int()` reads it, which the prototype's parser applies to a number:
/// blanks around it, a sign, and `_` between digits.
fn python_int(text: &str) -> Option<i64> {
    let text = regia_core::pystr::strip(text);
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
    if !well_formed {
        return None;
    }
    let value: i64 = digits.replace('_', "").parse().ok()?;
    Some(if negative { -value } else { value })
}

/// `task show`: the issue in any state, with its body, and with `--comments` or `--since` its comments. Only
/// its own status is read, so an option the config does not bind on another issue does not hide it.
fn task_show(args: TaskShowArgs) -> Result<u8, String> {
    let number = python_int(&args.number).ok_or_else(|| format!("argument number: invalid int value: '{}'", args.number))?;
    let (_, binding, tracker) = tracker()?;
    let issues = tracker.all_issues().map_err(|error| error.to_string())?;
    let mut issue = issues.into_iter().find(|issue| i64::try_from(issue.number).is_ok_and(|n| n == number)).ok_or_else(|| format!("#{number} not found"))?;
    let body = std::mem::take(&mut issue.body);
    let option = issue.status_option_id.take();
    let mut task = binding.resolve(issue).map_err(|error| error.to_string())?;
    task.status = binding.status(task.number, option.as_deref()).map_err(|error| error.to_string())?;
    let since = args.since.as_deref().filter(|since| !since.is_empty());
    let with_comments = args.comments || args.since.is_some();
    let comments = if with_comments {
        let since = since.map(parse_instant).transpose().map_err(|error| error.to_string())?;
        let raw = tracker.comments(task.number).map_err(|error| error.to_string())?.ok_or_else(|| format!("issue #{number} not found"))?;
        Some(comments(raw, since).map_err(|error| error.to_string())?)
    } else {
        None
    };
    let shown = ShownTask { task, body, comments };
    if args.json {
        print_json(&serde_json::to_value(&shown).map_err(|error| error.to_string())?)?;
        return Ok(0);
    }
    let task = &shown.task;
    println!("{}", task_line(task));
    println!("project: {}  ·  state: {}  ·  {}", task.project.as_deref().unwrap_or("-"), state(task.state), task.url);
    let pairs = |list: &[(u64, IssueState)]| list.iter().map(|(n, s)| format!("#{n} ({})", state(*s))).collect::<Vec<_>>().join(", ");
    if !task.sub_issues.is_empty() {
        println!("sub_issues: {}", pairs(&task.sub_issues));
    }
    if !task.blocked_by.is_empty() {
        println!("blocked_by: {}", pairs(&task.blocked_by));
    }
    if !args.short {
        println!("\n{}", regia_core::pystr::strip(&shown.body));
    }
    if let Some(comments) = &shown.comments {
        let after = since.map(|since| format!(" since {since}")).unwrap_or_default();
        println!("\ncomments{after}: {}", comments.len());
        for comment in comments {
            println!("\n[{} · {}] {}", comment.created_at, comment.role, comment.url);
            println!("{}{}", comment.body, if comment.shortened { "  (…)" } else { "" });
        }
    }
    Ok(0)
}

/// `--status`: canonical ids separated by commas, each trimmed, as the facade reads them. The key of the
/// tasks with no status is a value too, which keeps none of them, as the prototype's filter does.
fn statuses(list: &str) -> Result<Vec<Status>, String> {
    let mut out = Vec::new();
    for item in list.split(',').map(str::trim) {
        match Status::parse(item) {
            Some(status) => out.push(status),
            None if item == UNSET_STATUS => {}
            None => return Err(format!("`{item}` is not a status")),
        }
    }
    Ok(out)
}

/// `task list`: every open issue, or every one with `--include-closed`, in scope, ordered as asked.
fn task_list_command(args: &TaskListArgs, catalog: &Catalog) -> Result<u8, String> {
    let options = ListOptions {
        project: args.project.clone(),
        statuses: args.status.as_deref().map(statuses).transpose()?,
        ready: args.ready,
        waiting_on_owner: args.waiting_on_owner,
        objectives: args.objectives,
        by_graph: args.order.is_some(),
    };
    let (_, binding, tracker) = tracker()?;
    let issues = if args.include_closed { tracker.all_issues() } else { tracker.open_issues() };
    let tasks: Vec<Task> = issues
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|issue| binding.resolve(issue).map_err(|error| error.to_string()))
        .collect::<Result<_, _>>()?;
    let (listed, graph) = task_list(tasks, &options);
    if args.json {
        print_json(&serde_json::to_value(&listed).map_err(|error| error.to_string())?)?;
        return Ok(0);
    }
    warn_about(&graph, catalog);
    let by_graph = options.ready || options.by_graph;
    for entry in &listed {
        println!("{}{}", task_line(&entry.task), if by_graph { graph_suffix(entry) } else { String::new() });
    }
    if listed.is_empty() {
        println!("{}", catalog.message("brief-nothing"));
    }
    Ok(0)
}

fn state(state: IssueState) -> &'static str {
    match state {
        IssueState::Open => "open",
        IssueState::Closed => "closed",
    }
}

/// `#<n> [<status>] <title>`, then its maturity, its open blockers and its parent.
fn task_line(task: &Task) -> String {
    let mut flags = Vec::new();
    if let Some(maturity) = task.maturity {
        flags.push(maturity.id().to_owned());
    }
    let open: Vec<String> = task.blocked_by.iter().filter(|(_, state)| *state == IssueState::Open).map(|(n, _)| format!("#{n}")).collect();
    if !open.is_empty() {
        flags.push(format!("blocked_by {}", open.join(",")));
    }
    if let Some(parent) = task.parent {
        flags.push(format!("parent #{parent}"));
    }
    let extra = if flags.is_empty() { String::new() } else { format!("  ({})", flags.join("; ")) };
    format!("#{:<4} [{}] {}{extra}", task.number, task.status.map_or("-", Status::id), task.title)
}

/// The graph's measures after a line: nothing for a task that unblocks nothing.
fn graph_suffix(entry: &ListedTask) -> String {
    let metrics = &entry.metrics;
    if metrics.cycle {
        return "  · cycle".to_owned();
    }
    let unblocks = metrics.unblocks.unwrap_or(0);
    let total = metrics.unblocks_total.filter(|total| *total != 0).unwrap_or(unblocks);
    if total == 0 {
        return String::new();
    }
    let with = if total == unblocks { String::new() } else { format!(" (unblocks_total {total})") };
    format!("  · unblocks {unblocks}{with} · chain {}", metrics.chain.unwrap_or(0))
}

/// The graph's warnings, on stderr, as the prototype prints them beside its text.
fn warn_about(graph: &GraphOrder, catalog: &Catalog) {
    let numbers = |list: &[u64]| list.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ");
    for cycle in &graph.cycles {
        eprintln!("  ! {}", catalog.message_with("brief-cycle", "reason", &numbers(cycle)));
    }
    if !graph.truncated.is_empty() {
        eprintln!("  ! {}", catalog.message_with("brief-truncated", "reason", &numbers(&graph.truncated)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_reads_as_python_s_int() {
        for (text, value) in [("5", Some(5)), (" 7 ", Some(7)), ("+3", Some(3)), ("-2", Some(-2)), ("1_0", Some(10)), ("05", Some(5))] {
            assert_eq!(python_int(text), value, "{text}");
        }
        for wrong in ["", "x", "1__0", "_1", "1_", "0x5", "5.0", "--5"] {
            assert_eq!(python_int(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn statuses_read_as_the_facade_reads_them() {
        assert_eq!(statuses("approved, in_progress").unwrap(), [Status::Approved, Status::InProgress]);
        assert_eq!(statuses(UNSET_STATUS).unwrap(), [], "a value that keeps nothing");
        for wrong in ["", "approved,", "Approved", "approved;done"] {
            assert!(statuses(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_wrong_argument_exits_1_and_help_0() {
        let run = |command: &str, argv: &[&str]| -> Option<u8> {
            let args: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
            let catalog = Catalog::new(regia_i18n::Language::En);
            match super::run(command, &args, &catalog)? {
                Ok(code) => Some(code),
                Err(_) => Some(1),
            }
        };
        assert_eq!(run("task list", &["--order", "chain"]), Some(1));
        assert_eq!(run("task list", &["--status", "nope"]), Some(1));
        assert_eq!(run("task show", &[]), Some(1), "the number is required");
        assert_eq!(run("task show", &["five"]), Some(1));
        assert_eq!(run("projects", &["--project", "x"]), Some(1));
        assert_eq!(run("projects", &["--help"]), Some(0));
        assert_eq!(run("brief", &[]), None, "not a command of this module");
    }
}
