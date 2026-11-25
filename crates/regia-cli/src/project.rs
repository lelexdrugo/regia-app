//! `setup`, `project new` and `project set`, natively (the prototype's commands of the base labels, of a new project
//! and of a project's levers), with the arguments of the facade table (`table/setup.toml`, `table/project.toml`). Not subcommands of
//! the CLI's parser: while their group runs on the prototype, the facade takes their argv as it is.
//!
//! Each calls the tracker as the prototype does, reads included and in the same order, and checks its arguments where
//! the prototype checks them, some only after the read of the open issues. `project new` records its line in the sessions'
//! log after the parser's checks and before its own, with the id as given (#445/V5); the other two record none, as in
//! the prototype. One part of the prototype is not here: the Project's field that mirrors the project label, which its `project new`
//! fills last, belongs to #454, and a warning says the label stays the source.

use std::fs;
use std::path::{Component, Path, PathBuf};

use clap::Parser;
use regia_core::{
    ObjectiveWords, ProjectLever, Registry, ScaffoldValues, SignatureRole, Status, Task, Tracker, TrackerBinding, TrackerConfigV1, base_labels,
    comment_id, created_issue_number, default_playbook, no_products_word, objective_body, products_list, project_of_labels, pystr, registry_key,
    registry_key_count, registry_marker, registry_with, repo_argument, scaffold_files, scaffold_folder, scaffold_folders, sign,
};
use regia_exec::{DossierPath, Text};
use regia_facade::Table;
use regia_git::{FirstPush, Git, LinkError, Linked, PushStep};
use regia_github::{GhTracker, repo_url};
use regia_i18n::Catalog;
use regia_identity::PROTOTYPE_NAME;
use serde_json::json;

use crate::clock::Clock;
use crate::comment::{is_this_session_s_step, record_own, session};
use crate::declare::record_activity;
use crate::print_json;
use crate::reads::{parse, python_int};
use crate::task::{set_status, tracker_error};

/// What stands for a value the registry does not give, in the text, as the prototype prints it.
const NONE: &str = "—";

/// The longest description GitHub takes for a label, and for a repo, as the prototype cuts them: in characters.
const LABEL_DESCRIPTION: usize = 100;
const REPO_DESCRIPTION: usize = 350;

#[derive(Parser)]
#[command(name = "setup", args_override_self = true)]
struct SetupArgs {
    /// Accepted and ignored: the command prints a line of text, as the prototype does.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "project new", args_override_self = true)]
struct NewArgs {
    /// Lowercase letters, digits and dashes.
    #[arg(long)]
    id: String,
    #[arg(long)]
    title: String,
    /// The dossier's folder, where the scaffold goes. Without it there is none.
    #[arg(long, default_value = "")]
    folder: String,
    /// Without it: the prototype's general playbook.
    #[arg(long)]
    playbook: Option<String>,
    #[arg(long)]
    objective: String,
    #[arg(long)]
    definition_of_done: String,
    #[arg(long, default_value = "")]
    mandate: String,
    /// An integer of 1 or more, 1 the first.
    #[arg(long, default_value = "3")]
    priority: String,
    /// The tasks run at once: an integer from 1 to 10.
    #[arg(long, default_value = "2")]
    wip: String,
    /// `no`, or the hours after which a reversible choice is taken alone.
    #[arg(long, default_value = "no")]
    auto_defaults: String,
    #[arg(long, default_value = "5319e7")]
    color: String,
    /// The dossier's private repo to create, `<name>` or `<owner>/<name>`: it needs `--folder`.
    #[arg(long)]
    repo: Option<String>,
    /// The products' repos, separated by commas, or `none`.
    #[arg(long)]
    products: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "project set", args_override_self = true)]
struct SetArgs {
    #[arg(long)]
    project: String,
    /// The dossier's repo, which exists already: `<name>` or `<owner>/<name>`.
    #[arg(long)]
    repo: Option<String>,
    /// The products' repos, separated by commas, or `none`.
    #[arg(long)]
    products: Option<String>,
    #[arg(long)]
    wip: Option<String>,
    #[arg(long)]
    priority: Option<String>,
    /// What would be written, and nothing written.
    #[arg(long)]
    dry_run: bool,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// `command` natively, if this module has it: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let outcome = match command {
        "setup" => parse::<SetupArgs>(command, args).map(|_| setup(catalog)),
        "project new" => parse::<NewArgs>(command, args).map(|args| new(&args, catalog)),
        "project set" => parse::<SetArgs>(command, args).map(|args| set(&args, catalog)),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
}

