//! The controller's tick across the two languages (#467): the traces of #436 (`fixtures/controller/`) as golden.
//!
//! For each case the harness builds the state folder the prototype's shadow driver built: the rows of
//! `state.json` with the shadow's cleanup applied (the live controller's lock and the suspended records out), the
//! tracker's answers for the fake `gh`, the reports and the projects' folders. Then the Rust tick runs the case's
//! ticks on one injected clock, which moves only between ticks, with:
//!
//! - the product's `precheck` command as the precheck, once a tick at the tick's instant (`REGIA_NOW`), on the same
//!   fake `gh` and state folder, with the usage probes' cache answering the case's reading at every tick;
//! - a `Dry` surface, which starts nothing, and the tick's own lock, postponements and steps registry in the state;
//! - the consent to the probes given, so the token's wake-up runs, fake on `Dry`, as the prototype's with its fake
//!   launcher.
//!
//! Each tick gives what the prototype's trace holds: the precheck's line, the tick's line, the launches, the cursors
//! the tick changed and the state document. The comparison is on values, key order aside, after the normalizations
//! the trace's README names (`{dossier}`, `{runtime}`, `{controller}`, `{pid}`, `{rinvio-N}`, `{precheck-t}`) and these,
//! declared:
//!
//! - **the prompts** are English on the Rust side: each side's prompt is compared through its parts (kind, project,
//!   folder, the precheck's outcome and `t`, the attached payload byte for byte, the commands it cites mapped
//!   through the facade table, the other quoted words, the exit codes it cites);
//! - **the fake wake-up's sentence** is each side's own, in its language;
//! - **the precheck's alarm of a failing reading** (#466) is the product's alone: its count in the global detector's
//!   row and its keys in the payload's budget are taken out of the Rust side only;
//! - **the dossier's warnings** (#743) have no data in the traces: the prototype's driver recorded them with a stub of
//!   the precheck's dossier that answers none (`docs/app/ombra/driver.py` of the dossier), while the product reads the
//!   projects' folders with git. Each project's `dossier` is taken out of the Rust side only, in the precheck's line and
//!   in the payload a prompt attaches; `tests/precheck.rs` compares them with the prototype's real source.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use regia_agents::{Dry, LaunchError, LaunchRequest, LaunchSurface, ProbeConsent, Receipt, SurfaceKind, WakeOutcome, WakeUp};
use regia_conformance::{StateRow, fixture_for_rust, fixtures_root, read_json, state_rows, workspace_root, write_state};
use regia_controller::engine::native::{Processes, StoreLock, StoreSignals, StoreSteps, random_id};
use regia_controller::engine::{Controller, Folders, Host, Options, Ports, Precheck, PrecheckRun, prompt};
use regia_controller::precheck::Json;
use regia_identity::{BIN_NAME, env_var, prototype_env_var};
use regia_store::{Holder, ManualClock, ProjectId, Store, Ttl, WallTime};
use serde_json::{Map, Value, json};

mod common;
use common::Bin;

const BINARIES: [Bin; 2] = [Bin { package: "regia-controller", name: "precheck" }, Bin { package: "regia-cli", name: BIN_NAME }];
const DOSSIER: &str = "{dossier}";
const RECORD_GOLDEN: &str = "RECORD_CONTROLLER_PROMPTS";
/// The holder of the controller's lock in a run: the trace names it `{controller}`.
const HOLDER: &str = "controller-rust-shadow";

// ---------- the words ----------

fn toml_of(path: &[&str]) -> toml::Table {
    let file = path.iter().fold(workspace_root(), |dir, part| dir.join(part));
    toml::from_str(&fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))).expect("TOML")
}

fn store_word(section: &str, key: &str) -> String {
    let format = toml_of(&["crates", "regia-store", "formats", "store_v1.toml"]);
    format[section][key].as_str().unwrap_or_else(|| panic!("store {section}.{key}")).to_owned()
}

fn precheck_word(section: &str, key: &str) -> String {
    let format = toml_of(&["crates", "regia-controller", "formats", "precheck_v1.toml"]);
    format[section][key].as_str().unwrap_or_else(|| panic!("precheck {section}.{key}")).to_owned()
}

fn controller_word(path: &[&str]) -> String {
    let format = toml_of(&["crates", "regia-controller", "formats", "controller_v1.toml"]);
    let mut value = toml::Value::Table(format);
    for part in path {
        value = value.get(part).unwrap_or_else(|| panic!("controller {path:?}")).clone();
    }
    value.as_str().unwrap_or_else(|| panic!("controller {path:?}")).to_owned()
}

// ---------- a case ----------

struct CaseFiles {
    name: String,
    case: Value,
    state: Value,
    responses: Value,
    trace: Value,
}

fn cases() -> Vec<CaseFiles> {
    let dir = fixtures_root().join("controller").join("cases");
    let mut out: Vec<CaseFiles> = fs::read_dir(&dir)
        .expect("the controller's cases")
        .map(|entry| entry.expect("a case").path())
        .filter(|path| path.is_dir())
        .map(|path| CaseFiles {
            name: path.file_name().expect("a name").to_string_lossy().into_owned(),
            case: read_json(&path.join("case.json")),
            state: read_json(&path.join("state.json")),
            responses: read_json(&path.join("responses.json")),
            trace: read_json(&path.join("python.it.json")),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(out.len(), 8, "the eight scenarios of #436");
    out
}

/// `{dossier}` replaced by `dossier` in every string of `value`.
fn placed(value: &Value, dossier: &str) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace(DOSSIER, dossier)),
        Value::Array(items) => Value::Array(items.iter().map(|item| placed(item, dossier)).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, item)| (key.clone(), placed(item, dossier))).collect()),
        other => other.clone(),
    }
}

