mod reads;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand};
use chrono::{DateTime, FixedOffset, Utc};
use regia_agents::{
    Assignment, Diversity, Env, Https, LaunchRequest, Policy, ProbeConsent, Receipt, Role, ServiceId, Settings, SurfaceKind, UsageProbes,
};
use regia_core::{Brief, BriefOptions, BriefTask};
use regia_exec::{Effort, ExecError, ModelId, Program};
use regia_facade::{Engine, FacadeError, Printed, PythonPrototype, Table};
use regia_git::{Checkpoint, DossierSync, GitDate, Outcome};
use regia_i18n::{Catalog, Language};
use regia_identity::{BIN_NAME, config_dir, env_var};
use regia_runtime::{dossier_dir, load_brief, project_folder};
use regia_store::{MIN_INTERVAL_S, Reading, Store};
use serde_json::{Value, json};

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
    /// What is left of Claude's and Codex's windows, read only, and only with the user's consent to the
    /// usage probes. Exits 0, also when a value is unknown or the probes are off.
    Budget(BudgetArgs),
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

#[derive(clap::Args)]
struct BudgetArgs {
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
    /// Asks the providers at most once per interval, and keeps the last valid value.
    #[arg(long)]
    cache: bool,
    /// Seconds between two real readings, with `--cache`.
    #[arg(long, default_value_t = MIN_INTERVAL_S)]
    min_interval: u64,
}

