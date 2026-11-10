//! A stand-in for `gh`. It answers `gh api graphql` from a fixture and fails loudly on anything else.
//! The fixture is named by the environment (the prototype's runs) or by a file beside the fake (the
//! product's, which do not pass those variables on). Without either it refuses to run, so it can
//! never pass for the real tool by accident. It returns only what the query selects, and refuses a
//! query that drops the arguments the brief relies on, so a fixture richer than the query cannot
//! hide a broken query.
//!
//! It takes the request in both forms: `-f`/`-F` pairs, as the prototype sends it, and the whole
//! request on stdin with `--input=-`, as the product does, where `n` must be a number.
//!
//! A write to the tracker, a GraphQL mutation or a `gh` subcommand that changes something, is
//! appended to the writes log as one JSON line, in a form both sides produce for the same call: a
//! mutation by its name and its variables as text, a subcommand by its argv with `--name=value`
//! split and `-R` spelled `--repo`, and the body it read on stdin. It answers a mutation from the
//! fixture's `mutations` and a subcommand from its `cli`, by the subcommand's two words.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use regia_conformance::{FAKE_GH_SIDECAR, log_var, mutation_name, project_query, read_json, responses_var, writes_var};
use serde_json::{Map, Value, json};

const REQUIRED_ON_ISSUES: [&str; 2] = ["states:[OPEN]", "orderBy:{field:CREATED_AT,direction:ASC}"];

/// The subcommands that change the tracker, and those that only read it.
const WRITES: [&str; 8] =
    ["issue edit", "issue comment", "issue create", "issue close", "issue reopen", "label create", "repo create", "repo edit"];
const READS: [&str; 1] = ["issue view"];

struct Fixture {
    responses: PathBuf,
    log: Option<PathBuf>,
    writes: Option<PathBuf>,
}

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

/// The responses file and the logs, from the variables or from the file beside this executable.
fn fixture() -> Result<Fixture, String> {
    if let Ok(responses) = std::env::var(responses_var()) {
        let path = |name: String| std::env::var(name).ok().map(PathBuf::from);
        return Ok(Fixture { responses: responses.into(), log: path(log_var()), writes: path(writes_var()) });
    }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(exe.with_file_name(FAKE_GH_SIDECAR))
        .map_err(|_| format!("{} is not set and there is no {FAKE_GH_SIDECAR} beside the fake", responses_var()))?;
    let sidecar: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let responses = sidecar["responses"].as_str().ok_or("the fixture file names no responses")?;
    let path = |key: &str| sidecar[key].as_str().map(PathBuf::from);
    Ok(Fixture { responses: responses.into(), log: path("log"), writes: path("writes") })
}

fn append(path: Option<&PathBuf>, line: &str) -> Result<(), String> {
    let Some(path) = path else { return Ok(()) };
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|error| error.to_string())?;
    writeln!(file, "{line}").map_err(|error| error.to_string())
}

fn read_stdin() -> Result<String, String> {
    let mut body = String::new();
    std::io::stdin().read_to_string(&mut body).map_err(|error| error.to_string())?;
    Ok(body)
}

/// The request's variables as text, from either form.
fn variables(args: &[String]) -> Result<Vec<(String, String)>, String> {
    if args == ["--input=-"] {
        let body = read_stdin()?;
        let request: Value = serde_json::from_str(&body).map_err(|error| format!("the request is not JSON: {error}"))?;
        let mut out = vec![("query".to_owned(), request["query"].as_str().ok_or("the request has no query")?.to_owned())];
        for (key, value) in request["variables"].as_object().ok_or("the request has no variables")? {
            let text = match value {
                Value::Number(number) if key == "n" => number.to_string(),
                Value::String(text) if key != "n" => text.clone(),
                // A mutation's variables are the product's own: any scalar, compared as text.
                Value::Number(number) => number.to_string(),
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

/// The argv of a subcommand in one form for both sides: `--name=value` is `--name value`, and `-R` is
/// `--repo`.
fn normalized(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for arg in args {
        match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => out.extend([name.to_owned(), value.to_owned()]),
            _ if arg == "-R" => out.push("--repo".to_owned()),
            _ => out.push(arg.clone()),
        }
    }
    out
}

/// A subcommand of `gh` other than `api graphql`: a write is logged with the body it read.
fn subcommand(args: &[String], responses: &Value, fixture: &Fixture) -> Result<String, String> {
    let words = args.get(..2).map(|words| words.join(" ")).unwrap_or_default();
    let write = WRITES.contains(&words.as_str());
    if !write && !READS.contains(&words.as_str()) {
        return Err(format!("unexpected call: {:?}", args.first()));
    }
    if write {
        let argv = normalized(args);
        let from_stdin = argv.windows(2).any(|pair| matches!(pair[0].as_str(), "--body-file" | "--input") && pair[1] == "-");
        let stdin = if from_stdin { Some(read_stdin()?) } else { None };
        append(fixture.writes.as_ref(), &json!({"argv": argv, "stdin": stdin}).to_string())?;
    }
    append(fixture.log.as_ref(), &words)?;
    match &responses["cli"][&words] {
        Value::String(text) => Ok(text.clone()),
        Value::Null if write => Ok(String::new()),
        other => Err(format!("no fixture answer for `gh {words}`: {other}")),
    }
}

fn answer(args: &[String]) -> Result<String, String> {
    let fixture = fixture()?;
    let responses: Value = read_json(&fixture.responses);
    if args.get(..2) != Some(&["api".to_owned(), "graphql".to_owned()]) {
        return subcommand(args, &responses, &fixture);
    }
    let mut variables = variables(&args[2..])?;
    let at = variables.iter().position(|(key, _)| key == "query").ok_or("the request has no query")?;
    let query = variables.remove(at).1;
    // A mutation's variables are its own, `r` included: all of them are logged, none is checked.
    if let Some(name) = mutation_name(&query) {
        let variables: Map<String, Value> = variables.into_iter().map(|(key, value)| (key, Value::String(value))).collect();
        append(fixture.writes.as_ref(), &json!({"mutation": name, "variables": variables}).to_string())?;
        append(fixture.log.as_ref(), &format!("mutation {name}"))?;
        let data = responses["mutations"].get(&name).ok_or_else(|| format!("no fixture answer for mutation {name}"))?;
        let projected = project_query(&query, data)?;
        return serde_json::to_string(&json!({ "data": projected })).map_err(|error| error.to_string());
    }
    let mut number = None;
    let mut after = None;
    for (key, value) in variables {
        match key.as_str() {
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
    } else if query.contains("subIssues(first:100") {
        let number = number.ok_or("sub-issues query without n")?;
        let pages = &responses["sub_issue_pages"][&number];
        if pages.is_string() {
            return Err(format!("simulated failure for #{number}"));
        }
        (format!("sub-issues {number} {index}"), pages[index].clone())
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
    append(fixture.log.as_ref(), &kind)?;
    serde_json::to_string(&json!({ "data": projected })).map_err(|error| error.to_string())
}