/// The case's arguments as the prototype's parser reads them: the ticks and the interval.
fn ticks_and_interval(args: &[Value]) -> (usize, f64) {
    let args: Vec<&str> = args.iter().map(|arg| arg.as_str().expect("an argument")).collect();
    let value = |name: &str| args.iter().position(|arg| *arg == name).map(|at| args[at + 1]);
    let ticks = if args.contains(&"--una-volta") { 1 } else { value("--giri").map_or(0, |n| n.parse().expect("ticks")) };
    (ticks, value("--intervallo").map_or(900.0, |s| s.parse().expect("interval")))
}

/// The answers of the fake `gh`: the comments of an issue, which the trace keys by number as the prototype's
/// `comments(last:100)` answers them, become that issue's, with a URL the product's query selects.
fn fake_gh_responses(responses: &Value) -> Value {
    let mut out = responses.clone();
    let mut issues = Map::new();
    for (number, data) in responses["comments"].as_object().into_iter().flatten() {
        let mut issue = data["repository"]["issue"].clone();
        if let Some(comments) = issue.get_mut("comments").and_then(Value::as_object_mut) {
            comments.entry("pageInfo").or_insert(json!({"hasNextPage": false, "endCursor": null}));
        }
        if let Some(nodes) = issue.pointer_mut("/comments/nodes").and_then(Value::as_array_mut) {
            for node in nodes {
                let url = format!("https://tracker.invalid/issues/{number}#issuecomment-{}", node["databaseId"]);
                node.as_object_mut().expect("a comment").entry("url").or_insert(json!(url));
            }
        }
        issues.insert(number.clone(), issue);
    }
    out.as_object_mut().expect("the answers").remove("comments");
    out["issues"] = Value::Object(issues);
    out
}

/// The registry's folders, from the Objectives of the answers.
fn registry_folders(responses: &Value) -> BTreeMap<String, Option<String>> {
    let mut folders = BTreeMap::new();
    for page in responses["issue_pages"].as_array().into_iter().flatten() {
        for node in page["repository"]["issues"]["nodes"].as_array().into_iter().flatten() {
            let labels: Vec<&str> = node["labels"]["nodes"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str()).collect();
            if !labels.contains(&"tipo:obiettivo") {
                continue;
            }
            let Some(id) = labels.iter().find_map(|label| label.strip_prefix("progetto:")) else { continue };
            let registry = regia_core::Registry::parse(node["body"].as_str().unwrap_or_default());
            folders.insert(id.to_owned(), registry.folder().map(str::to_owned));
        }
    }
    folders
}

/// The rows of `state.json`, with the shadow's cleanup: the lock the live controller holds, and the steps
/// registry's records that are suspended or resuming. What it removed, as the trace says it.
fn cleaned_rows(state: &Value, dossier: &str) -> (Vec<StateRow>, Value) {
    let (lock, cursor) = (store_word("kinds", "lock"), store_word("kinds", "cursor"));
    let reserved = controller_word(&["reserved_project"]);
    let registry_key = format!("{reserved}:{}", controller_word(&["registry", "cursor"]));
    let (value, steps, state_key) = (store_word("cursor", "value"), controller_word(&["registry", "steps"]), controller_word(&["registry", "record", "state"]));
    let dropped_states = [controller_word(&["registry", "states", "suspended"]), controller_word(&["registry", "states", "resuming"])];
    let mut dropped_lock = false;
    let mut dropped: BTreeMap<String, i64> = BTreeMap::new();
    let mut rows = Vec::new();
    for document in state["documents"].as_array().expect("documents") {
        let (kind, key) = (document["tipo"].as_str().expect("a kind"), document["chiave"].as_str().expect("a key"));
        if kind == lock && key == reserved {
            dropped_lock = true;
            continue;
        }
        let mut doc = placed(&document["doc"], dossier);
        let records = doc.get_mut(&value).and_then(|value| value.get_mut(&steps)).and_then(Value::as_object_mut);
        if let (true, Some(records)) = (kind == cursor && key == registry_key, records) {
            records.retain(|_, record| {
                let state = record[&state_key].as_str().unwrap_or_default().to_owned();
                let keep = !dropped_states.contains(&state);
                if !keep {
                    *dropped.entry(state).or_default() += 1;
                }
                keep
            });
        }
        rows.push(StateRow { kind: kind.to_owned(), key: key.to_owned(), doc });
    }
    (rows, json!({"controller_lock_dropped": dropped_lock, "records_dropped": dropped}))
}

