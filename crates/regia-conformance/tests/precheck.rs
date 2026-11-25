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
//! **The budget and the policy (#466).** Both sides read the same cache of the usage probes from the
//! state database, written fresh at the run's instant so that neither asks a provider (the run checks that
//! the row is unchanged after it), and the same policy file and local override. The prototype's runtime
//! reads its clock at the run's instant too. The product has the user's consent to the probes in a
//! settings folder of the run, as the prototype, which has no notion of consent, always probes.
//!
//! **The comparison is one-sided, and declared.** The prototype's line also carries what #466 has not
//! ported yet: per project the series of a stall, a postponement, the backlog, the Proposals at their cap,
//! the silent workers and the dossier's warnings. Those keys are taken out of the prototype's line only,
//! the product's must not have them, and no recorded run may end with a reason not ported yet, so a case
//! that such a part decides cannot pass for equal. The `t` of the line is each run's own (the prototype's
//! comes from the machine's clock), and so is the sentence of an error, whose key is kept.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use regia_conformance::{
    FIXTURE_NOW, StateRow, fixture_for_python, fixture_for_rust, fixtures_root, log_var, read_json, state_rows, workspace_root, write_state,
    writes_of_run,
};
use regia_core::SignatureRole;
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::{env_var, prototype_env_var};
use serde::Deserialize;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde_json::{Value, json};

mod common;
use common::Bin;

/// The keys of the prototype's payload, and of each of its projects, that #466 has not ported yet.
const LATER_PAYLOAD_KEYS: [&str; 0] = [];
const LATER_PROJECT_KEYS: [&str; 3] = ["dossier", "silenti", "silenti_task"];
/// The reasons that only the parts not ported yet produce: a recorded run must end with none of them.
const LATER_REASONS: [&str; 1] = ["worker-silente"];

/// What the prototype's `t` reads as in a recording.
const T_PLACEHOLDER: &str = "{precheck-t}";
/// The value of an error's detail in a comparison: each side words its own.
const DETAIL_PLACEHOLDER: &str = "{detail}";
const RECORD_VARIABLE: &str = "RECORD_PRECHECK_CASES";
const DOSSIER: &str = "{dossier}";

/// The prototype's side, written into the run's folder: its precheck in process, with its real tracker reader
/// behind the fake `gh` and the deadline its own command sets, its real runtime on the run's state folder with
/// its clock at the run's instant (the cache of the budget included), its reports on disk, and the policy of
/// the run's file with the override of the state folder. The dossier's warnings and the workers have no data.
/// It prints the exit code and what the precheck printed.
const PROTOTYPE_SIDE: &str = r#"import io
import json
import sys
from datetime import datetime
from pathlib import Path

copy, run = Path(sys.argv[1]), json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
sys.path.insert(0, str(copy / "tools"))
import politica_budget  # noqa: E402
import precheck  # noqa: E402
import runtime  # noqa: E402

regia, _ = precheck._carica()
regia.gh = precheck._gh_chiuso
now = datetime.fromisoformat(run["now"].replace("Z", "+00:00"))
runtime._adesso = lambda: now
amb = precheck.Ambiente(regia, precheck.TrackerGh(regia), runtime, adesso=lambda: now,
                        dossier=lambda cartella, repo=None: [],
                        politica=politica_budget.carica(Path(run["policy"]), adesso=now))
out = io.StringIO()
code = precheck.main(run["args"], ambiente=amb, out=out)
print(json.dumps({"exit": code, "stdout": out.getvalue()}))
"#;

/// The variable that names the product's settings folder: `SETTINGS_DIR_SUFFIX` of `regia-controller`'s
/// precheck, which this crate does not depend on.
const SETTINGS_DIR_SUFFIX: &str = "CONFIG_DIR";

/// The share of Claude's 5-hour window of a run that does not say: under every threshold.
const DEFAULT_PCT: f64 = 10.0;
/// How long before the run's instant the cache was last asked: well within its interval, so neither side
/// asks a provider.
const CACHE_ATTEMPT_AGO_S: f64 = 5.0;

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
    /// How many dependencies the tracker counts, when more than it lists: read only in part.
    blocked_total: Option<usize>,
    #[serde(default)]
    sub: Vec<(u64, String)>,
    created: Option<String>,
    /// When the issue last changed; by default the run's instant, so that no issue waiting for the owner is
    /// one the prototype skips as unchanged since the cursor (#589 of the dossier).
    updated: Option<String>,
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
    /// Claude's reading in the cache of the probes; by default fresh, at [`DEFAULT_PCT`].
    usage: Option<UsageSpec>,
    /// The text of the policy file, in place of the product's copy of the prototype's.
    policy_file: Option<String>,
    /// The machine's local override of the policy, as its file holds it.
    local_override: Option<Value>,
    /// Ticks in a row on the run's state, each with its line: without them, the run is one tick.
    #[serde(default)]
    ticks: Vec<TickSpec>,
}

