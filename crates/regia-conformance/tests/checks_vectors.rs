//! Acceptance 2 of #448: the vectors of the checks give on the port what they give on the prototype. Each vector is
//! a call of a function of the prototype's `tools/controlli.py` with its arguments: those of `tests/checks/vectors.json`
//! are captured from the prototype's own unit tests (`oracle/checks_capture.py`), those of `tests/checks/unicode.json`
//! are written by hand where Python's rules for text differ from the `regex` crate's (`\w` without combining marks,
//! `\s` with U+001C to U+001F, `\b`, `lower()`, Unicode's digits, BOM, line breaks). `tests/checks/python.json` is
//! what the prototype answered, recorded by `oracle/checks_oracle.py`; here the port answers the same calls.
//!
//! The findings are compared in the contract's words: the prototype's pass through the facade table, as `check`
//! passes them, so the table's ids are checked too. The other answers are compared in the prototype's shapes.
//! A set compares as a set, a dict with keys that are not strings as its sorted pairs.
//!
//! The test that reruns the oracle, and so shows the golden is the prototype's of today, needs the dossier: it is
//! ignored, and runs with `REGIA_HOME` naming it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use regia_conformance::workspace_root;
use regia_core::checks::{
    self, AbsoluteInputs, CommentedIssue, Destination, IssueFacts, ProjectInputs, RemainingInputs, Thresholds, check_project,
};
use regia_core::{IssueState, RawComment, Status, Task};
use regia_exec::{Pin, Program};
use regia_facade::Table;
use serde_json::{Value, json};

fn dir() -> PathBuf {
    workspace_root().join("crates").join("regia-conformance").join("tests").join("checks")
}

fn read(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir().join(name)).unwrap()).unwrap()
}

/// `$runs` as their string, `$tuple` as a list; the other tags stay, for the typed readers below.
fn plain(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(plain).collect()),
        Value::Object(fields) if fields.len() == 1 && fields.contains_key("$runs") => {
            let runs = fields["$runs"].as_array().unwrap();
            let text: String = runs.iter().map(|run| run[0].as_str().unwrap().repeat(usize::try_from(run[1].as_u64().unwrap()).unwrap())).collect();
            Value::String(text)
        }
        Value::Object(fields) if fields.len() == 1 && fields.contains_key("$tuple") => plain(&fields["$tuple"]),
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, value)| (key.clone(), plain(value))).collect()),
        other => other.clone(),
    }
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn instant(value: &Value) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value["$datetime"].as_str().expect("a $datetime")).unwrap().with_timezone(&Utc)
}

fn numbers(value: &Value) -> BTreeSet<u64> {
    value.get("$set").or(Some(value)).and_then(Value::as_array).expect("a $set").iter().map(|n| n.as_u64().unwrap()).collect()
}

fn status(label: &Value) -> Option<Status> {
    let pairs = [
        ("Inbox", Status::Inbox),
        ("Proposta", Status::Proposal),
        ("Approvato", Status::Approved),
        ("In corso", Status::InProgress),
        ("Decisione", Status::Decision),
        ("Fatto", Status::Done),
        ("Scartato", Status::Discarded),
    ];
    pairs.iter().find(|(name, _)| label.as_str() == Some(name)).map(|(_, status)| *status)
}

fn state(value: &Value) -> IssueState {
    if value.as_str() == Some("OPEN") { IssueState::Open } else { IssueState::Closed }
}

/// The pairs of a dict with numbers for keys: `$dict` pairs, or an object (an empty dict is one).
fn numbered(value: &Value) -> Vec<(u64, Value)> {
    match value.get("$dict") {
        Some(pairs) => pairs.as_array().unwrap().iter().map(|pair| (pair[0].as_u64().unwrap(), pair[1].clone())).collect(),
        None => value.as_object().map(|fields| fields.iter().map(|(key, value)| (key.parse().unwrap(), value.clone())).collect()).unwrap_or_default(),
    }
}

fn open_issues(value: &Value) -> BTreeMap<u64, Option<Status>> {
    numbered(value).into_iter().map(|(n, label)| (n, status(&label))).collect()
}