/// `setup`: the base labels, in their order, each created or updated in place with its color and description; nothing
/// is read.
fn setup(catalog: &Catalog) -> Result<u8, String> {
    let config = crate::tracker_config()?;
    let tracker = GhTracker::new(&config);
    for label in base_labels() {
        tracker.create_label(&label.name, &label.color, &label.description).map_err(tracker_error)?;
    }
    println!("{}", catalog.message("setup-labels-ready"));
    Ok(0)
}

/// The tracker's configuration, its binding, and the tracker it names.
fn tracker() -> Result<(TrackerConfigV1, TrackerBinding, GhTracker), String> {
    let config = crate::tracker_config()?;
    let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
    let tracker = GhTracker::new(&config);
    Ok((config, binding, tracker))
}

/// The open issues, every page of them, in the tracker's order, each with its body, as the prototype reads them for
/// these writes (`all_issues()`, with no closed one): a write needs their labels, not their statuses.
fn open_issues(tracker: &dyn Tracker, binding: &TrackerBinding) -> Result<Vec<(Task, String)>, String> {
    let mut out = Vec::new();
    for mut issue in tracker.open_issues().map_err(tracker_error)? {
        issue.status_option_id = None;
        let body = issue.body.clone();
        out.push((binding.resolve(issue).map_err(|error| error.to_string())?, body));
    }
    Ok(out)
}

/// The value of `--products` in format v1: the contract's word for none becomes the prototype's, as the facade
/// translates it; any other passes as it is.
fn products_value(command: &str, value: &str) -> Result<String, String> {
    let argv = Table::embedded().python_argv(command, &[format!("--products={value}")]).map_err(|error| error.to_string())?;
    argv.last().and_then(|arg| arg.split_once('=')).map(|(_, value)| value.to_owned()).ok_or_else(|| format!("--products '{value}'"))
}

