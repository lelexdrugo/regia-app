//! The writes of the plan, natively: `task status` and the other commands that change a task, with the
//! arguments of the facade table (`table/task.toml`). Not subcommands of the CLI's parser: while their
//! group runs on the prototype, the facade takes their argv as it is.
//!
//! Each command calls the tracker as the prototype does, reads included and in the same order: the
//! conformance tests compare the calls one by one, and the order of the writes is part of the behaviour.
//! The prototype reads an issue's node before each write that names it, and so does this module. Each
//! command records its line in the sessions' log after the checks of the prototype's parser, and before
//! its first call.

use std::time::Duration;

use clap::Parser;
use clap::builder::PossibleValuesParser;
use regia_core::{
    IssueState, Maturity, SignatureRole, Status, Task, Tracker, TrackerBinding, TrackerConfigV1, TrackerError, comment_id,
    created_issue_number, project_of_labels, sign,
};
use regia_github::GhTracker;
use regia_i18n::Catalog;
use regia_identity::state_dir;
use regia_runtime::dossier_dir;
use serde_json::json;

use crate::clock::Clock;
use crate::comment::{is_this_session_s_step, record_own, session};
use crate::declare::record_activity;
use crate::print_json;
use crate::reads::{parse, python_int};

/// How long a closing status waits before it is written a second time (T11 of the dossier's conformance
/// study): the Project's workflows can rewrite the status when the item closes.
const REWRITE_AFTER: Duration = Duration::from_secs(3);

/// The room a signature takes after a body, its session's id included: a body written in argv must fit
/// with it.
const SIGNATURE_ROOM: usize = 1024;

#[derive(Parser)]
#[command(name = "task status", args_override_self = true)]
struct StatusArgs {
    /// The issue's number.
    number: String,
    #[arg(value_parser = PossibleValuesParser::new(Status::ALL.map(Status::id)))]
    status: String,
    /// A comment written after the change, signed as `comment` signs it.
    #[arg(long)]
    comment: Option<String>,
    /// The comment relays a decision of the owner.
    #[arg(long)]
    on_behalf_of_owner: bool,
    /// Who writes the comment, when it is not the orchestrator of the step open on the issue's project.
    #[arg(long, value_parser = PossibleValuesParser::new(SignatureRole::BY_HAND.map(SignatureRole::word)))]
    role: Option<String>,
    /// Accepted and ignored: the command prints a line of text, as the prototype does.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "task unblock", args_override_self = true)]
struct UnblockArgs {
    /// The issue's number.
    number: String,
    /// The blockers to remove: numbers separated by commas, `#` allowed.
    #[arg(long)]
    by: String,
    /// Why: it becomes a signed comment on the issue.
    #[arg(long)]
    reason: String,
    /// The comment relays a decision of the owner.
    #[arg(long)]
    on_behalf_of_owner: bool,
    #[arg(long, value_parser = PossibleValuesParser::new(SignatureRole::BY_HAND.map(SignatureRole::word)))]
    role: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "task new", args_override_self = true)]
struct NewArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    title: String,
    #[arg(long, default_value = "")]
    body: String,
    /// A UTF-8 file, read in place of `--body`.
    #[arg(long)]
    body_file: Option<String>,
    /// Without it: the project's Objective.
    #[arg(long)]
    parent: Option<String>,
    /// Numbers separated by commas, `#` allowed.
    #[arg(long)]
    blocked_by: Option<String>,
    #[arg(long, default_value = "draft", value_parser = PossibleValuesParser::new(Maturity::ALL.map(Maturity::id)))]
    maturity: String,
    #[arg(long, default_value = "inbox", value_parser = PossibleValuesParser::new(Status::ALL.map(Status::id)))]
    status: String,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "task maturity", args_override_self = true)]
