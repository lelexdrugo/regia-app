//! `init`, `init plan` and `init derive`, natively: the plan of the tracker and of this machine, and the local
//! configuration derived from a tracker that has everything, with the arguments of the facade table
//! (`table/setup.toml`). `init` alone is the plan with the prototype's defaults. Not subcommands of the CLI's parser:
//! while their group runs on the prototype, the facade takes their argv as it is. `init apply` stays with the prototype.
//!
//! The JSON is the table's; an item's detail and the text are the catalog's, in Italian the prototype's words. The
//! exit codes are the prototype's (`conformita.md` §7.2 of the dossier): 0 when the plan was computed, and for `derive`
//! when the file was written or was the same already; 1 when the plan cannot be computed; 2 for a wrong argument, as
//! argparse exits; 3 when the destination exists and differs, without `--overwrite`; 4 when the tracker is not ready.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use regia_core::init::{Detail, GhSaid, InitReads, Item, Outcome, Plan, PlanOptions, Scope, SkillCopy, default_project_title, default_repo, plan, skill_homes};
use regia_core::{local_config_file, pystr};
use regia_exec::RepoSlug;
use regia_github::GhInit;
use regia_i18n::Catalog;
use regia_identity::{prototype_env_var, state_dir};
use regia_runtime::dossier_dir;
use serde_json::{Value, json};

use crate::print_json;

#[derive(Parser)]
#[command(name = "init", args_override_self = true)]
struct AloneArgs {
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Default)]
struct TrackerArgs {
    /// Without it: the user `gh` is signed in as.
    #[arg(long, value_parser = owner)]
    owner: Option<String>,
    /// Without it: the prototype's tracker.
    #[arg(long, value_parser = repo)]
    repo: Option<String>,
    /// The Project's title. Without it: the prototype's.
    #[arg(long)]
    project_title: Option<String>,
}

