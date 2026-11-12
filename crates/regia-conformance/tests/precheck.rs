//! The precheck across the two languages (#465): events and decision. Each case of `fixtures/precheck/events/`
//! transcribes tests of the prototype's `tools/tests/test_precheck.py` and of #406, as compact issues,
//! comments, state and reports. From them the harness builds, for each run, what both sides read: the
//! answers of the fake `gh` (GraphQL), the rows of the state database and the report files with their dates
//! on disk, at the fixtures' instant.
//!
//! - **In CI**, the product's `precheck` binary runs on each run, and its outcome line must be the one the
//!   prototype printed, recorded in `python.it.json`, key order included. It must also write nothing: not the
//!   tracker, not the state.
//! - **Locally**, with a copy of the prototype in `REGIA_HOME`, the prototype's precheck runs in process
//!   on the same answers and files, through its real tracker reader, runtime and reports (only the
//!   usage probe answers a fixed budget, and its clock is the fixtures' instant), and must print the
//!   recorded line again. With `RECORD_PRECHECK_CASES=1` it writes the recording instead.
//!
//!       REGIA_HOME=<copy> cargo test -p regia-conformance --test precheck -- --include-ignored
//!
//! **The comparison is one-sided, and declared.** The prototype's line also carries what #466 ports:
//! the budget and the policy in the payload, and per project the series of a stall, a postponement, the
//! backlog, the Proposals at their cap, the silent workers and the dossier's warnings. Those keys are taken
//! out of the prototype's line only, the product's must not have them, and no recorded run may end with a
//! reason of #466, so a case that #466 decides cannot pass for equal. The `t` of the line is each run's
//! own (the prototype's comes from the machine's clock), and so is the sentence of an error, whose key is
//! kept.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use regia_conformance::{
    FIXTURE_NOW, StateRow, bin_dir, fixture_for_python, fixture_for_rust, fixtures_root, log_var, prepare_state_dir, read_json, state_rows,
    workspace_root, write_state, writes_of_run,
};
use regia_core::SignatureRole;
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::prototype_env_var;
use serde::Deserialize;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde_json::{Value, json};

/// The keys of the prototype's payload, and of each of its projects, that #466 ports.
const LATER_PAYLOAD_KEYS: [&str; 4] = ["budget", "politica", "politica_motivo", "politica_locale"];
const LATER_PROJECT_KEYS: [&str; 8] = ["serie", "rinviato", "arretrato", "arretrato_task", "proposte", "dossier", "silenti", "silenti_task"];
/// The reasons that only #466 produces: a recorded run must end with none of them.
const LATER_REASONS: [&str; 5] = ["budget", "stallo", "da-formulare", "proposte-esaurite", "worker-silente"];

/// What the prototype's `t` reads as in a recording.
const T_PLACEHOLDER: &str = "{precheck-t}";
/// The value of an error's detail in a comparison: each side words its own.
const DETAIL_PLACEHOLDER: &str = "{detail}";
const RECORD_VARIABLE: &str = "RECORD_PRECHECK_CASES";
const DOSSIER: &str = "{dossier}";

/// The prototype's side, written into the run's folder: its precheck in process, with its real tracker reader
/// behind the fake `gh` and the deadline its own command sets, its real runtime on the run's state folder,
/// its reports on disk. Only the usage probe answers the run's budget, the clock reads the run's instant, and
/// the dossier's warnings and the workers have no data. It prints the exit code and what the precheck printed.
const PROTOTYPE_SIDE: &str = r#"import io
import json
import sys
from datetime import datetime
from pathlib import Path

copy, run = Path(sys.argv[1]), json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
sys.path.insert(0, str(copy / "tools"))
import precheck  # noqa: E402
import runtime  # noqa: E402

regia, _ = precheck._carica()
regia.gh = precheck._gh_chiuso
now = datetime.fromisoformat(run["now"].replace("Z", "+00:00"))


class Runtime:
    def __getattr__(self, name):
        return getattr(runtime, name)

    def sonda_cache(self, *args, **kwargs):
        return run["usage"]


amb = precheck.Ambiente(regia, precheck.TrackerGh(regia), Runtime(), adesso=lambda: now,
                        dossier=lambda cartella, repo=None: [])
out = io.StringIO()
code = precheck.main(run["args"], ambiente=amb, out=out)
print(json.dumps({"exit": code, "stdout": out.getvalue()}))
"#;

