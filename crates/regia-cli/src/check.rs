//! `check` and `path`, natively (#448), with the arguments of the facade table (`table/check.toml`,
//! `table/path.toml`), and the checks `step finish` runs before the dossier's block. They are not subcommands of
//! the CLI's parser: while their group runs on the prototype, the facade takes their argv as it is.
//!
//! The rules are `regia-core`'s; here only what they read, read only: the tracker (the open issues, the recent
//! comments, and the closed issues when a dossier has reports), the leases of the state database, and the files of
//! each project's folder, with the previous version of its state from git (`show` and `log`, never a write). The
//! JSON is the contract; the findings' texts and reasons and the warnings are the prototype's sentences, data that
//! pass as they are. The text is the catalog's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use clap::Parser;
use regia_core::checks::{self, AbsoluteInputs, CheckId, CommentedIssue, Finding, IssueFacts, ProjectInputs, RemainingInputs, Thresholds};
use regia_core::{ReferenceKind, Registry, Status, Task, Tracker, TrackerBinding, TrackerConfigV1, project_field_name, project_of_labels, resolve};
use regia_exec::{DossierPath, GitRef};
use regia_git::Git;
use regia_github::GhTracker;
use regia_i18n::Catalog;
use regia_runtime::{dossier_dir, project_folder};
use regia_store::{LeaseEntry, ViewField};
use serde_json::{Value, json};

use crate::clock::Clock;
use crate::print_json;
use crate::reads::parse;

#[derive(Parser)]
#[command(name = "check", args_override_self = true)]
struct CheckArgs {
    /// Only this project; without it, every project of the registry.
    #[arg(long)]
    project: Option<String>,
    /// The state file of HEAD against the one on disk, as `step finish` compares them; without it, the last commit
    /// that touched it against the one before.
    #[arg(long)]
    against_disk: bool,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "path", args_override_self = true)]
struct PathArgs {
    /// Relative to the dossier, the data's placeholder, or anything else: the kind says what it is. Quote it: a
    /// shell expands `~` and `$VAR`.
    reference: String,
    /// The dossier of this project, from its registry; without it, the working folder, if it is a dossier.
    #[arg(long)]
    project: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// The exit of `path` with no dossier to resolve against: the prototype's.
const NO_DOSSIER: u8 = 2;
/// What `step finish` keeps of why its checks could not run, in characters, as the prototype cuts it.
const REASON_CHARS: usize = 200;

/// `command` natively, if this module has it: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let outcome = match command {
        "check" => parse::<CheckArgs>(command, args).map(|args| check(&args, catalog)),
        "path" => parse::<PathArgs>(command, args).map(|args| path(&args, catalog)),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
}

/// Which version of the state file the current one is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The last commit that touched it against the one before: `check`, and the brief.
    Commit,
    /// HEAD against the file on disk, before the dossier is committed: `step finish`, `check --against-disk`.
    Step,
}

/// A project of the registry, and the folder it names.
#[derive(Debug, Clone)]
struct Project {
    id: String,
    folder: Option<String>,
}

/// What the tracker gave: every open issue, of any project, and the projects of the open Objectives, in its order.
struct Tracked {
    tasks: Vec<Task>,
    projects: Vec<Project>,
    tracker: GhTracker,
    config: TrackerConfigV1,
}

fn read_tracker() -> Result<Tracked, String> {
    let config = crate::tracker_config()?;
    let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
    let tracker = GhTracker::new(&config);
    let mut tasks = Vec::new();
    let mut projects = Vec::new();
    for mut issue in tracker.open_issues().map_err(|error| error.to_string())? {
        let body = std::mem::take(&mut issue.body);
        let task = binding.resolve(issue).map_err(|error| error.to_string())?;
        if task.objective
            && let Some(id) = task.project.clone().filter(|id| !id.is_empty())
        {
            projects.push(Project { id, folder: Registry::parse(&body).folder().map(str::to_owned) });
        }
        tasks.push(task);
    }
    Ok(Tracked { tasks, projects, tracker, config })
}