#[derive(Parser)]
#[command(name = "init plan", args_override_self = true)]
struct PlanArgs {
    #[command(flatten)]
    tracker: TrackerArgs,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
#[command(name = "init derive", args_override_self = true)]
struct DeriveArgs {
    #[command(flatten)]
    tracker: TrackerArgs,
    /// The file to write. Without it: the local configuration of the state folder.
    #[arg(long)]
    destination: Option<String>,
    /// Replaces a destination that says something else.
    #[arg(long)]
    overwrite: bool,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// `command` natively, if this module has it: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let outcome = match command {
        "init" => parse::<AloneArgs>(command, args).map(|args| plan_command(&TrackerArgs::default(), args.json, catalog)),
        "init plan" => parse::<PlanArgs>(command, args).map(|args| plan_command(&args.tracker, args.json, catalog)),
        "init derive" => parse::<DeriveArgs>(command, args).map(|args| derive_command(&args, catalog)),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
}

/// The arguments, or the exit of a wrong one: 2, as the prototype's argparse exits, and 0 for help.
fn parse<T: Parser>(name: &str, args: &[String]) -> Result<T, u8> {
    T::try_parse_from(std::iter::once(name).chain(args.iter().map(String::as_str))).map_err(|error| {
        let _ = error.print();
        u8::try_from(error.exit_code()).unwrap_or(2)
    })
}

/// An owner GitHub allows, the only kind that reaches argv.
fn owner(text: &str) -> Result<String, String> {
    RepoSlug::parse(&format!("{text}/x")).map(|_| text.to_owned()).map_err(|error| error.to_string())
}

/// A repo's name GitHub allows, the only kind that reaches argv.
fn repo(text: &str) -> Result<String, String> {
    RepoSlug::parse(&format!("x/{text}")).map(|_| text.to_owned()).map_err(|error| error.to_string())
}

/// `init plan`, and `init` alone: the plan, in JSON or in text; exit 0 whatever its items say.
fn plan_command(tracker: &TrackerArgs, json: bool, catalog: &Catalog) -> Result<u8, String> {
    let mut gh = GhInit::new();
    let plan = compute(tracker, &mut gh)?;
    if json {
        print_json(&plan_json(&plan, &gh, catalog))?;
    } else {
        let items: Vec<&Item> = plan.items.iter().collect();
        print!("{}", plan_text(&plan, &items, catalog));
    }
    Ok(0)
}

/// `init derive`: the plan, then the local configuration if the tracker is ready.
fn derive_command(args: &DeriveArgs, catalog: &Catalog) -> Result<u8, String> {
    let mut gh = GhInit::new();
    let plan = compute(&args.tracker, &mut gh)?;
    // An empty destination is none, as for the prototype.
    let given = args.destination.as_deref().filter(|destination| !destination.is_empty());
    let (code, outcome) = derive(&plan, given, args.overwrite, args.json, catalog)?;
    if args.json {
        let destination = match given {
            Some(given) => given.to_owned(),
            None => default_destination()?.display().to_string(),
        };
        let mut value = plan_json(&plan, &gh, catalog);
        value["outcome"] = json!(outcome);
        value["exit_code"] = json!(code);
        value["destination"] = json!(destination);
        print_json(&value)?;
    }
    Ok(code)
}

/// The plan of the tracker the arguments name, with the skills of the prototype's dossier and the copies in the home.
fn compute(tracker: &TrackerArgs, reads: &mut dyn InitReads) -> Result<Plan, String> {
    let options = PlanOptions {
        owner: tracker.owner.clone(),
        repo: tracker.repo.clone().unwrap_or_else(default_repo),
        project_title: tracker.project_title.clone().unwrap_or_else(default_project_title),
        writes: false,
        skills: dossier_dir().map(|dossier| dossier.join("skills")),
        home: home(),
    };
    plan(&options, reads).map_err(|error| format!("the plan cannot be computed: {error}"))
}

/// The home that holds the agents' copies of the skills, found as Python's `Path.home()` finds it: `USERPROFILE` on
/// Windows, else `HOMEDRIVE` and `HOMEPATH`; `HOME` elsewhere. Not the system's known folder, which ignores the
/// environment a run is given.
fn home() -> PathBuf {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let found = if cfg!(windows) {
        var("USERPROFILE").map(PathBuf::from).or_else(|| Some(PathBuf::from(var("HOMEDRIVE").unwrap_or_default()).join(var("HOMEPATH")?)))
    } else {
        var("HOME").map(PathBuf::from)
    };
    found.unwrap_or_else(regia_exec::neutral_folder)
}

/// The local configuration of the state folder, where every command looks for it first.
fn default_destination() -> Result<PathBuf, String> {
    let dir = state_dir().ok_or_else(|| format!("no state folder: set {}", prototype_env_var("RUNTIME")))?;
    Ok(local_config_file(&dir))
}

/// What `derive` did: its exit code and its outcome in the table's words. The configuration is written only from a
/// ready tracker, and over a file that says something else only with `overwrite`; a file that says the same is left
/// as it is. What it says goes on stdout, or on stderr with `--json`; the refusals always on stderr.
fn derive(plan: &Plan, given: Option<&str>, overwrite: bool, json: bool, catalog: &Catalog) -> Result<(u8, &'static str), String> {
    let say = |line: String| if json { eprintln!("{line}") } else { println!("{line}") };
    let Some(config) = plan.local_config() else {
        eprintln!("{}", catalog.message("init-derive-not-ready"));
        let open: Vec<&Item> =
            plan.items.iter().filter(|item| matches!(item.scope, Scope::Environment | Scope::Tracker) && item.outcome != Outcome::Present).collect();
        eprint!("{}", plan_text(plan, &open, catalog));
        return Ok((4, "not_ready"));
    };
    let destination = match given {
        Some(given) => PathBuf::from(given),
        None => default_destination()?,
    };
    let shown = python_path(&destination);
    if destination.exists() {
        let existing = fs::read_to_string(&destination).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
        if existing.is_some_and(|existing| config.same_as(&existing)) {
            say(catalog.message_with("init-derive-same", "path", &shown));
            return Ok((0, "same"));
        }
        if !overwrite {
            eprintln!("{}", catalog.message_with("init-derive-different", "path", &shown));
            return Ok((3, "different"));
        }
    }
    write_atomically(&destination, &config.file_text()).map_err(|error| format!("{shown}: {error}"))?;
    say(catalog.message_with("init-derive-written", "path", &shown));
    Ok((0, "written"))
}

/// A path as Python's `pathlib` prints it: its parts joined with the system's separator.
fn python_path(path: &Path) -> String {
    path.components().collect::<PathBuf>().display().to_string()
}

/// Writes `text` at `path` as the prototype does: into a new temporary file beside it, flushed to the disk, then
/// moved into its place, so a reader finds the old file or the new one, never half of one. The folder is created if
/// it is missing; a temporary file that could not be moved is removed.
fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    fs::create_dir_all(&folder)?;
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let (temporary, mut file) = new_temporary(&folder, &name)?;
    let written = file.write_all(text.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    let moved = written.and_then(|()| fs::rename(&temporary, path));
    if moved.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    moved
}

/// A file of `folder` no one else has, named after `name`.
fn new_temporary(folder: &Path, name: &str) -> io::Result<(PathBuf, fs::File)> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.subsec_nanos());
    for attempt in 0..100_u32 {
        let path = folder.join(format!("{name}.{}-{stamp:x}-{attempt}.tmp", std::process::id()));
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no temporary name is free"))
}