/// The answer of the usage probe on the prototype's side: known, fresh, under every threshold.
fn usage() -> Value {
    json!({"provider": {"claude": {"provider": "claude", "stato": "ok", "eta_s": 0, "finestre": {"5h": {"stato": "ok", "usato_pct": 10.0}}}}})
}

// ---------- the cases ----------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseFile {
    #[serde(rename = "_note")]
    note: String,
    /// The tests of the prototype it transcribes, and how.
    transcribes: Vec<String>,
    #[serde(default)]
    issues: Vec<IssueSpec>,
    #[serde(default)]
    comments: BTreeMap<String, Vec<CommentSpec>>,
    runs: Vec<RunSpec>,
}

/// An issue, with the defaults of the prototype's test helper: a task of `p`, ready.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueSpec {
    n: u64,
    /// The name of the Status option; one the configuration does not know stays as it is.
    status: Option<String>,
    #[serde(default = "default_project")]
    project: Option<String>,
    #[serde(default)]
    objective: bool,
    /// The maturity's word; empty for no label. A task is ready by default, an Objective has none.
    maturity: Option<String>,
    #[serde(default)]
    blocked_by: Vec<(u64, String)>,
    #[serde(default)]
    sub: Vec<(u64, String)>,
    created: Option<String>,
    /// The lines of an Objective's registry; by default the id, the folder `{dossier}/<project>`, wip 2,
    /// priority 3.
    registry: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
}