/// A tick of a run: how many minutes after the one before (the first after the fixtures' instant), and
/// what changed in between, as a step or the controller would change it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TickSpec {
    #[serde(default)]
    minutes: f64,
    #[serde(default)]
    note: Option<String>,
    /// The issues from this tick on.
    issues: Option<Vec<IssueSpec>>,
    /// The events cursors written before it.
    #[serde(default)]
    cursor: BTreeMap<String, Value>,
    /// The locks written before it, `none` to remove one.
    #[serde(default)]
    locks: BTreeMap<String, String>,
    /// Other actors' cursors written before it, by `<project>:<name>`: the controller's `rinvio`, the
    /// alerting step's `segnalazioni`.
    #[serde(default)]
    signals: BTreeMap<String, Value>,
    /// Claude's reading from this tick on.
    usage: Option<UsageSpec>,
}

/// Claude's reading as the cache stores it: the last valid one, with its 5-hour window, and the last
/// attempt when it failed.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageSpec {
    /// The share of the window of the valid reading; none without a valid reading.
    pct: Option<f64>,
    /// How old the valid reading is.
    #[serde(default)]
    age_s: f64,
    /// When the window resets, with its offset; by default two hours after the run's instant.
    reset: Option<String>,
    /// The window is not open: no reset yet.
    #[serde(default)]
    not_open: bool,
    /// Why the last attempt failed.
    failed: Option<String>,
    /// The cache was asked and holds no provider at all.
    #[serde(default)]
    empty: bool,
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