struct MaturityArgs {
    /// The issue's number.
    number: String,
    #[arg(value_parser = PossibleValuesParser::new(Maturity::ALL.map(Maturity::id)))]
    maturity: String,
    /// Accepted and ignored: the command prints a line of text, as the prototype does.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "task block", args_override_self = true)]
struct BlockArgs {
    /// The issue's number.
    number: String,
    /// The blockers to add: numbers separated by commas, `#` allowed.
    #[arg(long)]
    by: String,
    /// Accepted and ignored: the command prints a line of text, as the prototype does.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "task parent", args_override_self = true)]
struct ParentArgs {
    /// The issue's number.
    number: String,
    /// The number of its new parent.
    parent: String,
    /// Accepted and ignored: the command prints a line of text, as the prototype does.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "task move", args_override_self = true)]
struct MoveArgs {
    /// The issue's number.
    number: String,
    #[arg(long)]
    project: String,
    /// Without it: the project's Objective.
    #[arg(long)]
    parent: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// `command` natively, if this module has it: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let outcome = match command {
        "task status" => parse::<StatusArgs>(command, args).map(|args| status(&args, catalog)),
        "task unblock" => parse::<UnblockArgs>(command, args).map(|args| unblock(&args, catalog)),
        "task new" => parse::<NewArgs>(command, args).map(|args| new(&args, catalog)),
        "task maturity" => parse::<MaturityArgs>(command, args).map(|args| maturity(&args, catalog)),
        "task block" => parse::<BlockArgs>(command, args).map(|args| block(&args, catalog)),
        "task parent" => parse::<ParentArgs>(command, args).map(|args| parent(&args, catalog)),
        "task move" => parse::<MoveArgs>(command, args).map(|args| move_task(&args, catalog)),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
}

/// `task maturity`: one edit of the labels, the new maturity added and the other one taken off; nothing
/// is read.
fn maturity(args: &MaturityArgs, catalog: &Catalog) -> Result<u8, String> {
    let number = number_argument("number", &args.number)?;
    let maturity = Maturity::parse(&args.maturity).ok_or_else(|| format!("unknown maturity `{}`", args.maturity))?;
    let (binding, tracker) = tracker()?;
    record_activity("task maturity", None, Some(number), catalog);
    let number = issue_number(number)?;
    let label = binding.maturity_label(maturity);
    tracker.edit_labels(number, label, Some(binding.maturity_label(maturity.other()))).map_err(tracker_error)?;
    println!("#{number} {}", catalog.message_with("task-maturity-set", "label", label));
    Ok(0)
}

/// `task block`: each blocker of the list added, in its order; the line names the list as it was given.
fn block(args: &BlockArgs, catalog: &Catalog) -> Result<u8, String> {
    let number = number_argument("number", &args.number)?;
    let (_, tracker) = tracker()?;
    record_activity("task block", None, Some(number), catalog);
    let blockers = numbers(&args.by).ok_or_else(|| format!("--by takes numbers separated by commas: '{}'", args.by))?;
    let number = issue_number(number)?;
    for blocker in blockers {
        add_blocker(&tracker, number, issue_number(blocker)?)?;
    }
    println!("#{number} {}", catalog.message_with("task-blocked", "by", &args.by));
    Ok(0)
}

/// `task parent`: nothing to do if the issue, read among every issue, is a sub-issue of that parent
/// already, which GitHub would refuse; else the parent's cap, then the link.
fn parent(args: &ParentArgs, catalog: &Catalog) -> Result<u8, String> {
    let number = number_argument("number", &args.number)?;
    let parent = number_argument("parent", &args.parent)?;
    let (_, tracker) = tracker()?;
    record_activity("task parent", None, Some(number), catalog);
    let (number, parent) = (issue_number(number)?, issue_number(parent)?);
    let issue = tracker.all_issues().map_err(tracker_error)?.into_iter().find(|issue| issue.number == number);
    let current = issue.as_ref().and_then(|issue| issue.parent);
    if issue.is_some() && current == Some(parent) {
        println!("#{number} {}", catalog.message_with("task-parent-already", "parent", &parent.to_string()));
        return Ok(0);
    }
    check_parent_cap(&tracker, parent, current)?;
    link_sub_issue(&tracker, parent, number, false)?;
    println!("#{number} {}", catalog.message_with("task-parent-set", "parent", &parent.to_string()));
    Ok(0)
}

/// `task move`: an open task, not an Objective, to another project: its project label changes, then its
/// parent, the project's first Objective or `--parent`, unless it is that already.
fn move_task(args: &MoveArgs, catalog: &Catalog) -> Result<u8, String> {
    let number = number_argument("number", &args.number)?;
    let parent = args.parent.as_deref().map(|parent| number_argument("--parent", parent)).transpose()?;
    let (binding, tracker) = tracker()?;
    record_activity("task move", Some(&args.project), Some(number), catalog);
    let number = issue_number(number)?;
    let tasks = open_tasks(&tracker, &binding)?;
    let task = tasks.iter().find(|task| task.number == number).ok_or_else(|| format!("#{number} not found among the open issues"))?;
    if task.objective {
        return Err(format!("#{number} is an Objective: it does not move"));
    }
    let project = args.project.as_str();
    let objective = tasks
        .iter()
        .find(|task| task.objective && task.project.as_deref() == Some(project))
        .ok_or_else(|| format!("unknown project '{project}': see `projects`"))?;
    // A parent of 0 is none, as for the prototype.
    let parent = match parent.filter(|parent| *parent != 0) {
        Some(parent) => issue_number(parent)?,
        None => objective.number,
    };
    let old = task.project.clone();
    check_parent_cap(&tracker, parent, task.parent)?;
    let remove = old.as_deref().filter(|old| !old.is_empty() && *old != project).map(|old| binding.project_label(old));
    tracker.edit_labels(number, &binding.project_label(project), remove.as_deref()).map_err(tracker_error)?;
    if task.parent != Some(parent) {
        link_sub_issue(&tracker, parent, number, true)?;
    }
    project_field_not_written(&tracker, &binding, number, project, catalog);
    if args.json {
        print_json(&json!({"task": number, "from": old, "to": project, "parent": parent}))?;
    } else {
        let from = catalog.message_with("task-moved-from", "project", old.as_deref().unwrap_or("-"));
        let under = catalog.message_with("task-moved-parent", "parent", &parent.to_string());
        println!("#{number}: {from} → {project}, {under}");
    }
    Ok(0)
}

/// A list of numbers as the prototype reads one: `#` dropped, split on commas, blank pieces skipped, each
/// piece read by `int()`; `None` if a piece is not a number.
fn numbers(text: &str) -> Option<Vec<i64>> {
    text.replace('#', "").split(',').filter(|piece| !regia_core::pystr::strip(piece).is_empty()).map(python_int).collect()
}

fn joined(numbers: &[i64]) -> String {
    numbers.iter().map(|number| format!("#{number}")).collect::<Vec<_>>().join(", ")
}

/// `task unblock`: each blocker of the list that blocks the issue is removed, in the list's order; then one
/// comment with the reason, signed, for those removed. A removal that fails stops the list: the ones
/// already removed still get their comment, and then the command fails.
fn unblock(args: &UnblockArgs, catalog: &Catalog) -> Result<u8, String> {
    let number = number_argument("number", &args.number)?;
    let given = args.role.as_deref().and_then(SignatureRole::from_word);
    let (_, tracker) = tracker()?;
    record_activity("task unblock", None, Some(number), catalog);
    let mut by: Vec<i64> = Vec::new();
    for blocker in numbers(&args.by).ok_or_else(|| format!("--by takes numbers separated by commas: '{}'", args.by))? {
        if !by.contains(&blocker) {
            by.push(blocker);
        }
    }
    if by.is_empty() {
        return Err("--by takes at least one number".to_owned());
    }
    let reason = regia_core::pystr::strip(&args.reason);
    if reason.is_empty() {
        return Err("--reason cannot be blank".to_owned());
    }
    let number = issue_number(number)?;
    let current = tracker.blocker_numbers(number).map_err(tracker_error)?.ok_or_else(|| not_found(number))?;
    let (mut unblocked, mut not_blocking, mut failure) = (Vec::new(), Vec::new(), None);
    for blocker in by {
        let Some(found) = current.iter().copied().find(|current| i64::try_from(*current).is_ok_and(|current| current == blocker)) else {
            not_blocking.push(blocker);
            eprintln!("{} #{number}", catalog.message_with("task-not-a-blocker", "blocker", &blocker.to_string()));
            continue;
        };
        if let Err(error) = remove_blocker(&tracker, number, found) {
            failure = Some(error);
            break;
        }
        unblocked.push(blocker);
    }
    let mut id = None;
    if !unblocked.is_empty() {
        let body = format!("{}: {reason}", catalog.message_with("task-unblock-comment", "blockers", &joined(&unblocked)));
        let session = session();
        let project = tracker.labels(number).map(|labels| project_of_labels(&labels));
        let clock = Clock::read()?;
        let step_of_this_session = || match &project {
            Ok(Some(project)) => is_this_session_s_step(&clock, project, session.as_deref()),
            _ => false,
        };
        let role = SignatureRole::of_comment(args.on_behalf_of_owner, !args.on_behalf_of_owner && step_of_this_session(), given);
        let printed = tracker.add_comment(number, &sign(&body, role, session.as_deref())).map_err(tracker_error)?;
        id = comment_id(regia_core::pystr::strip(&printed));
        if let Some(id) = id
            && let Err(error) = record_own(&clock, id, &project, session.as_deref())
        {
            eprintln!("{} ({error})", catalog.message_with("comment-id-not-recorded", "id", &id.to_string()));
        }
    }
    if let Some(failure) = failure {
        return Err(failure);
    }
    if args.json {
        print_json(&json!({"number": number, "unblocked": unblocked, "not_blocking": not_blocking, "comment": id}))?;
    } else if unblocked.is_empty() {
        println!("#{number}: {}", catalog.message("task-unblock-none"));
    } else {
        println!("#{number} {}", catalog.message_with("task-unblocked", "blockers", &joined(&unblocked)));
    }
    Ok(0)
}

/// Removes `blocker` from the blockers of `number`, each read by its node first.
fn remove_blocker(tracker: &dyn Tracker, number: u64, blocker: u64) -> Result<(), String> {
    let issue = tracker.issue_node(number).map_err(tracker_error)?.ok_or_else(|| not_found(number))?;
    let blocking = tracker.issue_node(blocker).map_err(tracker_error)?.ok_or_else(|| not_found(blocker))?;
    tracker.remove_blocked_by(&issue.id, &blocking.id).map_err(tracker_error)
}

/// Adds `blocker` to the blockers of `number`, each read by its node first.
fn add_blocker(tracker: &dyn Tracker, number: u64, blocker: u64) -> Result<(), String> {
    let issue = tracker.issue_node(number).map_err(tracker_error)?.ok_or_else(|| not_found(number))?;
    let blocking = tracker.issue_node(blocker).map_err(tracker_error)?.ok_or_else(|| not_found(blocker))?;
    tracker.add_blocked_by(&issue.id, &blocking.id).map_err(tracker_error)
}

/// Makes `child` a sub-issue of `parent`, each read by its node first, the parent before the child.
fn link_sub_issue(tracker: &dyn Tracker, parent: u64, child: u64, replace_parent: bool) -> Result<(), String> {
    let parent_issue = tracker.issue_node(parent).map_err(tracker_error)?.ok_or_else(|| not_found(parent))?;
    let child_issue = tracker.issue_node(child).map_err(tracker_error)?.ok_or_else(|| not_found(child))?;
    tracker.add_sub_issue(&parent_issue.id, &child_issue.id, replace_parent).map_err(tracker_error)
}

/// The sub-issues GitHub lets an issue have, closed ones included (#505 of the dossier).
const SUB_ISSUE_CAP: u64 = 100;

/// An error if `parent` is at the cap of sub-issues, before any write. The parent the issue has already
/// adds no child, and is not counted.
fn check_parent_cap(tracker: &dyn Tracker, parent: u64, current: Option<u64>) -> Result<(), String> {
    if current == Some(parent) {
        return Ok(());
    }
    let count = tracker.sub_issue_count(parent).map_err(tracker_error)?.ok_or_else(|| not_found(parent))?;
    if count >= SUB_ISSUE_CAP {
        return Err(format!("#{parent} has {count} sub-issues already, GitHub's cap ({SUB_ISSUE_CAP}): choose another parent; nothing was changed"));
    }
    Ok(())
}

/// The mirror of the project label in the Project's field is not written by this engine: the issue's node
/// is read, as the prototype reads it before it looks at the config, and a warning says the label stays
/// the source.
fn project_field_not_written(tracker: &dyn Tracker, binding: &TrackerBinding, number: u64, project: &str, catalog: &Catalog) {
    let label = binding.project_label(project);
    match tracker.issue_node(number) {
        Ok(_) => eprintln!("{}", catalog.message_with("task-project-field-not-written", "label", &label)),
        Err(error) => eprintln!("{} ({error})", catalog.message_with("task-project-field-not-written", "label", &label)),
    }
}

/// The open issues, resolved without their statuses: a write needs their labels and links only.
fn open_tasks(tracker: &dyn Tracker, binding: &TrackerBinding) -> Result<Vec<Task>, String> {
    let mut tasks = Vec::new();
    for mut issue in tracker.open_issues().map_err(tracker_error)? {
        issue.status_option_id = None;
        tasks.push(binding.resolve(issue).map_err(|error| error.to_string())?);
    }
    Ok(tasks)
}

/// `task new`: an issue in the project, under its Objective or `--parent`, with its labels, its status, its
/// parent and its blockers, in the prototype's order; the parent's cap is checked before the issue exists.
fn new(args: &NewArgs, catalog: &Catalog) -> Result<u8, String> {
    let parent = args.parent.as_deref().map(|parent| number_argument("--parent", parent)).transpose()?;
    let maturity = Maturity::parse(&args.maturity).ok_or_else(|| format!("unknown maturity `{}`", args.maturity))?;
    let status = Status::parse(&args.status).ok_or_else(|| format!("unknown status `{}`", args.status))?;
    let (binding, tracker) = tracker()?;
    record_activity("task new", Some(&args.project), None, catalog);
    // Checked before the issue exists, so a wrong list leaves no issue without its blockers.
    let blocked_by = match args.blocked_by.as_deref() {
        Some(list) => numbers(list).ok_or_else(|| format!("--blocked-by takes numbers separated by commas: '{list}'"))?,
        None => Vec::new(),
    };
    let blocked_by = blocked_by.into_iter().map(issue_number).collect::<Result<Vec<u64>, String>>()?;
    // The last Objective of a project wins, as in the prototype's map.
    let objective = open_tasks(&tracker, &binding)?
        .into_iter()
        .rfind(|task| task.objective && task.project.as_deref() == Some(args.project.as_str()))
        .ok_or_else(|| format!("unknown project '{}': create it with `project new`", args.project))?;
    // A parent of 0 is none, as for the prototype.
    let parent = match parent.filter(|parent| *parent != 0) {
        Some(parent) => issue_number(parent)?,
        None => objective.number,
    };
    check_parent_cap(&tracker, parent, None)?;
    let body = match args.body_file.as_deref().filter(|file| !file.is_empty()) {
        Some(file) => read_text(file)?,
        None => args.body.clone(),
    };
    let labels = [binding.project_label(&args.project), binding.maturity_label(maturity).to_owned()];
    let printed = tracker.create_issue(&args.title, &body, &labels).map_err(tracker_error)?;
    let number = created_issue_number(&printed).ok_or_else(|| format!("gh issue create printed no issue's URL: {printed}"))?;
    set_status(&tracker, &binding, number, status)?;
    project_field_not_written(&tracker, &binding, number, &args.project, catalog);
    link_sub_issue(&tracker, parent, number, false)?;
    for blocker in blocked_by {
        add_blocker(&tracker, number, blocker)?;
    }
    let url = tracker.issue_url(number).unwrap_or_default();
    if args.json {
        print_json(&json!({"task": number, "parent": parent, "url": url}))?;
    } else {
        let parts = [
            catalog.message_with("task-created-parent", "parent", &parent.to_string()),
            catalog.message_with("task-created-status", "status", &catalog.message(&format!("state-{}", status.id()))),
            catalog.message(&format!("maturity-{}", maturity.id())),
        ];
        println!("{} ({}) {url}", catalog.message_with("task-created", "number", &number.to_string()), parts.join(", "));
    }
    Ok(0)
}

/// A UTF-8 file as the prototype reads text: `\r\n` and `\r` as `\n`.
fn read_text(file: &str) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|error| format!("{file}: {error}"))?;
    let text = String::from_utf8(bytes).map_err(|error| format!("{file}: not UTF-8 ({error})"))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// The binding of the config, and the tracker it names.
fn tracker() -> Result<(TrackerBinding, GhTracker), String> {
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref()).map_err(|error| error.to_string())?;
    let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
    Ok((binding, GhTracker::new(&config)))
}

/// An issue's number as the prototype's parser reads it, `int()`.
fn number_argument(name: &str, text: &str) -> Result<i64, String> {
    python_int(text).ok_or_else(|| format!("argument {name}: invalid int value: '{text}'"))
}

/// A number the tracker can have: none below 1 has a node.
fn issue_number(number: i64) -> Result<u64, String> {
    u64::try_from(number).map_err(|_| format!("invalid issue number: {number}"))
}

fn tracker_error(error: TrackerError) -> String {
    error.to_string()
}

fn not_found(number: u64) -> String {
    format!("issue #{number} not found")
}

/// `task status`: the status, then the comment if there is one. The comment is written in argv, as the
/// prototype writes it, so a body argv cannot carry is refused before the status changes.
fn status(args: &StatusArgs, catalog: &Catalog) -> Result<u8, String> {
    let number = number_argument("number", &args.number)?;
    let status = Status::parse(&args.status).ok_or_else(|| format!("unknown status `{}`", args.status))?;
    let given = args.role.as_deref().and_then(SignatureRole::from_word);
    let (binding, tracker) = tracker()?;
    record_activity("task status", None, Some(number), catalog);
    let number = issue_number(number)?;
    // An empty comment is none, as for the prototype.
    let comment = args.comment.as_deref().filter(|comment| !comment.is_empty());
    if let Some(comment) = comment {
        inline_body(comment)?;
    }
    set_status(&tracker, &binding, number, status)?;
    if let Some(comment) = comment {
        let session = session();
        // Read once, before signing; a failure leaves no project, and the comment is signed as by hand.
        let project = tracker.labels(number).ok().and_then(|labels| project_of_labels(&labels));
        let step_of_this_session = || {
            let clock = Clock::read().ok();
            match (&project, clock) {
                (Some(project), Some(clock)) => is_this_session_s_step(&clock, project, session.as_deref()),
                _ => false,
            }
        };
        let role = SignatureRole::of_comment(args.on_behalf_of_owner, !args.on_behalf_of_owner && step_of_this_session(), given);
        tracker.add_comment_inline(number, &sign(comment, role, session.as_deref())).map_err(tracker_error)?;
    }
    println!("#{number} {}", catalog.message_with("task-status-set", "status", &catalog.message(&format!("state-{}", status.id()))));
    Ok(0)
}

/// A body that can go in argv: not starting with a dash, no control character but tabs and line breaks,
/// and room left for the signature.
fn inline_body(body: &str) -> Result<(), String> {
    let refused = |why: &str| format!("the comment cannot be written as an argument: {why}");
    if body.starts_with('-') {
        return Err(refused("it starts with a dash"));
    }
    if body.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')) {
        return Err(refused("it has a control character"));
    }
    if body.len() + SIGNATURE_ROOM > 64 * 1024 {
        return Err(refused("it is too long"));
    }
    Ok(())
}

