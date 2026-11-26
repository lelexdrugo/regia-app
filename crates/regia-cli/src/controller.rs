//! `controller run` (F27 of the dossier's plan, #469): the resident controller as a command of the binary, in place of
//! the prototype's `python tools/controller.py`, with the names of `docs/contract/commands.md` ("controller run").
//!
//! - **Its arguments** exit 1 when they do not hold, as the prototype's wrong usage does; the other commands keep the
//!   2 of clap.
//! - **The surfaces**, by `--launcher`: a step on `auto`, `cli` or `orca`, or on nothing with `fake`. A scheduled chat
//!   opens only where its owner sees it, the format's surface: never with `cli`, whose chat stays to open, and on
//!   nothing with `fake`. The token's expiry is read around a wake-up that starts a session, never around a fake one.
//! - **`--shadow`** runs on a fresh copy of the state folder, beside it, and launches nothing: every surface is `Dry`.
//! - **The clock** is the machine's. `REGIA_NOW` fixes it for a run of one round only: with the clock standing still
//!   the wait between two rounds would never end.
//! - **Ctrl+C** raises the flag the resident looks at: the wait ends at once, the lock is released, and the exit is 0.
//!
//! What the user reads is the catalog's (`controller.ftl`), rendered from the resident's typed notices and exit.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use regia_agents::{Auto, Cli as CliSurface, Dry, Env, Https, LaunchSurface, Orca, Policy, Role, SurfaceKind, UsageProbes};
use regia_controller::engine::native::{NativePrecheck, Processes, TrackerFolders, random_id};
use regia_controller::interrupt::stop_on_interrupt;
use regia_controller::resident::{Config, Console, Exit, HostPorts, Notice, Resident, StopSleep, Summary};
use regia_controller::schedule::{ChatAgent, ChatSurface, Operations, Source, StoreSchedule, chat_surface_kind};
use regia_controller::shadow;
use regia_exec::{Effort, ModelId};
use regia_i18n::{Catalog, Language};
use regia_identity::env_var;
use regia_runtime::dossier_dir;
use regia_store::{ManualClock, Store, SystemClock, default_folder};

use crate::clock::Clock;
use crate::reads::parse;

#[derive(Parser)]
#[command(name = "controller", args_override_self = true)]
struct ControllerArgs {
    #[command(subcommand)]
    command: ControllerCommand,
}

#[derive(Subcommand)]
enum ControllerCommand {
    /// Runs the rounds, every `--interval` seconds: the precheck, and a step for each project with work.
    Run(RunArgs),
}

#[derive(clap::Args)]
struct RunArgs {
    /// One round, then exits.
    #[arg(long)]
    once: bool,
    /// On a fresh copy of the state folder, beside it, launching nothing.
    #[arg(long)]
    shadow: bool,
    /// Exits after this many rounds; 0 for ever.
    #[arg(long, default_value_t = 0)]
    rounds: u64,
    /// Seconds between two rounds. Without it: 900.
    #[arg(long)]
    interval: Option<f64>,
    /// How a step starts: `fake` starts nothing.
    #[arg(long, value_parser = ["auto", "cli", "orca", "fake"], default_value = "auto")]
    launcher: String,
    /// Only this project; repeatable.
    #[arg(long = "project")]
    projects: Vec<String>,
    /// The model of a step. Without it: the `replanning` role's.
    #[arg(long)]
    model: Option<String>,
    /// The effort of a step. Without it: the `replanning` role's.
    #[arg(long)]
    effort: Option<String>,
    /// Steps launched per round. Without it: 1.
    #[arg(long)]
    max_steps: Option<usize>,
    /// Past this a step no longer counts as alive. Without it: 3600.
    #[arg(long)]
    step_limit_s: Option<f64>,
    /// Without it: 120.
    #[arg(long)]
    precheck_timeout: Option<f64>,
    /// With Claude's budget unknown, no minimal session wakes the token up.
    #[arg(long)]
    no_wake: bool,
    /// The scheduled operations. Without it: the regia's home's configuration, when there is a home.
    #[arg(long)]
    config: Option<PathBuf>,
}

/// `controller` with its arguments: its exit code, or the error to print.
pub(crate) fn run(args: &[String], catalog: &Catalog) -> Result<u8, String> {
    match parse::<ControllerArgs>("controller", args) {
        Ok(ControllerArgs { command: ControllerCommand::Run(args) }) => controller_run(&args, catalog),
        Err(code) => Ok(code),
    }
}