#[allow(clippy::unnecessary_wraps)]
fn default_project() -> Option<String> {
    Some("p".to_owned())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentSpec {
    id: u64,
    created: String,
    #[serde(default = "default_body")]
    body: String,
    /// Signed by the product's own signer, with this role's word and session.
    sign: Option<(String, String)>,
}

fn default_body() -> String {
    "A comment.".to_owned()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunSpec {
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    /// The issues of this run, in place of the case's.
    issues: Option<Vec<IssueSpec>>,
    /// The comments of this run, in place of the case's.
    comments: Option<BTreeMap<String, Vec<CommentSpec>>>,
    /// The value of each project's events cursor.
    #[serde(default)]
    cursor: BTreeMap<String, Value>,
    /// Each project's lock: `held`, `expired`, or `unreadable`.
    #[serde(default)]
    locks: BTreeMap<String, String>,
    #[serde(default)]
    leases: Vec<LeaseSpec>,
    #[serde(default)]
    reports: Vec<ReportSpec>,
    /// The issues whose comments the tracker does not answer.
    #[serde(default)]
    comments_missing: Vec<u64>,
    /// Every call to the tracker fails with this.
    tracker_fails: Option<String>,
    /// The state database is a file that is not one.
    #[serde(default)]
    database_garbage: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseSpec {
    task: u64,
    project: Option<String>,
    #[serde(default)]
    expired: bool,
    #[serde(default)]
    unreadable: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportSpec {
    #[serde(default = "default_project_name")]
    project: String,
    name: String,
    /// Seconds and nanoseconds since the epoch would be unreadable: an instant, with up to 7 decimals.
    modified: String,
}

fn default_project_name() -> String {
    "p".to_owned()
}

struct Case {
    name: String,
    dir: PathBuf,
    file: CaseFile,
}

fn cases() -> Vec<Case> {
    let root = fixtures_root().join("precheck").join("events");
    let mut out: Vec<Case> = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("{}: {error}", root.display()))
        .map(|entry| entry.expect("a case").path())
        .filter(|path| path.is_dir())
        .map(|dir| Case { name: dir.file_name().expect("a name").to_string_lossy().into_owned(), file: read_json(&dir.join("case.json")), dir })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ---------- what both sides read ----------

/// The words of the state's format, from `regia-store`.
fn store_word(section: &str, key: &str) -> String {
    let path = workspace_root().join("crates").join("regia-store").join("formats").join("store_v1.toml");
    let format: toml::Table = toml::from_str(&fs::read_to_string(path).expect("the v1 state format")).expect("TOML");
    format[section][key].as_str().unwrap_or_else(|| panic!("{section}.{key}")).to_owned()
}

fn tracker_config() -> Value {
    read_json(&fixtures_root().join("brief").join("tracker.json"))
}

/// The node of an issue as GraphQL answers the list of both sides.
fn node(spec: &IssueSpec, dossier: &str, config: &Value) -> Value {
    let mut labels: Vec<String> = Vec::new();
    if let Some(project) = &spec.project {
        labels.push(format!("progetto:{project}"));
    }
    if spec.objective {
        labels.push("tipo:obiettivo".to_owned());
    }
    let maturity = spec.maturity.clone().unwrap_or_else(|| if spec.objective { String::new() } else { "pronto".to_owned() });
    if !maturity.is_empty() {
        labels.push(format!("maturità:{maturity}"));
    }
    labels.extend(spec.labels.iter().cloned());
    let (field_by_name, field_values) = match &spec.status {
        Some(name) => {
            let option = config["status_options"][name].as_str().map_or_else(|| format!("opt-unknown-{}", spec.n), str::to_owned);
            let field = json!({"id": config["status_field_id"], "name": "Status"});
            (json!({"Status": {"name": name, "optionId": option}}), json!([{"optionId": option, "name": name, "field": field}]))
        }
        None => (json!({}), json!([])),
    };
    let body = if spec.objective {
        let project = spec.project.clone().unwrap_or_default();
        let registry = spec.registry.clone().unwrap_or_else(|| format!("id: {project}\ncartella: {DOSSIER}/{project}\nwip: 2\npriorita: 3"));
        format!("Synthetic objective.\n\n<!-- regia:progetto\n{}\n-->\n", registry.replace(DOSSIER, dossier))
    } else {
        String::new()
    };
    let pairs = |list: &[(u64, String)]| list.iter().map(|(number, state)| json!({"number": number, "state": state})).collect::<Vec<_>>();
    json!({
        "number": spec.n, "title": format!("t{}", spec.n), "state": "OPEN", "stateReason": null, "body": body,
        "url": format!("https://tracker.invalid/issues/{}", spec.n), "updatedAt": "2026-01-02T03:04:05Z",
        "createdAt": spec.created, "closedAt": null,
        "labels": {"nodes": labels.iter().map(|name| json!({"name": name})).collect::<Vec<_>>()},
        "parent": null,
        "subIssues": {"totalCount": spec.sub.len(), "nodes": pairs(&spec.sub)},
        "blockedBy": {"totalCount": spec.blocked_by.len(), "nodes": pairs(&spec.blocked_by)},
        "projectItems": {"nodes": [{"project": {"id": config["project_id"]}, "fieldValueByName": field_by_name, "fieldValues": {"nodes": field_values}}]},
    })
}

/// The body of a comment: as written, or signed by the product.
fn comment_body(spec: &CommentSpec) -> String {
    match &spec.sign {
        Some((role, session)) => {
            let role = SignatureRole::from_word(role).unwrap_or_else(|| panic!("no role {role}"));
            regia_core::sign(&spec.body, role, Some(session))
        }
        None => spec.body.clone(),
    }
}

/// The answers of the fake `gh` for a run.
fn responses(issues: &[IssueSpec], comments: &BTreeMap<String, Vec<CommentSpec>>, run: &RunSpec, dossier: &str) -> Value {
    if let Some(error) = &run.tracker_fails {
        return json!({"fail": {"error": error, "exit": 1}});
    }
    let config = tracker_config();
    let nodes: Vec<Value> = issues.iter().map(|spec| node(spec, dossier, &config)).collect();
    let mut by_number = serde_json::Map::new();
    for spec in issues.iter().filter(|spec| !run.comments_missing.contains(&spec.n)) {
        let list = comments.get(&spec.n.to_string()).cloned().unwrap_or_default();
        let nodes: Vec<Value> = list
            .iter()
            .map(|comment| {
                let url = format!("https://tracker.invalid/issues/{}#issuecomment-{}", spec.n, comment.id);
                json!({"databaseId": comment.id, "createdAt": comment.created, "url": url, "body": comment_body(comment)})
            })
            .collect();
        by_number.insert(spec.n.to_string(), json!({"comments": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": nodes}}));
    }
    json!({
        "expect": {"o": config["owner"], "r": config["repo"]},
        "issue_pages": [{"repository": {"issues": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": nodes}}}],
        "blocker_pages": {}, "sub_issue_pages": {}, "issues": by_number,
    })
}

/// The rows of the state database of a run.
fn rows(run: &RunSpec) -> Vec<StateRow> {
    let (value, updated) = (store_word("cursor", "value"), store_word("cursor", "updated"));
    let mut out: Vec<StateRow> = run
        .cursor
        .iter()
        .map(|(project, cursor)| StateRow {
            kind: store_word("kinds", "cursor"),
            key: project.clone(),
            doc: json!({value.as_str(): cursor, updated.as_str(): "2026-01-10T08:00:00.000+00:00"}),
        })
        .collect();
    let expires = |expired: bool| if expired { "2000-01-01T00:00:00.000+00:00" } else { "2999-01-01T00:00:00.000+00:00" };
    for (project, lock) in &run.locks {
        let doc = match lock.as_str() {
            "unreadable" => json!("not a document"),
            state => {
                let word = |key: &str| store_word("lock", key);
                json!({word("project"): project, word("holder"): "someone", word("taken_at"): "2026-01-10T08:00:00.000+00:00", word("ttl_s"): 600,
                    word("expires_at"): expires(state == "expired"), word("replaced"): null, word("stolen_at"): null})
            }
        };
        out.push(StateRow { kind: store_word("kinds", "lock"), key: project.clone(), doc });
    }
    for lease in &run.leases {
        let word = |key: &str| store_word("lease", key);
        let doc = if lease.unreadable {
            json!("not a document")
        } else {
            json!({word("task"): lease.task, word("project"): lease.project, word("holder"): "a-worker", word("taken_at"): "2026-01-10T08:00:00.000+00:00",
                word("heartbeat_at"): "2026-01-10T08:00:00.000+00:00", word("ttl_s"): 600, word("expires_at"): expires(lease.expired),
                word("replaced"): null, word("stolen_at"): null})
        };
        out.push(StateRow { kind: store_word("kinds", "lease"), key: lease.task.to_string(), doc });
    }
    out
}

/// An instant of a case as a time of the file system, to the 100 nanoseconds Windows keeps.
fn system_time(instant: &str) -> SystemTime {
    let (whole, fraction) = instant.trim_end_matches('Z').split_once('.').map_or((instant.trim_end_matches('Z'), ""), |(whole, fraction)| (whole, fraction));
    let at = regia_store::WallTime::parse_iso(&format!("{whole}Z")).unwrap_or_else(|| panic!("an instant: {instant}"));
    let nanos: u32 = format!("{fraction:0<9}")[..9].parse().expect("nanoseconds");
    let micros = u64::try_from(at.unix_micros()).expect("after the epoch");
    SystemTime::UNIX_EPOCH + Duration::from_micros(micros) + Duration::from_nanos(u64::from(nanos))
}

/// The folders of a run: the dossier with its reports, shared by both sides, which only read it.
fn prepare_dossier(label: &str, run: &RunSpec) -> PathBuf {
    let dossier = prepare_state_dir(&format!("{label}-dossier"));
    for report in &run.reports {
        let dir = dossier.join(&report.project).join(".regia").join("report");
        fs::create_dir_all(&dir).expect("the reports folder");
        let path = dir.join(&report.name);
        fs::write(&path, "---\ntipo: done\n---\n").expect("a report");
        let file = fs::OpenOptions::new().write(true).open(&path).expect("the report, to date it");
        file.set_modified(system_time(&report.modified)).expect("the report's date");
    }
    dossier
}

/// A state folder for one side of a run: the synthetic `tracker.json`, the rows, and the answers.
fn prepare_side(label: &str, run: &RunSpec, responses: &Value) -> (PathBuf, PathBuf) {
    let state = prepare_state_dir(label);
    if run.database_garbage {
        fs::write(state.join(regia_store::database_name()), "not a database, and long enough to be read as one").expect("garbage");
    } else {
        write_state(&state, &rows(run));
    }
    let path = state.join("responses.json");
    fs::write(&path, serde_json::to_string_pretty(responses).expect("JSON")).expect("the responses");
    (state, path)
}

fn slashed(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// The last line of an output that is the outcome line, and the lines before it.
fn outcome_of(stdout: &str) -> (String, Vec<String>) {
    let mut lines: Vec<String> = stdout.lines().map(str::to_owned).collect();
    let last = lines.pop().unwrap_or_default();
    (last, lines)
}

/// The precheck's line with its `t` as the placeholder.
fn with_placeholder_t(line: &str) -> String {
    let at = line.rfind(",\"t\":\"").unwrap_or_else(|| panic!("no t at the end of {line}"));
    format!("{},\"t\":\"{T_PLACEHOLDER}\"}}", &line[..at])
}

/// A recorded line: its `t`, and the sentence of an error, which can name the machine's folders, as their
/// placeholders. Inside the sentence every quote is escaped, so the end of the payload is the last one.
fn for_the_recording(line: &str) -> String {
    let line = with_placeholder_t(line);
    let start = "\"payload\":{\"dettaglio\":\"";
    match (line.find(start), line.rfind("\"},\"t\":\"")) {
        (Some(from), Some(to)) => format!("{}{start}{DETAIL_PLACEHOLDER}{}", &line[..from], &line[to..]),
        _ => line,
    }
}

// ---------- the two sides ----------

struct Ran {
    exit: Option<i32>,
    stdout: String,
    stderr: String,
}

fn precheck_binary() -> PathBuf {
    bin_dir().join(format!("precheck{}", std::env::consts::EXE_SUFFIX))
}

/// The product's precheck on a run's folder.
fn product(state: &Path, responses: &Path, args: &[String]) -> Ran {
    let launch = regia_exec::harness::launch(&precheck_binary(), args, state);
    let (launch, _) = fixture_for_rust(launch, responses, state);
    let output = launch.run().expect("run the precheck");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    Ran { exit: output.status.code(), stdout: text(&output.stdout), stderr: text(&output.stderr) }
}

/// The copy of the prototype in `REGIA_HOME`.
fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype's tools/ and policies/", prototype_env_var("HOME")));
    assert!(home.join("tools").join("precheck.py").is_file(), "no tools/precheck.py in {}", home.display());
    home
}

/// The prototype's precheck on a run's folder.
fn prototype_side(home: &Path, state: &Path, responses: &Path, args: &[String]) -> Ran {
    let script = state.join("prototype_side.py");
    fs::write(&script, PROTOTYPE_SIDE).expect("the prototype's side");
    let input = state.join("run.json");
    fs::write(&input, json!({"now": FIXTURE_NOW, "args": args, "usage": usage()}).to_string()).expect("the run");
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    argv.extend([script.display().to_string(), home.display().to_string(), input.display().to_string()]);
    let launch = regia_exec::harness::program(&interpreter, &argv, home).limits(PROTOTYPE_LIMITS);
    let launch = fixture_for_python(launch, responses, state).env(&log_var(), state.join("python-calls.log"));
    let output = launch.run().expect("run the prototype");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(0), "the prototype's side failed\nstdout: {stdout}\nstderr: {stderr}");
    let result: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|error| panic!("{error}: {stdout}"));
    Ran { exit: result["exit"].as_i64().and_then(|exit| i32::try_from(exit).ok()), stdout: result["stdout"].as_str().unwrap_or_default().replace("\r\n", "\n"), stderr }
}

// ---------- the comparison ----------

/// A JSON value whose objects keep their keys in the order written: the order is part of the line.
#[derive(Debug, Clone, PartialEq)]
enum Ordered {
    Scalar(Value),
    List(Vec<Ordered>),
    Object(Vec<(String, Ordered)>),
}

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(OrderedVisitor)
    }
}

struct OrderedVisitor;

impl<'de> Visitor<'de> for OrderedVisitor {
    type Value = Ordered;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Ordered, E> {
        Ok(Ordered::Scalar(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Ordered, E> {
        Ok(Ordered::Scalar(Value::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Ordered, E> {
        Ok(Ordered::Scalar(Value::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Ordered, E> {
        Ok(Ordered::Scalar(Value::from(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Ordered, E> {
        Ok(Ordered::Scalar(Value::from(value)))
    }

    fn visit_unit<E>(self) -> Result<Ordered, E> {
        Ok(Ordered::Scalar(Value::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Ordered, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Ordered::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
        let mut fields = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Ordered>()? {
            fields.push((key, value));
        }
        Ok(Ordered::Object(fields))
    }
}

impl Ordered {
    fn get(&self, key: &str) -> Option<&Ordered> {
        match self {
            Ordered::Object(fields) => fields.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Ordered> {
        match self {
            Ordered::Object(fields) => fields.iter_mut().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    fn remove(&mut self, keys: &[&str]) {
        if let Ordered::Object(fields) = self {
            fields.retain(|(name, _)| !keys.contains(&name.as_str()));
        }
    }

    fn keys(&self) -> Vec<&str> {
        match self {
            Ordered::Object(fields) => fields.iter().map(|(name, _)| name.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Ordered::Scalar(Value::String(text)) => Some(text),
            _ => None,
        }
    }

    /// The value as Python's `json.dumps(…, ensure_ascii=True, separators=(",", ":"))` writes it.
    fn compact(&self) -> String {
        match self {
            Ordered::Scalar(value) => {
                let text = serde_json::to_string(value).expect("JSON");
                let mut out = String::new();
                for c in text.chars() {
                    if (' '..='~').contains(&c) {
                        out.push(c);
                    } else {
                        let mut units = [0u16; 2];
                        for unit in c.encode_utf16(&mut units) {
                            out.push_str(&format!("\\u{unit:04x}"));
                        }
                    }
                }
                out
            }
            Ordered::List(items) => format!("[{}]", items.iter().map(Ordered::compact).collect::<Vec<_>>().join(",")),
            Ordered::Object(fields) => {
                let fields: Vec<String> =
                    fields.iter().map(|(key, value)| format!("{}:{}", Ordered::Scalar(Value::from(key.as_str())).compact(), value.compact())).collect();
                format!("{{{}}}", fields.join(","))
            }
        }
    }
}

fn parse_line(line: &str) -> Ordered {
    serde_json::from_str(line).unwrap_or_else(|error| panic!("not a JSON line ({error}): {line}"))
}

/// The projects of a line's payload.
fn projects_of(line: &mut Ordered) -> Vec<&mut Ordered> {
    match line.get_mut("payload").and_then(|payload| payload.get_mut("progetti")) {
        Some(Ordered::List(projects)) => projects.iter_mut().collect(),
        _ => Vec::new(),
    }
}

/// A line as the comparison reads it: the detail of an error as the placeholder.
fn comparable(line: &str) -> Ordered {
    let mut line = parse_line(line);
    if let Some(detail) = line.get_mut("payload").and_then(|payload| payload.get_mut("dettaglio")) {
        *detail = Ordered::Scalar(Value::from(DETAIL_PLACEHOLDER));
    }
    line
}

/// The prototype's line without what #466 ports. A reason of #466 anywhere fails: the case would not
/// test this slice.
fn without_later(line: &str) -> Ordered {
    let mut line = comparable(line);
    let reason = line.get("motivo").and_then(Ordered::text).unwrap_or_default().to_owned();
    assert!(!LATER_REASONS.contains(&reason.as_str()), "a recorded run ends with {reason}, a reason of #466");
    if let Some(payload) = line.get_mut("payload") {
        payload.remove(&LATER_PAYLOAD_KEYS);
    }
    for project in projects_of(&mut line) {
        let reason = project.get("motivo").and_then(Ordered::text).unwrap_or_default().to_owned();
        assert!(!LATER_REASONS.contains(&reason.as_str()), "a recorded project ends with {reason}, a reason of #466");
        project.remove(&LATER_PROJECT_KEYS);
    }
    line
}

/// Every way the product's run differs from the recording, and from the contract of the line.
fn differences(run: &Ran, recorded: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let (last, before) = outcome_of(&run.stdout);
    let canonical = serde_json::from_str::<Ordered>(&last).ok().map(|line| line.compact());
    if !last.starts_with('{') || !last.is_ascii() || canonical.as_deref() != Some(last.as_str()) {
        out.push(format!("the last line is not compact ASCII JSON as Python writes it: {last}"));
        return out;
    }
    if before.len() != 1 || !before[0].starts_with("precheck:") {
        out.push(format!("one line of diagnostics before the outcome, not {before:?}"));
    }
    if !run.stderr.is_empty() {
        out.push(format!("stderr: {}", run.stderr));
    }
    let rust = comparable(&with_placeholder_t(&last));
    if let Some(Ordered::Scalar(exit)) = rust.get("exit")
        && exit.as_i64().and_then(|exit| i32::try_from(exit).ok()) != run.exit
    {
        out.push(format!("exit: the line says {exit}, the process ended with {:?}", run.exit));
    }
    let mut product = rust.clone();
    for project in projects_of(&mut product) {
        let later: Vec<&str> = project.keys().into_iter().filter(|key| LATER_PROJECT_KEYS.contains(key)).collect();
        if !later.is_empty() {
            out.push(format!("a key of #466 in the product's project: {later:?}"));
        }
    }
    if product.get("payload").is_some_and(|payload| payload.keys().iter().any(|key| LATER_PAYLOAD_KEYS.contains(key))) {
        out.push("a key of #466 in the product's payload".to_owned());
    }
    let python = without_later(recorded["line"].as_str().expect("a recorded line"));
    if recorded["exit"].as_i64().and_then(|exit| i32::try_from(exit).ok()) != run.exit {
        out.push(format!("exit: rust {:?}, python {}", run.exit, recorded["exit"]));
    }
    if rust != python {
        out.push(format!("line:\n  rust   {last}\n  python {}", recorded["line"]));
    }
    out
}

// ---------- the tests ----------

/// The folders, answers and arguments of a run of a case.
struct Prepared {
    label: String,
    responses: Value,
    args: Vec<String>,
    run: RunSpec,
}

/// The folders of each side are their own: the two tests run at once.
fn prepare(case: &Case, index: usize, run: &RunSpec, side: &str) -> Prepared {
    let label = format!("precheck-{}-{index}-{side}", case.name);
    let dossier = prepare_dossier(&label, run);
    let issues = run.issues.clone().unwrap_or_else(|| case.file.issues.clone());
    let comments = run.comments.clone().unwrap_or_else(|| case.file.comments.clone());
    let responses = responses(&issues, &comments, run, &slashed(&dossier));
    Prepared { label, responses, args: run.args.clone(), run: run.clone() }
}

fn recording(case: &Case) -> Vec<Value> {
    let path = case.dir.join("python.it.json");
    if path.is_file() { read_json(&path) } else { Vec::new() }
}

/// Acceptance 1, 2 and 3: on every run of every case the product prints the line the prototype printed, and
/// writes nothing.
#[test]
fn every_precheck_case_natively_gives_the_prototype_s_line() {
    let cases = cases();
    assert!(cases.len() >= 20, "the cases of test_precheck.py and of #406 are missing");
    let mut failures = Vec::new();
    for case in &cases {
        let recorded = recording(case);
        assert_eq!(recorded.len(), case.file.runs.len(), "{}: one recorded line per run", case.name);
        for (index, run) in case.file.runs.iter().enumerate() {
            let prepared = prepare(case, index, run, "rust");
            let (state, responses) = prepare_side(&prepared.label, &prepared.run, &prepared.responses);
            let before = (!run.database_garbage).then(|| state_rows(&state));
            let ran = product(&state, &responses, &prepared.args);
            let mut diff = differences(&ran, &recorded[index]);
            if let Some(before) = before
                && state_rows(&state) != before
            {
                diff.push("the product wrote the state".to_owned());
            }
            if !writes_of_run(&state).is_empty() {
                diff.push("the product wrote the tracker".to_owned());
            }
            if !diff.is_empty() {
                failures.push(format!("{} run {index} ({}):\n{}", case.name, run.note.clone().unwrap_or_default(), diff.join("\n")));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The prototype prints the recorded line on every run; with the variable set, its lines become the
/// recording.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_prints_the_recorded_lines() {
    let home = prototype();
    let record = std::env::var_os(RECORD_VARIABLE).is_some_and(|value| value == "1");
    let mut failures = Vec::new();
    for case in cases() {
        let recorded = recording(&case);
        let mut lines = Vec::new();
        for (index, run) in case.file.runs.iter().enumerate() {
            let prepared = prepare(&case, index, run, "python");
            let (state, responses) = prepare_side(&prepared.label, &prepared.run, &prepared.responses);
            let ran = prototype_side(&home, &state, &responses, &prepared.args);
            let (last, _) = outcome_of(&ran.stdout);
            let line = json!({"exit": ran.exit, "line": for_the_recording(&last)});
            if !record && recorded.get(index).is_none_or(|recorded| comparable(recorded["line"].as_str().unwrap_or_default()) != comparable(line["line"].as_str().unwrap_or_default()) || recorded["exit"] != line["exit"]) {
                failures.push(format!("{} run {index}: the prototype printed {line}, recorded {:?}", case.name, recorded.get(index)));
            }
            lines.push(line);
        }
        if record {
            let text = serde_json::to_string_pretty(&lines).expect("JSON") + "\n";
            fs::write(case.dir.join("python.it.json"), text).expect("the recording");
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The vocabulary of the format is the prototype's: each outcome's reasons and exit code, and the line's
/// limit.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_format_s_vocabulary_is_the_prototype_s() {
    let home = prototype();
    let state = prepare_state_dir("precheck-vocabulary");
    let script = state.join("vocabulary.py");
    let text = [
        "import json, sys",
        "sys.path.insert(0, sys.argv[1])",
        "import precheck",
        "print(json.dumps({'reasons': {k: sorted(v) for k, v in precheck.MOTIVI.items()}, 'exits': precheck.USCITE,",
        "                  'limit': precheck.LIMITE_RIGA, 'events': [m for _, m in precheck.PRIORITA_EVENTI], 'idle': list(precheck.PRIORITA_IDLE)}))",
    ];
    fs::write(&script, text.join("\n") + "\n").expect("the script");
    let empty = state.join("no-responses.json");
    fs::write(&empty, "{}").expect("empty responses");
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    argv.extend([script.display().to_string(), home.join("tools").display().to_string()]);
    let launch = fixture_for_python(regia_exec::harness::program(&interpreter, &argv, &home).limits(PROTOTYPE_LIMITS), &empty, &state);
    let output = launch.run().expect("run the prototype");
    let python: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
    let path = workspace_root().join("crates").join("regia-controller").join("formats").join("precheck_v1.toml");
    let format: toml::Table = toml::from_str(&fs::read_to_string(path).expect("the precheck's format")).expect("TOML");
    for (_, outcome) in format["outcomes"].as_table().expect("outcomes") {
        let word = outcome["word"].as_str().expect("a word");
        let mut reasons: Vec<&str> = outcome["reasons"].as_table().expect("reasons").values().map(|reason| reason.as_str().expect("a reason")).collect();
        reasons.sort_unstable();
        assert_eq!(json!(reasons), python["reasons"][word], "{word}");
        assert_eq!(json!(outcome["exit"].as_integer()), python["exits"][word], "{word}");
    }
    assert_eq!(json!(format["line"]["max_length"].as_integer()), python["limit"]);
    assert_eq!(python["events"], json!(["risposta-a-gate", "report-nuovi", "lease-scadute", "worker-silente", "pronti", "inbox"]), "the order the engine keeps");
    assert_eq!(python["idle"], json!(["budget", "occupato", "limite-wip", "proposte-esaurite", "lavoro-in-corso", "attesa-del-titolare", "nessun-evento"]));
}

/// The comparison notices a difference: an outcome, a count, a key out of order, a key of #466 in the
/// product's line.
#[test]
fn the_comparison_notices_a_difference() {
    let line = r#"{"v":1,"outcome":"new-activity","exit":0,"motivo":"pronti","cursore":[{"progetto":"p","valore":null}],"payload":{"progetti":[{"id":"p","esito":"new-activity","motivo":"pronti","eventi":1,"pronti":1,"report":0,"gate":0,"inbox":0,"lease":0,"task":[2]}]},"t":"2026-01-10T09:00:00Z"}"#;
    let recorded = json!({"exit": 0, "line": with_placeholder_t(&line.replace(r#""lease":0,"task""#, r#""lease":0,"arretrato":1,"task""#).replace("]},\"t\"", "],\"politica\":\"file\"},\"t\""))});
    let ran = |line: &str| Ran { exit: Some(0), stdout: format!("precheck: rust\n{line}\n"), stderr: String::new() };
    assert_eq!(differences(&ran(line), &recorded), Vec::<String>::new(), "#466's keys are out of the prototype's side");
    for altered in [
        line.replace("\"pronti\":1", "\"pronti\":2"),
        line.replace(r#""report":0,"gate":0"#, r#""gate":0,"report":0"#),
        line.replace(r#""task":[2]"#, r#""task":[2],"serie":1"#),
        line.replace("\"motivo\":\"pronti\",\"cursore\"", "\"motivo\":\"inbox\",\"cursore\""),
    ] {
        assert!(!differences(&ran(&altered), &recorded).is_empty(), "unnoticed: {altered}");
    }
    let later = json!({"exit": 3, "line": line.replace("\"motivo\":\"pronti\",\"cursore\"", "\"motivo\":\"budget\",\"cursore\"")});
    assert!(std::panic::catch_unwind(|| without_later(later["line"].as_str().unwrap())).is_err(), "a reason of #466 cannot be recorded");
}

/// TestVincoli of the prototype: the command never reads stdin, and the engine has no way to write the
/// tracker, a cursor, a lock or a lease.
#[test]
fn the_precheck_reads_no_stdin_and_writes_nothing() {
    let root = workspace_root().join("crates").join("regia-controller").join("src");
    let mut sources = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("a folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    assert!(sources.len() >= 8, "the scan reads the sources");
    for path in sources {
        let text = fs::read_to_string(&path).expect("a source");
        for forbidden in ["stdin", "add_comment", "write_cursor", "acquire_lock", "release_lock", "acquire_lease", "heartbeat", "open_step", "close_step", "record_own_comment"] {
            assert!(!text.contains(forbidden), "{} uses {forbidden}", path.display());
        }
    }
}

/// Each case says what it transcribes.
#[test]
fn every_case_names_what_it_transcribes() {
    for case in cases() {
        assert!(!case.file.transcribes.is_empty() && case.file.note.starts_with("Synthetic."), "{}", case.name);
        assert!(!case.file.runs.is_empty(), "{}", case.name);
    }
}