/// `checkpoint`, natively: the arguments of the facade table (`table/checkpoint.toml`). It is not a
/// subcommand of [`Cli`]: while its group runs on the prototype, the facade takes its argv as it is.
#[derive(Parser)]
#[command(name = "checkpoint")]
struct CheckpointArgs {
    #[arg(long)]
    project: String,
    /// A file to commit besides the named ones, relative to the project's folder. Repeatable.
    #[arg(long)]
    file: Vec<String>,
    /// The project's folder, in place of the one its registry names.
    #[arg(long)]
    folder: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// The commands of the table this binary implements besides its own subcommands.
const NATIVE_THROUGH_THE_TABLE: [&str; 4] = ["checkpoint", "projects", "task show", "task list"];

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
        Commands::Budget(args) => {
            if let Some(prototype) = route(&table, "budget", true, catalog)? {
                let mut argv = Vec::new();
                if args.json {
                    argv.push("--json".to_owned());
                }
                if args.cache {
                    argv.extend(["--cache".to_owned(), "--min-interval".to_owned(), args.min_interval.to_string()]);
                }
                return passthrough(&table, &prototype, "budget", &argv, catalog);
            }
            let reading = budget(&args, &settings()?)?;
            let value = serde_json::to_value(&reading).map_err(|error| error.to_string())?;
            if args.json {
                print_json(&value)?;
            } else {
                for line in flat(&value) {
                    println!("{line}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::External(argv) => {
            let Some((command, args)) = table.split_command(&argv) else {
                return Err(format!("unknown command `{}`", argv.first().map_or("", String::as_str)));
            };
            let native = NATIVE_THROUGH_THE_TABLE.contains(&command.as_str());
            match route(&table, &command, native, catalog)? {
                Some(prototype) => passthrough(&table, &prototype, &command, args, catalog),
                None => match reads::run(&command, args, catalog) {
                    Some(outcome) => outcome.map(ExitCode::from),
                    None => checkpoint(args).map(ExitCode::from),
                },
            }
        }
    }
}

/// `checkpoint`, natively: the named files of the dossier committed and pushed, with no lock. Its exit
/// is the prototype's: 1 when it could not even try, a project whose folder could not be read, and for a
/// wrong argument; 0 otherwise, a failed push included.
fn checkpoint(args: &[String]) -> Result<u8, String> {
    let args = match CheckpointArgs::try_parse_from(std::iter::once("checkpoint").chain(args.iter().map(String::as_str))) {
        Ok(args) => args,
        Err(error) => {
            let _ = error.print();
            let shown = matches!(error.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion);
            return Ok(if shown { 0 } else { 1 });
        }
    };
    let clock = Clock::read()?;
    let message = format!("{}: checkpoint {}", args.project, clock.now.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%SZ"));
    // An empty `--folder` is none, as for the prototype.
    let folder = match args.folder.clone().filter(|folder| !folder.is_empty()) {
        Some(folder) => Ok(folder),
        None => project_folder(&args.project).map_err(|error| error.to_string()),
    };
    let dossier = match folder {
        Ok(folder) => regia_git::sync(&Checkpoint { folder: &folder, message: &message, files: &args.file, date: clock.commit_date() }),
        Err(error) => DossierSync::unavailable(None, format!("the project's folder was not read: {error}")),
    };
    let ok = dossier.outcome != Outcome::Unavailable;
    let exit = u8::from(!ok && dossier.folder.is_none());
    let value = json!({"ok": ok, "project": args.project, "dossier": dossier});
    if args.json {
        print_json(&value)?;
    } else {
        for line in flat(&value) {
            println!("{line}");
        }
    }
    Ok(exit)
}

/// The product's clock: `REGIA_NOW` when it is set, which fixes it, and the machine's otherwise.
#[derive(Debug)]
struct Clock {
    now: DateTime<FixedOffset>,
    fixed: bool,
}

impl Clock {
    /// A `REGIA_NOW` that is not an instant with its offset is an error, never the machine's clock in
    /// its place.
    fn read() -> Result<Clock, String> {
        let name = env_var("NOW");
        Clock::of(&name, std::env::var(&name).ok())
    }

    fn of(name: &str, value: Option<String>) -> Result<Clock, String> {
        match value.filter(|value| !value.is_empty()) {
            Some(value) => DateTime::parse_from_rfc3339(&value)
                .map(|now| Clock { now, fixed: true })
                .map_err(|_| format!("{name} is `{value}`, not an ISO 8601 instant with its offset")),
            None => Ok(Clock { now: Utc::now().fixed_offset(), fixed: false }),
        }
    }

    /// With the clock fixed, a commit is made at its instant, so it has the same id on every run;
    /// otherwise git dates it, as the user's own git would.
    fn commit_date(&self) -> Option<GitDate> {
        self.fixed.then(|| GitDate::new(self.now.timestamp(), self.now.offset().local_minus_utc()))
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

/// `budget`, natively: the probes as the settings consent to them, from the cache with `--cache`. With
/// the probes refused nothing is read, not even the cache, which holds what probes read.
fn budget(args: &BudgetArgs, settings: &Settings) -> Result<Reading, String> {
    let probes = UsageProbes::new(settings.probe_consent(), Env::of_this_process(), Https::new());
    if !args.cache || probes.consent() == ProbeConsent::Refused {
        return Ok(probes.read());
    }
    let store = Store::open().map_err(|error| error.to_string())?;
    store.budget(args.min_interval, false, || Ok::<_, std::convert::Infallible>(probes.read())).map_err(|error| error.to_string())
}

/// A JSON value as `key: value` lines, one per leaf, the keys of the contract joined by dots: the text
/// form of a command whose words would otherwise need a catalog.
fn flat(value: &Value) -> Vec<String> {
    fn walk(prefix: &str, value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(fields) if !fields.is_empty() => {
                for (key, value) in fields {
                    let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                    walk(&path, value, out);
                }
            }
            Value::Object(_) => out.push(format!("{prefix}: -")),
            Value::Null => out.push(format!("{prefix}: -")),
            Value::String(text) => out.push(format!("{prefix}: {text}")),
            other => out.push(format!("{prefix}: {other}")),
        }
    }
    let mut out = Vec::new();
    walk("", value, &mut out);
    out
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

    /// Acceptance 2 of #463, through the command: with no answer to the probes, or a refusal, both
    /// providers read `probe_off`, also with `--cache`, which is then not read either.
    #[test]
    fn with_the_probes_refused_the_budget_is_probe_off() {
        let args = |argv: &[&str]| match parse(&[&["budget"][..], argv].concat()).map(|cli| cli.command) {
            Ok(Commands::Budget(args)) => args,
            Ok(_) => panic!("not budget"),
            Err(error) => panic!("{error}"),
        };
        for settings in [Settings::default(), Settings::parse("probes = \"refused\"\n").unwrap()] {
            for argv in [&["--json"][..], &["--json", "--cache", "--min-interval", "60"]] {
                let json = serde_json::to_value(budget(&args(argv), &settings).unwrap()).unwrap();
                assert_eq!(json["provider"]["claude"]["status"], "probe_off", "{argv:?}");
                assert_eq!(json["provider"]["codex"]["status"], "probe_off", "{argv:?}");
                assert!(json.get("cache").is_none(), "the cache was not read: {json}");
            }
        }
        assert_eq!(args(&[]).min_interval, 300);
        assert!(parse(&["budget", "--min-interval", "soon"]).is_err());
    }

    /// `TestFinestraNonAperta.test_il_testo_di_regia_budget_dice_non_aperta`: the text form says the
    /// window is not open, with the contract's keys.
    #[test]
    fn the_text_of_budget_says_a_window_is_not_open() {
        let windows = vec![("5h".to_owned(), regia_store::Window::read(0.0, None)), ("7d".to_owned(), regia_store::Window::read(30.0, Some(("2099-01-01T00:00+00:00".to_owned(), 60))))];
        let claude = regia_store::ProviderUsage::new("claude", Some("max".to_owned()), windows, None, None);
        let reading = Reading { generated: None, providers: vec![("claude".to_owned(), claude)], cache: None };
        let lines = flat(&serde_json::to_value(&reading).unwrap());
        for line in ["provider.claude.windows.5h.open: false", "provider.claude.windows.5h.used_pct: 0.0", "provider.claude.windows.7d.used_pct: 30.0", "generated: -"] {
            assert!(lines.iter().any(|candidate| candidate == line), "{line} not in {lines:?}");
        }
    }

    /// `checkpoint` in text is the contract's keys, one leaf a line, as `budget` prints them: the
    /// prototype's sentence (`checkpoint alpha: niente da committare`) would need a catalog.
    #[test]
    fn the_text_of_checkpoint_is_the_contract_s_keys() {
        let dossier = DossierSync::unavailable(None, "the project's folder was not read: project 'alpha' is unknown".to_owned());
        let lines = flat(&json!({"ok": false, "project": "alpha", "dossier": dossier}));
        assert_eq!(
            lines,
            [
                "dossier.ahead: -",
                "dossier.branch: -",
                "dossier.files: []",
                "dossier.folder: -",
                "dossier.outcome: unavailable",
                "dossier.reason: the project's folder was not read: project 'alpha' is unknown",
                "dossier.sha: -",
                "ok: false",
                "project: alpha",
            ]
        );
    }

    /// A wrong argument exits 1, as in the prototype, whose parser exits 1 and not argparse's 2; help
    /// exits 0. None of them reads the clock, the tracker or a repo.
    #[test]
    fn a_wrong_argument_to_checkpoint_exits_1() {
        let run = |argv: &[&str]| checkpoint(&argv.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>()).unwrap();
        assert_eq!(run(&["--project", "alpha", "--no-push"]), 1, "checkpoint has no --no-push");
        assert_eq!(run(&["--file", "a.md"]), 1, "--project is required");
        assert_eq!(run(&["--project"]), 1);
        assert_eq!(run(&["--help"]), 0);
        assert!(CheckpointArgs::try_parse_from(["checkpoint", "--project", "alpha", "--file", "a", "--file=b", "--folder", "x", "--json"]).is_ok());
    }

    /// `REGIA_NOW` fixes the clock and dates the commit with its offset; a value that is not an instant
    /// is an error; without it, git dates the commit.
    #[test]
    fn the_fixed_clock_dates_the_commit() {
        let fixed = Clock::of("REGIA_NOW", Some("2026-01-10T09:00:00Z".to_owned())).unwrap();
        assert_eq!(fixed.now.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string(), "2026-01-10T09:00:00Z");
        assert_eq!(fixed.commit_date().map(|date| date.as_env()).as_deref(), Some("@1768035600 +0000"));
        let east = Clock::of("REGIA_NOW", Some("2026-01-10T10:30:00+01:30".to_owned())).unwrap();
        assert_eq!(east.commit_date().map(|date| date.as_env()).as_deref(), Some("@1768035600 +0130"));
        assert!(Clock::of("REGIA_NOW", Some("2026-01-10".to_owned())).unwrap_err().contains("REGIA_NOW"));
        assert!(Clock::of("REGIA_NOW", None).unwrap().commit_date().is_none());
        assert!(Clock::of("REGIA_NOW", Some(String::new())).unwrap().commit_date().is_none(), "empty is unset");
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
