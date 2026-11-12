//! Local only: the Rust and the Python reads against the real tracker, right now: `brief`, `projects`,
//! `task list` with its filters, and `task show` on issues `task list` names. Issue data is private, so this
//! prints only JSON paths and counts, with project ids, registry keys and table errors redacted, and writes
//! nothing. The tracker can change between the two reads, hence the retries. Sections the Rust brief
//! declares as not ported are excluded by that declaration.
//!
//!     ./target/debug/live-check --dossier <path> [--python <interpreter>]
//!
//! Run it directly: `cargo run` changes PATH, where both sides look their tools up.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use regia_conformance::{json_diff, regia_binary, take_not_ported, translate_python};
use regia_facade::{PythonPrototype, Table};
use regia_identity::{env_var, prototype_env_var};
use serde_json::Value;

/// The keys of the contract a difference path may name; any other key is data and is redacted.
const CONTRACT_KEYS: [&str; 38] = [
    "waiting_on_owner", "ready", "in_progress", "by_project", "session", "graph", "cycles", "truncated", "children_waiting",
    "due_ignored", "number", "title", "state", "state_reason", "status", "project", "project_field", "maturity", "objective",
    "labels", "parent", "sub_issues", "blocked_by", "blocked_by_total", "url", "updated_at", "created_at", "closed_at", "unblocks",
    "unblocks_total", "chain", "cycle", "due", "body", "comments", "id", "folder", "products",
];

/// A difference path without its value, each key that is not the contract's as `<key>`.
fn redacted(line: &str) -> String {
    let path = line.split(": ").next().unwrap_or(line);
    let mut out = String::new();
    for (i, part) in path.split('.').enumerate() {
        if i > 0 {
            out.push('.');
        }
        let name = part.split('[').next().unwrap_or(part);
        let known = i == 0 || CONTRACT_KEYS.contains(&name);
        out.push_str(if known { part } else { "<key>" });
        if !known && let Some(index) = part.find('[') {
            out.push_str(&part[index..]);
        }
    }
    out
}

struct Sides<'a> {
    dossier: &'a Path,
    prototype: PythonPrototype,
    table: Table,
}

impl Sides<'_> {
    /// The prototype's JSON for `args`, through the table.
    fn python(&self, args: &[String]) -> Result<Value, String> {
        let (command, rest) = self.table.split_command(args).ok_or("unknown command")?;
        let argv = self.table.python_argv(&command, rest).map_err(|_| "the table refuses the arguments")?;
        let output = self.prototype.launch(&argv).map_err(|error| error.to_string())?.run().map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!("the prototype exited {:?}", output.status.code()));
        }
        let python: Value = serde_json::from_slice(&output.stdout).map_err(|_| "the prototype printed no JSON")?;
        // The message can hold a private value, so only that it happened goes out.
        translate_python(&command, &python).map_err(|error| {
            let kind = format!("{error:?}");
            format!("the table does not cover the real output: {}", kind.split(' ').next().unwrap_or(""))
        })
    }

    /// The binary's JSON for `args`, natively.
    fn rust(&self, args: &[String]) -> Result<Value, String> {
        let output = regia_exec::harness::launch(&regia_binary(), args, self.dossier)
            .env(&prototype_env_var("HOME"), self.dossier)
            // With the dossier here, the reads would go to the prototype: this side must be the Rust one.
            .env(&env_var("ENGINE"), "rust")
            .run()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!("regia exited {:?}", output.status.code()));
        }
        serde_json::from_slice(&output.stdout).map_err(|_| "regia printed no JSON".to_owned())
    }

    /// Both sides on `args`, up to three times: the differences of the last attempt and the Rust output.
    fn compare(&self, label: &str, args: &[String]) -> Result<Value, String> {
        let mut last = Vec::new();
        for attempt in 1..=3 {
            let mut python = self.python(args)?;
            let mut rust = self.rust(args)?;
            let mut excluded = Vec::new();
            if args[0] == "brief" {
                let not_ported = take_not_ported(&mut rust)?;
                let object = python.as_object_mut().ok_or("the brief is not an object")?;
                excluded = not_ported.into_iter().filter(|key| object.remove(key).is_some()).collect();
            }
            last = json_diff(&rust, &python);
            println!("{label}: attempt {attempt}: {} differences; {}{}", last.len(), counts(&rust), if excluded.is_empty() { String::new() } else { format!("; not ported, present in python: {excluded:?}") });
            if last.is_empty() {
                return Ok(rust);
            }
        }
        for line in last.iter().take(20) {
            println!("  {}", redacted(line));
        }
        Err(format!("{label}: {} differences", last.len()))
    }
}