/// `[number, state]` pairs, or `{"state": …}` with no number.
fn pairs(value: &Value) -> Vec<(u64, IssueState)> {
    value.as_array().map_or_else(Vec::new, |items| {
        items.iter().map(|item| if item.is_object() { (0, state(&item["state"])) } else { (item[0].as_u64().unwrap_or(0), state(&item[1])) }).collect()
    })
}

fn task(value: &Value) -> Task {
    let blocked_by = pairs(&value["bloccato_da"]);
    Task {
        number: value["number"].as_u64().unwrap(),
        title: text(&value["title"]).unwrap_or_default(),
        state: state(&value["state"]),
        state_reason: None,
        status: status(&value["stato"]),
        project: text(&value["progetto"]),
        project_field: text(&value["campo_progetto"]),
        maturity: None,
        objective: value["obiettivo"].as_bool().unwrap_or(false),
        labels: Vec::new(),
        parent: None,
        sub_issues: pairs(&value["sub"]),
        blocked_by_total: blocked_by.len() as u64,
        blocked_by,
        url: text(&value["url"]).unwrap_or_default(),
        updated_at: text(&value["updatedAt"]).unwrap_or_default(),
        created_at: text(&value["createdAt"]),
        closed_at: text(&value["closedAt"]),
        due: None,
    }
}

