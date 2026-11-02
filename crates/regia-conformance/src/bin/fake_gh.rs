//! A stand-in for `gh`, a real `.exe` on Windows so `CreateProcess` finds it. It answers `gh api
//! graphql` from the fixture named by the environment and fails loudly on anything else. Without that
//! variable it refuses to run, so it can never pass for the real tool by accident.

use std::fs::OpenOptions;
use std::io::Write;
use std::process::ExitCode;

use regia_conformance::{log_var, read_json, responses_var};
use serde_json::Value;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match answer(&args) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("fake gh: {message}");
            ExitCode::FAILURE
        }
    }
}

fn answer(args: &[String]) -> Result<String, String> {
    let responses_path = std::env::var(responses_var()).map_err(|_| format!("{} is not set", responses_var()))?;
    if args.get(..2) != Some(&["api".to_owned(), "graphql".to_owned()]) {
        return Err(format!("unexpected call: {:?}", args.first()));
    }
    let mut query = String::new();
    let mut number = None;
    let mut after = None;
    for pair in args[2..].chunks(2) {
        let [flag, value] = pair else { return Err("odd arguments".to_owned()) };
        if flag != "-f" && flag != "-F" {
            return Err(format!("unexpected flag {flag}"));
        }
        let (key, value) = value.split_once('=').ok_or("argument without =")?;
        match key {
            "query" => query = value.to_owned(),
            "n" => number = Some(value.to_owned()),
            "after" => after = Some(value.to_owned()),
            "o" | "r" => {}
            other => return Err(format!("unexpected variable {other}")),
        }
    }
    let responses: Value = read_json(std::path::Path::new(&responses_path));
    let index = match after.as_deref() {
        None => 0,
        Some(cursor) => cursor
            .strip_prefix("page-")
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(|| format!("unknown cursor {cursor}"))?,
    };
    let (kind, page) = if query.contains("blockedBy(first:100") {
        let number = number.ok_or("blockers query without n")?;
        let pages = &responses["blocker_pages"][&number];
        if pages.is_string() {
            return Err(format!("simulated failure for #{number}"));
        }
        (format!("blockers {number} {index}"), pages[index].clone())
    } else if query.contains("issues(first:") {
        (format!("issues {index}"), responses["issue_pages"][index].clone())
    } else {
        return Err("unknown query".to_owned());
    };
    if page.is_null() {
        return Err(format!("no fixture page for {kind}"));
    }
    if let Ok(log) = std::env::var(log_var()) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{kind}").map_err(|error| error.to_string())?;
    }
    serde_json::to_string(&serde_json::json!({ "data": page })).map_err(|error| error.to_string())
}
