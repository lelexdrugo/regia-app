//! Local only: the Rust and the Python reads against the real tracker, right now: `brief`, `projects`,
//! `task list` with its filters, `task show` on issues `task list` names, and `plan` on a project and on its Objective
//! (#444), whose command line the native side quotes in its own words (`plan_names.rs`). Issue data is private, so this
//! prints only JSON paths and counts, with project ids, registry keys and table errors redacted, and writes
//! nothing. Neither stdout nor stderr of either side is ever printed, only that a run failed. The tracker
//! can change between the two reads, hence the retries. Sections the Rust brief declares as not ported are
//! excluded by that declaration. The Rust side runs without `REGIA_TODAY`, on the real clock, as the
//! prototype does (a difference of `TZ` on Windows is declared on `BriefOptions::today`).
//!
//!     ./target/debug/live-check --dossier <path> [--python <interpreter>]
//!
//! Run it directly: `cargo run` changes PATH, where both sides look their tools up.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use regia_conformance::{json_diff, live_python_launch, live_rust_launch, regia_binary, take_not_ported, translate_python};
use regia_facade::{PythonPrototype, Table};
use serde_json::Value;

#[path = "../plan_names.rs"]
mod plan_names;

/// The keys of the contract a difference path may name; any other key is data and is redacted.
const CONTRACT_KEYS: [&str; 49] = [
    "waiting_on_owner", "ready", "in_progress", "by_project", "session", "graph", "cycles", "truncated", "children_waiting",
    "due_ignored", "number", "title", "state", "state_reason", "status", "project", "project_field", "maturity", "objective",
    "labels", "parent", "sub_issues", "blocked_by", "blocked_by_total", "url", "updated_at", "created_at", "closed_at", "unblocks",
    "unblocks_total", "chain", "cycle", "due", "body", "comments", "id", "folder", "products", "command", "date", "root", "nodes",
    "external", "edges", "nested", "main_line", "inbox_hidden", "edges_lost", "mermaid",
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
        let output = live_python_launch(self.prototype.launch(&argv).map_err(|error| error.to_string())?, self.dossier).run().map_err(|error| error.to_string())?;
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
        let output = live_rust_launch(&regia_binary(), args, self.dossier).run().map_err(|error| error.to_string())?;
        // Only that it failed goes out, never what it said: an error of the product can carry a value of the
        // tracker, on stderr as on stdout (#439/D5).
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
            if args[0] == "plan" {
                // The one normalization of `plan`, declared: the command line the native side quotes, in its words.
                for key in ["command", "mermaid"] {
                    if let Some(text) = rust[key].as_str() {
                        rust[key] = Value::String(plan_names::in_the_prototype_s_names(text));
                    }
                }
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
        Value::Object(object) if object.contains_key("nodes") => {
            let length = |key: &str| value[key].as_array().map_or(0, Vec::len);
            let diagram = value["mermaid"].as_str().map_or(0, |text| text.lines().count());
            format!("nodes={} edges={} truncated={} main_line={} diagram_lines={diagram}", length("nodes"), length("edges"), length("truncated"), length("main_line"))
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
        run("plan --project <first project>", strings(&["plan", "--project", project, "--json"]));
        run("plan --project <first project> --mermaid --max-nodes 10", strings(&["plan", "--project", project, "--mermaid", "--max-nodes", "10", "--json"]));
    }
    // Under the first project's Objective, by its number: data, never printed.
    if let Some(objective) = projects.as_ref().and_then(|projects| projects[0]["objective"].as_u64()) {
        run("plan --root <its Objective> --mermaid --max-nodes 200", strings(&["plan", "--root", &objective.to_string(), "--mermaid", "--max-nodes", "200", "--json"]));
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
