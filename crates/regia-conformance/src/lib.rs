//! The conformance harness (ADR 0001, "Conformance").
//!
//! A fixture case holds what the tracker answered (`responses.json`) and what the Python prototype
//! printed for it (`python.it.json`, or `python.it.txt` for a command that prints text), recorded
//! locally because CI cannot read the private dossier. It also holds the exit code both sides must
//! end with, the rows of the state database before the run, and the writes the prototype made to the
//! tracker, in order (`calls.json`). Cases live in families, one folder of `fixtures/` per group of
//! commands, so slices that add cases do not edit the same folder.
//!
//! A case of a ported command runs the real `regia` binary against the same responses through a fake
//! `gh`, passes the recorded Python output through the facade table, and diffs the two. A case of a
//! command that goes through the facade runs the binary too, on a stand-in interpreter that checks
//! the argv the table built and replays what the prototype printed: that proves the translation, the
//! exit code and the text in CI, with no Python. Its writes are proven where the prototype is, by a
//! local test that runs the binary on the real prototype (`tests/facade.rs`).
//!
//! The two sides reach the fake `gh` differently. The prototype looks `gh` up on `PATH`, so for it the
//! fake goes first there. The product starts only an absolute path (threat model T06), so it gets a
//! copy of the fake named by `REGIA_GH`, which reads its fixture from a file beside it: the product
//! does not pass the fixture's variables to `gh`, and should not.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regia_exec::{Launch, Program};
use regia_facade::{FacadeError, SCRIPT, Table};
use regia_identity::{env_var, prototype_env_var};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Read by the fake `gh`: the responses file to answer from.
pub fn responses_var() -> String {
    env_var("FAKE_GH_RESPONSES")
}

/// Read by the fake `gh`: where to log each call, to prove it was the one that answered.
pub fn log_var() -> String {
    env_var("FAKE_GH_LOG")
}

/// Read by the fake `gh`: where to append each write to the tracker, one JSON line per call.
pub fn writes_var() -> String {
    env_var("FAKE_GH_WRITES")
}

/// Beside a copy of the fake `gh`: `{"responses": …, "log": …, "writes": …}`, read when the variables
/// are unset.
pub const FAKE_GH_SIDECAR: &str = "fixture.json";

/// Beside a copy of the fake interpreter: the argv it expects and what it replays.
pub const FAKE_PYTHON_SIDECAR: &str = "replay.json";

/// The fake interpreter's exit code when it is started with an argv the case does not expect.
pub const FAKE_PYTHON_WRONG_ARGV: u8 = 99;

/// The day every fixture is recorded and checked on, so due dates mean the same on both sides: the
/// prototype gets it through `oracle/fixed_today.py`, the product through `REGIA_TODAY`.
pub const FIXTURE_TODAY: &str = "2026-01-10";

/// What the prototype reads to sign and attribute a session (`CLAUDE_CODE_SESSION_ID` and the others
/// the allow-list of the Python passes), and the role of `REGIA_RUOLO`: a fixture run sees none of the
/// machine's.
const SESSION_VARIABLES: [&str; 3] = ["CLAUDE_CODE_SESSION_ID", "ORCA_AGENT_SESSION_ID", "CODEX_THREAD_ID"];

/// Folders of `fixtures/` whose cases drive something other than the CLI: the controller's oracle.
const NOT_CLI: [&str; 1] = ["controller"];

/// The prototype's driver that runs it on [`FIXTURE_TODAY`].
pub fn fixed_today_driver() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle").join("fixed_today.py")
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().expect("workspace root")
}

pub fn fixtures_root() -> PathBuf {
    workspace_root().join("fixtures")
}

/// The family of the brief, which holds the synthetic `tracker.json` every run gets.
pub fn fixtures_dir() -> PathBuf {
    fixtures_root().join("brief")
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
#[serde(deny_unknown_fields)]
pub struct CaseSpec {
    #[serde(default, rename = "_note")]
    pub note: Option<String>,
    /// The name of a file in the family's `scenarios/`: the tracker the fake `gh` answers from.
    #[serde(default)]
    pub scenario: Option<String>,
    /// The English command line, without the binary.
    pub args: Vec<String>,
    /// The exit code both sides end with.
    #[serde(default)]
    pub exit: u8,
    /// For a command that goes through the facade, the prototype's argv, written by hand: the table
    /// must build exactly this, and the stand-in interpreter refuses anything else.
    #[serde(default)]
    pub python: Option<Vec<String>>,
    /// Rows of the state database before the run.
    #[serde(default)]
    pub state: Vec<StateRow>,
}

/// A row of the documents table, as the prototype and `regia-store` write it: the document is data.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StateRow {
    pub kind: String,
    pub key: String,
    pub doc: Value,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub family: String,
    pub name: String,
    pub dir: PathBuf,
    pub spec: CaseSpec,
}