/// The plan in the table's JSON: the tracker it names, whether it is ready, its items, and the calls made to `gh`.
fn plan_json(plan: &Plan, reads: &dyn InitReads, catalog: &Catalog) -> Value {
    let items: Vec<Value> = plan
        .items
        .iter()
        .map(|item| json!({"item": item.name, "scope": item.scope, "outcome": item.outcome, "action": item.action, "detail": detail(&item.detail, catalog)}))
        .collect();
    json!({
        "owner": plan.owner,
        "repo": plan.repo,
        "project_title": plan.project_title,
        "ready": plan.ready(),
        "items": items,
        "gh_calls": reads.calls(),
    })
}

/// The plan in text, for `items`: a line for the tracker, then each item with its detail on the next line.
fn plan_text(plan: &Plan, items: &[&Item], catalog: &Catalog) -> String {
    let owner = plan.owner.as_deref().filter(|owner| !owner.is_empty()).unwrap_or("?");
    let header = catalog.message_with("init-plan-header", "tracker", &format!("{owner}/{}", plan.repo));
    let mut out = format!("{header}{}\n", catalog.message_with("init-plan-project", "title", &plan.project_title));
    for item in items {
        let outcome = catalog.message(&format!("init-outcome-{}", item.outcome.id()));
        let action = catalog.message_with("init-plan-action", "action", &catalog.message(&format!("init-action-{}", item.action.id())));
        out.push_str(&format!("  [{outcome:<8}] {}  {action}\n             {}\n", item.name, detail(&item.detail, catalog)));
    }
    out
}