/// The node of an issue as GraphQL answers the list of both sides, at the run's instant `now`.
fn node(spec: &IssueSpec, dossier: &str, config: &Value, now: &str) -> Value {
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
        "url": format!("https://tracker.invalid/issues/{}", spec.n), "updatedAt": spec.updated.as_deref().unwrap_or(now),
        "createdAt": spec.created, "closedAt": null,
        "labels": {"nodes": labels.iter().map(|name| json!({"name": name})).collect::<Vec<_>>()},
        "parent": null,
        "subIssues": {"totalCount": spec.sub.len(), "nodes": pairs(&spec.sub)},
        "blockedBy": {"totalCount": spec.blocked_total.unwrap_or(spec.blocked_by.len()), "nodes": pairs(&spec.blocked_by)},
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

/// The answers of the fake `gh` for a run at `now`.
fn responses(issues: &[IssueSpec], comments: &BTreeMap<String, Vec<CommentSpec>>, run: &RunSpec, dossier: &str, now: &str) -> Value {
    if let Some(error) = &run.tracker_fails {
        return json!({"fail": {"error": error, "exit": 1}});
    }
    let config = tracker_config();
    let nodes: Vec<Value> = issues.iter().map(|spec| node(spec, dossier, &config, now)).collect();
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

/// An instant `ago_s` seconds before `now`, as the state writes one.
fn state_instant(now: &str, ago_s: f64) -> String {
    let now = regia_store::WallTime::parse_iso(now).unwrap_or_else(|| panic!("an instant: {now}"));
    now.checked_add_secs(-ago_s).expect("an instant in range").to_iso()
}

/// The row of the cache of the usage probes at `now`: asked [`CACHE_ATTEMPT_AGO_S`] ago, with Claude's
/// reading as `usage` says.
fn cache_row(usage: &UsageSpec, now: &str) -> StateRow {
    let word = |section: &str, key: &str| store_word(section, key);
    let (ok, unknown) = (word("usage_statuses", "ok"), word("usage_statuses", "unknown"));
    let reading = |status: &str, windows: Value, reason: Option<&str>| {
        let mut doc = json!({word("usage", "provider"): "claude", word("usage", "status"): status, word("usage", "plan"): null, word("usage", "windows"): windows});
        if let Some(reason) = reason {
            doc[word("usage", "reason")] = json!(reason);
        }
        doc
    };
    let mut providers = serde_json::Map::new();
    if !usage.empty {
        let valid = usage.pct.map(|pct| {
            let window = if usage.not_open {
                json!({word("usage_window", "status"): ok, word("usage_window", "used_pct"): pct, word("usage_window", "remaining_pct"): 100.0 - pct,
                    word("usage_window", "open"): false, word("usage_window", "reset"): null, word("usage_window", "reset_in_s"): null})
            } else {
                let reset = usage.reset.clone().unwrap_or_else(|| state_instant(now, -7200.0));
                json!({word("usage_window", "status"): ok, word("usage_window", "used_pct"): pct, word("usage_window", "remaining_pct"): 100.0 - pct,
                    word("usage_window", "reset"): reset, word("usage_window", "reset_in_s"): 0})
            };
            reading(&ok, json!({"5h": window}), None)
        });
        let last = match (&usage.failed, &valid) {
            (Some(failed), _) => reading(&unknown, json!({}), Some(failed)),
            (None, Some(valid)) => valid.clone(),
            (None, None) => reading(&unknown, json!({}), None),
        };
        let valid_at = valid.as_ref().map(|_| state_instant(now, usage.age_s));
        providers.insert(
            "claude".to_owned(),
            json!({word("budget", "valid"): valid, word("budget", "valid_at"): valid_at, word("budget", "last"): last}),
        );
    }
    let doc = json!({word("budget", "providers"): providers, word("budget", "attempted_at"): state_instant(now, CACHE_ATTEMPT_AGO_S)});
    StateRow { kind: word("kinds", "cache"), key: word("budget", "key"), doc }
}

/// The rows of the state database of a run at `now`.
fn rows(run: &RunSpec, now: &str) -> Vec<StateRow> {
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
    let usage = run.usage.clone().unwrap_or(UsageSpec { pct: Some(DEFAULT_PCT), ..UsageSpec::default() });
    out.push(cache_row(&usage, now));
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

/// The folder of a run's policy file, `<home>/policies/budget.yaml`: the run's text, else the product's copy
/// of the prototype's file.
fn policy_home(state: &Path, run: &RunSpec) -> PathBuf {
    let home = state.join("home");
    let file = home.join(regia_core::policy::file_path());
    fs::create_dir_all(file.parent().expect("the policies folder")).expect("the policies folder");
    let text = match &run.policy_file {
        Some(text) => text.clone(),
        None => fs::read_to_string(workspace_root().join("crates").join("regia-core").join("formats").join("budget.yaml")).expect("the policy file"),
    };
    fs::write(&file, text).expect("the policy file");
    home
}

/// The user's settings of a run, which the product reads: the consent to the usage probes, given.
fn settings_dir(state: &Path, consent: &str) -> PathBuf {
    let dir = state.join("settings");
    fs::create_dir_all(&dir).expect("the settings folder");
    fs::write(dir.join("agents.toml"), format!("probes = \"{consent}\"\n")).expect("the settings");
    dir
}

/// A state folder for one side of a run at `now`: the synthetic `tracker.json`, the rows, the local
/// override, and the answers.
fn prepare_side(label: &str, run: &RunSpec, responses: &Value, now: &str) -> (PathBuf, PathBuf) {
    let state = prepare_state_dir(label);
    if run.database_garbage {
        fs::write(state.join(regia_store::database_name()), "not a database, and long enough to be read as one").expect("garbage");
    } else {
        write_state(&state, &rows(run, now));
    }
    if let Some(local) = &run.local_override {
        fs::write(state.join(regia_core::policy::local_file_name()), serde_json::to_string_pretty(local).expect("JSON")).expect("the override");
    }
    let path = state.join("responses.json");
    fs::write(&path, serde_json::to_string_pretty(responses).expect("JSON")).expect("the responses");
    (state, path)
}

/// The rows of a run's state, without the detector's own: the precheck writes those and nothing else.
fn rows_besides_the_detector(state: &Path) -> Value {
    let detector = format!(":{}", detector_word("cursor"));
    let rows = state_rows(state);
    let kept: Vec<Value> = rows
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| !(row["kind"] == store_word("kinds", "cursor") && row["key"].as_str().is_some_and(|key| key.ends_with(&detector))))
        .cloned()
        .collect();
    Value::Array(kept)
}

/// A word of the precheck's own format: its detector's section.
fn detector_word(key: &str) -> String {
    let path = workspace_root().join("crates").join("regia-controller").join("formats").join("precheck_v1.toml");
    let format: toml::Table = toml::from_str(&fs::read_to_string(path).expect("the precheck's format")).expect("TOML");
    format["detector"][key].as_str().unwrap_or_else(|| panic!("detector.{key}")).to_owned()
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

/// The binaries this file runs, of two other packages: the product's `precheck`, and `regia`, whose presence
/// is how the harness finds the folder of the workspace's binaries.
const BINARIES: [Bin; 2] = [Bin { package: "regia-controller", name: "precheck" }, Bin { package: "regia-cli", name: regia_identity::BIN_NAME }];

/// The folder with the workspace's binaries, built (or found fresh) first: a `cargo test` of this file alone builds none of them.
fn bin_dir() -> PathBuf {
    common::built(&BINARIES);
    regia_conformance::bin_dir()
}

/// The state folder of a run, after the binaries it needs are in place: the library's own reads them.
fn prepare_state_dir(label: &str) -> PathBuf {
    common::built(&BINARIES);
    regia_conformance::prepare_state_dir(label)
}

fn precheck_binary() -> PathBuf {
    bin_dir().join(format!("precheck{}", std::env::consts::EXE_SUFFIX))
}

/// The product's precheck on a run's folder at `now`, with the consent to the probes as given.
fn product(state: &Path, responses: &Path, args: &[String], run: &RunSpec, now: &str, consent: &str) -> Ran {
    let launch = regia_exec::harness::launch(&precheck_binary(), args, state);
    let (launch, _) = fixture_for_rust(launch, responses, state);
    let launch = launch
        .env(&env_var(SETTINGS_DIR_SUFFIX), settings_dir(state, consent))
        .env(&prototype_env_var("HOME"), policy_home(state, run))
        .env(&env_var("NOW"), now);
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

/// The prototype's precheck on a run's folder at `now`.
fn prototype_side(home: &Path, state: &Path, responses: &Path, args: &[String], run: &RunSpec, now: &str) -> Ran {
    let script = state.join("prototype_side.py");
    fs::write(&script, PROTOTYPE_SIDE).expect("the prototype's side");
    let input = state.join("run.json");
    let policy = policy_home(state, run).join(regia_core::policy::file_path());
    fs::write(&input, json!({"now": now, "args": args, "policy": policy}).to_string()).expect("the run");
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    argv.extend([script.display().to_string(), home.display().to_string(), input.display().to_string()]);
    let launch = regia_exec::harness::program(&interpreter, &argv, home).limits(PROTOTYPE_LIMITS);
    let launch = fixture_for_python(launch, responses, state).env(&log_var(), state.join("python-calls.log")).env(&env_var("NOW"), now);
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
    run: RunSpec,
}

/// The folders of each side are their own: the two tests run at once.
fn prepare(case: &Case, index: usize, run: &RunSpec, side: &str) -> Prepared {
    let label = format!("precheck-{}-{index}-{side}", case.name);
    let dossier = prepare_dossier(&label, run);
    let issues = run.issues.clone().unwrap_or_else(|| case.file.issues.clone());
    let comments = run.comments.clone().unwrap_or_else(|| case.file.comments.clone());
    let responses = responses(&issues, &comments, run, &slashed(&dossier), FIXTURE_NOW);
    Prepared { label, responses, run: run.clone() }
}

/// The consent the product has in a recorded run: the prototype always probes.
const GRANTED: &str = "granted";

/// The fixtures' instant `minutes` later, as the clock and the cursors write it.
fn later(minutes: f64) -> String {
    regia_store::WallTime::parse_iso(FIXTURE_NOW).and_then(|now| now.checked_add_secs(minutes * 60.0)).expect("an instant").to_iso_seconds()
}

/// Writes rows over those of a state, removing those whose document is `null`.
fn put_rows(state: &Path, rows: &[StateRow]) {
    let format: toml::Table = toml::from_str(&fs::read_to_string(workspace_root().join("crates").join("regia-store").join("formats").join("store_v1.toml")).expect("the format")).expect("TOML");
    let column = |name: &str| format["table"][name].as_str().expect("a column").to_owned();
    let [table, kind, key, generation, document] = ["name", "kind", "key", "generation", "document"].map(column);
    let connection = rusqlite::Connection::open(state.join(regia_store::database_name())).expect("the state database");
    for row in rows {
        connection.execute(&format!("DELETE FROM {table} WHERE {kind} = ?1 AND {key} = ?2"), (&row.kind, &row.key)).expect("a removal");
        if !row.doc.is_null() {
            connection
                .execute(&format!("INSERT INTO {table} ({kind}, {key}, {generation}, {document}) VALUES (?1, ?2, 1, ?3)"), (&row.kind, &row.key, row.doc.to_string()))
                .expect("a state row");
        }
    }
}

/// The rows a tick changes before it runs at `now`: cursors, locks, signals, and the cache asked again.
fn tick_rows(tick: &TickSpec, usage: &UsageSpec, now: &str) -> Vec<StateRow> {
    let partial = RunSpec { cursor: tick.cursor.clone(), locks: tick.locks.iter().filter(|(_, lock)| *lock != "none").map(|(p, l)| (p.clone(), l.clone())).collect(), usage: Some(usage.clone()), ..RunSpec::default() };
    let mut out: Vec<StateRow> = rows(&partial, now);
    for project in tick.locks.iter().filter(|(_, lock)| *lock == "none").map(|(project, _)| project) {
        out.push(StateRow { kind: store_word("kinds", "lock"), key: project.clone(), doc: Value::Null });
    }
    let (value, updated) = (store_word("cursor", "value"), store_word("cursor", "updated"));
    for (key, signal) in &tick.signals {
        out.push(StateRow { kind: store_word("kinds", "cursor"), key: key.clone(), doc: json!({value.as_str(): signal, updated.as_str(): "2026-01-10T08:00:00.000+00:00"}) });
    }
    out
}

/// The detector's rows of the projects after a run, by key, the global one aside: the prototype has none.
fn detector_rows(state: &Path) -> Value {
    let (cursor, global) = (detector_word("cursor"), format!("{}:{}", detector_word("global_project"), detector_word("cursor")));
    let value = store_word("cursor", "value");
    let rows = state_rows(state);
    let found = rows.as_array().expect("rows").iter().filter(|row| {
        row["kind"] == store_word("kinds", "cursor") && row["key"].as_str().is_some_and(|key| key.ends_with(&format!(":{cursor}")) && key != global)
    });
    Value::Object(found.map(|row| (row["key"].as_str().unwrap_or_default().to_owned(), row["doc"][&value].clone())).collect())
}

/// The ticks of a run, one when it gives none.
fn ticks_of(run: &RunSpec) -> Vec<TickSpec> {
    if run.ticks.is_empty() { vec![TickSpec::default()] } else { run.ticks.clone() }
}

/// What a side runs: each tick of a run at its instant on one state folder, after the tick's changes. The
/// run's folders go after it: each holds a copy of the fake `gh`, and a few hundred of them fill a
/// runner's disk; a difference is in the failure's message.
fn run_ticks(case: &Case, index: usize, run: &RunSpec, side: &str, mut each: impl FnMut(&Path, &Path, &str, &TickSpec)) {
    let prepared = prepare(case, index, run, side);
    let (state, responses_path) = prepare_side(&prepared.label, &prepared.run, &prepared.responses, FIXTURE_NOW);
    let (mut minutes, mut issues, mut usage) = (0.0, run.issues.clone().unwrap_or_else(|| case.file.issues.clone()), run.usage.clone());
    let comments = run.comments.clone().unwrap_or_else(|| case.file.comments.clone());
    let dossier = bin_dir().join("conformance-state").join(format!("{}-dossier", prepared.label));
    for tick in &ticks_of(run) {
        minutes += tick.minutes;
        let now = if run.ticks.is_empty() { FIXTURE_NOW.to_owned() } else { later(minutes) };
        if let Some(changed) = &tick.issues {
            issues = changed.clone();
        }
        if tick.usage.is_some() {
            usage = tick.usage.clone();
        }
        if !run.ticks.is_empty() {
            let answers = responses(&issues, &comments, run, &slashed(&dossier), &now);
            fs::write(&responses_path, serde_json::to_string_pretty(&answers).expect("JSON")).expect("the responses");
            let usage = usage.clone().unwrap_or(UsageSpec { pct: Some(DEFAULT_PCT), ..UsageSpec::default() });
            put_rows(&state, &tick_rows(tick, &usage, &now));
        }
        each(&state, &responses_path, &now, tick);
    }
    for dir in [&state, &dossier] {
        let _ = fs::remove_dir_all(dir);
    }
}

fn recording(case: &Case) -> Vec<Value> {
    let path = case.dir.join("python.it.json");
    if path.is_file() { read_json(&path) } else { Vec::new() }
}

/// Acceptance 1, 2 and 3: on every tick of every run of every case the product prints the line the
/// prototype printed, and writes nothing but its detector's rows, which hold the same series and digests as
/// the prototype's.
#[test]
fn every_precheck_case_natively_gives_the_prototype_s_line() {
    let cases = cases();
    assert!(cases.len() >= 20, "the cases of test_precheck.py and of #406 are missing");
    let mut failures = Vec::new();
    for case in &cases {
        let recorded = recording(case);
        let ticks: usize = case.file.runs.iter().map(|run| ticks_of(run).len()).sum();
        assert_eq!(recorded.len(), ticks, "{}: one recorded line per tick", case.name);
        let mut next = 0;
        for (index, run) in case.file.runs.iter().enumerate() {
            run_ticks(case, index, run, "rust", |state, responses, now, tick| {
                let before = (!run.database_garbage).then(|| rows_besides_the_detector(state));
                let ran = product(state, responses, &run.args, run, now, GRANTED);
                let mut diff = differences(&ran, &recorded[next]);
                if let Some(before) = before
                    && rows_besides_the_detector(state) != before
                {
                    diff.push("the product wrote the state besides its detector's rows, or a probe ran".to_owned());
                }
                if !run.database_garbage && recorded[next].get("detector").is_some_and(|rows| *rows != detector_rows(state)) {
                    diff.push(format!("the detector's rows:
  rust   {}
  python {}", detector_rows(state), recorded[next]["detector"]));
                }
                if !writes_of_run(state).is_empty() {
                    diff.push("the product wrote the tracker".to_owned());
                }
                if !diff.is_empty() {
                    let note = tick.note.clone().or_else(|| run.note.clone()).unwrap_or_default();
                    failures.push(format!("{} run {index} tick {next} ({note}):
{}", case.name, diff.join("
")));
                }
                next += 1;
            });
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("

"));
}

/// The prototype prints the recorded line on every tick, and writes the recorded detector's rows; with the
/// variable set, its lines and rows become the recording.
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
            run_ticks(&case, index, run, "python", |state, responses, now, _| {
                let ran = prototype_side(&home, state, responses, &run.args, run, now);
                let (last, _) = outcome_of(&ran.stdout);
                let mut line = json!({"exit": ran.exit, "line": for_the_recording(&last)});
                if !run.database_garbage {
                    line["detector"] = detector_rows(state);
                }
                let at = lines.len();
                let same = |recorded: &Value| {
                    comparable(recorded["line"].as_str().unwrap_or_default()) == comparable(line["line"].as_str().unwrap_or_default())
                        && recorded["exit"] == line["exit"]
                        && recorded.get("detector") == line.get("detector")
                };
                if !record && recorded.get(at).is_none_or(|recorded| !same(recorded)) {
                    failures.push(format!("{} run {index} tick {at}: the prototype printed {line}, recorded {:?}", case.name, recorded.get(at)));
                }
                lines.push(line);
            });
        }
        if record {
            let text = serde_json::to_string_pretty(&lines).expect("JSON") + "
";
            fs::write(case.dir.join("python.it.json"), text).expect("the recording");
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("
"));
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

/// The comparison notices a difference: an outcome, a count, a key out of order, the budget or the policy,
/// a key not ported yet in the product's line.
#[test]
fn the_comparison_notices_a_difference() {
    let line = r#"{"v":1,"outcome":"new-activity","exit":0,"motivo":"pronti","cursore":[{"progetto":"p","valore":null}],"payload":{"progetti":[{"id":"p","esito":"new-activity","motivo":"pronti","eventi":1,"pronti":1,"report":0,"gate":0,"inbox":0,"lease":0,"task":[2]}],"budget":{"claude_5h_pct":10.0},"politica":"file"},"t":"2026-01-10T09:00:00Z"}"#;
    let recorded = json!({"exit": 0, "line": with_placeholder_t(&line.replace(r#""task":[2]"#, r#""task":[2],"dossier":[{"problema":"senza-remoto"}]"#))});
    let ran = |line: &str| Ran { exit: Some(0), stdout: format!("precheck: rust\n{line}\n"), stderr: String::new() };
    assert_eq!(differences(&ran(line), &recorded), Vec::<String>::new(), "the keys not ported yet are out of the prototype's side");
    for altered in [
        line.replace("\"pronti\":1", "\"pronti\":2"),
        line.replace(r#""report":0,"gate":0"#, r#""gate":0,"report":0"#),
        line.replace(r#""task":[2]"#, r#""task":[2],"silenti_task":[3]"#),
        line.replace(r#""task":[2]"#, r#""task":[2],"serie":1"#),
        line.replace(r#""lease":0,"task""#, r#""lease":0,"arretrato":1,"task""#),
        line.replace("\"motivo\":\"pronti\",\"cursore\"", "\"motivo\":\"inbox\",\"cursore\""),
        line.replace("10.0", "10"),
        line.replace(",\"politica\":\"file\"", ""),
        line.replace("\"file\"", "\"default\""),
    ] {
        assert!(!differences(&ran(&altered), &recorded).is_empty(), "unnoticed: {altered}");
    }
    let later = json!({"exit": 0, "line": line.replace("\"motivo\":\"pronti\",\"cursore\"", "\"motivo\":\"worker-silente\",\"cursore\"")});
    assert!(std::panic::catch_unwind(|| without_later(later["line"].as_str().unwrap())).is_err(), "a reason not ported yet cannot be recorded");
}

/// TestVincoli of the prototype: the command never reads stdin, and the engine has no way to write the
/// tracker, a lock, a lease or a cursor other than its detector's own: one call writes a cursor, in the
/// live sources' `write_detector`, under the detector's name.
/// The Rust sources under `dir`, every level.
fn sources_under(dir: PathBuf) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut stack = vec![dir];
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
    sources
}

/// The calls of `sources` that write a cursor, each with the function it is in; and no source uses a word of
/// `forbidden`.
fn cursor_writes(sources: Vec<PathBuf>, forbidden: &[&str]) -> Vec<(PathBuf, String)> {
    let mut writes = Vec::new();
    for path in sources {
        let text = fs::read_to_string(&path).expect("a source");
        for word in forbidden {
            assert!(!text.contains(word), "{} uses {word}", path.display());
        }
        writes.extend(text.match_indices(".write_cursor(").map(|(at, _)| {
            let function = text[..at].rfind("fn ").map(|start| text[start..at].to_owned()).unwrap_or_default();
            (path.clone(), function)
        }));
    }
    writes
}

/// The precheck's sources (its module and its command, #465): no stdin, and the one write of a cursor is its
/// detector's. The tick of #467 lives in the same crate, and writes for its own sake: the test after this.
#[test]
fn the_precheck_reads_no_stdin_and_writes_only_its_detector() {
    let src = workspace_root().join("crates").join("regia-controller").join("src");
    let mut sources = sources_under(src.join("precheck"));
    sources.push(src.join("bin").join("precheck.rs"));
    assert!(sources.len() >= 8, "the scan reads the sources");
    let forbidden =
        ["stdin", "add_comment", "acquire_lock", "release_lock", "acquire_lease", "heartbeat", "open_step", "close_step", "record_own_comment", "write_state_file"];
    let writes = cursor_writes(sources, &forbidden);
    assert_eq!(writes.len(), 1, "one call writes a cursor: {writes:?}");
    let (path, function) = &writes[0];
    assert!(path.ends_with("sources.rs"), "{}", path.display());
    assert!(function.starts_with("fn write_detector(") && function.contains("detector_cursor(project)"), "the write is the detector's: {function}");
}

/// The tick's sources (#467): no stdin, no tracker write, no lock taken or released (the host's, #468), no step or
/// lease; its writes of a cursor are the postponement's and the steps registry's, and nothing else.
#[test]
fn the_tick_writes_only_its_postponements_and_its_registry() {
    let sources = sources_under(workspace_root().join("crates").join("regia-controller").join("src").join("engine"));
    assert!(sources.len() >= 4, "the scan reads the sources");
    let forbidden = ["stdin", "add_comment", "acquire_lock", "release_lock", "acquire_lease", "heartbeat", "open_step", "close_step", "record_own_comment", "write_state_file"];
    let mut functions: Vec<String> = cursor_writes(sources, &forbidden)
        .into_iter()
        .map(|(path, function)| {
            assert!(path.ends_with("native.rs"), "{}", path.display());
            function.split('(').next().unwrap_or_default().to_owned()
        })
        .collect();
    functions.sort();
    assert_eq!(functions, ["fn postpone", "fn save"], "the writes are the postponement's and the registry's");
}

/// The product's line of a run, parsed, after it ran on `state` with the consent given.
fn product_line(state: &Path, responses: &Path, run: &RunSpec, consent: &str) -> Value {
    let ran = product(state, responses, &[], run, FIXTURE_NOW, consent);
    let (last, _) = outcome_of(&ran.stdout);
    let line: Value = serde_json::from_str(&last).unwrap_or_else(|error| panic!("{error}: {}", ran.stdout));
    assert_eq!(line["exit"].as_i64().and_then(|exit| i32::try_from(exit).ok()), ran.exit, "{last}");
    line
}

/// A synthetic project of one run, with the product's folders: the issues, and the state as `run` says.
fn product_run(label: &str, issues: Value, run: Value) -> (PathBuf, PathBuf, RunSpec) {
    let run: RunSpec = serde_json::from_value(run).expect("a run");
    let issues: Vec<IssueSpec> = serde_json::from_value(issues).expect("issues");
    let dossier = prepare_dossier(label, &run);
    let responses = responses(&issues, &BTreeMap::new(), &run, &slashed(&dossier), FIXTURE_NOW);
    let (state, responses) = prepare_side(label, &run, &responses, FIXTURE_NOW);
    (state, responses, run)
}

/// S1 of the dossier's `docs/app/piano/da-decidere.md`, Acceptance 2, through the command: with the probes
/// refused the ready task starts, an Inbox item alone does not, and there is no alarm, whatever the cache
/// holds, which is not read.
#[test]
fn with_the_probes_refused_the_command_starts_the_ready_and_holds_backfill() {
    let objective = json!({"n": 1, "status": "Approvato", "objective": true});
    let failed = json!({"usage": {"failed": "network unreachable"}});
    let (state, responses, run) = product_run("precheck-probes-off-ready", json!([objective, {"n": 2, "status": "Approvato"}]), failed.clone());
    let before = rows_besides_the_detector(&state);
    let line = product_line(&state, &responses, &run, "refused");
    assert_eq!((line["motivo"].as_str(), line["exit"].as_i64()), (Some("pronti"), Some(0)), "{line}");
    assert_eq!(line["payload"]["budget"], json!({"sonda": "spenta"}), "{line}");
    assert_eq!(rows_besides_the_detector(&state), before, "nothing read or written for the budget");
    let (state, responses, run) = product_run("precheck-probes-off-inbox", json!([objective, {"n": 2, "status": "Inbox", "maturity": ""}]), failed);
    let line = product_line(&state, &responses, &run, "refused");
    assert_eq!((line["motivo"].as_str(), line["exit"].as_i64()), (Some("budget"), Some(3)), "{line}");
    assert_eq!(line["payload"]["progetti"][0]["motivo"], "budget", "{line}");
}

/// Change 3 and Acceptance 3 of #466, through the command on its real state: a reading that fails with
/// the consent given keeps the ready task from starting, tick after tick; from the fourth, the alarm, with
/// the count and the text that names the setting. Turned off, the probes let it start and the count goes.
/// No condition on the system: Claude's reading fails the same everywhere, the macOS runner included,
/// where the probes do not read it at all and the gate is open from the first tick.
#[test]
fn a_reading_failing_four_ticks_in_a_row_raises_the_alarm_until_the_probes_are_off() {
    let issues = json!([{"n": 1, "status": "Approvato", "objective": true}, {"n": 2, "status": "Approvato"}]);
    let (state, responses, run) = product_run("precheck-alarm", issues, json!({"usage": {"failed": "network unreachable"}}));
    let detector = || {
        let rows = state_rows(&state);
        let key = format!("{}:{}", detector_word("global_project"), detector_word("cursor"));
        let value = store_word("cursor", "value");
        rows.as_array().unwrap().iter().find(|row| row["key"] == key.as_str()).map(|row| row["doc"][&value][&detector_word("budget")].clone())
    };
    for tick in 1..=5 {
        let line = product_line(&state, &responses, &run, GRANTED);
        let budget = &line["payload"]["budget"];
        if cfg!(target_os = "macos") {
            assert_eq!((line["motivo"].as_str(), budget.clone()), (Some("pronti"), json!({"sonda": "non-letta"})), "tick {tick}: {line}");
            continue;
        }
        assert_eq!((line["motivo"].as_str(), line["exit"].as_i64()), (Some("budget"), Some(3)), "tick {tick}: {line}");
        assert_eq!((budget["claude_5h_pct"].clone(), budget["motivo"].clone()), (Value::Null, json!("sonda: network unreachable")), "tick {tick}");
        assert_eq!(detector().map(|count| count["giri"].clone()), Some(json!(tick)), "tick {tick}");
        match budget.get("allarme").and_then(Value::as_str) {
            Some(text) => {
                assert!(tick >= 4, "tick {tick}: {text}");
                assert!(text.contains("probes = \"refused\"") && text.contains("agents.toml") && text.contains(&tick.to_string()), "{text}");
                assert_eq!((budget["giri"].clone(), budget["da"].clone()), (json!(tick), json!(FIXTURE_NOW)), "tick {tick}");
            }
            None => assert!(tick < 4, "tick {tick}: no alarm in {line}"),
        }
    }
    let line = product_line(&state, &responses, &run, "refused");
    assert_eq!((line["motivo"].as_str(), line["payload"]["budget"].clone()), (Some("pronti"), json!({"sonda": "spenta"})), "{line}");
    assert!(detector().is_none_or(|count| count.is_null()), "the count went: {:?}", detector());
}

/// Each case says what it transcribes.
#[test]
fn every_case_names_what_it_transcribes() {
    for case in cases() {
        assert!(!case.file.transcribes.is_empty() && case.file.note.starts_with("Synthetic."), "{}", case.name);
        assert!(!case.file.runs.is_empty(), "{}", case.name);
    }
}
