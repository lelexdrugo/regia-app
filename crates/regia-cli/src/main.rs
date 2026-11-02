use std::process::ExitCode;

use clap::{Parser, Subcommand};
use regia_core::{Brief, BriefOptions, BriefTask};
use regia_facade::{PythonPrototype, Table};
use regia_i18n::{Catalog, Language};
use regia_identity::{BIN_NAME, env_var, prototype_env_var};
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
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{BIN_NAME}: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Commands::Brief { json, project } => {
            if std::env::var(env_var("ENGINE")).is_ok_and(|engine| engine == "python") {
                let mut args = Vec::new();
                if json {
                    args.push("--json".to_owned());
                }
                if let Some(project) = project {
                    args.extend(["--project".to_owned(), project]);
                }
                return passthrough("brief", &args);
            }
            let brief = load_brief(&BriefOptions { project }).map_err(|error| error.to_string())?;
            let catalog = Catalog::new(Language::detect());
            eprintln!("{}", catalog.message("brief-partial"));
            if json {
                print_json(&serde_json::to_value(&brief).map_err(|error| error.to_string())?)
            } else {
                print_human(&brief, &catalog);
                Ok(())
            }
        }
        Commands::External(argv) => {
            let (command, args) = argv.split_first().ok_or("missing command")?;
            passthrough(command, args)
        }
    }
}

fn passthrough(command: &str, args: &[String]) -> Result<(), String> {
    let table = Table::embedded();
    if !table.knows_command(command) {
        return Err(format!("unknown command `{command}`"));
    }
    if !args.iter().any(|arg| arg == "--json") {
        return Err("the facade only carries --json output for now".to_owned());
    }
    let dossier = dossier_dir().ok_or_else(|| {
        format!("`{command}` is not ported: set {} to the prototype's dossier", prototype_env_var("HOME"))
    })?;
    let interpreter = std::env::var(env_var("PYTHON")).ok();
    let prototype = PythonPrototype::in_dossier(&dossier, interpreter);
    let value = table
        .run_python_json(&prototype, command, args)
        .map_err(|error| error.to_string())?;
    print_json(&value)
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