/// The thresholds: the prototype's file of its dossier while the prototype is there, else the product's defaults;
/// with why the defaults apply, when a file did not read.
fn thresholds() -> (Thresholds, Option<String>) {
    let Some(dossier) = dossier_dir() else { return (Thresholds::defaults(), None) };
    let file = &checks::files().thresholds;
    let name = Path::new(file).file_name().and_then(|name| name.to_str()).unwrap_or(file);
    match std::fs::read(dossier.join(file)) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Thresholds::parse(&universal(&text), name),
            Err(_) => Thresholds::undecodable(name),
        },
        // Python's names of the errors, and its messages of the two a user meets.
        Err(error) => match error.kind() {
            std::io::ErrorKind::NotFound => Thresholds::unreadable(name, "FileNotFoundError", Some("No such file or directory")),
            std::io::ErrorKind::PermissionDenied => Thresholds::unreadable(name, "PermissionError", Some("Permission denied")),
            _ => Thresholds::unreadable(name, "OSError", None),
        },
    }
}

/// Python's text mode: `\r\n` and a lone `\r` read as `\n`.
fn universal(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// A file of a project's folder as the prototype reads it: UTF-8 or nothing, a BOM kept, its line breaks as `\n`
/// unless `exact` (the decisions' files, whose cap counts the `\r` too).
fn read_text(folder: &Path, relative: &str, exact: bool) -> Option<String> {
    let text = String::from_utf8(std::fs::read(folder.join(relative)).ok()?).ok()?;
    Some(if exact { text } else { universal(&text) })
}

/// The names in `folder` that start with `prefix` and end with `suffix`, as the prototype's glob finds them: in any
/// case on Windows, in this one elsewhere.
fn names(folder: &Path, prefix: &str, suffix: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let fold = |text: &str| if cfg!(windows) { text.to_lowercase() } else { text.to_owned() };
    let (prefix, suffix) = (fold(prefix), fold(suffix));
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            let name = fold(name);
            name.starts_with(&prefix) && name.ends_with(&suffix) && name.len() >= prefix.len() + suffix.len()
        })
        .collect();
    out.sort();
    out
}

/// Files by name or path, with their text.
type Texts = Vec<(String, String)>;

/// The reports of a folder dated from `from`, `(name, text)`, and its decisions' files `(path, text)`: the archives by
/// name, then the decisions themselves; a file that does not read is left out.
fn dossier_files(folder: &Path, from: &str) -> (Texts, Texts) {
    let files = checks::files();
    let reports_folder = folder.join(&files.reports);
    let reports = names(&reports_folder, "", &files.report_suffix)
        .into_iter()
        .filter(|name| name.chars().take(10).collect::<String>().as_str() >= from)
        .filter_map(|name| read_text(&reports_folder, &name, false).map(|text| (name, text)))
        .collect();
    let regia = folder.join(&files.regia);
    let mut decisions = names(&regia, &files.archive_prefix, &files.archive_suffix);
    decisions.extend(Path::new(&files.decisions).file_name().and_then(|name| name.to_str()).map(str::to_owned));
    let decisions = decisions.into_iter().filter_map(|name| read_text(&regia, &name, true).map(|text| (format!("{}/{name}", files.regia), text))).collect();
    (reports, decisions)
}

/// The state file before, and a warning when there was a version to read and it did not read.
fn previous_state(folder: &Path, mode: Mode, git: Option<&Git>, project: &str) -> (Option<String>, Option<String>) {
    let (Some(git), Ok(state)) = (git, DossierPath::parse(&checks::files().state)) else { return (None, None) };
    if mode == Mode::Step {
        let run = git.file_at(folder, &GitRef::head(), &state);
        return (run.ok().then_some(run.stdout), None);
    }
    let log = git.last_two_commits_of(folder, &state);
    let hashes: Vec<&str> = if log.ok() { log.stdout.split_whitespace().collect() } else { Vec::new() };
    let Some(before) = hashes.get(1) else { return (None, None) };
    let run = match GitRef::parse(before) {
        Ok(rev) => git.file_at(folder, &rev, &state),
        Err(error) => regia_git::Run { code: 1, stdout: String::new(), stderr: error.to_string() },
    };
    if run.ok() {
        return (Some(run.stdout), None);
    }
    let error: String = run.stderr.chars().take(80).collect();
    (None, Some(checks::warning("previous_state_unread", &[("project", project), ("error", &error)])))
}

