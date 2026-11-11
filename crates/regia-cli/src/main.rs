use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand};
use regia_agents::{
    Assignment, Diversity, Env, LaunchRequest, Policy, Receipt, Role, ServiceId, Settings, SurfaceKind,
};
use regia_core::{Brief, BriefOptions, BriefTask};
use regia_exec::{Effort, ExecError, ModelId, Program};
use regia_facade::{Engine, FacadeError, Printed, PythonPrototype, Table};
use regia_i18n::{Catalog, Language};
use regia_identity::{BIN_NAME, config_dir, env_var};
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
    /// The agent services, as the settings assign them to roles.
    Agents {
        #[command(subcommand)]
        command: AgentsCommand,
    },
    /// Starts an agent's session on a step and returns once it is open, with its receipt in JSON.
    Launch(LaunchArgs),
    /// Commands not ported yet go to the prototype through the facade table.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Subcommand)]
enum AgentsCommand {
    /// What a role gets: service, tier, model, effort, surface, deadline and fallback.
    Show {
        #[arg(long, value_parser = PossibleValuesParser::new(Role::ALL.map(Role::id)))]
        role: String,
        /// The service that wrote the work under review. Without it: the principal.
        #[arg(long, value_parser = PossibleValuesParser::new(ServiceId::ALL.map(ServiceId::id)))]
        writer: Option<String>,
        /// Machine-readable output: this is the contract the agents use.
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args)]
struct LaunchArgs {
    /// The project: lowercase letters, digits and dashes. It names the folder of the logs.
    #[arg(long)]
    project: String,
    /// The folder the agent works in.
    #[arg(long)]
    folder: PathBuf,
    #[command(flatten)]
    prompt: PromptSource,
    /// Where it runs. Without it: the settings' surface, else `cli`.
    #[arg(long, value_parser = PossibleValuesParser::new(SurfaceKind::ALL.map(SurfaceKind::id)))]
    surface: Option<String>,
    /// Without it: the principal.
    #[arg(long, value_parser = PossibleValuesParser::new(ServiceId::ALL.map(ServiceId::id)))]
    service: Option<String>,
    /// Without it: the service's own default.
    #[arg(long)]
    model: Option<String>,
    /// Without it: the service's own default.
    #[arg(long)]
    effort: Option<String>,
}