/// What the prototype printed for a case.
#[derive(Debug, Clone, PartialEq)]
pub enum Recorded {
    Json(Value),
    Text(String),
}

impl Case {
    /// Every case of every family of the CLI, sorted by family and name.
    pub fn all() -> Vec<Case> {
        let mut families: Vec<String> = fs::read_dir(fixtures_root())
            .expect("the fixtures folder")
            .map(|entry| entry.expect("a fixtures entry").path())
            .filter(|path| path.join("cases").is_dir())
            .map(|path| path.file_name().expect("a family name").to_string_lossy().into_owned())
            .filter(|family| !NOT_CLI.contains(&family.as_str()))
            .collect();
        families.sort();
        let out: Vec<Case> = families.iter().flat_map(|family| Case::of_family(family)).collect();
        assert!(!out.is_empty(), "no fixture cases in {}", fixtures_root().display());
        out
    }

    pub fn of_family(family: &str) -> Vec<Case> {
        let cases = fixtures_root().join(family).join("cases");
        let mut out: Vec<Case> = fs::read_dir(&cases)
            .unwrap_or_else(|error| panic!("{}: {error}", cases.display()))
            .map(|entry| entry.expect("case entry").path())
            .filter(|path| path.is_dir())
            .map(|dir| Case {
                family: family.to_owned(),
                name: dir.file_name().expect("case name").to_string_lossy().into_owned(),
                spec: read_json(&dir.join("case.json")),
                dir,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The case of that name, in whichever family: names are unique across families.
    pub fn named(name: &str) -> Case {
        let mut found: Vec<Case> = Case::all().into_iter().filter(|case| case.name == name).collect();
        assert_eq!(found.len(), 1, "case {name} must exist exactly once");
        found.remove(0)
    }

    pub fn family_dir(&self) -> PathBuf {
        fixtures_root().join(&self.family)
    }

    pub fn responses_path(&self) -> PathBuf {
        self.dir.join("responses.json")
    }

    pub fn python_output(&self) -> Value {
        read_json(&self.dir.join("python.it.json"))
    }

    /// The prototype's stdout: JSON if it printed JSON, text otherwise.
    pub fn recorded(&self) -> Recorded {
        let text = self.dir.join("python.it.txt");
        if text.is_file() {
            Recorded::Text(fs::read_to_string(&text).unwrap_or_else(|error| panic!("{}: {error}", text.display())))
        } else {
            Recorded::Json(self.python_output())
        }
    }

    /// What the prototype wrote on stderr, empty if nothing.
    pub fn recorded_stderr(&self) -> String {
        fs::read_to_string(self.dir.join("python.it.stderr.txt")).unwrap_or_default()
    }

    /// The writes the prototype made to the tracker, in order; none without `calls.json`.
    pub fn writes(&self) -> Vec<Value> {
        let path = self.dir.join("calls.json");
        if path.is_file() { read_json(&path) } else { Vec::new() }
    }

    /// The command the case runs, by the facade table's longest name.
    pub fn command_name(&self) -> String {
        Table::embedded().split_command(&self.spec.args).expect("the table knows the case's command").0
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

/// The state folder of a case: `tracker.json`, and the case's rows in the state database, which both
/// implementations read.
pub fn prepare_case_state(label: &str, case: &Case) -> PathBuf {
    let dir = prepare_state_dir(label);
    write_state(&dir, &case.spec.state);
    dir
}

/// `rows` in the state database of `dir`, in format v1: the table and its columns come from
/// `regia-store`'s description of the format, each row with generation 1.
pub fn write_state(dir: &Path, rows: &[StateRow]) {
    if rows.is_empty() {
        return;
    }
    let format = workspace_root().join("crates").join("regia-store").join("formats").join("store_v1.toml");
    let format: toml::Table = toml::from_str(&fs::read_to_string(&format).expect("the v1 state format")).expect("TOML");
    let column = |name: &str| format["table"][name].as_str().expect("a column of the v1 table").to_owned();
    let [table, kind, key, generation, document] = ["name", "kind", "key", "generation", "document"].map(column);
    let connection = rusqlite::Connection::open(dir.join(regia_store::database_name())).expect("the state database");
    connection
        .execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {table} ({kind} TEXT NOT NULL, {key} TEXT NOT NULL, {generation} INTEGER NOT NULL, \
             {document} TEXT NOT NULL, PRIMARY KEY ({kind}, {key}))"
        ))
        .expect("the documents table");
    for row in rows {
        connection
            .execute(
                &format!("INSERT INTO {table} ({kind}, {key}, {generation}, {document}) VALUES (?1, ?2, 1, ?3)"),
                (&row.kind, &row.key, row.doc.to_string()),
            )
            .expect("a state row");
    }
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

/// Where the fake `gh` of a run in `state` appends the writes, on either side.
pub fn writes_log(state: &Path) -> PathBuf {
    state.join("gh-writes.jsonl")
}

/// The writes the fake `gh` saw in a run in `state`, in order.
pub fn writes_of_run(state: &Path) -> Vec<Value> {
    let text = fs::read_to_string(writes_log(state)).unwrap_or_default();
    text.lines().map(|line| serde_json::from_str(line).expect("a JSON line of the writes log")).collect()
}

/// What every run against a fixture needs, on either side: its state folder, the fixed day, and no
/// engine, dossier or session of the machine. The Codex credentials point at a file that does not
/// exist: the prototype's brief reads them for a warning, and the machine's own must not leak in.
fn fixture_env(launch: Launch, state: &Path) -> Launch {
    let launch = SESSION_VARIABLES.iter().fold(launch, |launch, name| launch.without(name));
    launch
        .env(&prototype_env_var("RUNTIME"), state)
        .env(&prototype_env_var("CODEX_AUTH"), state.join("no-codex-auth.json"))
        .env(&env_var("TODAY"), FIXTURE_TODAY)
        .without(&env_var("ENGINE"))
        .without(&prototype_env_var("HOME"))
        .without(&prototype_env_var("RUOLO"))
}

/// The prototype against a fixture: the fake first on PATH, the fixture in its variables.
pub fn fixture_for_python(launch: Launch, responses: &Path, state: &Path) -> Launch {
    fixture_env(launch, state)
        .env("PATH", path_with_fake_gh())
        .env(&responses_var(), responses)
        .env(&writes_var(), writes_log(state))
}

/// A copy of the fake `gh` in `state/fake-gh/`, with its fixture beside it. Returns its path and the
/// log of its calls.
pub fn fake_gh_for_run(state: &Path, responses: &Path) -> (PathBuf, PathBuf) {
    let suffix = env::consts::EXE_SUFFIX;
    let dir = state.join("fake-gh");
    fs::create_dir_all(&dir).expect("fake gh folder");
    let source = bin_dir().join(format!("fake-gh{suffix}"));
    let copy = dir.join(format!("gh{suffix}"));
    fs::copy(&source, &copy).unwrap_or_else(|error| panic!("{}: {error}", source.display()));
    let log = dir.join("calls.log");
    let sidecar = json!({"responses": responses, "log": log, "writes": writes_log(state)});
    fs::write(dir.join(FAKE_GH_SIDECAR), sidecar.to_string()).expect("fake gh fixture");
    (copy, log)
}

/// The product, or the app, against a fixture: its `gh` is the copy at an absolute path, and the
/// fake is nowhere on PATH.
pub fn fixture_for_rust(launch: Launch, responses: &Path, state: &Path) -> (Launch, PathBuf) {
    let (gh, log) = fake_gh_for_run(state, responses);
    (fixture_env(launch, state).env(&Program::Gh.env_name(), gh), log)
}

pub fn regia_binary() -> PathBuf {
    bin_dir().join(format!("{}{}", regia_identity::BIN_NAME, env::consts::EXE_SUFFIX))
}

/// Runs the real binary on `responses` and returns its parsed stdout. The fake's log proves that the
/// copy named by its absolute path answered.
pub fn run_rust(label: &str, args: &[String], responses: &Path) -> Result<Value, String> {
    let state = prepare_state_dir(label);
    let launch = regia_exec::harness::launch(&regia_binary(), args, &state);
    let (launch, log) = fixture_for_rust(launch, responses, &state);
    let output = launch.run().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("regia failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    if fs::read_to_string(&log).unwrap_or_default().is_empty() {
        return Err("the fake gh at its absolute path was not called".to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("regia printed invalid JSON: {error}"))
}

/// A run of the binary on a case.
pub struct CliRun {
    /// `None` if it ended without an exit code.
    pub exit: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
    /// The writes the fake `gh` saw, in order.
    pub writes: Vec<Value>,
}

/// What the stand-in interpreter replays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replay {
    pub argv: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit: u8,
}

impl Replay {
    /// What the prototype did for `case`, started as the facade starts it: the interpreter's flags,
    /// the script, and the argv the case expects.
    pub fn of(case: &Case) -> Replay {
        let python = case.spec.python.clone().unwrap_or_else(|| panic!("case {} does not go through the facade", case.name));
        let stdout = match case.recorded() {
            Recorded::Json(value) => format!("{}\n", serde_json::to_string_pretty(&value).expect("JSON")),
            Recorded::Text(text) => text,
        };
        let argv = regia_exec::python::FLAGS.iter().map(|flag| (*flag).to_owned()).chain([SCRIPT.to_owned()]).chain(python).collect();
        Replay { argv, stdout, stderr: case.recorded_stderr(), exit: case.spec.exit }
    }
}

/// A dossier with only the prototype's entry point in it, and a copy of the stand-in interpreter that
/// replays `replay`. Returns the variables that point the binary at them.
pub fn replay_prototype(state: &Path, replay: &Replay) -> Vec<(String, PathBuf)> {
    let dossier = state.join("dossier");
    let script = dossier.join(SCRIPT);
    fs::create_dir_all(script.parent().expect("the script's folder")).expect("the dossier");
    fs::write(&script, "# The stand-in interpreter replays a recording and never reads this file.\n").expect("the script");
    let dir = state.join("fake-python");
    fs::create_dir_all(&dir).expect("fake interpreter folder");
    let suffix = env::consts::EXE_SUFFIX;
    let source = bin_dir().join(format!("fake-python{suffix}"));
    let copy = dir.join(format!("python{suffix}"));
    fs::copy(&source, &copy).unwrap_or_else(|error| panic!("{}: {error}", source.display()));
    fs::write(dir.join(FAKE_PYTHON_SIDECAR), serde_json::to_string(replay).expect("JSON")).expect("the replay");
    vec![(prototype_env_var("HOME"), dossier), (env_var("PYTHON"), copy)]
}

/// The real binary on `case`, with the fake `gh` and the case's state, plus the variables in `extra`.
pub fn run_case(label: &str, case: &Case, extra: impl FnOnce(&Path) -> Vec<(String, OsString)>) -> CliRun {
    let state = prepare_case_state(label, case);
    let responses = if case.responses_path().is_file() {
        case.responses_path()
    } else {
        let empty = state.join("no-responses.json");
        fs::write(&empty, "{}").expect("empty responses");
        empty
    };
    let launch = regia_exec::harness::launch(&regia_binary(), &case.spec.args, &state);
    let (mut launch, _) = fixture_for_rust(launch, &responses, &state);
    for (name, value) in extra(&state) {
        launch = launch.env(&name, value);
    }
    let output = launch.run().expect("run regia");
    CliRun {
        exit: output.status.code(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        writes: writes_of_run(&state),
    }
}

/// Whether a comparison counts the writes. Only a run that reaches a real implementation makes them:
/// the stand-in interpreter does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writes {
    Compare,
    Skip,
}

/// Text with its line breaks as LF. The one normalization of text, declared: the prototype writes
/// CRLF on Windows, the repo stores LF, and the Rust side writes LF (ADR 0001 §11).
pub fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Every way `run` differs from what the prototype did for `case`: exit code, stdout through the
/// table or as text, stderr, and with [`Writes::Compare`] the writes in order. Empty when they agree.
pub fn differences(case: &Case, run: &CliRun, writes: Writes) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    match case.recorded() {
        Recorded::Json(python) => match (serde_json::from_slice::<Value>(&run.stdout), translate_python(&case.command_name(), &python)) {
            (Ok(rust), Ok(python)) => out.extend(json_diff(&rust, &python).into_iter().map(|line| format!("stdout {line}"))),
            (Err(error), _) => out.push(format!("stdout: rust printed no JSON ({error}): {}", String::from_utf8_lossy(&run.stdout))),
            (_, Err(error)) => out.push(format!("stdout: the table does not cover the prototype's output: {error}")),
        },
        Recorded::Text(python) => {
            let rust = lf(&String::from_utf8_lossy(&run.stdout));
            if rust != python {
                out.push(format!("stdout: rust {rust:?}, python {python:?}"));
            }
        }
    }
    let stderr = case.recorded_stderr();
    if case.spec.python.is_some() && lf(&run.stderr) != stderr {
        out.push(format!("stderr: rust {:?}, python {stderr:?}", run.stderr));
    }
    if writes == Writes::Compare {
        let diff = json_diff(&Value::Array(run.writes.clone()), &Value::Array(case.writes()));
        out.extend(diff.into_iter().map(|line| format!("writes {line}")));
    }
    out
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

/// The first field of a mutation, which names it: `closeIssue` in `mutation($i:ID!){closeIssue(…){…}}`.
/// `None` for a query.
pub fn mutation_name(query: &str) -> Option<String> {
    let text: Vec<char> = query.chars().collect();
    let mut at = 0;
    skip_blank(&text, &mut at);
    if !text[at..].starts_with(&['m', 'u', 't', 'a', 't', 'i', 'o', 'n']) {
        return None;
    }
    at += 8;
    skip_blank(&text, &mut at);
    if text.get(at) == Some(&'(') {
        skip_arguments(&text, &mut at).ok()?;
        skip_blank(&text, &mut at);
    }
    if text.get(at) != Some(&'{') {
        return None;
    }
    at += 1;
    skip_blank(&text, &mut at);
    Some(read_name(&text, &mut at)).filter(|name| !name.is_empty())
}

#[derive(Debug)]
struct Selection {
    name: String,
    /// The key of the answer, when it is not the field's name.
    alias: Option<String>,
    /// A `name: "…"` argument written in the query, for the fields that pick a value by name.
    by_name: Option<String>,
    fragment: bool,
    children: Vec<Selection>,
}

/// The one field the fake evaluates with its argument: the fixture holds its values in a map keyed by
/// field name, and a name with no value answers null, as on the real API.
const BY_NAME_FIELD: &str = "fieldValueByName";

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

/// Skips `( … )`, which can hold braces and quoted strings, and returns what was inside.
fn skip_arguments(text: &[char], at: &mut usize) -> Result<String, String> {
    let start = *at + 1;
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
                    return Ok(text[start..*at - 1].iter().collect());
                }
            }
            _ => {}
        }
    }
    Err("unbalanced parentheses in the query".to_owned())
}

