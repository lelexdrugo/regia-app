//! The moviola's `consolidate`, its raw store and its reader of reports (#491 of the dossier) against the prototype's
//! `consolida.py`, `store.py` and `report.py`. Each case of `fixtures/moviola-consolidate/oracle.json` is a list of
//! steps with what the prototype gave and every file of the case's folder after it, recorded by
//! `oracle/consolidate_oracle.py`, which the ignored test below runs again on a copy of the prototype:
//!
//! ```text
//! REGIA_HOME=<copy> cargo test -p regia-conformance --test moviola_consolidate -- --ignored
//! REGIA_HOME=<copy> RECORD_MOVIOLA_CONSOLIDATE=1 cargo test -p regia-conformance --test moviola_consolidate -- --ignored
//! ```
//!
//! Here each step runs on the product in a folder of the build's own, and must give what the prototype gave: the same
//! result in the prototype's shape, an error where the prototype raised, and the same bytes in every file, the
//! instants the store writes from its clock aside. The cases, and the prototype's test each one comes from, are listed
//! in `fixtures/moviola-consolidate/CASES.md`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use regia_conformance::workspace_root;
use regia_core::moviola::pyjson::{self, Json, Object};
use regia_core::moviola::{self, ActivityStore, Consolidated, Pruned, Report, Upserted};
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::prototype_env_var;
use serde_json::Value;

fn fixtures() -> PathBuf {
    workspace_root().join("fixtures").join("moviola-consolidate")
}

/// The recording, read with the keys in their order: the records of a step go to the store as written.
fn recording() -> Json {
    pyjson::loads(&fs::read_to_string(fixtures().join("oracle.json")).unwrap()).unwrap()
}

fn cases(recording: &Json) -> &[Json] {
    recording.as_object().unwrap().value("cases").as_list().unwrap()
}

fn text<'a>(step: &'a Object, key: &str) -> &'a str {
    step.value(key).as_str().unwrap_or_else(|| panic!("`{key}` of a step"))
}

fn instant(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc)
}

/// A value of the product as `serde_json` reads it, for a comparison that does not depend on the keys' order.
fn value(json: &Json) -> Value {
    serde_json::from_str(&pyjson::dumps(json)).unwrap()
}

fn object(pairs: Vec<(&str, Json)>) -> Json {
    Json::Object(pairs.into_iter().map(|(key, value)| (key.to_owned(), value)).collect())
}

fn int(value: impl TryInto<i128>) -> Json {
    Json::Int(value.try_into().unwrap_or_else(|_| panic!("a count past i128")))
}

fn ints(values: &[i128]) -> Json {
    Json::List(values.iter().map(|value| Json::Int(*value)).collect())
}

fn optional(value: Option<i128>) -> Json {
    value.map_or(Json::Null, Json::Int)
}

// The product's answers in the prototype's shape: its keys, which these tests name on purpose.

fn upserted(stats: Upserted) -> Json {
    object(vec![
        ("nuovi", int(stats.new)),
        ("aggiornati", int(stats.updated)),
        ("invariati", int(stats.unchanged)),
        ("saltati", int(stats.skipped)),
        ("rimossi", int(stats.removed)),
    ])
}

fn pruned(stats: Pruned) -> Json {
    object(vec![("eliminati", int(stats.removed)), ("trattenuti_non_consolidati", int(stats.kept_unconsolidated))])
}

/// A path as the oracle writes it: under the case's folder, `<root>` and `/`.
fn shown(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or_else(|_| panic!("{} is not under {}", path.display(), root.display()));
    format!("<root>/{}", relative.to_string_lossy().replace('\\', "/"))
}

fn consolidated(outcome: &Consolidated, root: &Path) -> Json {
    object(vec![
        ("progetto", Json::Str(outcome.project.clone())),
        ("aggregati", int(outcome.aggregates)),
        ("ricalcolati", ints(&outcome.recomputed)),
        ("rinviati_in_corso", ints(&outcome.deferred_in_progress)),
        ("solo_report", ints(&outcome.report_only)),
        ("senza_campi_di_processo", ints(&outcome.without_process_fields)),
        ("file", Json::Str(shown(&outcome.file, root))),
        ("potatura", pruned(outcome.pruning)),
    ])
}