/// The live leases' tasks; why not, when the state database does not read.
fn live_leases() -> Result<BTreeSet<u64>, String> {
    let entries = Clock::read()?.store()?.leases(false).map_err(|error| error.to_string())?;
    Ok(entries
        .iter()
        .filter_map(|entry| match entry {
            LeaseEntry::Lease(lease) if !lease.is_expired() => lease.field(ViewField::Task).and_then(Value::as_u64),
            _ => None,
        })
        .collect())
}

/// The findings and the warnings of the checks.
struct Outcome {
    findings: Vec<Finding>,
    warnings: Vec<String>,
}

/// The checks of `projects`, as the prototype's wiring runs them: one read of the comments for all, the leases, the
/// closed issues once and only for a dossier with reports, and for each project its folder's files.
fn compute(tracked: &Tracked, projects: &[Project], mode: Mode, now: DateTime<Utc>, thresholds: &Thresholds) -> Result<Outcome, String> {
    let open: BTreeMap<u64, Option<Status>> = tracked.tasks.iter().map(|task| (task.number, task.status)).collect();
    let mut warnings = vec![checks::warning("approved_history", &[])];
    let comments: Vec<CommentedIssue> = if projects.is_empty() {
        Vec::new()
    } else {
        let days = i64::try_from(thresholds.comment_window_days).unwrap_or(i64::MAX).min(36_500_000);
        let since = now - Duration::days(days);
        let read = tracked.tracker.recent_comments(since).map_err(|error| error.to_string())?;
        read.into_iter()
            .map(|issue| CommentedIssue {
                number: issue.number,
                state: issue.state,
                closed_at: issue.closed_at,
                project: project_of_labels(&issue.labels),
                comments: issue.comments,
            })
            .collect()
    };
    let leases = match live_leases() {
        Ok(leases) => Some(leases),
        Err(error) => {
            warnings.push(checks::warning("leases_unread", &[("error", &error)]));
            None
        }
    };
    let project_field = tracked.config.project_field_configured;
    if !project_field {
        warnings.push(checks::warning("project_field_unset", &[("field", &project_field_name())]));
    }
    let git = Git::resolve().ok();
    // Every issue, open and closed, for the destinations of `resta`: read once, at the first dossier with reports.
    let mut facts: Option<Option<BTreeMap<u64, IssueFacts>>> = None;
    let mut findings = Vec::new();
    for project in projects {
        let folder = project.folder.as_deref().map(Path::new).filter(|folder| folder.is_dir());
        let mut inputs = ProjectInputs {
            project: &project.id,
            open: open.clone(),
            issues: Some(&tracked.tasks),
            comments: Some(&comments),
            live_leases: leases.as_ref(),
            project_field,
            ..ProjectInputs::default()
        };
        let Some(folder) = folder else {
            warnings.push(checks::warning("folder_missing", &[("project", &project.id)]));
            findings.extend(checks::check_project(&inputs, now, thresholds));
            continue;
        };
        let files = checks::files();
        let state = read_text(folder, &files.state, false);
        let questions = read_text(folder, &files.questions, false);
        let (previous, warning) = if state.is_some() { previous_state(folder, mode, git.as_ref(), &project.id) } else { (None, None) };
        warnings.extend(warning);
        let reports_folder = folder.join(&files.reports);
        let with_report = checks::numbers_with_report(names(&reports_folder, "", &files.report_suffix).iter().map(String::as_str));
        let (reports, decisions) = dossier_files(folder, checks::remaining_from());
        let (absolute_reports, _) = dossier_files(folder, checks::absolute_from());
        let all_decisions: Vec<(String, Option<String>)> = decisions.iter().map(|(path, text)| (path.clone(), Some(text.clone()))).collect();
        if !reports.is_empty() && facts.is_none() {
            facts = Some(match tracked.tracker.closed_issues() {
                Ok(closed) => {
                    let mut map: BTreeMap<u64, IssueFacts> = closed
                        .into_iter()
                        .map(|issue| (issue.number, IssueFacts { state: issue.state, state_reason: issue.state_reason, closed_at: issue.closed_at, url: issue.url }))
                        .collect();
                    for task in tracked.tasks.iter().filter(|task| task.state == regia_core::IssueState::Open) {
                        let url = Some(task.url.clone()).filter(|url| !url.is_empty());
                        map.insert(task.number, IssueFacts { state: task.state, state_reason: task.state_reason.clone(), closed_at: task.closed_at.clone(), url });
                    }
                    Some(map)
                }
                Err(error) => {
                    warnings.push(checks::warning("closed_unread", &[("error", &error.to_string())]));
                    None
                }
            });
        }
        let known = facts.as_ref().and_then(Option::as_ref).filter(|map| !map.is_empty() && !reports.is_empty());
        inputs.state = state.as_deref();
        inputs.previous_state = previous.as_deref();
        inputs.questions = questions.as_deref();
        inputs.with_report = Some(&with_report);
        inputs.remaining = known.map(|issues| RemainingInputs { reports: &reports, decisions: &decisions, issues });
        inputs.absolute = Some(AbsoluteInputs { reports: &absolute_reports, decisions: Some(&all_decisions) });
        findings.extend(checks::check_project(&inputs, now, thresholds));
    }
    Ok(Outcome { findings, warnings })
}