fn commented(value: &Value) -> Vec<CommentedIssue> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|issue| CommentedIssue {
            number: issue["number"].as_u64().unwrap(),
            state: if issue["state"].as_str() == Some("CLOSED") { IssueState::Closed } else { IssueState::Open },
            closed_at: text(&issue["closedAt"]),
            project: text(&issue["progetto"]),
            comments: issue["commenti"]
                .as_array()
                .map(|comments| {
                    comments
                        .iter()
                        .map(|c| RawComment {
                            id: c["id"].as_u64(),
                            created_at: text(&c["createdAt"]).unwrap_or_default(),
                            url: text(&c["url"]).unwrap_or_default(),
                            body: text(&c["body"]).unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

fn texts(value: &Value) -> Vec<(String, String)> {
    value.as_array().map_or_else(Vec::new, |items| items.iter().map(|item| (text(&item[0]).unwrap(), text(&item[1]).unwrap_or_default())).collect())
}

/// The decisions as `percorso_assoluto` takes them: the text of `decisioni.md`, a list of `(path, text or None)`, or none.
fn decisions(value: &Value) -> Option<Vec<(String, Option<String>)>> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(vec![(".regia/decisioni.md".to_owned(), Some(text.clone()))]),
        Value::Array(items) => Some(items.iter().map(|item| (text(&item[0]).unwrap(), text(&item[1]))).collect()),
        other => panic!("decisions: {other}"),
    }
}

fn facts(value: &Value) -> BTreeMap<u64, IssueFacts> {
    numbered(value)
        .into_iter()
        .map(|(n, issue)| {
            let facts = IssueFacts {
                state: if issue["state"].as_str() == Some("CLOSED") { IssueState::Closed } else { IssueState::Open },
                state_reason: text(&issue["stateReason"]),
                closed_at: text(&issue["closedAt"]),
                url: text(&issue["url"]),
            };
            (n, facts)
        })
        .collect()
}

fn thresholds(value: &Value) -> Thresholds {
    let get = |key: &str| value[key].as_u64().unwrap();
    Thresholds {
        inbox_days: get("inbox_giorni"),
        approved_days: get("approvato_giorni"),
        in_progress_days: get("in_corso_fermo_giorni"),
        comment_window_days: get("finestra_commenti_giorni"),
        brief_max: get("max_voci_brief"),
        decisions_cap: get("decisioni_tetto_caratteri"),
    }
}

fn findings(findings: &[checks::Finding]) -> Value {
    serde_json::to_value(findings).unwrap()
}

/// The port's answer to `call`, in the shape the golden compares with.
fn port(name: &str, args: &Value, file: &str, index: usize) -> Value {
    let s = |key: &str| text(&args[key]).unwrap_or_default();
    let n = |key: &str| args[key].as_u64().unwrap();
    match name {
        "segmenti" => json!(checks::segments(&s("riga"))),
        "riferimenti" => json!(checks::references(&s("testo"))),
        "tronca" => json!(checks::truncate(&s("testo"), usize::try_from(n("parole")).unwrap())),
        "numeri_con_report" => json!(checks::numbers_with_report(args["nomi"].as_array().unwrap().iter().map(|name| name.as_str().unwrap()))),
        "ruolo_del_commento" => json!(checks::comment_role(&s("corpo"))),
        "stato_senza_issue" => findings(&checks::state_without_issue(&s("testo"), &open_issues(&args["aperte"]), &s("progetto"), &s("percorso"), args["eredita"].as_bool().unwrap())),
        "stato_righe_tolte" => findings(&checks::state_lines_removed(
            &s("vecchio"),
            &s("nuovo"),
            &open_issues(&args["aperte"]),
            &s("progetto"),
            &s("percorso"),
            args["solo_aperte"].as_bool().unwrap(),
            args["eredita"].as_bool().unwrap(),
        )),
        "domande_aperte" => findings(&checks::open_questions(&s("testo"), &s("progetto"), &s("percorso"))),
        "commenti_dopo_chiusura" => findings(&checks::comments_after_close(&commented(&args["dati"]), &s("progetto"), instant(&args["adesso"]), n("giorni"))),
        "decisione_senza_flag" => findings(&checks::decision_without_flag(&commented(&args["dati"]), &s("progetto"), instant(&args["adesso"]), n("giorni"))),
        "inbox_ferme" | "approvati_mai_partiti" | "in_corso_fermi" | "progetto_campo" => {
            let issues: Vec<Task> = args["issues"].as_array().unwrap().iter().map(task).collect();
            let project = s("progetto");
            findings(&match name {
                "inbox_ferme" => checks::inbox_stale(&issues, &project, instant(&args["adesso"]), n("giorni")),
                "approvati_mai_partiti" => checks::approved_never_started(&issues, &project, instant(&args["adesso"]), n("giorni"), &numbers(&args["con_report"])),
                "in_corso_fermi" => checks::in_progress_stalled(&issues, &project, instant(&args["adesso"]), n("giorni"), &numbers(&args["lease_vive"])),
                _ => checks::project_field(&issues, &project),
            })
        }
        "leggi_report_resta" => checks::read_report(&s("nome"), &s("testo")).map_or(Value::Null, |report| {
            json!({
                "task": report.task, "data": report.date, "nome": report.name, "ha_campo": report.has_field,
                "voci": report.entries, "difetti": report.defects, "sul_campo": report.on_field, "nessuno": report.none,
            })
        }),
        "leggi_riconciliazioni" => {
            let read = checks::read_reconciliations(&texts(&args["file_decisioni"]));
            let rows: Vec<Value> = read
                .rows
                .iter()
                .map(|((task, n), rows)| json!([[task, n], rows.iter().map(|row| json!([row.date, row.file, row.line, row.destination, row.path])).collect::<Vec<_>>()]))
                .collect();
            json!([{"$dict": rows}, read.none])
        }
        "classifica_destino" => match checks::classify_destination(&s("dest")) {
            Destination::Issue(number) => json!(["issue", number]),
            Destination::Unanswered => json!(["interrogativo"]),
            Destination::Discarded => json!(["scartato"]),
            Destination::Done => json!(["fatto"]),
            Destination::Invalid => json!(["invalido"]),
        },
        "resta_non_riconciliato" => findings(&checks::remaining_unreconciled(
            &texts(&args["report"]),
            &texts(&args["file_decisioni"]),
            &facts(&args["issue"]),
            &s("progetto"),
            args["dal"].as_str(),
        )),
        "percorso_assoluto" => {
            let decisions = decisions(&args["decisioni"]);
            findings(&checks::absolute_paths(args["stato"].as_str(), &texts(&args["report"]), decisions.as_deref(), &s("progetto"), args["dal"].as_str()))
        }
        "decisioni_oltre_tetto" => findings(&checks::decisions_over_cap(args["testo"].as_str(), &s("progetto"), n("tetto"), &s("percorso"))),
        "carica_soglie" => {
            let (values, reason) = Thresholds::parse(&s("testo"), &format!("{}-{index}.yaml", file.trim_end_matches(".json")));
            let values: serde_json::Map<String, Value> = values.by_key().into_iter().map(|(key, value)| (key.to_owned(), json!(value))).collect();
            json!([values, reason])
        }
        "data_roma" => checks::rome_date(args["s"].as_str()).map_or(Value::Null, |date| json!({"$date": date.format("%Y-%m-%d").to_string()})),
        "controlla_progetto" => {
            let issues: Option<Vec<Task>> = args["issues"].as_array().map(|items| items.iter().map(task).collect());
            let comments = (!args["commenti"].is_null()).then(|| commented(&args["commenti"]));
            let with_report = (!args["con_report"].is_null()).then(|| numbers(&args["con_report"]));
            let leases = (!args["lease_vive"].is_null()).then(|| numbers(&args["lease_vive"]));
            let remaining = &args["resta"];
            let (remaining_reports, remaining_decisions) = (texts(&remaining["report"]), texts(&remaining["decisioni"]));
            let remaining_issues = (!remaining.is_null()).then(|| facts(&remaining["issue"]));
            let absolute = &args["assoluto"];
            let absolute_reports = texts(&absolute["report"]);
            let absolute_decisions = decisions(&absolute["decisioni"]);
            let cap_decisions = decisions(&args["decisioni"]);
            let inputs = ProjectInputs {
                project: args["progetto"].as_str().unwrap(),
                open: open_issues(&args["aperte"]),
                issues: issues.as_deref(),
                comments: comments.as_deref(),
                state: args["stato"].as_str(),
                previous_state: args["stato_precedente"].as_str(),
                questions: args["domande"].as_str(),
                with_report: with_report.as_ref(),
                live_leases: leases.as_ref(),
                only_open_removed: args["solo_aperte_tolte"].as_bool().unwrap(),
                project_field: args["campo_progetto"].as_bool().unwrap(),
                remaining: remaining_issues.as_ref().map(|issues| RemainingInputs { reports: &remaining_reports, decisions: &remaining_decisions, issues }),
                absolute: (!absolute.is_null()).then(|| AbsoluteInputs { reports: &absolute_reports, decisions: absolute_decisions.as_deref() }),
                decisions: cap_decisions.as_deref(),
            };
            findings(&check_project(&inputs, instant(&args["adesso"]), &thresholds(&args["soglie"])))
        }
        other => panic!("no port for {other}"),
    }
}

/// The prototype's answer in the shape the port's is compared in: findings through the facade table, sets and
/// dicts with keys that are not strings sorted, the other tags as they are.
fn expected(name: &str, result: &Value) -> Value {
    let finding_lists = [
        "stato_senza_issue",
        "stato_righe_tolte",
        "domande_aperte",
        "commenti_dopo_chiusura",
        "decisione_senza_flag",
        "inbox_ferme",
        "approvati_mai_partiti",
        "in_corso_fermi",
        "progetto_campo",
        "resta_non_riconciliato",
        "percorso_assoluto",
        "decisioni_oltre_tetto",
        "controlla_progetto",
    ];
    if finding_lists.contains(&name) {
        let report = json!({"voci": result, "per_controllo": {}, "avvisi": []});
        let translated = Table::embedded().translate_output("check", &report).unwrap_or_else(|error| panic!("{name}: the table: {error}"));
        return translated["findings"].clone();
    }
    sorted(&plain(result))
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(fields) if fields.len() == 1 && fields.contains_key("$set") => {
            let mut items: Vec<Value> = fields["$set"].as_array().unwrap().iter().map(sorted).collect();
            items.sort_by_key(Value::to_string);
            Value::Array(items)
        }
        Value::Object(fields) if fields.len() == 1 && fields.contains_key("$dict") => {
            let mut items: Vec<Value> = fields["$dict"].as_array().unwrap().iter().map(sorted).collect();
            items.sort_by_key(|pair| pair[0].to_string());
            json!({"$dict": items})
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, value)| (key.clone(), sorted(value))).collect()),
        other => other.clone(),
    }
}