fn report(report: &Report) -> Json {
    object(vec![
        ("file", Json::Str(report.file.clone())),
        ("data", report.date.clone().map_or(Json::Null, Json::Str)),
        ("task", Json::Int(report.task)),
        ("tipo", report.kind.clone()),
        ("stop_reason", report.stop_reason.clone()),
        ("round", optional(report.round)),
        ("sessione", report.session.clone()),
        ("sottoagenti", optional(report.subagents)),
        ("controlli", Json::List(report.checks.iter().cloned().map(Json::Object).collect())),
        ("ha_controlli", Json::Bool(report.has_checks)),
        ("ha_processo", Json::Bool(report.has_process)),
        ("difetti_chiusi", Json::List(report.closed_defects.iter().cloned().map(Json::Str).collect())),
    ])
}

fn records(step: &Object) -> Vec<Object> {
    step.value("records").as_list().unwrap().iter().map(|record| record.as_object().unwrap().clone()).collect()
}

/// The instant the oracle writes for the store's clock.
const STAMP: &str = "<ts>";

/// `"ts": "<instant>"` written `"ts": "<ts>"`, as the oracle does in `consolidati.json`.
fn without_stamps(text: &str) -> String {
    let marker = "\"ts\": \"";
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        let (before, after) = rest.split_at(at + marker.len());
        out.push_str(before);
        let instant = after.get(..20).filter(|candidate| candidate.ends_with('Z') && DateTime::parse_from_rfc3339(candidate).is_ok());
        match instant {
            Some(instant) => {
                out.push_str(STAMP);
                rest = &after[instant.len()..];
            }
            None => rest = after,
        }
    }
    out.push_str(rest);
    out
}

/// Every file under `root`, as text, by its path relative to it.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
                continue;
            }
            let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            let mut text = String::from_utf8(fs::read(&path).unwrap()).unwrap();
            if path.file_name().is_some_and(|file| file == "consolidati.json") {
                text = without_stamps(&text);
            }
            out.insert(name, text);
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// One step on the product: its result in the prototype's shape, or the product's error.
fn execute(step: &Object, root: &Path) -> Result<Json, String> {
    let project = root.join("project");
    let store = || ActivityStore::new(root.join(text(step, "store")));
    let now = || instant(text(step, "now"));
    let error = |error: moviola::MoviolaError| error.to_string();
    Ok(match text(step, "op") {
        "mkdir" => {
            fs::create_dir_all(root.join(text(step, "path"))).unwrap();
            Json::Null
        }
        op @ ("write" | "report") => {
            let path = if op == "write" { root.join(text(step, "path")) } else { project.join(".regia").join("report").join(text(step, "name")) };
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text(step, "text")).unwrap();
            Json::Null
        }
        "upsert" => {
            let remove: Vec<String> =
                step.get("remove").and_then(Json::as_list).unwrap_or_default().iter().map(|key| key.as_str().unwrap().to_owned()).collect();
            upserted(store().upsert(&records(step), &remove, now()).map_err(error)?)
        }
        "mark" => {
            let records = records(step);
            let task = match step.value("task") {
                Json::Int(task) => *task,
                other => panic!("a task {other:?}"),
            };
            // The prototype takes the instant from its clock, which the recording leaves out.
            store().mark_consolidated(text(step, "project"), task, &records.iter().collect::<Vec<_>>(), Utc::now()).map_err(error)?;
            Json::Null
        }
        "prune" => pruned(store().prune(now()).map_err(error)?),
        "consolidate" => consolidated(&moviola::consolidate(text(step, "project"), &project, &store(), now()).map_err(error)?, root),
        "read" => Json::List(
            store().read().map_err(error)?.iter().map(|(key, record)| Json::List(vec![Json::Str(key.to_owned()), Json::Object(record.clone())])).collect(),
        ),
        "consolidated" => {
            let mut out = Object::new();
            for (key, entry) in store().consolidated().map_err(error)?.iter() {
                let entry = match entry {
                    Json::Object(fields) if fields.contains_key("ts") => {
                        let mut fields = fields.clone();
                        fields.insert("ts", Json::Str(STAMP.to_owned()));
                        Json::Object(fields)
                    }
                    other => other.clone(),
                };
                out.insert(key, entry);
            }
            Json::Object(out)
        }
        "front_matter" => Json::Object(moviola::front_matter(text(step, "text"))),
        "reports" => Json::List(moviola::read_reports(&project).map_err(error)?.iter().map(report).collect()),
        "fingerprint" => Json::Str(moviola::fingerprint(&records(step).iter().collect::<Vec<_>>())?),
        other => panic!("a step the oracle does not know: {other}"),
    })
}