/// How many entries the output has, by section for the brief.
fn counts(value: &Value) -> String {
    match value {
        Value::Array(items) => format!("entries={}", items.len()),
        Value::Object(object) if object.contains_key("waiting_on_owner") => {
            let sections: Vec<String> = ["waiting_on_owner", "ready", "in_progress", "session"]
                .iter()
                .map(|key| format!("{key}={}", value[key].as_array().map_or(0, Vec::len)))
                .collect();
            format!("{}; projects={}", sections.join(" "), value["by_project"].as_object().map_or(0, serde_json::Map::len))
        }
        Value::Object(object) => format!("comments={}", object.get("comments").and_then(Value::as_array).map_or(0, Vec::len)),
        _ => String::new(),
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|arg| arg == name).map(|i| args[i + 1].clone());
    let dossier = PathBuf::from(flag("--dossier").expect("--dossier <path>"));
    let sides = Sides { dossier: &dossier, prototype: PythonPrototype::in_dossier(&dossier, flag("--python")), table: Table::embedded() };
    let mut failed = Vec::new();
    let mut run = |label: &str, args: Vec<String>| match sides.compare(label, &args) {
        Ok(value) => Some(value),
        Err(error) => {
            println!("{error}");
            failed.push(label.to_owned());
            None
        }
    };
    run("brief", strings(&["brief", "--json"]));
    let projects = run("projects", strings(&["projects", "--json"]));
    run("task list", strings(&["task", "list", "--json"]));
    run("task list --ready", strings(&["task", "list", "--ready", "--json"]));
    run("task list --waiting-on-owner", strings(&["task", "list", "--waiting-on-owner", "--json"]));
    run("task list --status approved,in_progress", strings(&["task", "list", "--status", "approved,in_progress", "--json"]));
    let all = run("task list --include-closed --objectives --order graph", strings(&["task", "list", "--include-closed", "--objectives", "--order", "graph", "--json"]));
    if let Some(project) = projects.as_ref().and_then(|projects| projects[0]["id"].as_str()) {
        run("task list --project <first project> --order graph", strings(&["task", "list", "--project", project, "--order", "graph", "--json"]));
    }
    // One open task, one closed issue and one Objective, each by its number: data, never printed.
    let entries = all.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
    let pick = |test: &dyn Fn(&Value) -> bool| entries.iter().find(|entry| test(entry)).and_then(|entry| entry["number"].as_u64());
    let shows = [
        ("an open task", pick(&|entry| entry["state"] == "OPEN" && entry["objective"] == false), true),
        ("a closed issue", pick(&|entry| entry["state"] == "CLOSED"), true),
        ("an objective", pick(&|entry| entry["objective"] == true), true),
    ];
    for (what, number, comments) in shows {
        let Some(number) = number else {
            println!("task show {what}: none in the tracker");
            continue;
        };
        let mut args = strings(&["task", "show", &number.to_string(), "--json"]);
        if comments {
            args.push("--comments".to_owned());
        }
        run(&format!("task show <{what}>{}", if comments { " --comments" } else { "" }), args);
    }
    if let Some(number) = shows[1].1 {
        let args = strings(&["task", "show", &number.to_string(), "--since", "2026-11-01T00:00:00Z", "--json"]);
        run("task show <a closed issue> --since <an instant>", args);
    }
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        println!("failed: {failed:?}");
        ExitCode::FAILURE
    }
}