/// The port's answer with its sets as sorted lists, like the golden's.
fn sorted_port(name: &str, value: Value) -> Value {
    match name {
        "riferimenti" | "numeri_con_report" => {
            let mut items: Vec<Value> = value.as_array().unwrap().clone();
            items.sort_by_key(Value::to_string);
            Value::Array(items)
        }
        "leggi_riconciliazioni" => {
            let mut none: Vec<Value> = value[1].as_array().unwrap().clone();
            none.sort_by_key(Value::to_string);
            json!([sorted(&value[0]), none])
        }
        _ => value,
    }
}

#[test]
fn every_vector_gives_on_the_port_what_it_gives_on_the_prototype() {
    let golden = read("python.json");
    let results = golden["results"].as_array().unwrap();
    let vectors: Vec<(String, usize, Value)> = ["vectors.json", "unicode.json"]
        .iter()
        .flat_map(|file| read(file).as_array().unwrap().iter().enumerate().map(|(i, vector)| ((*file).to_owned(), i, vector.clone())).collect::<Vec<_>>())
        .collect();
    assert_eq!(vectors.len(), results.len(), "the golden answers every vector: rerun the oracle");
    assert!(vectors.len() >= 300, "the vectors of the prototype's tests are there: {}", vectors.len());
    let mut differences = Vec::new();
    for ((file, index, vector), result) in vectors.iter().zip(results) {
        let name = vector["fn"].as_str().unwrap();
        assert_eq!((result["file"].as_str(), result["index"].as_u64(), result["fn"].as_str()), (Some(file.as_str()), Some(*index as u64), Some(name)));
        let args = plain(&vector["args"]);
        let port = sorted_port(name, port(name, &args, file, *index));
        let expected = expected(name, &result["result"]);
        if port != expected {
            differences.push(format!("{file}[{index}] {name}\n  port:      {port}\n  prototype: {expected}"));
        }
    }
    assert!(differences.is_empty(), "{} of {} vectors differ:\n{}", differences.len(), vectors.len(), differences.join("\n"));
}