fn now() -> Result<DateTime<Utc>, String> {
    Ok(Clock::read()?.now.with_timezone(&Utc))
}

fn unknown(project: &str) -> String {
    format!("project '{project}' is unknown")
}

/// `check`: the findings of every project of the registry, or of one. Exit 0, findings included; 1 for a project the
/// registry does not have, or a tracker that does not answer.
fn check(args: &CheckArgs, catalog: &Catalog) -> Result<u8, String> {
    let (thresholds, reason) = thresholds();
    if let Some(reason) = reason {
        eprintln!("{}", catalog.message_with("check-thresholds", "reason", &reason));
    }
    let tracked = read_tracker()?;
    let projects: Vec<Project> = tracked.projects.iter().filter(|project| args.project.as_ref().is_none_or(|only| &project.id == only)).cloned().collect();
    if let Some(project) = &args.project
        && projects.is_empty()
    {
        return Err(unknown(project));
    }
    let mode = if args.against_disk { Mode::Step } else { Mode::Commit };
    let outcome = compute(&tracked, &projects, mode, now()?, &thresholds)?;
    if args.json {
        print_json(&json!({"findings": outcome.findings, "per_check": checks::per_check(&outcome.findings), "warnings": outcome.warnings}))?;
        return Ok(0);
    }
    println!("{}", summary(&outcome.findings, catalog));
    for finding in &outcome.findings {
        println!("  {}", line(finding));
    }
    for warning in &outcome.warnings {
        eprintln!("  {}", catalog.message_with("check-warning", "warning", warning));
    }
    Ok(0)
}

/// The summary line: how many findings, and of which checks, in their order.
fn summary(findings: &[Finding], catalog: &Catalog) -> String {
    let count = catalog.message_with("check-summary", "count", &findings.len().to_string());
    if findings.is_empty() {
        return count;
    }
    let counts: Vec<String> = CheckId::ALL
        .iter()
        .filter_map(|check| {
            let n = findings.iter().filter(|finding| finding.check == *check).count();
            (n > 0).then(|| format!("{} {n}", check.id()))
        })
        .collect();
    format!("{count} ({})", counts.join(", "))
}