/// The status of issue `number`, as the prototype sets it (T10, T11): a closing status closes an open
/// issue, any other reopens a closed one; then the status field of its item, added to the Project if it
/// is not there; and for a closing status the same field again after a pause.
fn set_status(tracker: &dyn Tracker, binding: &TrackerBinding, number: u64, status: Status) -> Result<(), String> {
    let option = binding.option_id(status).ok_or_else(|| format!("the config binds no option to `{}`", status.id()))?;
    let issue = tracker.issue_node(number).map_err(tracker_error)?.ok_or_else(|| not_found(number))?;
    match status.close_reason() {
        Some(reason) if issue.state == IssueState::Open => tracker.close_issue(&issue.id, reason).map_err(tracker_error)?,
        None if issue.state == IssueState::Closed => tracker.reopen_issue(&issue.id).map_err(tracker_error)?,
        _ => {}
    }
    write_status(tracker, &issue.id, option)?;
    if status.close_reason().is_some() {
        std::thread::sleep(REWRITE_AFTER);
        write_status(tracker, &issue.id, option)?;
    }
    Ok(())
}

fn write_status(tracker: &dyn Tracker, issue: &str, option: &str) -> Result<(), String> {
    let item = tracker.project_item(issue).map_err(tracker_error)?;
    tracker.set_status_option(&item, option).map_err(tracker_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checks of a body written in argv, which `regia-exec` would refuse later, after the status.
    #[test]
    fn a_body_argv_cannot_carry_is_refused_first() {
        assert!(inline_body("ok").is_ok());
        assert!(inline_body("line\n\tand tab\r\n").is_ok());
        assert!(inline_body("-x").is_err());
        assert!(inline_body("bell\u{7}").is_err());
        assert!(inline_body(&"x".repeat(64 * 1024)).is_err());
    }
}