/// The row of the usage probes' cache that answers `usage` (the prototype's `sonda_cache` of the case) at `now`:
/// asked as long ago as the case says, with Claude's reading as old as it says.
fn cache_row(usage: &Value, now: f64) -> StateRow {
    let at = |ago: f64| WallTime::from_unix_secs_f64(now - ago).expect("an instant").to_iso();
    let mut providers = Map::new();
    for (provider, reading) in usage["provider"].as_object().into_iter().flatten() {
        let mut stored = reading.clone();
        let fields = stored.as_object_mut().expect("a reading");
        let age = fields.remove("eta_s").and_then(|age| age.as_f64());
        fields.remove("da_cache");
        let valid = reading[store_word("usage", "status")] == json!(store_word("usage_statuses", "ok"));
        providers.insert(
            provider.clone(),
            json!({
                store_word("budget", "valid"): if valid { stored.clone() } else { Value::Null },
                store_word("budget", "valid_at"): if valid { json!(at(age.unwrap_or(0.0))) } else { Value::Null },
                store_word("budget", "last"): stored,
            }),
        );
    }
    let attempted = usage["cache"]["eta_tentativo_s"].as_f64().unwrap_or(5.0);
    let doc = json!({store_word("budget", "providers"): providers, store_word("budget", "attempted_at"): at(attempted)});
    StateRow { kind: store_word("kinds", "cache"), key: store_word("budget", "key"), doc }
}

/// A row written over the one there, as the probes' cache rewrites its own.
fn put_row(state: &Path, row: &StateRow) {
    let table = |name: &str| store_word("table", name);
    let connection = rusqlite::Connection::open(state.join(regia_store::database_name())).expect("the state database");
    connection
        .execute(
            &format!(
                "INSERT OR REPLACE INTO {} ({}, {}, {}, {}) VALUES (?1, ?2, 1, ?3)",
                table("name"),
                table("kind"),
                table("key"),
                table("generation"),
                table("document")
            ),
            (&row.kind, &row.key, row.doc.to_string()),
        )
        .expect("the row");
}

/// The rows of the state, by kind and key, without the monotonic stamp.
fn documents(state: &Path) -> BTreeMap<(String, String), Value> {
    state_rows(state)
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| ((row["kind"].as_str().unwrap_or_default().to_owned(), row["key"].as_str().unwrap_or_default().to_owned()), row["doc"].clone()))
        .collect()
}

/// What a tick changed, as the shadow's driver takes it: the cursors by key with their new value (`null` when
/// removed), the other documents by `kind:key`; never the controller's lock, nor the probes' cache this harness
/// rewrites before each tick.
fn changes(before: &BTreeMap<(String, String), Value>, after: &BTreeMap<(String, String), Value>) -> (Map<String, Value>, Map<String, Value>) {
    let (lock, cursor, cache) = (store_word("kinds", "lock"), store_word("kinds", "cursor"), store_word("kinds", "cache"));
    let (value, stamp) = (store_word("cursor", "value"), "_m".to_owned());
    let reserved = controller_word(&["reserved_project"]);
    let (mut cursors, mut others) = (Map::new(), Map::new());
    let keys: std::collections::BTreeSet<&(String, String)> = before.keys().chain(after.keys()).collect();
    for key in keys {
        let (kind, name) = key;
        if (*kind == lock && *name == reserved) || *kind == cache {
            continue;
        }
        let (a, b) = (before.get(key), after.get(key));
        if *kind == cursor {
            let (va, vb) = (a.map(|doc| doc[&value].clone()), b.map(|doc| doc[&value].clone()));
            if a.is_none() != b.is_none() || va != vb {
                cursors.insert(name.clone(), vb.unwrap_or(Value::Null));
            }
        } else {
            let strip = |doc: Option<&Value>| doc.map(|doc| {
                let mut doc = doc.clone();
                if let Some(fields) = doc.as_object_mut() {
                    fields.remove(&stamp);
                }
                doc
            });
            if strip(a) != strip(b) {
                others.insert(format!("{kind}:{name}"), strip(b).unwrap_or(Value::Null));
            }
        }
    }
    (cursors, others)
}

// ---------- the ports of the harness ----------

/// The product's `precheck` command on the run's folder, at the clock's instant, with the case's reading in the
/// probes' cache.
struct CommandPrecheck {
    state: PathBuf,
    responses: PathBuf,
    settings: PathBuf,
    home: PathBuf,
    usage: Value,
    clock: ManualClock,
    lines: Mutex<Vec<Value>>,
}

impl Precheck for CommandPrecheck {
    fn run(&self, projects: &[String], _timeout_s: f64) -> PrecheckRun {
        let now = regia_store::Clock::now(&self.clock);
        put_row(&self.state, &cache_row(&self.usage, now.unix_secs_f64()));
        let args: Vec<String> = projects.iter().flat_map(|project| [precheck_word("options", "project"), project.clone()]).collect();
        let binary = regia_conformance::bin_dir().join(format!("precheck{}", std::env::consts::EXE_SUFFIX));
        let (launch, _) = fixture_for_rust(regia_exec::harness::launch(&binary, &args, &self.state), &self.responses, &self.state);
        let launch = launch.env(&env_var("CONFIG_DIR"), &self.settings).env(&prototype_env_var("HOME"), &self.home).env(&env_var("NOW"), now.to_iso_seconds());
        // The precheck reads the workers of the tasks in progress from the agents' app (#599): a program that is not
        // there, so that no case reads the machine's.
        let launch = launch.env(&regia_exec::Program::Orca.env_name(), self.state.join("no-agents-app"));
        let output = launch.run().expect("run the precheck");
        let run = PrecheckRun::from_output(output.status.code(), &String::from_utf8_lossy(&output.stdout));
        self.lines.lock().expect("lines").push(run.line.as_ref().map_or(Value::Null, Json::to_value));
        run
    }
}

