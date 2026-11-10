//! A stand-in for `gh`. It answers `gh api graphql` from a fixture and fails loudly on anything else.
//! The fixture is named by the environment (the prototype's runs) or by a file beside the fake (the
//! product's, which do not pass those variables on). Without either it refuses to run, so it can
//! never pass for the real tool by accident. It returns only what the query selects, and refuses a
//! query that drops the arguments the brief relies on, so a fixture richer than the query cannot
//! hide a broken query.
//!
//! It takes the request in both forms: `-f`/`-F` pairs, as the prototype sends it, and the whole
//! request on stdin with `--input=-`, as the product does, where `n` must be a number.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use regia_conformance::{FAKE_GH_SIDECAR, log_var, project_query, read_json, responses_var};
use serde_json::Value;

const REQUIRED_ON_ISSUES: [&str; 2] = ["states:[OPEN]", "orderBy:{field:CREATED_AT,direction:ASC}"];

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

/// The responses file and the log, from the variables or from the file beside this executable.
fn fixture() -> Result<(PathBuf, Option<PathBuf>), String> {
    if let Ok(responses) = std::env::var(responses_var()) {
        return Ok((responses.into(), std::env::var(log_var()).ok().map(PathBuf::from)));
    }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(exe.with_file_name(FAKE_GH_SIDECAR))
        .map_err(|_| format!("{} is not set and there is no {FAKE_GH_SIDECAR} beside the fake", responses_var()))?;
    let sidecar: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let responses = sidecar["responses"].as_str().ok_or("the fixture file names no responses")?;
    Ok((responses.into(), sidecar["log"].as_str().map(PathBuf::from)))
}

/// The request's variables as text, from either form.
fn variables(args: &[String]) -> Result<Vec<(String, String)>, String> {
    if args == ["--input=-"] {
        let mut body = String::new();
        std::io::stdin().read_to_string(&mut body).map_err(|error| error.to_string())?;
        let request: Value = serde_json::from_str(&body).map_err(|error| format!("the request is not JSON: {error}"))?;
        let mut out = vec![("query".to_owned(), request["query"].as_str().ok_or("the request has no query")?.to_owned())];
        for (key, value) in request["variables"].as_object().ok_or("the request has no variables")? {
            let text = match value {
                Value::Number(number) if key == "n" => number.to_string(),
                Value::String(text) if key != "n" => text.clone(),
                other => return Err(format!("variable {key} has the wrong type: {other}")),
            };
            out.push((key.clone(), text));
        }
        return Ok(out);
    }
    let mut out = Vec::new();
    for pair in args.chunks(2) {
        let [flag, value] = pair else { return Err("odd arguments".to_owned()) };
        if flag != "-f" && flag != "-F" {
            return Err(format!("unexpected flag {flag}"));
        }
        let (key, value) = value.split_once('=').ok_or("argument without =")?;
        out.push((key.to_owned(), value.to_owned()));
    }
    Ok(out)
}

fn answer(args: &[String]) -> Result<String, String> {
    let (responses_path, log) = fixture()?;
    if args.get(..2) != Some(&["api".to_owned(), "graphql".to_owned()]) {
        return Err(format!("unexpected call: {:?}", args.first()));
    }
    let responses: Value = read_json(&responses_path);
    let mut query = String::new();
    let mut number = None;
    let mut after = None;
    for (key, value) in variables(&args[2..])? {
        match key.as_str() {
            "query" => query = value,
            "n" => number = Some(value),
            "after" => after = Some(value),
            "o" | "r" => {
                if responses["expect"][&key] != value.as_str() {
                    return Err(format!("variable {key} is {value}, the fixture expects {}", responses["expect"][&key]));
                }
            }
            other => return Err(format!("unexpected variable {other}")),
        }
    }
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
        for required in REQUIRED_ON_ISSUES {
            if !query.contains(required) {
                return Err(format!("the issues query lost {required}"));
            }
        }
        (format!("issues {index}"), responses["issue_pages"][index].clone())
    } else {
        return Err("unknown query".to_owned());
    };
    if page.is_null() {
        return Err(format!("no fixture page for {kind}"));
    }
    let projected = project_query(&query, &page)?;
    if let Some(log) = log {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{kind}").map_err(|error| error.to_string())?;
    }
    serde_json::to_string(&serde_json::json!({ "data": projected })).map_err(|error| error.to_string())
}
