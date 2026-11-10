//! Local only: the Rust and the Python brief against the real tracker, right now. Issue data is
//! private, so this prints only JSON paths and counts, with project ids and table errors redacted, and
//! writes nothing. The tracker can change between the two reads, hence the retries. Sections the Rust
//! brief declares as not ported are excluded by that declaration.
//!
//!     ./target/debug/live-check --dossier <path> [--python <interpreter>]
//!
//! Run it directly: `cargo run` changes PATH, where both sides look their tools up.

use std::path::PathBuf;
use std::process::ExitCode;

use regia_conformance::{json_diff, regia_binary, take_not_ported, translate_python};
use regia_facade::{PythonPrototype, Table};
use regia_identity::{env_var, prototype_env_var};
use serde_json::Value;

/// A difference path without its value, and without the project id if it has one.
fn redacted(line: &str) -> String {
    let path = line.split(": ").next().unwrap_or(line);
    match path.strip_prefix("$.by_project.") {
        Some(_) => "$.by_project.<project>".to_owned(),
        None => path.to_owned(),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|arg| arg == name).map(|i| args[i + 1].clone());
    let dossier = PathBuf::from(flag("--dossier").expect("--dossier <path>"));
    let prototype = PythonPrototype::in_dossier(&dossier, flag("--python"));
    let cli_args = ["--json".to_owned()];
    let argv = Table::embedded().python_argv("brief", &cli_args).expect("known command");

    for attempt in 1..=3 {
        let output = prototype.launch(&argv).expect("the prototype").run().expect("run the prototype");
        assert!(output.status.success(), "the prototype failed");
        let python: Value = serde_json::from_slice(&output.stdout).expect("JSON from the prototype");
        let mut python = match translate_python("brief", &python) {
            Ok(value) => value,
            Err(error) => {
                // The message can hold a private value, so only that it happened goes out.
                let kind = format!("{error:?}");
                eprintln!("the table does not cover the real output: {}", kind.split(' ').next().unwrap_or(""));
                return ExitCode::FAILURE;
            }
        };

        let args = ["brief".to_owned(), "--json".to_owned()];
        let output = regia_exec::harness::launch(&regia_binary(), &args, &dossier)
            .env(&prototype_env_var("HOME"), &dossier)
            .without(&env_var("ENGINE"))
            .run()
            .expect("run regia");
        assert!(output.status.success(), "regia failed: {}", String::from_utf8_lossy(&output.stderr));
        let mut rust: Value = serde_json::from_slice(&output.stdout).expect("JSON from regia");
        let not_ported = take_not_ported(&mut rust).expect("the marker");

        let object = python.as_object_mut().expect("an object");
        let present: Vec<&str> =
            not_ported.iter().map(String::as_str).filter(|key| object.contains_key(*key)).collect();
        for key in &present {
            object.remove(*key);
        }
        let diff = json_diff(&rust, &python);
        let counts: Vec<String> = ["waiting_on_owner", "ready", "in_progress", "session"]
            .iter()
            .map(|key| format!("{key}={}", rust[key].as_array().map_or(0, Vec::len)))
            .collect();
        println!(
            "attempt {attempt}: {} differences; {}; projects={}; not ported, present in python: {present:?}",
            diff.len(),
            counts.join(" "),
            rust["by_project"].as_object().map_or(0, serde_json::Map::len),
        );
        if diff.is_empty() {
            return ExitCode::SUCCESS;
        }
        for line in diff.iter().take(20) {
            println!("  {}", redacted(line));
        }
    }
    ExitCode::FAILURE
}