/// The resident's configuration from the arguments, on `folder`: the prototype's defaults where they say nothing, the
/// agent of a step by the `replanning` role, and the arguments over it.
fn config_of(args: &RunArgs, folder: PathBuf, policy: &Policy) -> Result<Config, String> {
    let mut config = Config::new(folder);
    config.rounds = if args.once { 1 } else { args.rounds };
    config.interval_s = args.interval.unwrap_or(config.interval_s);
    let mut options = config.options.clone().assigned(&policy.assign(Role::Replanning, None));
    options.projects.clone_from(&args.projects);
    options.max_steps = args.max_steps.unwrap_or(options.max_steps);
    options.step_limit_s = args.step_limit_s.unwrap_or(options.step_limit_s);
    options.precheck_timeout_s = args.precheck_timeout.unwrap_or(options.precheck_timeout_s);
    options.no_wake = args.no_wake;
    if let Some(model) = &args.model {
        options.model = Some(ModelId::parse(model).map_err(|error| error.to_string())?);
    }
    if let Some(effort) = &args.effort {
        options.effort = Some(Effort::parse(effort).map_err(|error| error.to_string())?);
    }
    config.options = options;
    Ok(config)
}

/// A surface of `kind` on this process's environment, shared by the rounds.
fn surface_of(kind: SurfaceKind, env: &Env) -> Arc<dyn LaunchSurface + Send + Sync> {
    match kind {
        SurfaceKind::Cli => Arc::new(CliSurface::new(env.clone())),
        SurfaceKind::Orca => Arc::new(Orca::new(env.clone())),
        SurfaceKind::Auto => Arc::new(Auto::new(Orca::new(env.clone()), CliSurface::new(env.clone()))),
        SurfaceKind::Dry => Arc::new(Dry::new()),
    }
}

