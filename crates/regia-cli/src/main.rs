use std::io::Write;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use regia_core::{Brief, BriefOptions, BriefTask};
use regia_exec::{ExecError, Program};
use regia_facade::{Engine, FacadeError, Printed, PythonPrototype, Table};
use regia_i18n::{Catalog, Language};
use regia_identity::{BIN_NAME, env_var};
use regia_runtime::{dossier_dir, load_brief};
use serde_json::Value;

#[derive(Parser)]
#[command(name = BIN_NAME, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// What waits for you, what is ready, what is running.
    Brief {
        /// Machine-readable output: this is the contract the agents use.
        #[arg(long)]
        json: bool,
        /// Only this project. The graph still reads all of them.
        #[arg(long)]
        project: Option<String>,
    },
    /// Commands not ported yet go to the prototype through the facade table.
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() -> ExitCode {
    let catalog = Catalog::new(Language::detect());
    match run(Cli::parse(), &catalog) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{BIN_NAME}: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli, catalog: &Catalog) -> Result<ExitCode, String> {
    let table = Table::embedded();
    match cli.command {
        Commands::Brief { json, project } => {
            if let Some(prototype) = route(&table, "brief", true, catalog)? {
                let mut args = Vec::new();
                if json {
                    args.push("--json".to_owned());
                }
                if let Some(project) = project {
                    args.extend(["--project".to_owned(), project]);
                }
                return passthrough(&table, &prototype, "brief", &args, catalog);
            }
            let brief =
                load_brief(&BriefOptions { project, ..BriefOptions::default() }).map_err(|error| error.to_string())?;
            eprintln!("{}", catalog.message("brief-partial"));
            if json {
                print_json(&serde_json::to_value(&brief).map_err(|error| error.to_string())?)?;
            } else {
                print_human(&brief, catalog);
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::External(argv) => {
            let Some((command, args)) = table.split_command(&argv) else {
                return Err(format!("unknown command `{}`", argv.first().map_or("", String::as_str)));
            };
            let prototype = route(&table, &command, false, catalog)?
                .expect("a command this binary does not implement runs on the prototype or nowhere");
            passthrough(&table, &prototype, &command, args, catalog)
        }
    }
}

/// Where `command` runs: `Some` with the prototype, `None` in this binary. `REGIA_ENGINE` forces one
/// side, for checks: `python` the prototype, `rust` this binary.
fn route(table: &Table, command: &str, native: bool, catalog: &Catalog) -> Result<Option<PythonPrototype>, String> {
    let name = env_var("ENGINE");
    let forced = match std::env::var(&name).ok().filter(|value| !value.is_empty()).as_deref() {
        None => None,
        Some("python") => Some(Engine::Prototype),
        Some("rust") => Some(Engine::Native),
        Some(other) => return Err(format!("{name} is `{other}`, not `python` or `rust`")),
    };
    let prototype = PythonPrototype::find(dossier_dir().as_deref(), std::env::var(env_var("PYTHON")).ok());
    match table.engine(command, native, prototype.is_some(), forced) {
        Ok(Engine::Native) => Ok(None),
        Ok(Engine::Prototype) => Ok(prototype),
        Err(error) => Err(explain(&error, command, catalog)),
    }
}

/// The catalog's sentence for what the user can act on: no prototype, no interpreter. The rest is the
/// error as it is.
fn explain(error: &FacadeError, command: &str, catalog: &Catalog) -> String {
    let id = match error {
        FacadeError::NotPorted(_) => "facade-not-ported",
        FacadeError::NoPrototype(_) => "facade-no-prototype",
        FacadeError::Exec(ExecError::NotFound { program: Program::Python, .. }) => "facade-no-python",
        _ => return error.to_string(),
    };
    let sentence = catalog.message_with(id, "command", command);
    match error {
        FacadeError::Exec(cause) => format!("{sentence}\n  ({cause})"),
        _ => sentence,
    }
}

/// The prototype's answer, as the table translates it, with its exit code: 3 stays a refusal.
fn passthrough(table: &Table, prototype: &PythonPrototype, command: &str, args: &[String], catalog: &Catalog) -> Result<ExitCode, String> {
    let run = table.run_prototype(prototype, command, args).map_err(|error| explain(&error, command, catalog))?;
    // Its warnings are the only sign that a section was lost, and its errors say why it refused.
    std::io::stderr().write_all(&run.stderr).map_err(|error| error.to_string())?;
    match run.stdout {
        Printed::Json(value) => print_json(&value)?,
        Printed::Text(text) => std::io::stdout().write_all(&text).map_err(|error| error.to_string())?,
    }
    Ok(ExitCode::from(run.exit))
}

fn print_json(value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    println!("{text}");
    Ok(())
}

fn numbers(list: &[u64]) -> String {
    list.iter().map(|number| format!("#{number}")).collect::<Vec<_>>().join(", ")
}

fn print_human(brief: &Brief, catalog: &Catalog) {
    let print_section = |id: &str, tasks: &[BriefTask]| {
        println!("== {}", catalog.message(id));
        if tasks.is_empty() {
            println!("  {}", catalog.message("brief-nothing"));
        }
        for entry in tasks {
            let status = entry.task.status.map_or("-", |status| status.id());
            println!("  #{:<4} [{status}] {}", entry.task.number, entry.task.title);
        }
    };
    if let Some(graph) = &brief.graph {
        for cycle in &graph.cycles {
            println!("  ! {}", catalog.message_with("brief-cycle", "reason", &numbers(cycle)));
        }
        if !graph.truncated.is_empty() {
            println!("  ! {}", catalog.message_with("brief-truncated", "reason", &numbers(&graph.truncated)));
        }
    }
    // A task that needs a session shows there, not twice.
    let short: Vec<BriefTask> = brief
        .waiting_on_owner
        .iter()
        .filter(|entry| !brief.session.contains(entry))
        .cloned()
        .collect();
    print_section("brief-waiting", &short);
    if !brief.session.is_empty() {
        print_section("brief-session", &brief.session);
    }
    print_section("brief-ready", &brief.ready);
    print_section("brief-in-progress", &brief.in_progress);
    println!("== {}", catalog.message("brief-by-project"));
    for (project, counts) in &brief.by_project {
        let counts: Vec<String> = counts.iter().map(|(key, count)| format!("{key} {count}")).collect();
        println!("  {project}: {}", counts.join(", "));
    }
}
