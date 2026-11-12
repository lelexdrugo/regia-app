//! The step's proofs across the two languages (#449, Acceptance 2 and 3).
//!
//! **D3 of #417, as a black box.** Between taking the project's lock and writing the step, the lock passes
//! to another holder: `step start` must refuse with 3, leave the step's cursor as it was, and leave the lock
//! to the other holder. A trigger of SQLite makes the window certain on any implementation: the row the
//! acquire inserts is rewritten, in the transaction that inserts it, as another holder's lock, so the
//! acquire answers that the lock is the caller's while the stored lock is someone else's by the time the
//! step is written, which is the state D3 guards against, with no clock and no wait. The same test runs on
//! the product, everywhere, and on the prototype named by `REGIA_HOME`, locally:
//!
//!     REGIA_HOME=<dossier> cargo test -p regia-conformance --test step_cross -- --ignored
//!
//! On the prototype's `tools/` before #417 (its parent commit, `8898f7c`) the same test fails.
//!
//! **The events cursor the product writes is the one the precheck reads.** The product opens and closes a
//! step with two of three reports processed, whose dates have a part below the second; the prototype's
//! real `precheck.py` then counts only the third as new, a date a microsecond earlier makes a report new
//! again, and a cursor without the reports counts all three. Local, with `REGIA_HOME`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use regia_conformance::{Case, FIXTURE_NOW, fixed_now_driver, fixture_for_python, fixture_for_rust, prepare_state_dir, regia_binary, workspace_root};
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::prototype_env_var;
use regia_store::{CursorName, ProjectId, Store, WallTime};
use serde_json::{Value, json};

const OTHER: &str = "other-holder";

/// The run's output: exit code, stdout, stderr.
type Ran = (Option<i32>, String, String);

fn v1() -> toml::Table {
    let path = workspace_root().join("crates").join("regia-store").join("formats").join("store_v1.toml");
    toml::from_str(&fs::read_to_string(path).expect("the v1 state format")).expect("TOML")
}

fn name(section: &str, key: &str) -> String {
    v1()[section][key].as_str().unwrap_or_else(|| panic!("{section}.{key} in the v1 format")).to_owned()
}

fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to the prototype's dossier", prototype_env_var("HOME")));
    assert!(home.join("tools").join("regia.py").is_file(), "no tools/regia.py in {}", home.display());
    home
}

/// A state folder whose database turns the lock any implementation inserts into [`OTHER`]'s, in the
/// transaction that inserts it. With `other_step`, the other holder's step is open as well.
fn d3_state(label: &str, other_step: bool) -> PathBuf {
    let dir = prepare_state_dir(label);
    let format = v1();
    let column = |key: &str| format["table"][key].as_str().expect("a column of the v1 table").to_owned();
    let [table, kind, key, generation, document] = ["name", "kind", "key", "generation", "document"].map(column);
    let stolen = json!({
        name("lock", "project"): "alpha",
        name("lock", "holder"): OTHER,
        name("lock", "taken_at"): "2026-01-10T09:00:00.000+00:00",
        name("lock", "ttl_s"): 1800,
        name("lock", "expires_at"): "2099-01-01T00:00:00.000+00:00",
        name("lock", "replaced"): null,
        name("lock", "stolen_at"): null,
    });
    let conn = rusqlite::Connection::open(dir.join(regia_store::database_name())).expect("the state database");
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {table} ({kind} TEXT NOT NULL, {key} TEXT NOT NULL, {generation} INTEGER NOT NULL, \
         {document} TEXT NOT NULL, PRIMARY KEY ({kind}, {key}));
         CREATE TRIGGER d3_lock_taken_by_another AFTER INSERT ON {table} WHEN NEW.{kind} = '{lock}' BEGIN
           UPDATE {table} SET {document} = '{stolen}' WHERE {kind} = NEW.{kind} AND {key} = NEW.{key};
         END;",
        lock = name("kinds", "lock"),
    ))
    .expect("the trigger");
    if other_step {
        let step = json!({
            name("cursor", "value"): {
                name("step", "version"): 1,
                name("step", "holder"): OTHER,
                name("step", "session"): null,
                name("step", "t"): "2026-01-10T08:00:00Z",
                name("step", "comments"): [7],
            },
            name("cursor", "updated"): "2026-01-10T08:00:00.000+00:00",
        });
        conn.execute(
            &format!("INSERT INTO {table} ({kind}, {key}, {generation}, {document}) VALUES (?1, ?2, 1, ?3)"),
            (name("kinds", "cursor"), format!("alpha:{}", name("cursor", "step")), step.to_string()),
        )
        .expect("the other holder's step");
    }
    dir
}