/// The folder of a case for the product, emptied.
fn case_folder(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("moviola-consolidate").join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// Runs the recorded case `name` on the product, step by step, and says where it first differs.
fn run(name: &str) {
    let recording = recording();
    let case = cases(&recording)
        .iter()
        .find(|case| case.as_object().unwrap().value("name").as_str() == Some(name))
        .unwrap_or_else(|| panic!("no case {name} in the recording"))
        .as_object()
        .unwrap();
    let root = case_folder(name);
    for (at, step) in case.value("steps").as_list().unwrap().iter().enumerate() {
        let step = step.as_object().unwrap();
        let expected = step.value("expected").as_object().unwrap();
        let what = format!("{name}, step {at} ({})", text(step, "op"));
        let outcome = execute(step, &root);
        match (expected.get("error"), &outcome) {
            (Some(raised), Ok(result)) => panic!("{what}: the prototype raised {raised:?}, the product gave {}", pyjson::dumps(result)),
            (None, Err(error)) => panic!("{what}: the product failed where the prototype did not: {error}"),
            (None, Ok(result)) => assert_eq!(value(result), value(expected.value("result")), "{what}: the result"),
            (Some(_), Err(_)) => {}
        }
        let files: BTreeMap<String, String> = expected
            .value("files")
            .as_object()
            .unwrap()
            .iter()
            .map(|(path, text)| (path.to_owned(), text.as_str().unwrap().to_owned()))
            .collect();
        let actual = snapshot(&root);
        let names = |files: &BTreeMap<String, String>| files.keys().cloned().collect::<Vec<_>>();
        assert_eq!(names(&actual), names(&files), "{what}: the files");
        for (path, text) in &files {
            assert_eq!(&actual[path], text, "{what}: the bytes of {path}");
        }
    }
}

macro_rules! cases {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                run($name);
            }
        )*

        /// Every case of the recording has its test here, and every test its case.
        #[test]
        fn every_recorded_case_has_a_test() {
            let recording = recording();
            let mut recorded: Vec<&str> = cases(&recording).iter().map(|case| case.as_object().unwrap().value("name").as_str().unwrap()).collect();
            let mut listed = vec![$($name),*];
            recorded.sort_unstable();
            listed.sort_unstable();
            assert_eq!(recorded, listed);
        }
    };
}