/// The first `count` characters of `text`, as Python cuts a string.
fn truncated(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

/// A project's id, as the prototype takes it: lowercase letters, digits and dashes, not starting with a dash.
fn valid_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `project new`: the repo of the dossier when `--repo` asks for one, the label, the Objective with its registry
/// and its status, the scaffold of the dossier, and its folder linked to the repo, in the prototype's order.
fn new(args: &NewArgs, catalog: &Catalog) -> Result<u8, String> {
    let (config, binding, tracker) = tracker()?;
    record_activity("project new", Some(&args.id), None, catalog);
    let id = pystr::strip(&args.id).to_lowercase();
    if !valid_id(&id) {
        return Err("project id: lowercase letters, digits and dashes only".to_owned());
    }
    let wip = ProjectLever::Wip.parse(&args.wip).map_err(|error| error.to_string())?;
    let priority = ProjectLever::Priority.parse(&args.priority).map_err(|error| error.to_string())?;
    let issues = open_issues(&tracker, &binding)?;
    if issues.iter().any(|(task, _)| task.objective && task.project.as_deref() == Some(id.as_str())) {
        return Err(format!("the project '{id}' exists already"));
    }
    let git = Git::resolve();
    let repo = match args.repo.as_deref().filter(|repo| !repo.is_empty()) {
        None => None,
        Some(value) => {
            if args.folder.is_empty() {
                return Err("--repo needs --folder: the dossier is the project's folder".to_owned());
            }
            let full = repo_argument(value, &config.owner)?;
            let url = repo_url(&full);
            if let Ok(git) = &git
                && let Some(origin) = regia_git::other_origin(git, Path::new(&args.folder), &url)
            {
                return Err(format!("the folder has an origin already ({origin}), other than {url}: no repo created"));
            }
            Some(full)
        }
    };
    let products = args.products.as_deref().map(|value| products_value("project new", value).and_then(|value| products_list(&value, &config.owner)));
    let products = products.transpose()?;
    // Before the label and the issue: when the repo cannot be made, no Objective is left without its dossier.
    if let Some(full) = &repo {
        let description = truncated(&format!("[{PROTOTYPE_NAME}] {}", args.title), REPO_DESCRIPTION);
        tracker.create_private_repo(full, &description).map_err(tracker_error)?;
        if let Err(error) = tracker.add_repo_topic(full, PROTOTYPE_NAME) {
            eprintln!("{} ({error})", catalog.message_with("project-new-topic-not-added", "repo", full));
        }
    }
    let label = binding.project_label(&id);
    tracker.create_label(&label, &args.color, &truncated(&args.title, LABEL_DESCRIPTION)).map_err(tracker_error)?;
    let playbook = args.playbook.clone().unwrap_or_else(|| default_playbook().to_owned());
    let mut registry = vec![
        (registry_key("id"), id.clone()),
        (registry_key("folder"), args.folder.clone()),
        (registry_key("playbook"), playbook),
        (ProjectLever::Priority.key(), priority.to_string()),
        (ProjectLever::Wip.key(), wip.to_string()),
        (registry_key("auto_defaults"), args.auto_defaults.clone()),
    ];
    if let Some(full) = &repo {
        registry.push((registry_key("repo"), full.clone()));
    }
    if let Some(products) = &products {
        let joined = if products.is_empty() { no_products_word().to_owned() } else { products.join(", ") };
        registry.push((registry_key("products"), joined));
    }
    let (objective, done, mandate, to_come) = (
        catalog.message("project-objective-heading"),
        catalog.message("project-definition-of-done-heading"),
        catalog.message("project-mandate-heading"),
        catalog.message("project-mandate-to-come"),
    );
    let words = ObjectiveWords { objective: &objective, definition_of_done: &done, mandate: &mandate, mandate_to_come: &to_come };
    let body = objective_body(&registry, &words, &args.objective, &args.definition_of_done, &args.mandate);
    let labels = [label, binding.objective_label().to_owned()];
    let printed = tracker.create_issue(&format!("[{id}] {}", args.title), &body, &labels).map_err(tracker_error)?;
    let number = created_issue_number(&printed).ok_or_else(|| format!("gh issue create printed no issue's URL: {printed}"))?;
    set_status(&tracker, &binding, number, Status::Approved)?;
    eprintln!("{}", catalog.message_with("project-new-field-not-written", "project", &id));
    let clock = Clock::read()?;
    let created = if args.folder.is_empty() {
        Vec::new()
    } else {
        let tracker_repo = format!("{}/{}", config.owner, config.repo);
        let date = clock.local_day().format("%Y-%m-%d").to_string();
        let values = ScaffoldValues { id: &id, title: &args.title, objective: number, repo: &tracker_repo, date: &date };
        write_scaffold(&args.folder, &values)?
    };
    let mut linked = None;
    if let Some(full) = &repo {
        let folder = Path::new(&args.folder);
        let git = git.map_err(|error| link_error(full, &LinkError::Init(format!("git cannot run: {error}"))))?;
        let link = regia_git::link(&git, folder, &repo_url(full)).map_err(|error| link_error(full, &error))?;
        let first = (link == Linked::Init).then(|| {
            let files: Vec<DossierPath> = created.iter().map(|(_, path)| path.clone()).collect();
            let message = Text::parse(&catalog.message_with("project-first-commit", "project", &id)).map_err(|error| error.to_string());
            match message {
                Ok(message) => first_push_sentence(&regia_git::first_push(&git, folder, &files, &message, clock.commit_date()), catalog),
                Err(error) => first_push_sentence(&FirstPush::Failed { step: PushStep::Commit, reason: error }, catalog),
            }
        });
        linked = Some((full.clone(), link, first));
    }
    let shown: Vec<String> = created.iter().map(|(shown, _)| shown.clone()).collect();
    if args.json {
        let mut out = json!({"project": id, "objective": number, "scaffold": shown});
        if let Some((full, link, first)) = &linked {
            out["repo"] = json!(full);
            out["git"] = json!(match link {
                Linked::Init => "init",
                Linked::Existing => "existing",
            });
            out["first_push"] = json!(first);
        }
        print_json(&out)?;
        return Ok(0);
    }
    println!("{} {}", catalog.message_with("project-new-project", "project", &id), catalog.message_with("project-new-objective", "number", &number.to_string()));
    for path in &shown {
        println!("  {}", catalog.message_with("project-new-created", "path", path));
    }
    if let Some((full, link, first)) = &linked {
        let folder = match link {
            Linked::Init => catalog.message("project-new-folder-init"),
            Linked::Existing => catalog.message("project-new-folder-existing"),
        };
        let origin = catalog.message_with("project-new-origin", "url", &repo_url(full));
        println!("  {} {folder} {origin}", catalog.message_with("project-new-repo", "repo", full));
        if let Some(first) = first {
            println!("  {}", catalog.message_with("project-new-first-push", "outcome", first));
        }
    }
    Ok(0)
}

/// The error of a link that failed: the repo exists by then, and the message says so.
fn link_error(full: &str, error: &LinkError) -> String {
    match error {
        LinkError::Init(reason) => format!("repo {full} created, but git init in the folder failed: {reason}"),
        LinkError::Remote(reason) => format!("repo {full} created, but git remote add failed: {reason}"),
    }
}

/// The outcome of the first push as the sentence the prototype gives it, in the user's language.
fn first_push_sentence(outcome: &FirstPush, catalog: &Catalog) -> String {
    match outcome {
        FirstPush::Pushed(sha) => catalog.message_with("project-first-push-pushed", "sha", sha),
        FirstPush::Nothing => catalog.message("project-first-push-nothing"),
        FirstPush::NoBranch(reason) => catalog.message_with("project-first-push-no-branch", "reason", reason),
        FirstPush::Failed { step, reason } => {
            let id = match step {
                PushStep::Add => "project-first-push-add-failed",
                PushStep::Commit => "project-first-push-commit-failed",
                PushStep::Push => "project-first-push-push-failed",
            };
            catalog.message_with(id, "reason", reason)
        }
    }
}

/// `folder` as Python's `pathlib` holds it: its separators the platform's, and no `.` part.
fn pathlib(folder: &str) -> PathBuf {
    Path::new(folder).components().filter(|component| !matches!(component, Component::CurDir)).collect()
}

/// The scaffold written in `folder`, as the prototype writes it: each model only where nothing is at its path, its
/// folder made first; then the prototype's empty folders and the moviola's, then the moviola's rule. Each file written,
/// as the prototype names it and as a path of the dossier.
fn write_scaffold(folder: &str, values: &ScaffoldValues<'_>) -> Result<Vec<(String, DossierPath)>, String> {
    let base = pathlib(folder).join(scaffold_folder());
    let files = scaffold_files(values);
    let mut written = Vec::new();
    let mut write = |file: &regia_core::ScaffoldFile| -> Result<(), String> {
        let path = file.segments.iter().fold(base.clone(), |path, segment| path.join(segment));
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        fs::write(&path, &file.text).map_err(|error| format!("{}: {error}", path.display()))?;
        let relative = format!("{}/{}", scaffold_folder(), file.segments.join("/"));
        written.push((path.display().to_string(), DossierPath::parse(&relative).map_err(|error| error.to_string())?));
        Ok(())
    };
    for file in files.iter().filter(|file| !file.added) {
        write(file)?;
    }
    for name in scaffold_folders() {
        let path = base.join(&name);
        fs::create_dir_all(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    for file in files.iter().filter(|file| file.added) {
        write(file)?;
    }
    Ok(written)
}

/// The integer of a lever in the registry, as Python's `int()` reads it; none when it is missing or not a number.
fn lever_value(registry: &Registry, lever: ProjectLever) -> Option<i64> {
    registry.get(lever.key()).and_then(python_int)
}

/// `project set`: the keys of the registry in the Objective's body, the rest of the body as it was, and when a
/// lever changes a signed comment with the before and the after (#156 of the dossier).
fn set(args: &SetArgs, catalog: &Catalog) -> Result<u8, String> {
    let (config, binding, tracker) = tracker()?;
    let mut keys: Vec<(&str, String)> = Vec::new();
    if let Some(value) = &args.repo {
        keys.push((registry_key("repo"), repo_argument(value, &config.owner)?));
    }
    if let Some(value) = &args.products {
        let products = products_list(&products_value("project set", value)?, &config.owner)?;
        keys.push((registry_key("products"), if products.is_empty() { no_products_word().to_owned() } else { products.join(", ") }));
    }
    let mut levers: Vec<(ProjectLever, u64)> = Vec::new();
    for (lever, value) in [(ProjectLever::Wip, &args.wip), (ProjectLever::Priority, &args.priority)] {
        if let Some(value) = value {
            levers.push((lever, lever.parse(value).map_err(|error| error.to_string())?));
        }
    }
    if keys.is_empty() && levers.is_empty() {
        return Err("nothing to set: pass --repo, --products, --wip and/or --priority".to_owned());
    }
    let id = pystr::strip(&args.project).to_lowercase();
    // The first open Objective of the project, in the tracker's order.
    let (number, body) = open_issues(&tracker, &binding)?
        .into_iter()
        .find(|(task, _)| task.objective && task.project.as_deref() == Some(id.as_str()))
        .map(|(task, body)| (task.number, body))
        .ok_or_else(|| format!("unknown project '{id}'"))?;
    // A lever written twice would be rewritten where it is first and read where it is last.
    for (lever, _) in &levers {
        if registry_key_count(&body, lever.key()) > 1 {
            return Err(format!("the registry of '{id}' has the key '{}' more than once: fix it by hand in the Objective", lever.key()));
        }
    }
    let before = Registry::parse(&body);
    // A lever already at its value is not written again.
    let changes: Vec<(ProjectLever, Option<String>, u64)> = levers
        .iter()
        .filter(|(lever, value)| lever_value(&before, *lever) != i64::try_from(*value).ok())
        .map(|(lever, value)| (*lever, before.get(lever.key()).map(str::to_owned), *value))
        .collect();
    keys.extend(changes.iter().map(|(lever, _, value)| (lever.key(), value.to_string())));
    let pairs: Vec<(&str, &str)> = keys.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let new_body = registry_with(&body, &pairs).ok_or_else(|| format!("the issue has no block <!-- {} -->", registry_marker()))?;
    let written = !args.dry_run && new_body != body;
    if written {
        tracker.edit_body(number, &new_body).map_err(tracker_error)?;
    }
    let changed: Vec<String> = changes
        .iter()
        .map(|(lever, before, after)| format!("{} {} → {after}", lever.key(), before.as_deref().filter(|before| !before.is_empty()).unwrap_or(NONE)))
        .collect();
    let changed = changed.join(", ");
    let comment = if written && !changes.is_empty() { levers_comment(&tracker, number, &changed, catalog) } else { None };
    let after = Registry::parse(&new_body);
    let (repo, products) = (after.repo(), after.products(&config.owner));
    let (wip, priority) = (lever_value(&after, ProjectLever::Wip), lever_value(&after, ProjectLever::Priority));
    if args.json {
        print_json(&json!({
            "project": id, "objective": number, "repo": repo, "products": products, "wip": wip, "priority": priority,
            "written": written, "comment": comment,
        }))?;
        return Ok(0);
    }
    let shown = |value: Option<String>| value.unwrap_or_else(|| NONE.to_owned());
    let products = if products.is_empty() { NONE.to_owned() } else { products.join(", ") };
    let mut line = format!(
        "{} (#{number}): {} {} {} {}",
        catalog.message_with("project-set-project", "project", &id),
        catalog.message_with("project-set-repo", "value", &shown(repo)),
        catalog.message_with("project-set-products", "value", &products),
        catalog.message_with("project-set-wip", "value", &shown(wip.map(|wip| wip.to_string()))),
        catalog.message_with("project-set-priority", "value", &shown(priority.map(|priority| priority.to_string()))),
    );
    if args.dry_run {
        line.push(' ');
        line.push_str(&catalog.message("project-set-dry-run"));
    }
    println!("{line}");
    if written && !changes.is_empty() {
        println!("  {}", catalog.message_with("project-set-levers-changed", "changes", &changed));
    }
    Ok(0)
}

/// The comment that records the levers on the Objective, signed as every comment of the CLI, and its id. The body is
/// written by then: a failure here is a warning, and no comment.
fn levers_comment(tracker: &GhTracker, number: u64, changed: &str, catalog: &Catalog) -> Option<u64> {
    let body = catalog.message_with("project-levers-comment", "changes", changed);
    let session = session();
    // Read once, before signing; a failure leaves no project, and is told after the write.
    let project = tracker.labels(number).map(|labels| project_of_labels(&labels));
    let clock = Clock::read().ok();
    let step_of_this_session = match (&project, &clock) {
        (Ok(Some(project)), Some(clock)) => is_this_session_s_step(clock, project, session.as_deref()),
        _ => false,
    };
    let role = SignatureRole::of_comment(false, step_of_this_session, None);
    let printed = match tracker.add_comment(number, &sign(&body, role, session.as_deref())) {
        Ok(printed) => printed,
        Err(error) => {
            eprintln!("{} ({error})", catalog.message("project-levers-comment-failed"));
            return None;
        }
    };
    let id = comment_id(pystr::strip(&printed));
    if let (Some(id), Some(clock)) = (id, &clock)
        && let Err(error) = record_own(clock, id, &project, session.as_deref())
    {
        eprintln!("{} ({error})", catalog.message_with("comment-id-not-recorded", "id", &id.to_string()));
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::current_exe().unwrap().parent().unwrap().join(format!("project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn values() -> ScaffoldValues<'static> {
        ScaffoldValues { id: "gamma", title: "A title", objective: 9, repo: "example/tracker", date: "2026-01-10" }
    }

    #[test]
    fn an_id_is_lowercase_letters_digits_and_dashes() {
        for id in ["a", "a-b", "0", "9x-"] {
            assert!(valid_id(id), "{id}");
        }
        for id in ["", "-a", "A", "a b", "a_b", "è"] {
            assert!(!valid_id(id), "{id}");
        }
    }

    #[test]
    fn a_description_is_cut_in_characters() {
        assert_eq!(truncated("àèìòù", 3), "àèì");
        assert_eq!(truncated("ab", 100), "ab");
    }

    /// The contract's word for no products becomes the prototype's, and any other value passes as it is.
    #[test]
    fn none_is_the_contract_s_word_for_no_products() {
        assert_eq!(products_value("project set", "none").unwrap(), no_products_word());
        assert_eq!(products_value("project new", "a/b, c").unwrap(), "a/b, c");
        assert_eq!(products_value("project set", "None").unwrap(), "None");
        assert_eq!(products_value("project set", "a=b").unwrap(), "a=b");
    }

    /// A second run writes nothing: each file is there, and a file that was there before is left as it is and not
    /// listed. The folders are made, the moviola's too, and an empty moviola reads as none.
    #[test]
    fn the_scaffold_writes_only_what_is_not_there() {
        let root = scratch("twice");
        let folder = root.display().to_string();
        let existing = root.join(scaffold_folder()).join(&scaffold_files(&values())[2].segments[0]);
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(&existing, "mine\n").unwrap();
        let first = write_scaffold(&folder, &values()).unwrap();
        assert_eq!(first.len(), 4, "three models and the moviola's rule");
        assert_eq!(fs::read_to_string(&existing).unwrap(), "mine\n");
        assert_eq!(first.last().unwrap().1.as_str(), format!("{}/moviola/.gitignore", scaffold_folder()));
        for name in scaffold_folders() {
            assert!(root.join(scaffold_folder()).join(name).is_dir());
        }
        assert!(write_scaffold(&folder, &values()).unwrap().is_empty(), "nothing the second time");
        let project = regia_core::moviola::Project { id: "gamma".to_owned(), folder: root.clone(), title: None };
        let summary = regia_core::moviola::portfolio(&[project]).unwrap();
        assert_eq!((summary[0].milestones, summary[0].directions, summary[0].last.is_none()), (0, 0, true));
        fs::remove_dir_all(&root).unwrap();
    }

    /// The folder as pathlib holds it: no `.` part, and the platform's separators.
    #[test]
    fn a_folder_is_named_as_pathlib_names_it() {
        assert_eq!(pathlib("./proj"), PathBuf::from("proj"));
        assert_eq!(pathlib("proj/./x/"), Path::new("proj").join("x"));
        assert_eq!(pathlib("."), PathBuf::new());
    }
}
