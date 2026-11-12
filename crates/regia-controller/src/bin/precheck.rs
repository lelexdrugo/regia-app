//! The precheck as a command, for the conformance harness: a line of diagnostics, then the outcome line,
//! and the outcome's exit code. It takes the prototype's arguments and prints its line's events and
//! decision; the budget and the series of a stall join the line with #466, and only then can it stand in
//! for the prototype's precheck. It never reads its standard input. The clock is `REGIA_NOW` when set,
//! which also fixes the state's.

use std::process::ExitCode;

use regia_controller::precheck::{Fault, Verdict, diagnostic, evaluate, live_sources, parse_options};
use regia_identity::{env_var, prototype_env_var};
use regia_store::WallTime;

/// The instant of the tick, and whether it is fixed; a fixed one that does not read is an error.
fn clock() -> Result<Option<WallTime>, String> {
    let name = env_var("NOW");
    match std::env::var(&name).ok().filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => WallTime::parse_iso(&value).map(Some).ok_or_else(|| format!("{name} is `{value}`, not an instant")),
    }
}

fn run(argv: &[String], fixed: Result<Option<WallTime>, String>) -> Verdict {
    let options = match parse_options(argv) {
        Ok(options) => options,
        Err(fault) => return Verdict::of_fault(&fault),
    };
    let sources = fixed.and_then(|fixed| live_sources(options.deadline_s, fixed));
    match sources {
        Ok(sources) => evaluate(&sources, &options),
        Err(detail) => Verdict::of_fault(&Fault::internal(detail)),
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args_os().skip(1).map(|arg| arg.to_string_lossy().into_owned()).collect();
    let variable = |name: &str| std::env::var(name).unwrap_or_else(|_| "-".to_owned());
    let cwd = std::env::current_dir().map_or_else(|_| "-".to_owned(), |dir| dir.display().to_string());
    let runtime = prototype_env_var("RUNTIME");
    let gh = env_var("GH");
    println!(
        "{}",
        diagnostic(&[("rust", env!("CARGO_PKG_VERSION").to_owned()), ("gh", variable(&gh)), (runtime.as_str(), variable(&runtime)), ("cwd", cwd)])
    );
    let fixed = clock();
    let now = fixed.clone().ok().flatten().unwrap_or_else(WallTime::now);
    let (exit, line) = run(&argv, fixed).line(now);
    println!("{line}");
    ExitCode::from(exit)
}
