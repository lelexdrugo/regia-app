//! `declare`, natively, and the record each command that touches a task or a project leaves in the
//! sessions' log (R8 of the dossier's conformance study): with the arguments of the facade table
//! (`table/declare.toml`), local only, no network and no tracker. The command a line names is the
//! prototype's, its words from the table: a datum the moviola attributes on its first word.

use clap::Parser;
use regia_facade::Table;
use regia_i18n::Catalog;
use regia_identity::prototype_env_var;
use regia_store::{Declaration, ProjectId, SessionIds, declarations_folder, step_role, write_declaration};
use serde_json::json;

use crate::clock::Clock;
use crate::print_json;
use crate::reads::{parse, python_int};

#[derive(Parser)]
#[command(name = "declare", args_override_self = true)]
struct DeclareArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    task: Option<String>,
    /// Who works: orchestrator, worker, reviewer…, if known. Without it, the prototype's variable's.
    #[arg(long)]
    role: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// `command` natively, if this module has it: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    let outcome = match command {
        "declare" => parse::<DeclareArgs>(command, args).map(|args| declare(&args, catalog)),
        _ => return None,
    };
    Some(outcome.unwrap_or_else(Ok))
}

/// `declare`: one line in the log, or an error that says why there is none.
fn declare(args: &DeclareArgs, catalog: &Catalog) -> Result<u8, String> {
    let task = args.task.as_deref().map(|task| python_int(task).ok_or_else(|| format!("argument --task: invalid int value: '{task}'"))).transpose()?;
    let not_recorded = |error: String| format!("declaration not recorded: {error}");
    let project = ProjectId::new(args.project.as_str()).map_err(|error| not_recorded(error.to_string()))?;
    let role = args.role.clone().filter(|role| !role.is_empty()).or_else(role_of_the_environment);
    let command = prototype_command("declare")?;
    let declaration = Declaration { command: &command, project: Some(project.as_str()), task, role: role.as_deref() };
    let now = Clock::read()?.wall()?;
    let folder = declarations_folder().map_err(|error| not_recorded(error.to_string()))?;
    let written = write_declaration(&folder, &SessionIds::from_env(), &declaration, now).map_err(|error| not_recorded(error.to_string()))?;
    if args.json {
        print_json(&json!({"ok": true, "session": written.session, "project": project.as_str(), "task": task, "ts": written.ts}))?;
    } else {
        let mut line = catalog.message_with("declare-done", "project", project.as_str());
        if let Some(task) = task {
            line.push_str(&format!(" #{task}"));
        }
        line.push(' ');
        line.push_str(&catalog.message_with("declare-session", "session", &written.session));
        println!("{line}");
    }
    Ok(0)
}

/// The words of `command` in the prototype's language, as the table names it: its `python` argv.
fn prototype_command(command: &str) -> Result<String, String> {
    Table::embedded().python_argv(command, &[]).map(|words| words.join(" ")).map_err(|error| error.to_string())
}

/// The role the prototype's variable names, if it is set and not empty.
fn role_of_the_environment() -> Option<String> {
    std::env::var(prototype_env_var("RUOLO")).ok().filter(|role| !role.is_empty())
}

/// Records that this session touched `project` or `task` with `command`, which has passed the checks of
/// the prototype's parser and not yet run (`_registra_attivita`). A step's commands are the orchestrator's;
/// the others take the role of the prototype's variable. Best effort: it never changes the command's
/// output or exit code, and only with the prototype's debug variable set does a failure print a line on
/// stderr.
pub(crate) fn record_activity(command: &str, project: Option<&str>, task: Option<i64>, catalog: &Catalog) {
    if let Err(error) = try_record_activity(command, project, task)
        && std::env::var(prototype_env_var("DEBUG")).is_ok_and(|debug| !debug.is_empty())
    {
        eprintln!("{} ({error})", catalog.message("activity-not-recorded"));
    }
}

fn try_record_activity(command: &str, project: Option<&str>, task: Option<i64>) -> Result<(), String> {
    let words = prototype_command(command)?;
    let role = if command.starts_with("step ") { Some(step_role().to_owned()) } else { role_of_the_environment() };
    let declaration = Declaration { command: &words, project, task, role: role.as_deref() };
    let now = Clock::read()?.wall()?;
    let folder = declarations_folder().map_err(|error| error.to_string())?;
    write_declaration(&folder, &SessionIds::from_env(), &declaration, now).map(|_| ()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands a line names are the prototype's words, from the table.
    #[test]
    fn a_line_names_the_prototype_s_command() {
        let table = Table::embedded();
        for command in ["declare", "comment", "lease acquire", "lease heartbeat", "lease release", "step start", "step renew", "step finish"] {
            let words = prototype_command(command).unwrap();
            assert!(!words.is_empty() && words != command, "{command}: {words}");
            assert_eq!(table.python_argv(command, &[]).unwrap().join(" "), words);
        }
        assert!(prototype_command("no such command").is_err());
    }

    /// A failure to record is an error of its own, which the command never sees.
    #[test]
    fn a_command_unknown_to_the_table_is_not_recorded() {
        assert!(try_record_activity("no such command", None, None).is_err());
    }
}