/// Negative controls: an answer of the prototype altered by hand is a difference, for a set and for a finding, and
/// a vector that tells Python's `` from the `regex` crate's gives Python's answer.
#[test]
fn the_comparison_notices_a_difference() {
    let golden = read("python.json");
    let unicode = read("unicode.json");
    let answer = |index: u64| golden["results"].as_array().unwrap().iter().find(|r| r["file"] == "unicode.json" && r["index"] == index).unwrap()["result"].clone();
    let run = |name: &str, index: usize| sorted_port(name, port(name, &plain(&unicode[index]["args"]), "unicode.json", index));
    let references = run("riferimenti", 4);
    assert_eq!(references, expected("riferimenti", &answer(4)));
    assert!(references.as_array().unwrap().contains(&json!(13)), "a combining mark after #13 is no word character in Python");
    let mut altered = answer(4);
    altered["$set"].as_array_mut().unwrap().push(json!(99));
    assert_ne!(references, expected("riferimenti", &altered));
    let state = run("stato_senza_issue", 8);
    assert_eq!(state, expected("stato_senza_issue", &answer(8)));
    let mut altered = answer(8);
    altered[0]["perche"] = json!("altro");
    assert_ne!(state, expected("stato_senza_issue", &altered));
}

/// The golden is what the prototype of the dossier answers today: the oracle, rerun, writes the same file.
#[test]
#[ignore = "needs the dossier (REGIA_HOME) and Python"]
fn the_golden_is_the_prototype_s_answer() {
    let dossier = std::env::var_os("REGIA_HOME").map(PathBuf::from).expect("REGIA_HOME names the dossier");
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let out = regia_conformance::bin_dir().join("conformance-state").join("checks-oracle");
    std::fs::create_dir_all(&out).unwrap();
    let golden = out.join("python.json");
    let oracle = workspace_root().join("crates").join("regia-conformance").join("oracle").join("checks_oracle.py");
    let args: Vec<String> = ["-I".to_owned(), oracle.display().to_string(), "--dossier".to_owned(), dossier.display().to_string(), "--dir".to_owned(), out.display().to_string(), "--out".to_owned(), golden.display().to_string()].to_vec();
    let output = regia_exec::harness::program(&interpreter, &args, &out).run().expect("the oracle runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let fresh: Value = serde_json::from_str(&std::fs::read_to_string(&golden).unwrap()).unwrap();
    assert_eq!(fresh["results"], read("python.json")["results"], "the prototype answers otherwise now: rerun the oracle and look at what changed");
}