/// A finding on one line: the project, the check, where, the text and why.
fn line(finding: &Finding) -> String {
    format!("{}: [{}] {} · {} · {}", finding.project, finding.check.id(), finding.location, finding.text, finding.why)
}

/// How many findings `step finish` lists in its text line before it says how many more there are: the prototype's.
const STEP_LINES: usize = 3;

/// The checks of `step finish`: on `folder` when it is given, else on the folder of the project's registry, HEAD
/// against the disk. `unavailable`, with why, when they could not run: they never change the step's exit code.
/// With the JSON, the text of the step's line: `checks: N findings (…)` and the first of them, or the line that
/// says they are unavailable (#701).
pub(crate) fn step_checks(project: &str, folder: Option<&str>, catalog: &Catalog) -> (Value, Vec<String>) {
    let run = || -> Result<Outcome, String> {
        let (thresholds, reason) = thresholds();
        if let Some(reason) = reason {
            eprintln!("{}", catalog.message_with("check-thresholds", "reason", &reason));
        }
        let tracked = read_tracker()?;
        let projects = match folder {
            Some(folder) => vec![Project { id: project.to_owned(), folder: Some(folder.to_owned()) }],
            None => tracked.projects.iter().filter(|entry| entry.id == project).cloned().collect(),
        };
        if projects.is_empty() {
            return Err(unknown(project));
        }
        compute(&tracked, &projects, Mode::Step, now()?, &thresholds)
    };
    match run() {
        Ok(outcome) => {
            let value = json!({
                "total": outcome.findings.len(),
                "per_check": checks::per_check(&outcome.findings),
                "findings": outcome.findings,
                "warnings": outcome.warnings,
            });
            (value, step_lines(&outcome.findings, catalog))
        }
        Err(reason) => {
            let reason: String = reason.chars().take(REASON_CHARS).collect();
            let lines = vec![catalog.message_with("check-unavailable", "reason", &reason)];
            (json!({"unavailable": reason}), lines)
        }
    }
}

/// The summary line of `step finish`, then the first three findings, each on a line of its own, then how many more
/// there are: as the prototype's `controlli_riga` prints them.
fn step_lines(findings: &[Finding], catalog: &Catalog) -> Vec<String> {
    let mut lines = vec![summary(findings, catalog)];
    lines.extend(findings.iter().take(STEP_LINES).map(|finding| format!("  {}", line(finding))));
    if findings.len() > STEP_LINES {
        lines.push(format!("  {}", catalog.message_with("check-more", "count", &(findings.len() - STEP_LINES).to_string())));
    }
    lines
}

/// The contract's name of a reference's kind.
fn kind(kind: ReferenceKind) -> &'static str {
    match kind {
        ReferenceKind::Relative => "relative",
        ReferenceKind::Data => "data",
        ReferenceKind::Absolute => "absolute",
        ReferenceKind::Outside => "outside",
        ReferenceKind::NotAPath(_) => "not_a_path",
    }
}