/// `controller run`.
fn controller_run(args: &RunArgs, catalog: &Catalog) -> Result<u8, String> {
    let settings = crate::settings()?;
    let consent = settings.probe_consent();
    let policy = Policy::new(settings).map_err(|error| error.to_string())?;
    let live = default_folder().map_err(|error| error.to_string())?;
    let mut config = config_of(args, live.clone(), &policy)?;
    // The prototype checks its options before anything else: no copy for a run that would not start.
    if config.check().is_err() {
        eprintln!("{}", catalog.message("controller-bad-option"));
        return Ok(1);
    }
    let clock = Clock::read()?;
    let fixed = match clock.fixed() {
        true if config.rounds != 1 => return Err(catalog.message_with("controller-fixed-clock", "variable", &env_var("NOW"))),
        true => Some(clock.wall()?),
        false => None,
    };
    let folder = if args.shadow {
        let copy = shadow::fresh(&live, &shadow::folder_of(&live).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        println!("{}", catalog.message_with("controller-shadow", "folder", &copy.folder.display().to_string()));
        if let Some(holder) = &copy.released {
            println!("{}", catalog.message_with("controller-shadow-released", "holder", holder));
        }
        copy.folder
    } else {
        live
    };
    config.folder.clone_from(&folder);
    let store_clock: Arc<dyn regia_store::Clock> = match fixed {
        Some(at) => Arc::new(ManualClock::fixed(at)),
        None => Arc::new(SystemClock),
    };
    let store = Store::at(&folder).map_err(|error| error.to_string())?;
    let store = match fixed {
        Some(at) => store.with_clock(ManualClock::fixed(at)),
        None => store,
    };
    let env = Env::of_this_process();
    let dry = args.shadow || args.launcher == "fake";
    let kind = if dry { SurfaceKind::Dry } else { SurfaceKind::parse(&args.launcher).unwrap_or(SurfaceKind::Auto) };
    let chat = match (dry, kind, chat_surface_kind()) {
        (true, _, _) => ChatSurface::Open(Arc::new(Dry::new())),
        (false, SurfaceKind::Cli, _) | (false, _, None) => ChatSurface::None,
        (false, _, Some(chat)) => ChatSurface::Open(surface_of(chat, &env)),
    };
    let token_expiry: Option<Arc<dyn Fn() -> Option<String> + Send + Sync>> = (!dry).then(|| {
        let env = env.clone();
        Arc::new(move || UsageProbes::new(consent, env.clone(), Https::new()).claude_token_expiry()) as Arc<dyn Fn() -> Option<String> + Send + Sync>
    });
    let stop = stop_on_interrupt()?;
    let agent = ChatAgent { service: config.options.service, model: config.options.model.clone(), effort: config.options.effort };
    let ports = HostPorts {
        store: store.clone(),
        clock: store_clock,
        sleep: Arc::new(StopSleep::new(Arc::clone(&stop))),
        precheck: Arc::new(NativePrecheck { fixed, folder: Some(folder.clone()) }),
        folders: Arc::new(TrackerFolders { folder: Some(folder.clone()) }),
        liveness: Arc::new(Processes::new()),
        surface: surface_of(kind, &env),
        ids: Arc::new(random_id),
        consent,
        token_expiry,
        console: Arc::new(Terminal { language: Language::detect() }),
        stop,
    };
    let resident = Resident::new(config, ports);
    let state = Arc::new(StoreSchedule::new(store, resident.holder().clone()));
    let home = dossier_dir();
    let source = args.config.clone().map_or_else(|| Source::of_home(home.as_deref()), Source::File);
    let operations = Operations::new(source, state, chat).with_home(home).with_agent(agent);
    let exit = resident.with_before_precheck(Box::new(operations)).run();
    report(&exit, catalog);
    Ok(exit.code())
}

/// What the user reads of a resident's exit.
fn report(exit: &Exit, catalog: &Catalog) {
    match exit {
        Exit::Done => {}
        Exit::Stopped => println!("{}", catalog.message("controller-stopped")),
        Exit::AnotherController { holder } => {
            let mut line = catalog.message("controller-another");
            if let Some(holder) = holder {
                line.push(' ');
                line.push_str(&catalog.message_with("controller-another-holder", "holder", holder));
            }
            eprintln!("{line}");
        }
        Exit::LockLost(error) => eprintln!("{}", catalog.message_with("controller-lock-lost", "error", error)),
        Exit::BadConfig(_) => eprintln!("{}", catalog.message("controller-bad-option")),
        Exit::Failed(error) => eprintln!("{}", catalog.message_with("controller-failed", "error", error)),
    }
}

/// The resident's notices on the terminal, in the user's language: a round's summary and a wake-up on the standard
/// output, the rest on the standard error, as the prototype prints them. The catalog is made per notice, which comes
/// once a round: it is not shared between threads.
struct Terminal {
    language: Language,
}

impl Console for Terminal {
    fn notice(&self, notice: &Notice) {
        let catalog = Catalog::new(self.language);
        match notice {
            Notice::Ticked(record) => println!("{}", round_line(&Summary::of(record), &catalog)),
            Notice::BudgetAlarm { rounds, since, reason } => {
                let mut line = catalog.message_with("controller-budget-alarm", "rounds", &rounds.to_string());
                if let Some(since) = since {
                    line.push(' ');
                    line.push_str(&catalog.message_with("controller-budget-since", "since", since));
                }
                line.push_str(&catalog.message("controller-budget-stand-still"));
                if let Some(reason) = reason {
                    line.push(' ');
                    line.push_str(&catalog.message_with("controller-budget-reason", "reason", reason));
                }
                eprintln!("{line}");
            }
            Notice::TickFailed { round, error } => eprintln!("{} {error}", catalog.message_with("controller-round-failed", "round", &round.to_string())),
            Notice::Wakeup => println!("{}", catalog.message("controller-wakeup")),
            Notice::LogNotWritten(error) => eprintln!("{}", catalog.message_with("controller-log-not-written", "error", error)),
            Notice::StatusNotWritten(error) => eprintln!("{}", catalog.message_with("controller-status-not-written", "error", error)),
        }
    }
}

/// A round as the prototype's summary line has it: the round and its instant, the precheck, the decision, then the
/// launches, the operations that did something, the token's wake-up and the wake from a suspension.
fn round_line(summary: &Summary, catalog: &Catalog) -> String {
    let text = |value: &Option<String>| value.clone().unwrap_or_else(|| "-".to_owned());
    let mut line = format!("{} {}: ", catalog.message_with("controller-round", "round", &summary.round.to_string()), summary.t);
    match &summary.precheck {
        Some(precheck) => {
            let exit = precheck.exit.map_or_else(|| "-".to_owned(), |exit| exit.to_string());
            line.push_str(&format!("{} {}/{}", catalog.message_with("controller-precheck", "exit", &exit), text(&precheck.outcome), text(&precheck.reason)));
        }
        None => line.push_str(&catalog.message("controller-no-precheck")),
    }
    line.push_str(&format!(" -> {}", text(&summary.decision)));
    if !summary.launched.is_empty() {
        let launched: Vec<String> = summary.launched.iter().map(|(project, surface)| format!("{project} ({surface})")).collect();
        line.push_str(&format!(" [{}]", launched.join(", ")));
    }
    if !summary.operations.is_empty() {
        let operations: Vec<String> = summary.operations.iter().map(|(name, action)| format!("{name} {action}")).collect();
        line.push(' ');
        line.push_str(&catalog.message_with("controller-operations", "list", &operations.join(", ")));
    }
    if let Some(wake) = &summary.wake {
        let mut outcome = if wake.ok {
            catalog.message("controller-wake-ok")
        } else {
            let error: String = text(&wake.error).chars().take(120).collect();
            catalog.message_with("controller-wake-failed", "error", &error)
        };
        if wake.renewed {
            outcome.push_str(&catalog.message("controller-wake-renewed"));
        }
        line.push(' ');
        line.push_str(&catalog.message_with("controller-wake", "outcome", &outcome));
    }
    if summary.wakeup {
        line.push(' ');
        line.push_str(&catalog.message("controller-after-suspension"));
    }
    line
}

#[cfg(test)]
mod tests {
    use regia_controller::resident::{PrecheckSummary, WakeSummary};

    use super::*;

    fn parsed(argv: &[&str]) -> Result<RunArgs, clap::Error> {
        let argv = ["controller", "run"].iter().chain(argv).copied();
        ControllerArgs::try_parse_from(argv).map(|args| match args.command {
            ControllerCommand::Run(args) => args,
        })
    }

    /// The names of the facade table, and the prototype's defaults where an argument says nothing.
    #[test]
    fn the_arguments_are_the_table_s_and_the_defaults_the_prototype_s() {
        let args = parsed(&["--once", "--shadow", "--launcher", "fake", "--project", "alpha", "--project", "beta", "--no-wake"]).unwrap();
        assert!(args.once && args.shadow && args.no_wake);
        assert_eq!((args.launcher.as_str(), args.projects.as_slice()), ("fake", &["alpha".to_owned(), "beta".to_owned()][..]));
        let policy = Policy::new(regia_agents::Settings::default()).unwrap();
        let config = config_of(&parsed(&[]).unwrap(), PathBuf::from("state"), &policy).unwrap();
        assert_eq!((config.rounds, config.interval_s, config.options.max_steps), (0, 900.0, 1));
        assert_eq!((config.options.step_limit_s, config.options.precheck_timeout_s), (3600.0, 120.0));
        let argv = ["--rounds", "3", "--interval", "60", "--max-steps", "2", "--step-limit-s", "10", "--precheck-timeout", "5", "--model", "haiku", "--effort", "low"];
        let config = config_of(&parsed(&argv).unwrap(), PathBuf::from("state"), &policy).unwrap();
        assert_eq!((config.rounds, config.interval_s, config.options.max_steps), (3, 60.0, 2));
        assert_eq!((config.options.model.as_ref().map(ModelId::as_str), config.options.effort), (Some("haiku"), Some(Effort::Low)));
        assert_eq!(config_of(&parsed(&["--once", "--rounds", "5"]).unwrap(), PathBuf::from("state"), &policy).unwrap().rounds, 1);
        for wrong in [&["--launcher", "dry"][..], &["--rounds", "-1"], &["--max-steps", "x"], &["--nope"]] {
            assert!(parsed(wrong).is_err(), "{wrong:?}");
        }
    }

    /// The prototype's summary line: the round and its instant, the precheck, the decision, then what the round did.
    /// The format's words are data: the line in Italian, with the prototype's words, is a case of the command's test.
    #[test]
    fn a_round_reads_as_the_prototype_s_summary() {
        let summary = Summary {
            round: 2,
            t: "2026-10-22T07:00:00Z".to_owned(),
            precheck: Some(PrecheckSummary { exit: Some(0), outcome: Some("new-activity".to_owned()), reason: Some("ready".to_owned()) }),
            decision: Some("launch".to_owned()),
            launched: vec![("alpha".to_owned(), "cli".to_owned())],
            operations: vec![("dailies".to_owned(), "launched".to_owned())],
            wake: Some(WakeSummary { ok: true, error: None, renewed: true }),
            wakeup: true,
        };
        let english = round_line(&summary, &Catalog::new(Language::En));
        assert_eq!(
            english,
            "round 2 2026-10-22T07:00:00Z: precheck exit 0 new-activity/ready -> launch [alpha (cli)] [operations: dailies launched] \
             [wake-up: ok, token renewed] (wake-up from a suspension)"
        );
        let bare = Summary { precheck: None, launched: Vec::new(), operations: Vec::new(), wake: Some(WakeSummary { ok: false, error: None, renewed: false }), wakeup: false, ..summary };
        assert_eq!(round_line(&bare, &Catalog::new(Language::En)), "round 2 2026-10-22T07:00:00Z: precheck not run -> launch [wake-up: failed (-)]");
    }
}