/// The value of a `name: "…"` argument, if the arguments have one written as a string.
fn name_argument(arguments: &str) -> Option<String> {
    arguments.split(',').find_map(|argument| {
        let value = argument.trim().strip_prefix("name")?.trim_start().strip_prefix(':')?;
        let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
        Some(value.to_owned())
    })
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
                let children = parse_selection(text, at)?;
                out.push(Selection { name, alias: None, by_name: None, fragment: true, children });
            }
            Some(_) => {
                let mut name = read_name(text, at);
                if name.is_empty() {
                    return Err(format!("unexpected text in the query at {at}"));
                }
                skip_blank(text, at);
                let mut alias = None;
                if text.get(*at) == Some(&':') {
                    *at += 1;
                    skip_blank(text, at);
                    alias = Some(std::mem::replace(&mut name, read_name(text, at)));
                    if name.is_empty() {
                        return Err(format!("alias {alias:?} names no field"));
                    }
                    skip_blank(text, at);
                }
                let mut by_name = None;
                if text.get(*at) == Some(&'(') {
                    by_name = name_argument(&skip_arguments(text, at)?);
                    skip_blank(text, at);
                }
                let mut children = Vec::new();
                if text.get(*at) == Some(&'{') {
                    *at += 1;
                    children = parse_selection(text, at)?;
                }
                out.push(Selection { name, alias, by_name, fragment: false, children });
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
                let key = selection.alias.clone().unwrap_or_else(|| selection.name.clone());
                let found = match (&selection.by_name, object.get(&selection.name)) {
                    (Some(field), Some(values)) if selection.name == BY_NAME_FIELD => {
                        Some(values.get(field).cloned().unwrap_or(Value::Null))
                    }
                    (_, found) => found.cloned(),
                };
                match found {
                    Some(inner) if selection.children.is_empty() => {
                        out.insert(key, inner);
                    }
                    Some(inner) => {
                        out.insert(key, project(&selection.children, &inner, false)?);
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
    for keyword in ["query", "mutation"] {
        let word: Vec<char> = keyword.chars().collect();
        if text[at..].starts_with(&word) {
            at += word.len();
            skip_blank(&text, &mut at);
            if text.get(at) == Some(&'(') {
                skip_arguments(&text, &mut at)?;
                skip_blank(&text, &mut at);
            }
            break;
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