cases! {
    report_front_matter_complete => "report-front-matter-complete",
    report_closed_defects_in_a_block_and_no_fields => "report-closed-defects-in-a-block-and-no-fields",
    report_absent_is_not_zero => "report-absent-is-not-zero",
    report_stop_reason => "report-stop-reason",
    report_the_four_models_of_the_contract => "report-the-four-models-of-the-contract",
    store_upsert_is_idempotent => "store-upsert-is-idempotent",
    store_a_running_dispatch_is_replaced => "store-a-running-dispatch-is-replaced",
    store_an_absorbed_session_is_removed => "store-an-absorbed-session-is-removed",
    store_the_same_records_twice => "store-the-same-records-twice",
    store_retention_and_consolidation => "store-retention-and-consolidation",
    store_a_late_record_holds_the_pruning => "store-a-late-record-holds-the-pruning",
    consolidate_with_and_without_process_fields => "consolidate-with-and-without-process-fields",
    consolidate_a_task_with_reports_only => "consolidate-a-task-with-reports-only",
    consolidate_a_review_in_session_counts_what_another_task_closed => "consolidate-a-review-in-session-counts-what-another-task-closed",
    consolidate_reports_only_does_not_bypass_a_running_task => "consolidate-reports-only-does-not-bypass-a-running-task",
    consolidate_a_row_of_expired_records_stays => "consolidate-a-row-of-expired-records-stays",
    consolidate_closes_on_the_fixing_task => "consolidate-closes-on-the-fixing-task",
    consolidate_cache_tokens_only_where_they_are => "consolidate-cache-tokens-only-where-they-are",
    consolidate_round_counts_the_dispatches => "consolidate-round-counts-the-dispatches",
    consolidate_a_running_task_is_deferred => "consolidate-a-running-task-is-deferred",
    consolidate_again_and_expired_rows_are_kept => "consolidate-again-and-expired-rows-are-kept",
    consolidate_nothing_reserved => "consolidate-nothing-reserved",
    cited_the_orchestrator_s_chat_goes_to_the_tasks_that_name_it => "cited-the-orchestrator-s-chat-goes-to-the-tasks-that-name-it",
    cited_a_dispatch_of_the_task_and_a_named_session => "cited-a-dispatch-of-the-task-and-a-named-session",
    cited_a_dispatch_without_a_task_by_its_id => "cited-a-dispatch-without-a-task-by-its-id",
    cited_the_link_outlives_the_store_and_the_row_stays_open => "cited-the-link-outlives-the-store-and-the-row-stays-open",
    own_no_store_reports_only => "own-no-store-reports-only",
    own_no_dossier_folder => "own-no-dossier-folder",
    own_rows_as_written => "own-rows-as-written",
    own_a_row_from_reports_alone_keeps_its_cited_entries => "own-a-row-from-reports-alone-keeps-its-cited-entries",
    own_reports_the_edges_of_the_subset => "own-reports-the-edges-of-the-subset",
    own_a_check_that_is_a_number => "own-a-check-that-is-a-number",
    own_defects_that_are_a_number => "own-defects-that-are-a-number",
    own_store_text_keys_and_stretches => "own-store-text-keys-and-stretches",
    own_store_files_and_pruning => "own-store-files-and-pruning",
    own_a_record_without_its_end => "own-a-record-without-its-end",
}

/// Acceptance 2 of #491, as a property besides the recording: with no store, `consolidate` gives the report-only
/// rows, and the store's folder is still not there.
#[test]
fn without_a_store_nothing_fails_and_no_store_is_made() {
    let root = case_folder("no-store-property");
    let project = root.join("project");
    let reports = project.join(".regia").join("report");
    fs::create_dir_all(&reports).unwrap();
    fs::write(reports.join("2026-11-02-7-a.md"), "---\ntipo: done\ntask: 7\nround: 2\n---\n").unwrap();
    let store = ActivityStore::new(root.join("no-store"));
    let outcome = moviola::consolidate("alpha", &project, &store, instant("2026-11-26T00:00:00Z")).unwrap();
    assert_eq!((outcome.aggregates, outcome.report_only.clone(), outcome.recomputed.clone()), (1, vec![7], vec![]));
    assert_eq!(outcome.pruning, Pruned::default());
    assert!(!root.join("no-store").exists(), "the store's folder was made");
    let rows = fs::read_to_string(moviola::aggregates_file(&project)).unwrap();
    let row = pyjson::loads(rows.trim_end()).unwrap();
    let row = row.as_object().unwrap();
    assert_eq!(pyjson::dumps(row.value("modelli")), "[]");
    assert!(row.value("token").is_null());
}

/// The copy of the prototype in `REGIA_HOME`.
fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to a copy of the prototype's tools/", prototype_env_var("HOME")));
    assert!(home.join("tools").join("moviola").join("consolida.py").is_file(), "no tools/moviola/consolida.py in {}", home.display());
    home
}

#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_gives_the_recorded_answers() {
    let home = prototype();
    let script = workspace_root().join("crates").join("regia-conformance").join("oracle").join("consolidate_oracle.py");
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("moviola-consolidate-oracle");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    argv.extend([script.display().to_string(), work.display().to_string()]);
    let output = regia_exec::harness::program(&interpreter, &argv, &home)
        .limits(PROTOTYPE_LIMITS)
        .env(&prototype_env_var("HOME"), &home)
        .run()
        .expect("run the oracle");
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let recorded = fixtures().join("oracle.json");
    if std::env::var_os("RECORD_MOVIOLA_CONSOLIDATE").is_some() {
        fs::write(&recorded, &stdout).unwrap();
    }
    let answers: Value = serde_json::from_str(&stdout).unwrap();
    let kept: Value = serde_json::from_str(&fs::read_to_string(&recorded).unwrap()).unwrap();
    assert_eq!(answers, kept, "the prototype's answers changed: record them again");
}