struct MapFolders(BTreeMap<String, Option<String>>);

impl Folders for MapFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        Ok(self.0.clone())
    }
}

/// A `Dry` surface that also keeps each launch as the trace lists it: the project, the folder as the registry
/// names it, the agent's options, and the prompt.
struct TracedDry {
    dry: Dry,
    folders: BTreeMap<String, Option<String>>,
    launches: Mutex<Vec<Value>>,
}

impl LaunchSurface for TracedDry {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Dry
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        let project = request.label.as_str().to_owned();
        let options = json!({
            "agente": request.service.id(),
            "modello": request.model.as_ref().map(|model| model.as_str().to_owned()),
            "effort": request.effort.map(|effort| effort.as_str()),
        });
        let folder = self.folders.get(&project).cloned().flatten();
        self.launches.lock().expect("launches").push(json!({"project": project, "folder": folder, "options": options, "prompt": request.prompt}));
        self.dry.launch(request)
    }

    fn wake(&self) -> WakeOutcome {
        self.dry.wake()
    }
}

// ---------- a run ----------

struct Run {
    trace: Value,
    /// The strings of the machine a trace must not hold, by their placeholder.
    places: Vec<(String, &'static str)>,
}

fn slashed(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// The case on the Rust tick, its trace with the run's own values as the trace's placeholders.
fn run_case(case: &CaseFiles, alter: impl FnOnce(&mut Value)) -> Run {
    common::built(&BINARIES);
    let label = format!("controller-{}", case.name);
    let state = regia_conformance::prepare_state_dir(&label);
    for (name, content) in case.state["files"].as_object().expect("files") {
        fs::write(state.join(name), serde_json::to_string_pretty(content).expect("JSON")).expect("a state file");
    }
    let dossier = state.join("dossier");
    let posix = slashed(&dossier);
    let mut responses = placed(&case.responses, &posix);
    alter(&mut responses);
    let folders = registry_folders(&responses);
    for folder in folders.values().flatten() {
        fs::create_dir_all(folder).expect("a project's folder");
    }
    assert!(case.case["reports"].as_object().is_none_or(Map::is_empty), "{}: reports are not placed yet", case.name);
    let responses_path = state.join("responses.json");
    fs::write(&responses_path, serde_json::to_string_pretty(&fake_gh_responses(&responses)).expect("JSON")).expect("the answers");
    let (rows, cleanup) = cleaned_rows(&case.state, &posix);
    write_state(&state, &rows);
    let settings = state.join("settings");
    fs::create_dir_all(&settings).expect("the settings");
    fs::write(settings.join("agents.toml"), "probes = \"granted\"\n").expect("the consent");
    let home = state.join("home");
    let policy = home.join(regia_core::policy::file_path());
    fs::create_dir_all(policy.parent().expect("policies")).expect("policies");
    fs::copy(workspace_root().join("crates").join("regia-core").join("formats").join("budget.yaml"), &policy).expect("the policy");

    let start = WallTime::parse_iso(case.case["start"].as_str().expect("start")).expect("an instant");
    let clock = ManualClock::fixed(start);
    let store = Store::at(&state).expect("the store").with_clock(clock.clone());
    let reserved = ProjectId::new(controller_word(&["reserved_project"])).expect("the reserved project");
    let holder = Holder::new(HOLDER).expect("a holder");
    let after_cleanup = documents(&state).contains_key(&(store_word("kinds", "lock"), reserved.as_str().to_owned()));
    assert!(store.acquire_lock(&reserved, &holder, Ttl::from_secs(600).expect("ttl")).expect("the lock").is_ok(), "the lock is free after the cleanup");

    let precheck = Arc::new(CommandPrecheck {
        state: state.clone(),
        responses: responses_path,
        settings,
        home,
        usage: case.case["usage"].clone(),
        clock: clock.clone(),
        lines: Mutex::new(Vec::new()),
    });
    let surface = Arc::new(TracedDry { dry: Dry::new(), folders: folders.clone(), launches: Mutex::new(Vec::new()) });
    let ports = Ports {
        clock: Arc::new(clock.clone()),
        precheck: precheck.clone(),
        folders: Arc::new(MapFolders(folders)),
        liveness: Arc::new(Processes::new()),
        surface: surface.clone(),
        lock: Arc::new(StoreLock::new(store.clone(), holder.clone())),
        signals: Arc::new(StoreSignals::new(store.clone(), holder.clone())),
        steps: Arc::new({
            let steps = StoreSteps::new(store.clone(), holder.clone());
            steps.load().expect("the registry, read after the lock");
            steps
        }),
        ids: Arc::new(random_id),
    };
    let assignment = regia_agents::Policy::new(regia_agents::Settings::parse("").expect("settings")).expect("policy").assign(regia_agents::Role::Replanning, None);
    let options = Options::default().assigned(&assignment);
    let mut controller = Controller::new(options, ports, WakeUp::new(ProbeConsent::Granted));
    let (ticks, interval) = ticks_and_interval(case.case["args"].as_array().expect("args"));
    let host = Host { pid: std::process::id(), holder: HOLDER.to_owned(), started: start.unix_secs_f64(), interval_s: interval };
    let mut out = Vec::new();
    let mut held = false;
    for tick in 0..ticks {
        if tick > 0 {
            clock.advance(interval);
        }
        let before = documents(&state);
        let record = controller.tick(false).unwrap_or_else(|error| panic!("{}: tick {}: {error}", case.name, tick + 1));
        held = held || documents(&state).contains_key(&(store_word("kinds", "lock"), reserved.as_str().to_owned()));
        let (cursors, others) = changes(&before, &documents(&state));
        let line = precheck.lines.lock().expect("lines").get(tick).cloned().unwrap_or(Value::Null);
        let launches: Vec<Value> = surface.launches.lock().expect("launches").drain(..).collect();
        let mut entry = json!({"precheck": line, "log": record, "launches": launches, "cursors": cursors, "status": controller.status(&record, &host)});
        if !others.is_empty() {
            entry["documents"] = Value::Object(others);
        }
        out.push(entry);
    }
    store.release_lock(&reserved, &holder).expect("the release");
    let after_exit = documents(&state).contains_key(&(store_word("kinds", "lock"), reserved.as_str().to_owned()));
    let trace = json!({
        "exit": 0,
        "cleanup": cleanup,
        "controller_lock": {"after_cleanup": after_cleanup, "held_during_run": held, "after_exit": after_exit},
        "ticks": out,
    });
    let canonical = dossier.canonicalize().map(|path| path.display().to_string().trim_start_matches(r"\\?\").to_owned()).unwrap_or_default();
    let places = vec![
        (canonical.clone(), "{dossier}"),
        (slashed(Path::new(&canonical)), "{dossier}"),
        (dossier.display().to_string(), "{dossier}"),
        (posix, "{dossier}"),
        (state.display().to_string(), "{runtime}"),
        (slashed(&state), "{runtime}"),
    ];
    Run { trace, places }
}

// ---------- the normalizations ----------

fn replace_strings(value: &mut Value, places: &[(String, &str)], exact: &BTreeMap<String, String>) {
    match value {
        Value::String(text) => {
            if let Some(placeholder) = exact.get(text.as_str()) {
                *text = placeholder.clone();
                return;
            }
            for (place, placeholder) in places {
                if !place.is_empty() {
                    *text = text.replace(place.as_str(), placeholder);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| replace_strings(item, places, exact)),
        Value::Object(fields) => fields.values_mut().for_each(|item| replace_strings(item, places, exact)),
        _ => {}
    }
}

/// The ids of the postponements, by order of appearance: tick by tick, by cursor key.
fn postponement_ids(trace: &Value) -> BTreeMap<String, String> {
    let suffix = format!(":{}", controller_word(&["postpone", "cursor"]));
    let mut ids = BTreeMap::new();
    for tick in trace["ticks"].as_array().into_iter().flatten() {
        for (key, value) in tick["cursors"].as_object().into_iter().flatten() {
            if let (true, Some(id)) = (key.ends_with(&suffix), value["id"].as_str()) {
                let next = format!("{{rinvio-{}}}", ids.len() + 1);
                ids.entry(id.to_owned()).or_insert(next);
            }
        }
    }
    ids
}

/// The sentence of the precheck's error, each harness's own (the trace's README): as a placeholder on both.
fn error_detail(trace: &mut Value) {
    let format = toml_of(&["crates", "regia-controller", "formats", "precheck_v1.toml"]);
    let (payload, detail) = (format["line"]["keys"]["payload"].as_str().expect("payload").to_owned(), precheck_word("payload", "detail"));
    for tick in trace["ticks"].as_array_mut().into_iter().flatten() {
        if let Some(value) = tick.pointer_mut(&format!("/precheck/{payload}/{detail}")).filter(|value| value.is_string()) {
            *value = json!("{detail}");
        }
    }
}

/// The fake wake-up's sentence, each side's own: as a placeholder on both.
fn fake_wake_sentence(value: &mut Value) {
    let (fake, error) = (controller_word(&["wake", "fake"]), controller_word(&["wake", "error"]));
    match value {
        Value::Object(fields) => {
            if fields.get(&fake) == Some(&Value::Bool(true)) && fields.contains_key(&error) {
                fields.insert(error.clone(), json!("{fake-wake}"));
            }
            fields.values_mut().for_each(fake_wake_sentence);
        }
        Value::Array(items) => items.iter_mut().for_each(fake_wake_sentence),
        _ => {}
    }
}

/// The product's alarm of a failing reading (#466), which the prototype does not have: its keys in the payload's
/// budget and its count in the global detector's row, from the Rust side only.
fn without_the_product_s_alarm(trace: &mut Value) {
    let budget = (precheck_word("payload", "budget"), [precheck_word("budget", "rounds"), precheck_word("budget", "since"), precheck_word("budget", "alarm")]);
    let global = format!("{}:{}", precheck_word("detector", "global_project"), precheck_word("detector", "cursor"));
    for tick in trace["ticks"].as_array_mut().into_iter().flatten() {
        if let Some(fields) = tick.pointer_mut(&format!("/precheck/payload/{}", budget.0)).and_then(Value::as_object_mut) {
            for key in &budget.1 {
                fields.remove(key);
            }
        }
        if let Some(cursors) = tick["cursors"].as_object_mut() {
            cursors.remove(&global);
        }
    }
}

/// The dossier's warnings of each project (#743), which the traces were recorded without: from the precheck's line, and
/// from the payload a prompt attaches, where the key and its list are cut out of the text and nothing else changes.
fn without_the_dossier_s_warnings(trace: &mut Value) {
    let (key, projects) = (precheck_word("dossier", "key"), precheck_word("payload", "projects"));
    for tick in trace["ticks"].as_array_mut().into_iter().flatten() {
        for project in tick.pointer_mut(&format!("/precheck/payload/{projects}")).and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(fields) = project.as_object_mut() {
                fields.remove(&key);
            }
        }
        for launch in tick["launches"].as_array_mut().into_iter().flatten() {
            if let Some(prompt) = launch["prompt"].as_str() {
                launch["prompt"] = json!(without_key_in_text(prompt, &key));
            }
        }
    }
}

/// `text` without each `,"<key>":[…]` of the compact JSON it holds: the list ends at the bracket that closes it, outside
/// the strings in it.
fn without_key_in_text(text: &str, key: &str) -> String {
    let opening = format!(",\"{key}\":[");
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(&opening) {
        out.push_str(&rest[..at]);
        let after = &rest[at + opening.len()..];
        let (mut depth, mut quoted, mut escaped, mut end) = (1usize, false, false, after.len());
        for (index, c) in after.char_indices() {
            match (quoted, escaped, c) {
                (true, true, _) => escaped = false,
                (true, false, '\\') => escaped = true,
                (true, false, '"') | (false, _, '"') => quoted = !quoted,
                (false, _, '[') => depth += 1,
                (false, _, ']') => {
                    depth -= 1;
                    if depth == 0 {
                        end = index + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// The cut takes the key and its whole list, brackets and quotes inside its strings included, and leaves the rest as it is.
#[test]
fn the_dossier_s_warnings_are_cut_out_of_a_prompt_s_payload_alone() {
    let text = "before\n{\"progetti\":[{\"id\":\"x\",\"dossier\":[{\"problema\":\"b]\\\"[\"},{\"altri\":2}]},{\"id\":\"y\"}],\"politica\":\"file\"}\nafter";
    assert_eq!(without_key_in_text(text, "dossier"), "before\n{\"progetti\":[{\"id\":\"x\"},{\"id\":\"y\"}],\"politica\":\"file\"}\nafter");
    assert_eq!(without_key_in_text("no key here", "dossier"), "no key here");
}

/// The Rust trace with the placeholders of the prototype's.
fn normalized_rust(run: &Run) -> Value {
    let mut trace = run.trace.clone();
    let mut exact = postponement_ids(&trace);
    exact.insert(HOLDER.to_owned(), "{controller}".to_owned());
    let mut places = run.places.clone();
    let format = toml_of(&["crates", "regia-controller", "formats", "precheck_v1.toml"]);
    let t_key = format["line"]["keys"]["t"].as_str().expect("the line's t").to_owned();
    for tick in trace["ticks"].as_array().into_iter().flatten() {
        if let Some(t) = tick["precheck"][&t_key].as_str() {
            places.push((format!("precheck of {t}"), "precheck of {precheck-t}"));
            places.push((format!("Precheck outcome ({t})"), "Precheck outcome ({precheck-t})"));
        }
    }
    replace_strings(&mut trace, &places, &exact);
    // The precheck's `t` is the tick's own instant here, so only where the prototype's trace has it.
    let (record_precheck, record_t) = (controller_word(&["record", "precheck"]), controller_word(&["record", "precheck_keys", "t"]));
    for tick in trace["ticks"].as_array_mut().into_iter().flatten() {
        tick["status"][controller_word(&["status", "pid"])] = json!("{pid}");
        for at in [format!("/precheck/{t_key}"), format!("/log/{record_precheck}/{record_t}"), format!("/status/{record_precheck}/{record_t}")] {
            if let Some(value) = tick.pointer_mut(&at).filter(|value| value.is_string()) {
                *value = json!("{precheck-t}");
            }
        }
    }
    without_the_product_s_alarm(&mut trace);
    without_the_dossier_s_warnings(&mut trace);
    error_detail(&mut trace);
    fake_wake_sentence(&mut trace);
    digests(&mut trace, Side::Rust);
    trace
}

/// The recorded trace, with the parts the Rust side compares.
fn normalized_python(case: &CaseFiles) -> Value {
    let mut trace = json!({
        "exit": case.trace["exit"],
        "cleanup": case.trace["cleanup"],
        "controller_lock": case.trace["controller_lock"],
        "ticks": case.trace["ticks"],
    });
    error_detail(&mut trace);
    fake_wake_sentence(&mut trace);
    digests(&mut trace, Side::Python);
    trace
}

// ---------- the prompts ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Python,
    Rust,
}

/// The prototype's commands, by their words, with their English command and arguments: the facade's table.
fn command_map() -> Vec<(Vec<String>, String, BTreeMap<String, String>)> {
    let dir = workspace_root().join("crates").join("regia-facade").join("table");
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).expect("the table") {
        let path = entry.expect("a file").path();
        let table: toml::Table = toml::from_str(&fs::read_to_string(&path).expect("a table")).expect("TOML");
        for (english, command) in table.get("commands").and_then(toml::Value::as_table).into_iter().flatten() {
            let Some(words) = command.get("python").and_then(toml::Value::as_array) else { continue };
            let words: Vec<String> = words.iter().filter_map(toml::Value::as_str).map(str::to_owned).collect();
            let mut args = BTreeMap::new();
            for (name, value) in command.get("args").and_then(toml::Value::as_table).into_iter().flatten() {
                let python = value.as_str().or_else(|| value.get("to").and_then(toml::Value::as_str));
                if let Some(python) = python {
                    args.insert(python.to_owned(), name.clone());
                }
            }
            out.push((words, english.clone(), args));
        }
    }
    out.sort_by_key(|(words, _, _)| std::cmp::Reverse(words.len()));
    out
}

/// A command the prototype's prompt cites, in the contract's words; `<...>` stands for any value to fill.
fn mapped(span: &str, map: &[(Vec<String>, String, BTreeMap<String, String>)]) -> String {
    let tokens: Vec<&str> = span.split_whitespace().skip(1).collect();
    let Some((words, english, args)) = map.iter().find(|(words, _, _)| tokens.len() >= words.len() && words.iter().zip(&tokens).all(|(a, b)| a == b)) else {
        return format!("unmapped: {span}");
    };
    let rest: Vec<String> = tokens[words.len()..].iter().map(|token| args.get(*token).cloned().unwrap_or_else(|| (*token).to_owned())).collect();
    filled(&[BIN_NAME, english.as_str()].iter().copied().chain(rest.iter().map(String::as_str)).collect::<Vec<_>>().join(" "))
}

/// `<il tuo id>`, `<your id>`: a value the step fills.
fn filled(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else { break };
        out.push_str(&rest[..start]);
        out.push_str("<value>");
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let to = text[from..].find(end)? + from;
    Some(&text[from..to])
}

/// The quoted spans of a prompt, in order.
fn spans(text: &str) -> Vec<String> {
    text.split('`').skip(1).step_by(2).map(str::to_owned).collect()
}

/// The digits a prompt cites as exit codes.
fn exits(text: &str, word: &str) -> Vec<String> {
    text.match_indices(word).filter_map(|(at, _)| text[at + word.len()..].split(|c: char| !c.is_ascii_digit()).next()).filter(|digits| !digits.is_empty()).map(str::to_owned).collect()
}

/// What a prompt says, in parts both languages share.
fn digest(prompt: &str, side: Side, map: &[(Vec<String>, String, BTreeMap<String, String>)]) -> Value {
    let (stall, project_at, folder_at, exit_word) = match side {
        Side::Python => ("SEGNALAZIONE", "progetto \"", "\" (cartella ", "esce "),
        Side::Rust => ("REPORTS", "project \"", "\" (folder ", "exits "),
    };
    let stall = prompt.contains(stall);
    let (t, work) = match (side, stall) {
        (Side::Python, false) => (between(prompt, "precheck del ", ")"), between(prompt, "lavoro (", ", precheck")),
        (Side::Rust, false) => (between(prompt, "precheck of ", ")"), between(prompt, "work (", ", precheck")),
        (Side::Python, true) => (between(prompt, "Esito del precheck (", ")"), None),
        (Side::Rust, true) => (between(prompt, "Precheck outcome (", ")"), None),
    };
    let command = format!("{BIN_NAME} ");
    let (commands, others): (Vec<String>, Vec<String>) = spans(prompt).into_iter().partition(|span| span.starts_with(&command));
    let commands: Vec<String> = commands.iter().map(|span| if side == Side::Python { mapped(span, map) } else { filled(span) }).collect();
    json!({
        "kind": if stall { "stall" } else { "step" },
        "project": between(prompt, project_at, folder_at),
        "folder": between(prompt, folder_at, ")"),
        "precheck_t": t,
        "work": work,
        "payload": prompt.lines().find(|line| line.starts_with('{')),
        "commands": commands,
        "quoted": others,
        "exits": exits(prompt, exit_word),
    })
}

fn digests(trace: &mut Value, side: Side) {
    let map = command_map();
    for tick in trace["ticks"].as_array_mut().into_iter().flatten() {
        for launch in tick["launches"].as_array_mut().into_iter().flatten() {
            let prompt = launch["prompt"].as_str().unwrap_or_default().to_owned();
            launch["prompt"] = digest(&prompt, side, &map);
        }
    }
}

// ---------- the comparison ----------

fn differences(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for key in x.keys().chain(y.keys().filter(|key| !x.contains_key(*key))) {
                differences(x.get(key).unwrap_or(&Value::Null), y.get(key).unwrap_or(&Value::Null), &format!("{path}/{key}"), out);
                if x.contains_key(key) != y.contains_key(key) {
                    out.push(format!("{path}/{key}: only on the {} side", if x.contains_key(key) { "Rust" } else { "Python" }));
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (a, b)) in x.iter().zip(y).enumerate() {
                differences(a, b, &format!("{path}/{i}"), out);
            }
        }
        _ if a != b => out.push(format!("{path}: Rust {a} | Python {b}")),
        _ => {}
    }
}

fn compare(case: &CaseFiles, run: &Run) -> Vec<String> {
    let mut out = Vec::new();
    differences(&normalized_rust(run), &normalized_python(case), "", &mut out);
    out
}

#[test]
fn every_trace_of_the_prototype_comes_out_of_the_rust_tick_with_dry() {
    let mut failures = Vec::new();
    for case in cases() {
        let run = run_case(&case, |_| {});
        let text = serde_json::to_string(&normalized_rust(&run)).expect("JSON");
        for (place, _) in &run.places {
            assert!(place.is_empty() || !text.contains(place.as_str()), "{}: the trace still holds a path of the machine", case.name);
        }
        let found = compare(&case, &run);
        if !found.is_empty() {
            failures.push(format!("{}:\n  {}", case.name, found.iter().take(25).cloned().collect::<Vec<_>>().join("\n  ")));
        }
    }
    assert!(failures.is_empty(), "traces that differ:\n{}", failures.join("\n"));
}

/// The negative control: a changed answer of the tracker (#3 a Proposal, not approved) changes the trace.
#[test]
fn the_comparison_notices_a_changed_answer() {
    let case = cases().into_iter().find(|case| case.name == "work").expect("the work case");
    let approved = "opt-c3";
    let run = run_case(&case, |responses| {
        let text = responses.to_string().replace(&format!("\"optionId\":\"{approved}\""), "\"optionId\":\"opt-b2\"");
        *responses = serde_json::from_str(&text).expect("JSON");
    });
    assert!(!compare(&case, &run).is_empty(), "the comparison does not see a changed answer");
}

// ---------- the goldens of the prompts ----------

fn golden_dir() -> PathBuf {
    workspace_root().join("crates").join("regia-conformance").join("tests").join("golden").join("controller")
}

/// The three prompts on a fixed entry and payload, against their goldens: the step's, the step's with the backlog,
/// and the stall's. `RECORD_CONTROLLER_PROMPTS=1` writes them.
#[test]
fn the_prompts_are_their_goldens() {
    let entry = |outcome: &str, reason: &str| Json::Object(vec![("id".to_owned(), Json::Str("alpha".to_owned())), (precheck_word("project", "outcome"), Json::Str(outcome.to_owned())), (precheck_word("project", "reason"), Json::Str(reason.to_owned()))]);
    let payload: Json = serde_json::from_str("{\"progetti\":[{\"id\":\"alpha\",\"esito\":\"new-activity\",\"motivo\":\"pronti\",\"eventi\":1}],\"budget\":{\"claude_5h_pct\":20.0},\"politica\":\"file\"}").expect("a payload");
    let format = toml_of(&["crates", "regia-controller", "formats", "precheck_v1.toml"]);
    let word = |path: &str| path.split('.').fold(&toml::Value::Table(format.clone()), |value, part| &value[part]).as_str().expect("a word").to_owned();
    let cases = [
        ("step.en.txt", entry(&word("outcomes.new_activity.word"), &word("outcomes.new_activity.reasons.ready"))),
        ("backlog.en.txt", entry(&word("outcomes.new_activity.word"), &word("outcomes.new_activity.reasons.backlog"))),
        ("stall.en.txt", entry(&word("outcomes.terminal_state.word"), &word("outcomes.terminal_state.reasons.stall"))),
    ];
    let record = std::env::var_os(RECORD_GOLDEN).is_some();
    for (name, entry) in cases {
        let text = prompt("alpha", "/dossier/alpha", &entry, &payload, Some("2026-01-10T09:00:00Z"));
        let path = golden_dir().join(name);
        if record {
            fs::create_dir_all(golden_dir()).expect("the goldens");
            fs::write(&path, &text).expect("a golden");
            continue;
        }
        let golden = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display())).replace("\r\n", "\n");
        assert_eq!(text, golden, "{name} is not its golden");
    }
}

/// The prompts of the traces cite the same commands, through the facade's table, and the same exit codes as the
/// prototype's: a launch of each kind is in the traces, the stall's in `stall`.
#[test]
fn the_prompts_cite_the_prototype_s_commands_and_exits() {
    let map = command_map();
    let mut kinds = std::collections::BTreeSet::new();
    for case in cases() {
        for tick in case.trace["ticks"].as_array().into_iter().flatten() {
            for launch in tick["launches"].as_array().into_iter().flatten() {
                let python = digest(launch["prompt"].as_str().expect("a prompt"), Side::Python, &map);
                assert!(python["commands"].as_array().expect("commands").iter().all(|command| !command.as_str().unwrap_or_default().starts_with("unmapped")), "{}: {python}", case.name);
                kinds.insert(python["kind"].as_str().expect("a kind").to_owned());
            }
        }
    }
    assert_eq!(kinds, ["stall".to_owned(), "step".to_owned()].into_iter().collect());
    let step = digest(&fs::read_to_string(golden_dir().join("step.en.txt")).expect("the golden"), Side::Rust, &map);
    assert_eq!(step["exits"], json!(["3"]), "the step cites exit 3 of `step start`");
}