/// The step cursor's row as stored, text and generation, if there is one.
fn step_row(dir: &Path) -> Option<(i64, String)> {
    let (kind, key) = (name("kinds", "cursor"), format!("alpha:{}", name("cursor", "step")));
    let rows = Store::at(dir).and_then(|store| store.rows()).expect("the rows");
    rows.into_iter().find(|row| row.kind == kind && row.key == key).map(|row| (row.generation, row.document))
}

fn lock_holder(dir: &Path) -> Option<String> {
    let lock = name("kinds", "lock");
    let rows = Store::at(dir).and_then(|store| store.rows()).expect("the rows");
    let row = rows.into_iter().find(|row| row.kind == lock && row.key == "alpha")?;
    let doc: Value = serde_json::from_str(&row.document).ok()?;
    doc[name("lock", "holder")].as_str().map(str::to_owned)
}

fn ran(output: regia_exec::Output) -> Ran {
    (output.status.code(), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
}

fn empty_responses(dir: &Path) -> PathBuf {
    let path = dir.join("no-responses.json");
    fs::write(&path, "{}").expect("empty responses");
    path
}

/// The product with no prototype, on the state folder `dir` and the tracker of `responses`.
fn product(dir: &Path, responses: &Path, args: &[&str]) -> Ran {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (launch, _) = fixture_for_rust(regia_exec::harness::launch(&regia_binary(), &args, dir), responses, dir);
    ran(launch.run().expect("run regia"))
}

/// The prototype's `tools/<script>` of `home`, as `record` starts it: at the fixtures' instant for `regia.py`.
fn prototype_run(home: &Path, dir: &Path, responses: &Path, script: &str, args: &[&str]) -> Ran {
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    let script = home.join("tools").join(script).display().to_string();
    if script.ends_with("regia.py") {
        argv.extend([fixed_now_driver().display().to_string(), script, FIXTURE_NOW.to_owned()]);
    } else {
        argv.push(script);
    }
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let launch = regia_exec::harness::program(&interpreter, &argv, home).limits(PROTOTYPE_LIMITS);
    ran(fixture_for_python(launch, responses, dir).run().expect("run the prototype"))
}

/// D3 on one implementation: `start` runs `step start` for `w1` on a state folder, `refused` reads the
/// refusal's state from its JSON.
fn assert_d3(label: &str, start: impl Fn(&Path) -> Ran, refused: impl Fn(&Value) -> Option<String>) {
    for other_step in [false, true] {
        let dir = d3_state(&format!("d3-{label}-{other_step}"), other_step);
        let before = step_row(&dir);
        let (exit, stdout, stderr) = start(&dir);
        assert_eq!(exit, Some(3), "{label}, other's step {other_step}: refused with 3\nstdout: {stdout}\nstderr: {stderr}");
        assert_eq!(step_row(&dir), before, "{label}, other's step {other_step}: the step's cursor is as it was");
        assert_eq!(lock_holder(&dir).as_deref(), Some(OTHER), "{label}: the lock stays the other holder's");
        let json: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
        assert_eq!(refused(&json).as_deref(), Some("foreign"), "{label}: refused as someone else's lock: {stdout}");
    }
}

#[test]
fn d3_on_the_product() {
    assert_d3(
        "product",
        |dir| product(dir, &empty_responses(dir), &["step", "start", "--project", "alpha", "--holder", "w1", "--json"]),
        |json| json["status"].as_str().map(str::to_owned),
    );
}

#[test]
#[ignore = "needs the prototype's dossier in REGIA_HOME"]
fn d3_on_the_prototype() {
    let home = prototype();
    assert_d3(
        "prototype",
        |dir| {
            let args = ["--json", "passo", "inizio", "--progetto", "alpha", "--proprietario", "w1"];
            prototype_run(&home, dir, &empty_responses(dir), "regia.py", &args)
        },
        |json| json["stato"].as_str().map(|state| if state == "altrui" { "foreign".to_owned() } else { state.to_owned() }),
    );
}

/// The precheck's count of new reports for alpha, on the state folder `dir`.
fn precheck_reports(home: &Path, dir: &Path, responses: &Path) -> i64 {
    let (exit, stdout, stderr) = prototype_run(home, dir, responses, "precheck.py", &["--progetto", "alpha"]);
    assert!(matches!(exit, Some(0 | 3)), "the precheck ended with {exit:?}\nstdout: {stdout}\nstderr: {stderr}");
    let last = stdout.lines().rev().find(|line| line.starts_with('{')).unwrap_or_else(|| panic!("no outcome line: {stdout}"));
    let outcome: Value = serde_json::from_str(last).expect("the outcome line is JSON");
    let alpha = outcome["payload"]["progetti"].as_array().and_then(|projects| projects.iter().find(|project| project["id"] == "alpha"));
    alpha.and_then(|alpha| alpha["report"].as_i64()).unwrap_or_else(|| panic!("no count of reports for alpha: {last}"))
}

#[test]
#[ignore = "needs the prototype's dossier in REGIA_HOME"]
fn the_precheck_reads_the_product_s_cursor_and_its_reports() {
    let home = prototype();
    let dir = prepare_state_dir("precheck-reads-the-product");
    let folder = dir.join("project-dossier");
    let reports = folder.join(".regia").join("report");
    fs::create_dir_all(&reports).expect("the reports' folder");
    // After the step's instant, each with a part below the second, which the precheck reads to the microsecond.
    let ten = UNIX_EPOCH + Duration::from_secs(1_768_039_200);
    for (report, nanos) in [("a.md", 123_456_700), ("b.md", 987_654_300), ("c.md", 500_000_100)] {
        let path = reports.join(report);
        fs::write(&path, "---\ntipo: done\n---\n").expect("a report");
        fs::File::options().write(true).open(&path).and_then(|file| file.set_modified(ten + Duration::from_nanos(nanos))).expect("its date");
    }
    // The registry of alpha's objective names this run's folder.
    let recorded = fs::read_to_string(Case::named("step-finish-registry").responses_path()).expect("the case's responses");
    let quoted = serde_json::to_string(&folder.display().to_string()).expect("JSON");
    let placed = recorded.replace("/no-such-folder/alpha", quoted.trim_matches('"'));
    assert_ne!(placed, recorded, "the registry names the run's folder");
    let responses = dir.join("responses.json");
    fs::write(&responses, placed).expect("the responses");

    let (exit, _, stderr) = product(&dir, &responses, &["step", "start", "--project", "alpha", "--holder", "w1", "--json"]);
    assert_eq!(exit, Some(0), "{stderr}");
    let finish = ["step", "finish", "--project", "alpha", "--holder", "w1", "--no-push", "--reports-processed", "a.md", "c.md", "--json"];
    let (exit, stdout, stderr) = product(&dir, &responses, &finish);
    assert_eq!((exit, stderr.as_str()), (Some(0), ""), "{stdout}");
    let closed: Value = serde_json::from_str(&stdout).expect("JSON");
    let dates = closed["cursor"]["own"]["reports"].as_object().expect("the processed reports, from the registry's folder").clone();
    assert_eq!(dates.keys().collect::<Vec<_>>(), ["a.md", "c.md"]);
    assert!(dates.values().filter_map(Value::as_str).all(|date| date.contains('.')), "each date has its microseconds: {dates:?}");

    assert_eq!(precheck_reports(&home, &dir, &responses), 1, "only b.md is new");

    // A date a microsecond before the file's makes the report new again; a cursor without them, all three.
    let store = Store::at(&dir).expect("the store");
    let (alpha, events) = (ProjectId::new("alpha").expect("an id"), CursorName::events());
    let mut cursor = store.cursor(&alpha, &events).expect("the cursor");
    let reports_key = name("events", "reports");
    let own = name("events", "own");
    let a = cursor[&own][&reports_key]["a.md"].as_str().and_then(WallTime::parse_iso).expect("a.md's date");
    let earlier = WallTime::from_unix_micros(a.unix_micros() - 1).expect("a date").to_python_iso();
    cursor[&own][&reports_key]["a.md"] = json!(earlier);
    store.write_cursor(&alpha, &events, cursor.clone()).expect("the altered cursor");
    assert_eq!(precheck_reports(&home, &dir, &responses), 2, "a.md, a microsecond after its recorded date, is new");
    cursor[&own].as_object_mut().expect("own").remove(&reports_key);
    store.write_cursor(&alpha, &events, cursor).expect("the cursor without reports");
    assert_eq!(precheck_reports(&home, &dir, &responses), 3, "without the reports, all three are new");
}

/// `test_passo_e_cursore_sono_scritti_prima_di_rilasciare_il_lock`, as a black box: a trigger copies the
/// cursors aside at the moment the lock's row is deleted. By then the step must be closed and the events
/// cursor written, or another `step start` could write a step the end would then empty.
fn assert_closed_before_release(label: &str, finish: impl Fn(&Path) -> Ran, start: impl Fn(&Path) -> Ran) {
    let dir = prepare_state_dir(&format!("release-order-{label}"));
    let (exit, stdout, stderr) = start(&dir);
    assert_eq!(exit, Some(0), "{label}: the step opens\nstdout: {stdout}\nstderr: {stderr}");
    let format = v1();
    let column = |key: &str| format["table"][key].as_str().expect("a column of the v1 table").to_owned();
    let [table, kind, key, _, document] = ["name", "kind", "key", "generation", "document"].map(column);
    let conn = rusqlite::Connection::open(dir.join(regia_store::database_name())).expect("the state database");
    conn.execute_batch(&format!(
        "CREATE TABLE witness ({kind} TEXT, {key} TEXT, {document} TEXT);
         CREATE TRIGGER witness_of_the_release AFTER DELETE ON {table} WHEN OLD.{kind} = '{lock}' BEGIN
           INSERT INTO witness SELECT {kind}, {key}, {document} FROM {table} WHERE {kind} = '{cursor}';
         END;",
        lock = name("kinds", "lock"),
        cursor = name("kinds", "cursor"),
    ))
    .expect("the witness");
    let (exit, stdout, stderr) = finish(&dir);
    assert_eq!(exit, Some(0), "{label}: the step closes\nstdout: {stdout}\nstderr: {stderr}");
    let seen: Vec<(String, Value)> = conn
        .prepare(&format!("SELECT {key}, {document} FROM witness ORDER BY {key}"))
        .and_then(|mut statement| {
            statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()
        })
        .expect("the witness's rows")
        .into_iter()
        .map(|(key, text)| (key, serde_json::from_str(&text).expect("JSON")))
        .collect();
    let value = name("cursor", "value");
    let step = format!("alpha:{}", name("cursor", "step"));
    assert_eq!(seen.len(), 2, "{label}: the cursors at the release: {seen:?}");
    assert_eq!(seen[0].0, "alpha");
    assert_eq!(seen[0].1[&value][name("events", "own")], json!({name("events", "comments"): []}), "{label}: the cursor is written");
    assert_eq!((seen[1].0.as_str(), &seen[1].1[&value]), (step.as_str(), &Value::Null), "{label}: the step is closed");
}

#[test]
fn the_step_closes_before_its_lock_is_released_on_the_product() {
    let args = |command: &'static str| move |dir: &Path| {
        let args = ["step", command, "--project", "alpha", "--holder", "w1", "--no-push", "--json"];
        let args = if command == "start" { &args[..6] } else { &args[..] };
        product(dir, &empty_responses(dir), args)
    };
    assert_closed_before_release("product", args("finish"), args("start"));
}

#[test]
#[ignore = "needs the prototype's dossier in REGIA_HOME"]
fn the_step_closes_before_its_lock_is_released_on_the_prototype() {
    let home = prototype();
    let run = |args: &'static [&'static str]| {
        let home = home.clone();
        move |dir: &Path| prototype_run(&home, dir, &empty_responses(dir), "regia.py", args)
    };
    assert_closed_before_release(
        "prototype",
        run(&["--json", "passo", "fine", "--progetto", "alpha", "--proprietario", "w1", "--no-push"]),
        run(&["--json", "passo", "inizio", "--progetto", "alpha", "--proprietario", "w1"]),
    );
}
