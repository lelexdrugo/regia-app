//! The conformance harness (ADR 0001, "Conformance").
//!
//! A fixture case holds what the tracker answered (`responses.json`) and what the Python prototype
//! printed for it (`python.it.json`), recorded locally because CI cannot read the private dossier.
//! Each test runs the real `regia` binary against the same responses through a fake `gh`, passes the
//! recorded Python output through the facade table, and diffs the two.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use regia_facade::{FacadeError, Table};
use regia_identity::{env_var, prototype_env_var};
use serde::Deserialize;
use serde_json::Value;

/// Read by the fake `gh`: the responses file to answer from.
pub fn responses_var() -> String {
    env_var("FAKE_GH_RESPONSES")
}

/// Read by the fake `gh`: where to log each call, to prove it was the one that answered.
pub fn log_var() -> String {
    env_var("FAKE_GH_LOG")
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().expect("workspace root")
}

pub fn fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures").join("brief")
}

/// The folder with the workspace's binaries, found from the running test or tool.
pub fn bin_dir() -> PathBuf {
    let exe = env::current_exe().expect("current executable");
    let wanted = format!("{}{}", regia_identity::BIN_NAME, env::consts::EXE_SUFFIX);
    exe.ancestors()
        .skip(1)
        // Cargo can leave a copy in `deps`, where the other binaries are not.
        .find(|dir| dir.file_name().is_some_and(|name| name != "deps") && dir.join(&wanted).is_file())
        .unwrap_or_else(|| panic!("{wanted} not found above {}: run `cargo build --workspace` first", exe.display()))
        .to_path_buf()
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaseSpec {
    /// The name of a file in `fixtures/brief/scenarios/`.
    pub scenario: String,
    /// The English command line, without the binary.
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub dir: PathBuf,
    pub spec: CaseSpec,
}

impl Case {
    pub fn all() -> Vec<Case> {
        let cases = fixtures_dir().join("cases");
        let mut out: Vec<Case> = fs::read_dir(&cases)
            .unwrap_or_else(|error| panic!("{}: {error}", cases.display()))
            .map(|entry| entry.expect("case entry").path())
            .filter(|path| path.is_dir())
            .map(|dir| Case {
                name: dir.file_name().expect("case name").to_string_lossy().into_owned(),
                spec: read_json(&dir.join("case.json")),
                dir,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        assert!(!out.is_empty(), "no fixture cases in {}", cases.display());
        out
    }

    pub fn named(name: &str) -> Case {
        Case::all().into_iter().find(|case| case.name == name).expect("known case")
    }

    pub fn responses_path(&self) -> PathBuf {
        self.dir.join("responses.json")
    }

    pub fn python_output(&self) -> Value {
        read_json(&self.dir.join("python.it.json"))
    }

    pub fn command_name(&self) -> &str {
        &self.spec.args[0]
    }
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    let text = fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A state folder with only the synthetic `tracker.json`, outside the temp folder (the prototype
/// refuses it) and under `target/`, so nothing from the machine leaks in and nothing dirties the repo.
pub fn prepare_state_dir(label: &str) -> PathBuf {
    let dir = bin_dir().join("conformance-state").join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean state folder");
    }
    fs::create_dir_all(&dir).expect("state folder");
    fs::copy(fixtures_dir().join("tracker.json"), dir.join("tracker.json")).expect("copy tracker.json");
    dir
}

/// A folder holding the fake as `gh`. It must not sit next to `regia`: Windows looks in the folder of
/// the running executable before PATH, and a `gh` there would shadow the real one in `live-check`.
fn fake_gh_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let suffix = env::consts::EXE_SUFFIX;
        let dir = bin_dir().join("conformance-state").join("fake-gh-bin");
        fs::create_dir_all(&dir).expect("fake gh folder");
        let source = bin_dir().join(format!("fake-gh{suffix}"));
        let scratch = dir.join(format!("gh-{}{suffix}", std::process::id()));
        fs::copy(&source, &scratch).unwrap_or_else(|error| panic!("{}: {error}", source.display()));
        // Another process may be running the current copy; that copy is just as good.
        if fs::rename(&scratch, dir.join(format!("gh{suffix}"))).is_err() {
            let _ = fs::remove_file(&scratch);
        }
        dir
    })
    .clone()
}

/// PATH with the fake `gh` first.
pub fn path_with_fake_gh() -> OsString {
    let mut paths = vec![fake_gh_dir()];
    if let Some(current) = env::var_os("PATH") {
        paths.extend(env::split_paths(&current));
    }
    env::join_paths(paths).expect("PATH")
}

/// Everything a process needs to run against a fixture.
pub fn apply_fixture_env(command: &mut Command, responses: &Path, state: &Path) {
    command
        .env("PATH", path_with_fake_gh())
        .env(prototype_env_var("RUNTIME"), state)
        .env(responses_var(), responses)
        .env_remove(env_var("ENGINE"))
        .env_remove(prototype_env_var("HOME"));
}

pub fn regia_binary() -> PathBuf {
    bin_dir().join(format!("{}{}", regia_identity::BIN_NAME, env::consts::EXE_SUFFIX))
}

/// Runs the real binary on `responses` and returns its parsed stdout.
pub fn run_rust(label: &str, args: &[String], responses: &Path) -> Result<Value, String> {
    let state = prepare_state_dir(label);
    let mut command = Command::new(regia_binary());
    command.args(args);
    apply_fixture_env(&mut command, responses, &state);
    let output: Output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("regia failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("regia printed invalid JSON: {error}"))
}

/// The Python output in English, through the table the CLI's passthrough uses.
pub fn translate_python(command: &str, python: &Value) -> Result<Value, FacadeError> {
    Table::embedded().translate_output(command, python)
}

/// The Rust brief marks the sections it does not produce; the prototype has no such key. The harness
/// checks the marker and removes it, so an unported section is declared, never silently absent.
pub fn take_not_ported(rust: &mut Value) -> Result<Vec<String>, String> {
    let marker = rust
        .as_object_mut()
        .and_then(|object| object.remove("not_ported"))
        .ok_or("the Rust brief has no not_ported marker")?;
    serde_json::from_value(marker).map_err(|error| error.to_string())
}

#[derive(Debug)]
struct Selection {
    name: String,
    fragment: bool,
    children: Vec<Selection>,
}

fn skip_blank(text: &[char], at: &mut usize) {
    while text.get(*at).is_some_and(|c| c.is_whitespace() || *c == ',') {
        *at += 1;
    }
}

fn read_name(text: &[char], at: &mut usize) -> String {
    let start = *at;
    while text.get(*at).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
        *at += 1;
    }
    text[start..*at].iter().collect()
}

/// Skips `( … )`, which can hold braces and quoted strings.
fn skip_arguments(text: &[char], at: &mut usize) -> Result<(), String> {
    let mut depth = 0;
    let mut quoted = false;
    while let Some(&c) = text.get(*at) {
        *at += 1;
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Err("unbalanced parentheses in the query".to_owned())
}

/// Reads a selection set; `at` is just after its `{`.
fn parse_selection(text: &[char], at: &mut usize) -> Result<Vec<Selection>, String> {
    let mut out = Vec::new();
    loop {
        skip_blank(text, at);
        match text.get(*at) {
            None => return Err("unbalanced braces in the query".to_owned()),
            Some('}') => {
                *at += 1;
                return Ok(out);
            }
            Some('.') => {
                *at += 3;
                skip_blank(text, at);
                if read_name(text, at) != "on" {
                    return Err("expected an inline fragment".to_owned());
                }
                skip_blank(text, at);
                let name = read_name(text, at);
                skip_blank(text, at);
                if text.get(*at) != Some(&'{') {
                    return Err(format!("fragment on {name} has no selection"));
                }
                *at += 1;
                out.push(Selection { name, fragment: true, children: parse_selection(text, at)? });
            }
            Some(_) => {
                let name = read_name(text, at);
                if name.is_empty() {
                    return Err(format!("unexpected text in the query at {at}"));
                }
                skip_blank(text, at);
                if text.get(*at) == Some(&'(') {
                    skip_arguments(text, at)?;
                    skip_blank(text, at);
                }
                let mut children = Vec::new();
                if text.get(*at) == Some(&'{') {
                    *at += 1;
                    children = parse_selection(text, at)?;
                }
                out.push(Selection { name, fragment: false, children });
            }
        }
    }
}

fn project(selections: &[Selection], value: &Value, optional: bool) -> Result<Value, String> {
    match value {
        Value::Array(items) => {
            items.iter().map(|item| project(selections, item, optional)).collect::<Result<Vec<_>, _>>().map(Value::Array)
        }
        Value::Object(object) => {
            let mut out = serde_json::Map::new();
            for selection in selections {
                if selection.fragment {
                    // A fragment on another type selects nothing, as on the real API.
                    if let Value::Object(part) = project(&selection.children, value, true)? {
                        out.extend(part);
                    }
                    continue;
                }
                match object.get(&selection.name) {
                    Some(inner) if selection.children.is_empty() => {
                        out.insert(selection.name.clone(), inner.clone());
                    }
                    Some(inner) => {
                        out.insert(selection.name.clone(), project(&selection.children, inner, false)?);
                    }
                    None if optional => {}
                    None => return Err(format!("the query selects `{}`, unknown to the fixture", selection.name)),
                }
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

/// What a GraphQL server would return for `query` on `value`: only the selected fields, and an error
/// on a field that does not exist.
pub fn project_query(query: &str, value: &Value) -> Result<Value, String> {
    let text: Vec<char> = query.chars().collect();
    let mut at = 0;
    skip_blank(&text, &mut at);
    if text[at..].starts_with(&['q', 'u', 'e', 'r', 'y']) {
        at += 5;
        skip_blank(&text, &mut at);
        if text.get(at) == Some(&'(') {
            skip_arguments(&text, &mut at)?;
            skip_blank(&text, &mut at);
        }
    }
    if text.get(at) != Some(&'{') {
        return Err("the query has no selection set".to_owned());
    }
    at += 1;
    project(&parse_selection(&text, &mut at)?, value, false)
}

/// Differences as JSON paths. Object key order does not count; array order does, because the order
/// of the sections is the logic under test.
pub fn json_diff(left: &Value, right: &Value) -> Vec<String> {
    let mut out = Vec::new();
    diff_at("$", left, right, &mut out);
    out
}

fn diff_at(path: &str, left: &Value, right: &Value, out: &mut Vec<String>) {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                match b.get(key) {
                    Some(other) => diff_at(&format!("{path}.{key}"), value, other, out),
                    None => out.push(format!("{path}.{key}: only in rust")),
                }
            }
            for key in b.keys().filter(|key| !a.contains_key(*key)) {
                out.push(format!("{path}.{key}: only in python"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                out.push(format!("{path}: rust has {} items, python {}", a.len(), b.len()));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff_at(&format!("{path}[{i}]"), x, y, out);
            }
        }
        (a, b) if a != b => out.push(format!("{path}: rust {a}, python {b}")),
        _ => {}
    }
}