/// `path`: where a reference of the dossier is, for an agent that must not join paths by hand. Read only. Exit 0;
/// 2 with no `--project` outside a dossier; 1 for a project the registry does not have.
fn path(args: &PathArgs, catalog: &Catalog) -> Result<u8, String> {
    let folder = match &args.project {
        Some(project) => PathBuf::from(project_folder(project).map_err(|error| error.to_string())?),
        None => {
            let here = std::env::current_dir().map_err(|error| error.to_string())?;
            if !here.join(&checks::files().regia).is_dir() {
                eprintln!("{}", catalog.message("path-no-dossier"));
                return Ok(NO_DOSSIER);
            }
            here
        }
    };
    let reference = resolve(&args.reference, &folder);
    let value = json!({
        "reference": reference.original(),
        "kind": kind(reference.kind()),
        "path": reference.path().map(|path| path.display().to_string()),
        "exists": reference.exists(),
        "is_folder": reference.is_folder(),
        "reason": reference.reason_sentence(),
    });
    if args.json {
        print_json(&value)?;
        return Ok(0);
    }
    let shown = reference.path().map_or_else(|| catalog.message("path-unresolved"), |path| path.display().to_string());
    let exists = match (reference.exists(), reference.is_folder()) {
        (true, true) => catalog.message("path-exists-folder"),
        (true, false) => catalog.message("path-exists-file"),
        _ => catalog.message("path-exists-no"),
    };
    println!("{}", catalog.message_with("path-reference", "value", reference.original()));
    println!("{}", catalog.message_with("path-kind", "value", kind(reference.kind())));
    println!("{}", catalog.message_with("path-path", "value", &shown));
    println!("{}", catalog.message_with("path-exists", "value", &exists));
    if let Some(reason) = reference.reason_sentence() {
        println!("{}", catalog.message_with("path-reason", "value", reason));
    }
    // How a document cites it, with the catalog's sentence for one that is not here (#443/V9).
    println!("{}", catalog.message_with("path-cite", "value", &reference.render(&catalog.message("reference-absent"))));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args<T: Parser>(name: &str, argv: &[&str]) -> Result<T, u8> {
        parse::<T>(name, &argv.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    /// The prototype's parser: `check` takes a project and the flag; `path` a reference and a project; a wrong
    /// argument exits 1, help 0.
    #[test]
    fn the_arguments_are_the_prototype_s() {
        let check: CheckArgs = args("check", &["--project", "alpha", "--against-disk", "--json"]).unwrap();
        assert_eq!((check.project.as_deref(), check.against_disk, check.json), (Some("alpha"), true, true));
        assert!(args::<CheckArgs>("check", &[]).is_ok_and(|check| check.project.is_none() && !check.against_disk));
        assert_eq!(args::<CheckArgs>("check", &["--folder", "x"]).err(), Some(1));
        let path: PathArgs = args("path", &["a/b.md", "--project", "alpha"]).unwrap();
        assert_eq!((path.reference.as_str(), path.project.as_deref()), ("a/b.md", Some("alpha")));
        assert_eq!(args::<PathArgs>("path", &[]).err(), Some(1), "the reference is required");
        assert_eq!(args::<PathArgs>("path", &["--help"]).err(), Some(0));
    }

    #[test]
    fn files_read_as_python_s_text_mode_and_the_globs_match_as_its() {
        assert_eq!(universal("a\r\nb\rc\n"), "a\nb\nc\n");
        let dir = std::env::temp_dir().join(format!("regia-cli-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.md", "a.md", "c.txt", "x-1.md", "x-.md"] {
            std::fs::write(dir.join(name), "t\r\n").unwrap();
        }
        assert_eq!(names(&dir, "", ".md"), ["a.md", "b.md", "x-.md", "x-1.md"]);
        assert_eq!(names(&dir, "x-", ".md"), ["x-.md", "x-1.md"]);
        assert_eq!(read_text(&dir, "a.md", false).as_deref(), Some("t\n"));
        assert_eq!(read_text(&dir, "a.md", true).as_deref(), Some("t\r\n"));
        assert_eq!(read_text(&dir, "none.md", false), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_summary_counts_the_checks_in_their_order() {
        let catalog = Catalog::new(regia_i18n::Language::En);
        let finding = |check| Finding { check, project: "p".into(), location: "#1".into(), text: "t".into(), why: "w".into(), url: None };
        let findings = [finding(CheckId::InboxStale), finding(CheckId::StateWithoutIssue), finding(CheckId::InboxStale)];
        let mut sorted = findings.to_vec();
        sorted.sort_by_key(|finding| finding.check);
        assert_eq!(summary(&sorted, &catalog), "checks: 3 findings (state-without-issue 1, inbox-stale 2)");
        assert_eq!(summary(&[], &catalog), "checks: 0 findings");
        assert_eq!(line(&sorted[0]), "p: [state-without-issue] #1 · t · w");
    }
}
