//! `comment`, natively: a comment on an issue, signed by role (M1–M5 of the dossier's conformance study),
//! with the arguments of the facade table (`table/comment.toml`). Not a subcommand of the CLI's parser:
//! while its group runs on the prototype, the facade takes its argv as it is.
//!
//! In the prototype's order: the issue's labels are read once, for its project; the role is decided; the
//! body is signed and written; then the comment's id, if `gh` printed it, goes into the open step of this
//! session on that project (M4). Nothing after the write changes the exit code: the comment exists, and
//! writing it again would make two. What goes wrong there is a warning on stderr.

use clap::Parser;
use clap::builder::PossibleValuesParser;
use regia_core::{SignatureRole, Tracker, TrackerConfigV1, TrackerError, comment_id, project_of_labels, pystr, sign};
use regia_github::GhTracker;
use regia_i18n::Catalog;
use regia_identity::state_dir;
use regia_runtime::dossier_dir;
use regia_store::ProjectId;
use serde_json::json;

use crate::clock::Clock;
use crate::declare::record_activity;
use crate::print_json;
use crate::reads::{parse, python_int};

/// Claude Code's id of the session a command runs in: the one the marker names, and the one an open step
/// records, as the prototype reads them.
const SESSION_VARIABLE: &str = "CLAUDE_CODE_SESSION_ID";

#[derive(Parser)]
#[command(name = "comment", args_override_self = true)]
struct CommentArgs {
    /// The issue's number.
    number: String,
    #[arg(long)]
    body: Option<String>,
    /// A UTF-8 file, read in place of `--body`.
    #[arg(long)]
    body_file: Option<String>,
    /// The comment relays a decision of the owner: the one comment the precheck counts as an answer.
    #[arg(long)]
    on_behalf_of_owner: bool,
    /// Who writes, when it is not the orchestrator of the step open on the issue's project.
    #[arg(long, value_parser = PossibleValuesParser::new(SignatureRole::BY_HAND.map(SignatureRole::word)))]
    role: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// `command` natively, if this module has it: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let outcome = match command {
        "comment" => parse::<CommentArgs>(command, args).map(|args| comment(&args, catalog)),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
}

fn comment(args: &CommentArgs, catalog: &Catalog) -> Result<u8, String> {
    let number = python_int(&args.number).ok_or_else(|| format!("argument number: invalid int value: '{}'", args.number))?;
    let given = args.role.as_deref().and_then(SignatureRole::from_word);
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref()).map_err(|error| error.to_string())?;
    record_activity("comment", None, Some(number), catalog);
    let number = u64::try_from(number).map_err(|_| format!("invalid issue number: {number}"))?;
    let body = body(args)?;
    let clock = Clock::read()?;
    let tracker = GhTracker::new(&config);
    let session = session();
    // Read once, before signing; a failure leaves no project, and is told after the write.
    let project = tracker.labels(number).map(|labels| project_of_labels(&labels));
    let step_of_this_session = || match &project {
        Ok(Some(project)) => is_this_session_s_step(&clock, project, session.as_deref()),
        _ => false,
    };
    let role = SignatureRole::of_comment(args.on_behalf_of_owner, !args.on_behalf_of_owner && step_of_this_session(), given);
    let printed = tracker.add_comment(number, &sign(&body, role, session.as_deref())).map_err(|error| error.to_string())?;
    let url = pystr::strip(&printed).to_owned();
    let id = comment_id(&url);
    if let Some(id) = id
        && let Err(error) = record_own(&clock, id, &project, session.as_deref())
    {
        eprintln!("{} ({error})", catalog.message_with("comment-id-not-recorded", "id", &id.to_string()));
    }
    if args.json {
        print_json(&json!({"number": number, "id": id, "url": (!url.is_empty()).then_some(url)}))?;
    } else {
        println!("{}", catalog.message_with("comment-added", "number", &number.to_string()));
    }
    Ok(0)
}

/// The body: the file's when one is named, read as the prototype reads text, with `\r\n` and `\r` as `\n`;
/// else `--body`. Without either there is nothing to write, and nothing is called.
fn body(args: &CommentArgs) -> Result<String, String> {
    if let Some(file) = args.body_file.as_deref().filter(|file| !file.is_empty()) {
        let bytes = std::fs::read(file).map_err(|error| format!("{file}: {error}"))?;
        let text = String::from_utf8(bytes).map_err(|error| format!("{file}: not UTF-8 ({error})"))?;
        return Ok(text.replace("\r\n", "\n").replace('\r', "\n"));
    }
    args.body.clone().ok_or_else(|| "no body: pass --body or --body-file".to_owned())
}

/// The id of the session the command runs in, if it has one.
fn session() -> Option<String> {
    std::env::var(SESSION_VARIABLE).ok().filter(|id| !id.is_empty())
}

/// Whether the step open on `project` is this session's. In doubt, a state that cannot be read, it is not.
fn is_this_session_s_step(clock: &Clock, project: &str, session: Option<&str>) -> bool {
    let Some(session) = session else { return false };
    let Ok(project) = ProjectId::new(project) else { return false };
    let opened_by = clock.store().ok().and_then(|store| store.step_session(&project).ok().flatten());
    opened_by.is_some_and(|opened_by| !opened_by.is_empty() && opened_by == session)
}

/// The comment's id among the comments of the step open on the issue's project (M4): nothing without a
/// project or a step, an error when the labels or the state could not be read.
fn record_own(clock: &Clock, id: u64, project: &Result<Option<String>, TrackerError>, session: Option<&str>) -> Result<(), String> {
    let project = match project {
        Ok(Some(project)) => ProjectId::new(project.as_str()).map_err(|error| error.to_string())?,
        Ok(None) => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    // Another session's step, or none, leaves the comment an event, as it should be.
    clock.store()?.record_own_comment(&project, id, session).map(|_| ()).map_err(|error| error.to_string())
}