/// The prompt: one of the two, never both.
#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct PromptSource {
    /// A UTF-8 file, sent byte for byte.
    #[arg(long)]
    prompt_file: Option<PathBuf>,
    #[arg(long)]
    prompt: Option<String>,
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
        Commands::Agents { command: AgentsCommand::Show { role, writer, json } } => {
            let assignment = agents_show(&role, writer.as_deref(), settings()?)?;
            if json {
                print_json(&serde_json::to_value(&assignment).map_err(|error| error.to_string())?)?;
            } else {
                print_assignment(&assignment, catalog);
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Launch(args) => {
            let (kind, request) = launch_request(&args, settings()?)?;
            let launched = regia_agents::surface(kind, Env::of_this_process()).launch(&request);
            let receipt: Receipt = launched.map_err(|error| error.to_string())?;
            print_json(&serde_json::to_value(&receipt).map_err(|error| error.to_string())?)?;
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

/// The user's agent settings in the configuration folder, or the defaults where there are none.
fn settings() -> Result<Settings, String> {
    match config_dir() {
        Some(dir) => Settings::load(&dir).map_err(|error| error.to_string()),
        None => Ok(Settings::default()),
    }
}

/// `agents show`: what `role` gets under `settings`, for work `writer` wrote.
fn agents_show(role: &str, writer: Option<&str>, settings: Settings) -> Result<Assignment, String> {
    let role = Role::parse(role).ok_or_else(|| format!("unknown role `{role}`"))?;
    let writer = writer.map(|writer| ServiceId::parse(writer).ok_or_else(|| format!("unknown service `{writer}`"))).transpose()?;
    Ok(Policy::new(settings).map_err(|error| error.to_string())?.assign(role, writer))
}

/// `launch`, before any effect: the surface the arguments or the settings name, and the request,
/// checked as the prototype checks it.
fn launch_request(args: &LaunchArgs, settings: Settings) -> Result<(SurfaceKind, LaunchRequest), String> {
    let policy = Policy::new(settings).map_err(|error| error.to_string())?;
    let service = match &args.service {
        Some(id) => ServiceId::parse(id).ok_or_else(|| format!("unknown service `{id}`"))?,
        None => policy.principal(),
    };
    let kind = match args.surface.as_deref() {
        Some(name) => SurfaceKind::parse(name).ok_or_else(|| format!("unknown surface `{name}`"))?,
        None => policy.settings().surface.unwrap_or(SurfaceKind::Cli),
    };
    let prompt = match (&args.prompt.prompt, &args.prompt.prompt_file) {
        (Some(prompt), _) => prompt.clone(),
        (None, Some(file)) => std::fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?,
        (None, None) => return Err("no prompt".to_owned()),
    };
    let model = args.model.as_deref().map(ModelId::parse).transpose().map_err(|error| error.to_string())?;
    let effort = args.effort.as_deref().map(Effort::parse).transpose().map_err(|error| error.to_string())?;
    let secrets = policy.settings().service(service).map(|entry| entry.secret_env.clone()).unwrap_or_default();
    let request = LaunchRequest::new(service, &args.project, &args.folder, &prompt)
        .map_err(|error| error.to_string())?
        .model(model)
        .effort(effort)
        .secret_env(secrets);
    Ok((kind, request))
}

/// The assignment as `key: value` lines, with the contract's keys, and the one sentence a reader
/// needs: that the diversity rule fell back to the same service.
fn print_assignment(assignment: &Assignment, catalog: &Catalog) {
    let value = serde_json::to_value(assignment).unwrap_or_default();
    let text = |value: &Value| match value {
        Value::Null => "-".to_owned(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    for key in ["role", "service", "tier", "model", "effort", "surface", "deadline_minutes", "clean_context", "diversity"] {
        println!("{key}: {}", text(&value[key]));
    }
    if let Some(fallback) = value["fallback"].as_object() {
        let parts: Vec<String> = ["service", "tier", "model", "effort"].iter().map(|key| text(&fallback[*key])).collect();
        println!("fallback: {}", parts.join(" "));
    }
    if assignment.diversity == Diversity::Degraded {
        println!("{}", catalog.message("agents-degraded"));
    }
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

#[cfg(test)]
mod tests {
    use regia_agents::{Dry, LaunchSurface};

    use super::*;

    const FOLDER: &str = env!("CARGO_MANIFEST_DIR");
    const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");

    fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once(BIN_NAME).chain(argv.iter().copied()))
    }

    fn launch_args(argv: &[&str]) -> LaunchArgs {
        let argv = [&["launch"][..], argv].concat();
        match parse(&argv).map(|cli| cli.command) {
            Ok(Commands::Launch(args)) => args,
            Ok(_) => panic!("not a launch"),
            Err(error) => panic!("{error}"),
        }
    }

    /// Acceptance 2 of #460, through the command: with one service, the review goes to the same
    /// service in a clean context, and says so.
    #[test]
    fn with_one_service_the_review_is_the_same_service_in_a_clean_context() {
        let one = Settings::parse("principal = \"claude\"\nservices = [\"claude\"]\n").unwrap();
        let json = serde_json::to_value(agents_show("adversarial_review", None, one).unwrap()).unwrap();
        assert_eq!(
            (json["service"].as_str(), json["clean_context"].as_bool(), json["diversity"].as_str()),
            (Some("claude"), Some(true), Some("degraded"))
        );
        for key in ["role", "service", "tier", "model", "effort", "surface", "deadline_minutes", "fallback"] {
            assert!(json.get(key).is_some(), "{key}");
        }
        assert_eq!(json["deadline_minutes"], 20);
        // With both, another service than the writer's.
        assert_eq!(agents_show("adversarial_review", None, Settings::default()).unwrap().service, ServiceId::Codex);
        assert_eq!(agents_show("adversarial_review", Some("codex"), Settings::default()).unwrap().service, ServiceId::Claude);
        assert!(parse(&["agents", "show", "--role", "adversarial_review", "--json"]).is_ok());
        assert!(parse(&["agents", "show", "--role", "nobody"]).is_err() && parse(&["agents", "show"]).is_err());
        assert!(parse(&["agents", "show", "--role", "code", "--writer", "gemini"]).is_err());
    }

    /// The prototype's command line, on the Dry surface: the prompt from a file, the model and the
    /// effort; the service is the principal.
    #[test]
    fn the_command_line_builds_the_request() {
        let argv = ["--project", "trial", "--folder", FOLDER, "--prompt-file", FILE, "--surface", "dry", "--model", "haiku", "--effort", "low"];
        let (kind, request) = launch_request(&launch_args(&argv), Settings::default()).unwrap();
        assert_eq!(kind, SurfaceKind::Dry);
        let receipt = serde_json::to_value(Dry::new().launch(&request).unwrap()).unwrap();
        assert_eq!(
            (receipt["surface"].as_str(), receipt["service"].as_str(), receipt["model"].as_str(), receipt["effort"].as_str()),
            (Some("dry"), Some("claude"), Some("haiku"), Some("low"))
        );
        assert!(receipt["request"]["prompt"].as_str().unwrap().starts_with("[package]"), "the prompt is the file");
        let codex = Settings::parse("principal = \"codex\"\nsurface = \"dry\"\n[service.codex]\nsecret_env = [\"OPENAI_API_KEY\"]\n").unwrap();
        let (kind, request) = launch_request(&launch_args(&["--project", "trial", "--folder", FOLDER, "--prompt", "p"]), codex).unwrap();
        assert_eq!((kind, request.service, request.secret_env.as_slice()), (SurfaceKind::Dry, ServiceId::Codex, &["OPENAI_API_KEY".to_owned()][..]));
        assert_eq!(launch_request(&launch_args(&["--project", "trial", "--folder", FOLDER, "--prompt", "p"]), Settings::default()).unwrap().0, SurfaceKind::Cli);
    }

    /// The prototype's checks: each of these fails before a surface is even chosen.
    #[test]
    fn invalid_arguments_launch_nothing() {
        let base = ["--project", "trial", "--folder", FOLDER, "--prompt", "p"];
        for extra in [["--service", "gemini"], ["--surface", "ssh"]] {
            assert!(parse(&[&["launch"][..], &base, &extra].concat()).is_err(), "{extra:?}");
        }
        assert!(parse(&["launch", "--project", "trial", "--folder", FOLDER]).is_err(), "a prompt is required");
        assert!(parse(&["launch", "--project", "trial", "--folder", FOLDER, "--prompt", "p", "--prompt-file", FILE]).is_err(), "one, not both");
        let missing = format!("{FOLDER}/does-not-exist");
        for argv in [
            vec!["--project", "Trial X", "--folder", FOLDER, "--prompt", "p"],
            vec!["--project", "", "--folder", FOLDER, "--prompt", "p"],
            vec!["--project", "trial", "--folder", FOLDER, "--prompt", "p", "--effort", "alto; rm"],
            vec!["--project", "trial", "--folder", FOLDER, "--prompt", "p", "--model", "a b"],
            vec!["--project", "trial", "--folder", FOLDER, "--prompt", "  "],
            vec!["--project", "trial", "--folder", missing.as_str(), "--prompt", "p"],
            vec!["--project", "trial", "--folder", FOLDER, "--prompt-file", missing.as_str()],
        ] {
            assert!(launch_request(&launch_args(&argv), Settings::default()).is_err(), "{argv:?}");
        }
    }
}
