//! A stand-in for `orca`, for the precheck's workers (#599, F275 of the dossier's inventory) and the command of the
//! workers (#744, F277): it answers the four reads of Orca's adapter, `orchestration worker-list`, `task-list`,
//! `worker-read` and `worker-show`, each with `--json`, from the `orca` part of a run's fixture, and fails on anything
//! else. Both sides start it: the product by its absolute path, the prototype's adapter through its search of the
//! program. It takes a flag's value in both forms, `--limit 100` as the prototype sends it and `--limit=100` as the
//! product does.
//!
//! The fixture is named by the file beside this executable, which every run writes: without it the fake refuses
//! to run, so it never passes for the real Orca. A cursor reads as `page-<n>`; a page the fixture does not hold is
//! an empty one. A transcript, or what `worker-show` says of a dispatch, that is a text fails with that code;
//! `worker-show` without `--full` is Orca's concise view. Each call is appended to the log the file names, as its words.

use std::fs::OpenOptions;
use std::io::Write;
use std::process::ExitCode;

use regia_conformance::read_json;
use serde_json::{Value, json};

/// The file beside the fake that names its fixture and its log.
const SIDECAR: &str = "fixture.json";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match answer(&args) {
        Ok(result) => {
            println!("{}", json!({"ok": true, "result": result}));
            ExitCode::SUCCESS
        }
        Err(code) => {
            println!("{}", json!({"ok": false, "error": {"code": code, "message": "fake orca"}}));
            ExitCode::FAILURE
        }
    }
}

/// The words of a call, with `--name=value` split in two.
fn words(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for arg in args {
        match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => out.extend([name.to_owned(), value.to_owned()]),
            _ => out.push(arg.clone()),
        }
    }
    out
}

/// The value after `flag`.
fn value<'a>(words: &'a [String], flag: &str) -> Option<&'a str> {
    words.iter().position(|word| word == flag).and_then(|at| words.get(at + 1)).map(String::as_str)
}

/// The page a cursor names, the first without one.
fn page(words: &[String]) -> Result<usize, String> {
    match value(words, "--cursor") {
        None => Ok(0),
        Some(cursor) => cursor.strip_prefix("page-").and_then(|n| n.parse().ok()).ok_or_else(|| format!("unknown cursor {cursor}")),
    }
}

fn answer(args: &[String]) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let sidecar: Value = read_json(&exe.with_file_name(SIDECAR));
    let responses: Value = read_json(std::path::Path::new(sidecar["responses"].as_str().ok_or("the fixture file names no responses")?));
    let orca = &responses["orca"];
    let words = words(args);
    if let Some(log) = sidecar["log"].as_str() {
        let mut file = OpenOptions::new().create(true).append(true).open(log).map_err(|error| error.to_string())?;
        file.write_all(format!("{}\n", words.join(" ")).as_bytes()).map_err(|error| error.to_string())?;
    }
    if words.first().map(String::as_str) != Some("orchestration") || words.last().map(String::as_str) != Some("--json") {
        return Err(format!("unexpected call: {words:?}"));
    }
    if let Some(code) = orca["fail"].as_str() {
        return Err(code.to_owned());
    }
    match words.get(1).map(String::as_str) {
        Some("worker-list") => {
            let index = page(&words)?;
            Ok(orca["worker_list"].get(index).cloned().unwrap_or_else(|| json!({"workers": [], "page": {"hasMore": false}})))
        }
        Some("task-list") => {
            let run = value(&words, "--run").ok_or("task-list without --run")?;
            Ok(orca["task_lists"].get(run).cloned().unwrap_or_else(|| json!({"tasks": []})))
        }
        Some("worker-read") => {
            let dispatch = value(&words, "--dispatch").ok_or("worker-read without --dispatch")?;
            let pages = &orca["transcripts"][dispatch];
            if let Some(code) = pages.as_str() {
                return Err(code.to_owned());
            }
            Ok(pages.get(page(&words)?).cloned().unwrap_or_else(|| json!({"transcript": {"messages": []}})))
        }
        Some("worker-show") => {
            let dispatch = value(&words, "--dispatch").ok_or("worker-show without --dispatch")?;
            // The concise view, as Orca's: no `dispatch`, no `observation`, no `projection`.
            if !words.iter().any(|word| word == "--full") {
                return Ok(json!({"dispatchId": dispatch, "state": "ready", "liveness": "unverifiable", "waitingOnHuman": "unknown"}));
            }
            let shown = &orca["shows"][dispatch];
            if let Some(code) = shown.as_str() {
                return Err(code.to_owned());
            }
            Ok(shown.clone())
        }
        other => Err(format!("unexpected command {other:?}")),
    }
}