/// An item's detail in the catalog's words.
fn detail(detail: &Detail, catalog: &Catalog) -> String {
    let message = |id: &str| catalog.message(id);
    let with = |id: &str, name: &str, value: &str| catalog.message_with(id, name, value);
    let said = |said: &GhSaid| match said {
        GhSaid::Text(text) => text.clone(),
        GhSaid::Exit(code) => with("init-gh-exit", "code", &code.to_string()),
    };
    let options = |count: usize| with("init-field-options", "count", &count.to_string());
    let extra = |names: &[String]| if names.is_empty() { String::new() } else { with("init-status-options-extra", "names", &names.join(", ")) };
    let signed_in = |login: &str| with("init-gh-signed-in-as", "login", login);
    match detail {
        Detail::GhSignedOut { said: told, scopes } => {
            // On one line: what gh said may have line breaks.
            let told: Vec<String> = said(told).split(pystr::is_space).filter(|word| !word.is_empty()).map(str::to_owned).collect();
            format!("{}{}", with("init-gh-signed-out", "said", &told.join(" ")), with("init-gh-sign-in", "scopes", &scopes.join(",")))
        }
        Detail::GhUnreadable { error } => with("init-gh-unreadable", "error", error),
        Detail::GhScopesHidden { login } => format!("{}{}", signed_in(login), message("init-gh-scopes-hidden")),
        Detail::GhScopesMissing { login, missing, scopes, add } => {
            let or = format!(" {} ", message("init-gh-scope-or"));
            let missing: Vec<String> = missing.iter().map(|alternatives| alternatives.join(&or)).collect();
            format!(
                "{}{}{}{}",
                signed_in(login),
                with("init-gh-scope-missing", "scopes", &missing.join(", ")),
                with("init-gh-scopes-seen", "scopes", &scopes.join(", ")),
                with("init-gh-refresh", "scopes", &add.join(","))
            )
        }
        Detail::GhSignedIn { login, scopes } => format!("{}{}", signed_in(login), with("init-gh-scopes", "scopes", &scopes.join(", "))),
        Detail::UncheckedSignedOut => message("init-unchecked-signed-out"),
        Detail::RepoMissing { repo } => with("init-repo-missing", "repo", repo),
        Detail::RepoFailed { repo, said: told } => format!("{repo}: {}", said(told)),
        Detail::RepoPublic { repo } => with("init-repo-public", "repo", repo),
        Detail::RepoPrivate { repo } => with("init-repo-private", "repo", repo),
        Detail::ProjectFailed { title, owner, said: told } => {
            format!("{}{}: {}", with("init-project-named", "title", title), with("init-of-owner", "owner", owner), said(told))
        }
        Detail::UncheckedProjectUnread => message("init-unchecked-project-unread"),
        Detail::ProjectMissing { title, owner } => {
            format!("{}{}{}", with("init-project-none", "title", title), with("init-of-owner", "owner", owner), message("init-would-create"))
        }
        Detail::OwnerUnknown { owner } => format!("{}{}", with("init-owner-unknown", "owner", owner), message("init-would-create")),
        Detail::StatusNeedsProject => message("init-status-needs-project"),
        Detail::ProjectFieldWithFirstProject { field } => with("init-project-field-born", "field", field),
        Detail::ProjectsAmbiguous { title, numbers } => {
            let numbers: Vec<String> = numbers.iter().map(|number| format!("#{number}")).collect();
            format!("{} ({}){}", with("init-projects-titled", "title", title), numbers.join(", "), message("init-projects-rename"))
        }
        Detail::UncheckedAmbiguous => message("init-unchecked-ambiguous"),
        Detail::ProjectFound { number, id, url } => format!("#{number} {id} {url}"),
        Detail::StatusFieldMissing => message("init-status-field-missing"),
        Detail::StatusFieldKind { kind } => with("init-status-field-kind", "kind", kind),
        Detail::StatusOptionsMissing { missing, found, extra: beyond } => format!(
            "{}{}{}",
            with("init-status-options-missing", "names", &missing.join(", ")),
            with("init-status-options-found", "names", &found.join(", ")),
            extra(beyond)
        ),
        Detail::StatusPresent { field_id, options: count, extra: beyond } => format!("{field_id}, {}{}", options(*count), extra(beyond)),
        Detail::ProjectFieldKind { field, kind } => format!("{}{}", with("init-field-named", "field", field), with("init-field-kind", "kind", kind)),
        Detail::ProjectFieldPresent { field_id, options: count } => format!("{field_id}, {}", options(*count)),
        Detail::ProjectFieldToCreate { field, projects } => format!(
            "{}{}: {}",
            with("init-project-field-missing", "field", field),
            with("init-would-create-with", "options", &options(projects.len())),
            projects.join(", ")
        ),
        Detail::UncheckedRepoUnread => message("init-unchecked-repo-unread"),
        Detail::LabelNeedsRepo => message("init-label-needs-repo"),
        Detail::LabelsFailed { repo, said: told } => format!("{}: {}", with("init-labels-of", "repo", repo), said(told)),
        Detail::LabelPresent { description } => description.clone(),
        Detail::LabelMissing { description, color } => format!("{description}{}", with("init-label-color", "color", color)),
        Detail::NoSkills { folder: Some(folder) } => with("init-no-skills", "folder", &folder.display().to_string()),
        Detail::NoSkills { folder: None } => message("init-no-skills-folder"),
        Detail::SkillCopiesSame => message("init-skill-copies-same"),
        Detail::SkillCopies { name, claude, agents, install } => {
            let copies: Vec<String> = skill_homes()
                .into_iter()
                .zip([claude, agents])
                .map(|(agent, copy)| format!("~/.{agent}/skills/{name}: {}", message(copy_id(*copy))))
                .collect();
            format!("{}{}", copies.join(", "), with("init-skill-install", "command", install))
        }
        Detail::ProjectViews => message("init-project-views"),
    }
}

fn copy_id(copy: SkillCopy) -> &'static str {
    match copy {
        SkillCopy::Missing => "init-copy-missing",
        SkillCopy::Same => "init-copy-same",
        SkillCopy::Different => "init-copy-different",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_prints_as_pathlib_prints_it() {
        let separator = std::path::MAIN_SEPARATOR;
        assert_eq!(python_path(Path::new("a/b//c/")), format!("a{separator}b{separator}c"));
    }

    #[test]
    fn the_file_is_moved_into_its_place_and_no_temporary_is_left() {
        // Beside the test's own executable, under `target/`.
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().unwrap().join(format!("init-write-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("deeper").join("config.json");
        write_atomically(&path, "one\n").unwrap();
        write_atomically(&path, "two\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two\n");
        let left: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left, ["config.json"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_owner_or_a_repo_that_github_refuses_is_a_wrong_argument() {
        assert!(owner("example").is_ok() && repo("tracker.v2").is_ok());
        for bad in ["-x", "", "a b", "a/b"] {
            assert!(owner(bad).is_err(), "{bad:?}");
        }
        for bad in ["-x", "", "..", "a/b"] {
            assert!(repo(bad).is_err(), "{bad:?}");
        }
    }
}
