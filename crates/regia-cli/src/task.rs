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
use regia_core::{IssueState, SignatureRole, Status, Tracker, TrackerBinding, TrackerConfigV1, TrackerError, project_of_labels, sign};
use regia_github::GhTracker;
use regia_i18n::Catalog;
use regia_identity::state_dir;
use regia_runtime::dossier_dir;

use crate::clock::Clock;
use crate::comment::{is_this_session_s_step, session};
use crate::declare::record_activity;
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

/// `command` natively, if this module has it: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let outcome = match command {
        "task status" => parse::<StatusArgs>(command, args).map(|args| status(&args, catalog)),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
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
