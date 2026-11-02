//! Local only: the Rust and the Python brief against the real tracker, right now. Issue data is
//! private, so this prints only JSON paths and counts, and writes nothing. The tracker can change
//! between the two reads, hence the retries. Sections that read the local machine are not ported yet
//! and are excluded by name.
//!
//!     cargo run -p regia-conformance --bin live-check -- --dossier <path> [--python <interpreter>]

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use regia_conformance::{json_diff, regia_binary, translate_python};
use regia_facade::{PythonPrototype, Table};
use regia_identity::env_var;
use serde_json::Value;

const NOT_PORTED: [&str; 3] = ["controller", "dossier", "outside_tracker"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|arg| arg == name).map(|i| args[i + 1].clone());
    let dossier = PathBuf::from(flag("--dossier").expect("--dossier <path>"));
    let prototype = PythonPrototype::in_dossier(&dossier, flag("--python"));
    let cli_args = ["--json".to_owned()];
    let argv = Table::embedded().python_argv("brief", &cli_args).expect("known command");

    for attempt in 1..=3 {
        let output = prototype.command(&argv).output().expect("run the prototype");
        assert!(output.status.success(), "the prototype failed");
        let python: Value = serde_json::from_slice(&output.stdout).expect("JSON from the prototype");
        let mut python = translate_python("brief", &python).expect("the table covers the real output");

        let output = Command::new(regia_binary())
            .args(["brief", "--json"])
            .env(env_var("HOME"), &dossier)
            .env_remove(env_var("ENGINE"))
            .output()
            .expect("run regia");
        assert!(output.status.success(), "regia failed: {}", String::from_utf8_lossy(&output.stderr));
        let rust: Value = serde_json::from_slice(&output.stdout).expect("JSON from regia");

        let object = python.as_object_mut().expect("an object");
        let present: Vec<&str> = NOT_PORTED.into_iter().filter(|key| object.contains_key(*key)).collect();
        for key in &present {
            object.remove(*key);
        }
        let diff = json_diff(&rust, &python);
        let counts: Vec<String> = ["waiting_on_owner", "ready", "in_progress", "session"]
            .iter()
            .map(|key| format!("{key}={}", rust[key].as_array().map_or(0, Vec::len)))
            .collect();
        println!(
            "attempt {attempt}: {} differences; {}; projects={}; excluded (not ported, present in python): {present:?}",
            diff.len(),
            counts.join(" "),
            rust["by_project"].as_object().map_or(0, serde_json::Map::len),
        );
        if diff.is_empty() {
            return ExitCode::SUCCESS;
        }
        // Paths only: a difference line also carries values, which can be private.
        for line in diff.iter().take(20) {
            println!("  {}", line.split(": ").next().unwrap_or(line));
        }
    }
    ExitCode::FAILURE
}
