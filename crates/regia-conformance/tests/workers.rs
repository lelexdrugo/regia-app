//! `regia workers` across the two languages (#744; F277 of the dossier's inventory): the native command and the
//! prototype's `tools/worker_orca.py` of #698 give, on the same answers of the fake `orca`, the same facts for every
//! task asked: how many dispatches, their minutes, the wait for a person, the silence, a block and its case, a
//! session's cause, whether each reads. Every scenario runs in four forms, text and `--json`, without and with
//! `--dispatch`, and some with arguments of their own.
//!
//! - **In CI** the product's binary runs on every scenario and form, in Italian and, for the text, in English, and must
//!   print what the prototype printed, recorded in `tests/golden/workers-prototype.json`, through the names below. The
//!   calls the fake `orca` saw must be the prototype's, word for word: the same reads, in the same order, and only reads.
//! - **Locally**, with a copy of the prototype in `REGIA_HOME`, the prototype's own `main` runs on the same scenarios at
//!   the fixtures' instant, with its real call of Orca, which finds the fake first on `PATH` through `shutil.which` (the
//!   precheck's harness replaces `shutil.which` instead), and must print the recording again. With
//!   `RECORD_WORKERS_CASES=1` it writes the recording.
//!
//!       REGIA_HOME=<copy> cargo test -p regia-conformance --test workers -- --include-ignored
//!
//! The names, declared: the prototype's first, the product's provisional ones after (#223 decides them).
//! - **The command** `python tools/worker_orca.py` is `regia workers`; its options keep their names, `--task`, `--json`,
//!   `--dispatch`. The options are taken in full only (argparse takes `--disp` too), a task's number is a natural one (the
//!   prototype's `int` takes `-5`), and the help is clap's: none of these is in a run.
//! - **The keys:** `soglia_minuti` `silence_minutes`, `blocco_minuti` `block_minutes`, the list `task` `tasks`; of a
//!   task `in_corso` `in_progress`, `minuti` `minutes`, `attesa` `waiting`, `oltre_soglia` `silent`, `bloccato`
//!   `blocked`, `causa` `cause`, `dispatch` `dispatches`; of a dispatch also `leggibile` `readable`, `attivita`
//!   `activity`, `id_dispatch` `dispatch_id`, `id_run` `run_id`. `task`, `worker` and `n` stay. The product's keys come
//!   in its own order.
//! - **The values:** `worker` `si` `read`, `no` `none`, `illeggibile` `unreadable`; `bloccato` `chiamata-aperta`
//!   `open-call`, `attesa-persona` `waiting-for-person`; `causa` `limite-sessione` `session-limit`, `fatturazione`
//!   `billing`. Orca's activity passes as it is.
//! - **The text** in Italian is the prototype's, line for line, with a block's case in the contract's word
//!   (`BLOCCATO (open-call, oltre 180 min)`), as the ids of the checks in the line of `step finish` (#701). In English
//!   every line starts as the prototype's (`#N:`, `    Dispatch i/n:`) and ends with the same ids, or none.
//! - **stderr** is each side's own sentence: both empty, or both not; with exit 1 one line each, and the Italian one
//!   starts as the prototype's.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::{
    FIXTURE_NOW, fixture_for_python, fixture_for_rust, lf, path_with_fake_gh, place_fake,
    prepare_state_dir, regia_binary,
};
use regia_exec::{Pin, Program, python};
use regia_facade::PROTOTYPE_LIMITS;
use regia_identity::{env_var, prototype_env_var};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod common;
use common::Bin;

/// The product's binary, of another package: built (or found fresh) first.
const BINARIES: [Bin; 1] = [Bin {
    package: "regia-cli",
    name: regia_identity::BIN_NAME,
}];
const FAKE_ORCA: &str = env!("CARGO_BIN_EXE_fake-orca");
const RECORD_VARIABLE: &str = "RECORD_WORKERS_CASES";

/// The prototype's side, in the run's folder: its `main` at the run's instant, with its own call of Orca, whose search
/// finds the fake first on `PATH`, and the thresholds of `REGIA_HOME`, the run's policy home. It prints the exit code and
/// what the command printed; an argument the parser refuses is its `SystemExit`.
const PROTOTYPE_SIDE: &str = r#"import io
import json
import sys
from contextlib import redirect_stderr
from datetime import datetime
from pathlib import Path

sys.dont_write_bytecode = True  # no __pycache__ in the copy
copy, run = Path(sys.argv[1]), json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
sys.path.insert(0, str(copy / "tools"))
import worker_orca  # noqa: E402

now = datetime.fromisoformat(run["now"].replace("Z", "+00:00"))
out, err = io.StringIO(), io.StringIO()
try:
    with redirect_stderr(err):
        code = worker_orca.main(run["args"], adesso=now, out=out, err=err)
except SystemExit as stop:
    code = stop.code
print(json.dumps({"exit": code, "stdout": out.getvalue(), "stderr": err.getvalue()}))
"#;

// ---------- the scenarios ----------

/// A minute, in the seconds the lines are dated by.
const M: i64 = 60;
const LIMIT: &str = "You've hit your session limit · resets 3:30pm (UTC)";
const BILLING: &str =
    "Claude Code can't use your Claude subscription, usually because a payment is past due";
const PERMISSION: &str = "Allow Bash?\n{\n  \"command\": \"mkdir -p x\"";

/// Milliseconds since the epoch, `ago` seconds before the fixtures' instant, as Orca dates a transcript's lines.
fn ms(ago: i64) -> i64 {
    regia_store::WallTime::parse_iso(FIXTURE_NOW)
        .expect("the fixtures' instant")
        .unix_micros()
        / 1000
        - ago * 1000
}

fn text(ago: i64) -> Value {
    json!({"role": "assistant", "timestamp": ms(ago), "blocks": [{"type": "text", "text": "working"}]})
}

/// A line of the assistant with a call to a tool: open (`running`, no result), or not.
fn call(ago: i64, state: &str, result: bool) -> Value {
    let mut blocks = vec![
        json!({"type": "tool-call", "name": "Bash", "input": {}, "state": state, "callId": "call-1"}),
    ];
    if result {
        blocks.push(
            json!({"type": "tool-result", "output": "", "callId": "call-1", "isError": false}),
        );
    }
    json!({"role": "assistant", "timestamp": ms(ago), "blocks": blocks})
}

fn system(ago: i64, text: &str) -> Value {
    json!({"role": "system", "timestamp": ms(ago), "blocks": [{"type": "text", "text": text}]})
}

fn user(ago: i64) -> Value {
    json!({"role": "user", "timestamp": ms(ago), "blocks": [{"type": "text", "text": "status?"}]})
}

fn completed(mut line: Value, ago: i64) -> Value {
    line["completedAt"] = json!(ms(ago));
    line
}

/// A session's warning two seconds after a finished call, as the prototype's `con_avviso`.
fn after_a_call(ago: i64, warning: &str) -> Vec<Value> {
    vec![
        text(ago + M),
        call(ago, "completed", true),
        system(ago - 2, warning),
    ]
}

fn agent_wait() -> Value {
    json!({"dispatch": {"lastHeartbeatAt": null}, "observation": {"agentWait": {"evidence": "hook"}}})
}

/// A dispatch of Orca, compactly, as the prototype's tests write one (`record`, `OrcaFinta`): its record of the list,
/// its task's title, its transcript and what `worker-show --full` says. Ids are made up.
#[derive(Clone)]
struct D {
    id: &'static str,
    /// The number of its task's title, `<product> #N work`; `None` for a title of another kind.
    task: Option<u64>,
    /// Its task in Orca, `task_<id>` by default.
    orca_task: Option<&'static str>,
    run: &'static str,
    outcome: &'static str,
    /// `projection.stage.activity` in its record of the list.
    activity: Option<&'static str>,
    /// Its transcript's lines; `None` for Orca's terminal output only.
    lines: Option<Vec<Value>>,
    /// The code `worker-read` fails with.
    unreadable: Option<&'static str>,
    /// What `worker-show --full` answers, or the code it fails with.
    show: Value,
}

fn d(id: &'static str, task: u64, lines: Vec<Value>) -> D {
    let show = json!({"dispatch": {"lastHeartbeatAt": null}, "observation": {"status": "live"}});
    D {
        id,
        task: Some(task),
        orca_task: None,
        run: "run_1",
        outcome: "in_progress",
        activity: None,
        lines: Some(lines),
        unreadable: None,
        show,
    }
}

impl D {
    fn other_title(mut self) -> D {
        self.task = None;
        self
    }

    fn orca_task(mut self, task: &'static str) -> D {
        self.orca_task = Some(task);
        self
    }

    fn run(mut self, run: &'static str) -> D {
        self.run = run;
        self
    }

    fn outcome(mut self, outcome: &'static str) -> D {
        self.outcome = outcome;
        self
    }

    fn activity(mut self, activity: &'static str) -> D {
        self.activity = Some(activity);
        self
    }

    fn no_transcript(mut self) -> D {
        self.lines = None;
        self
    }

    fn unreadable(mut self, code: &'static str) -> D {
        self.unreadable = Some(code);
        self
    }

    fn show(mut self, show: Value) -> D {
        self.show = show;
        self
    }

    fn task_id(&self) -> String {
        self.orca_task
            .map_or_else(|| format!("task_{}", self.id), str::to_owned)
    }

    /// Its record of `worker-list --full`, in the form of the real ones.
    fn record(&self) -> Value {
        let mut projection = json!({
            "outcome": self.outcome,
            "liveness": {"verdict": "unverifiable", "reason": "missing_status"},
            "evidence": {"durable": true, "liveStatus": "unavailable", "lastObservedAt": null},
        });
        if let Some(activity) = self.activity {
            projection["stage"] = json!({"worker": "ready", "dispatch": "dispatched", "detail": "input_accepted", "activity": activity});
        }
        json!({"dispatchId": self.id, "taskId": self.task_id(), "runId": self.run, "terminalState": "retained", "projection": projection})
    }
}

/// What both sides read: the dispatches Orca shows, the run's policy, and how Orca answers.
struct Scenario {
    name: &'static str,
    /// The tests of the prototype it transcribes.
    #[allow(dead_code)]
    transcribes: &'static str,
    tasks: &'static [&'static str],
    dispatches: Vec<D>,
    /// The text of `policies/worker.yaml`; without it the file is not there.
    policy: Option<&'static str>,
    /// Dispatches on a page of `worker-list`; past them, a cursor.
    page: usize,
    /// The list is limited to one run, as inside a run.
    scoped: bool,
    /// Every call fails with this code.
    fail: Option<&'static str>,
    /// Runs besides the four forms.
    extra: &'static [&'static [&'static str]],
}

impl Scenario {
    fn new(
        name: &'static str,
        transcribes: &'static str,
        tasks: &'static [&'static str],
        dispatches: Vec<D>,
    ) -> Scenario {
        Scenario {
            name,
            transcribes,
            tasks,
            dispatches,
            policy: None,
            page: 100,
            scoped: false,
            fail: None,
            extra: &[],
        }
    }

    /// The four forms on `--task <tasks>`, then the scenario's own.
    fn argvs(&self) -> Vec<Vec<String>> {
        let base: Vec<String> = std::iter::once("--task")
            .chain(self.tasks.iter().copied())
            .map(str::to_owned)
            .collect();
        let with = |flags: &[&str]| {
            [
                base.clone(),
                flags.iter().map(|flag| (*flag).to_owned()).collect(),
            ]
            .concat()
        };
        let mut argvs = vec![
            base.clone(),
            with(&["--json"]),
            with(&["--dispatch"]),
            with(&["--dispatch", "--json"]),
        ];
        argvs.extend(
            self.extra
                .iter()
                .map(|argv| argv.iter().map(|word| (*word).to_owned()).collect()),
        );
        argvs
    }

    /// What the fake `orca` answers: the list in pages, the tasks of each run with their titles, each transcript in pages
    /// of fifty lines and then an empty one, and what `worker-show --full` says of each dispatch.
    fn orca(&self) -> Value {
        if let Some(code) = self.fail {
            return json!({"fail": code});
        }
        let scope = json!({"source": if self.scoped { "run" } else { "all" }});
        let records: Vec<Value> = self.dispatches.iter().map(D::record).collect();
        let chunks: Vec<&[Value]> = if records.is_empty() {
            vec![&[]]
        } else {
            records.chunks(self.page).collect()
        };
        let list: Vec<Value> = chunks
            .iter()
            .enumerate()
            .map(|(at, chunk)| {
                let more = at + 1 < chunks.len();
                let next = if more { json!(format!("page-{}", at + 1)) } else { Value::Null };
                json!({"scope": scope, "workers": chunk, "page": {"limit": 100, "hasMore": more, "nextCursor": next}})
            })
            .collect();
        let (mut runs, mut transcripts, mut shows) = (
            serde_json::Map::new(),
            serde_json::Map::new(),
            serde_json::Map::new(),
        );
        for dispatch in &self.dispatches {
            let title = dispatch.task.map_or_else(
                || "Step of an orchestrator".to_owned(),
                |n| format!("{} #{n} work", regia_identity::PRODUCT_NAME),
            );
            let tasks = runs
                .entry(dispatch.run)
                .or_insert_with(|| json!({"tasks": []}));
            let tasks = tasks["tasks"].as_array_mut().expect("a list of tasks");
            if !tasks
                .iter()
                .any(|task| task["id"] == dispatch.task_id().as_str())
            {
                tasks.push(
                    json!({"id": dispatch.task_id(), "title": title, "status": "dispatched"}),
                );
            }
            let pages = match (dispatch.unreadable, &dispatch.lines) {
                (Some(code), _) => json!(code),
                (None, None) => json!([{"source": "terminal", "output": "…"}]),
                (None, Some(lines)) => Value::Array(
                    lines
                        .chunks(50)
                        .enumerate()
                        .map(|(at, chunk)| json!({"transcript": {"messages": chunk, "nextCursor": format!("page-{}", at + 1)}, "cursor": format!("page-{}", at + 1)}))
                        .collect(),
                ),
            };
            transcripts.insert(dispatch.id.to_owned(), pages);
            shows.insert(dispatch.id.to_owned(), dispatch.show.clone());
        }
        json!({"worker_list": list, "task_lists": runs, "transcripts": transcripts, "shows": shows})
    }
}

fn scenarios() -> Vec<Scenario> {
    let other = |id: &'static str| d(id, 0, vec![text(0)]).other_title();
    vec![
        Scenario {
            extra: &[&["--task", "10", "--task", "99"], &["--task"], &["--task", "ten"]],
            ..Scenario::new(
                "healthy-and-none",
                "TestComando: test_json_con_gli_stessi_fatti, test_argomenti_sbagliati_exit_2; TestLeggi.test_solo_i_task_voluti_e_con_titolo_regia",
                &["10", "99", "10"],
                vec![d("ctx_a", 10, vec![text(2 * M)]), d("ctx_c", 12, vec![text(500 * M)]).outcome("succeeded"), other("ctx_x")],
            )
        },
        Scenario::new(
            "silence-and-waiting",
            "TestComando.test_task_senza_worker_in_attesa_e_oltre_la_soglia; TestLeggi: battito, transcript vuoto o assente, righe dell'utente, completedAt",
            &["10", "11", "13", "14", "15", "16", "17", "18", "99"],
            vec![
                d("ctx_a", 10, vec![text(95 * M)]),
                d("ctx_b", 11, vec![text(120 * M), text(90 * M)]).show(agent_wait()),
                d("ctx_d", 13, vec![text(5 * M)]),
                d("ctx_e", 14, vec![]).no_transcript().show(json!({"dispatch": {"lastHeartbeatAt": null, "dispatchedAt": "2026-01-10 07:00:00"}, "observation": {}})),
                d("ctx_f", 15, vec![]).show(json!({"dispatch": {"lastHeartbeatAt": null, "dispatchedAt": "2026-01-10T08:00:00Z"}, "observation": {}})),
                d("ctx_g", 16, vec![call(300 * M, "running", false)]).show(json!({"dispatch": {"lastHeartbeatAt": "2026-01-10T08:50:00+00:00"}, "observation": {}})),
                d("ctx_h", 17, vec![text(120 * M), user(M)]),
                d("ctx_i", 18, vec![completed(text(120 * M), 4 * M)]),
            ],
        ),
        Scenario::new(
            "two-dispatches-one-blocked",
            "TestPiuDispatchEAttivita (a) e (c), su scenario_698",
            &["10", "11"],
            vec![
                d("ctx_a", 10, vec![text(M)]).orca_task("task_a"),
                d("ctx_old", 10, vec![call(240 * M, "running", false)]).orca_task("task_a2").run("run_2"),
                d("ctx_b", 11, vec![text(3 * M)]),
                d("ctx_closed", 10, vec![text(900 * M)]).orca_task("task_a").outcome("succeeded"),
            ],
        ),
        Scenario::new(
            "activity",
            "TestPiuDispatchEAttivita (b): blocked e needs-input sotto e oltre il blocco, dal record di worker-show, altri valori",
            &["20", "21", "22", "23", "24", "25", "26", "27"],
            vec![
                d("ctx_20", 20, vec![text(240 * M)]).activity("blocked"),
                d("ctx_21", 21, vec![text(40 * M)]).activity("needs-input"),
                d("ctx_22", 22, vec![call(240 * M, "running", false)]).activity("Blocked"),
                d("ctx_23", 23, vec![text(240 * M)]).show(json!({"dispatch": {"lastHeartbeatAt": null}, "observation": {}, "projection": {"stage": {"activity": "blocked"}}})),
                d("ctx_24", 24, vec![text(240 * M)]).activity("working"),
                d("ctx_25", 25, vec![text(240 * M)]).activity("done").show(json!({"dispatch": {}, "observation": {"status": "live"}})),
                d("ctx_26", 26, vec![text(240 * M)]).activity("unknown"),
                d("ctx_27", 27, vec![text(5 * M)]).activity("blocked"),
            ],
        ),
        Scenario::new(
            "session-causes",
            "TestCausaDiSessioneDalVivo; la precedenza di #644: la causa con chiamata-aperta, attesa-persona prima della causa",
            &["30", "31", "32", "33", "34", "35", "36", "37", "38", "39"],
            vec![
                d("ctx_30", 30, after_a_call(300 * M, LIMIT)),
                d("ctx_31", 31, after_a_call(300 * M, BILLING)),
                d("ctx_32", 32, vec![call(300 * M, "running", false), system(300 * M, "Claude hit its session limit.")]),
                d("ctx_33", 33, vec![call(300 * M, "completed", true), system(300 * M, LIMIT), system(300 * M, PERMISSION)]),
                d("ctx_34", 34, vec![text(300 * M), system(300 * M - 1, LIMIT), system(300 * M - 2, BILLING)]),
                d("ctx_35", 35, after_a_call(10 * M, LIMIT)),
                d("ctx_36", 36, [after_a_call(300 * M, LIMIT), vec![text(200 * M)]].concat()),
                d("ctx_37", 37, after_a_call(300 * M, LIMIT)).activity("blocked"),
                d("ctx_38", 38, vec![call(300 * M, "running", false), system(300 * M, BILLING)]),
                d("ctx_39", 39, after_a_call(31 * M, LIMIT)),
            ],
        ),
        Scenario::new(
            "unreadable",
            "TestComando.test_worker_illeggibile_non_si_dice_senza_worker; TestPiuDispatchEAttivita.test_un_dispatch_illeggibile_resta_nell_elenco",
            &["40", "41", "42", "43", "44", "45"],
            vec![
                d("ctx_40", 40, vec![]).unreadable("source_changed"),
                d("ctx_41a", 41, vec![text(2 * M)]),
                d("ctx_41b", 41, vec![]).unreadable("source_changed"),
                d("ctx_42a", 42, vec![]).unreadable("source_changed"),
                d("ctx_42b", 42, vec![]).unreadable("source_changed"),
                d("ctx_43", 43, vec![]).unreadable("source_changed").activity("blocked"),
                d("ctx_44", 44, vec![]).no_transcript().show(json!({"dispatch": {"lastHeartbeatAt": null}, "observation": {}})),
                d("ctx_45", 45, vec![text(100 * M)]).show(json!("not_found")),
            ],
        ),
        Scenario {
            fail: Some("fake-down"),
            ..Scenario::new("orca-fails", "TestComando.test_orca_non_raggiungibile_exit_1_una_riga_senza_traceback", &["10"], vec![d("ctx_a", 10, vec![text(2 * M)])])
        },
        Scenario {
            scoped: true,
            ..Scenario::new("scoped-list", "TestLeggi.test_elenco_limitato_a_una_run_non_vale", &["10"], vec![d("ctx_a", 10, vec![text(2 * M)])])
        },
        Scenario {
            page: 2,
            ..Scenario::new(
                "pages-and-ties",
                "TestLeggi: test_una_pagina_piena_non_e_l_ultima, test_scorre_le_pagine_dell_elenco_col_cursore_di_page; TestSegnaleDiBlocco.test_la_chiamata_aperta_si_legge_anche_su_piu_pagine; il pari merito di max",
                &["50", "51", "52"],
                vec![
                    d("ctx_50", 50, (0..120).map(|i| text((540 - i) * M)).chain([call(300 * M, "running", false)]).collect()),
                    d("ctx_52a", 52, vec![text(200 * M)]),
                    d("ctx_52b", 52, vec![call(200 * M, "running", false)]),
                    d("ctx_51", 51, vec![text(3 * M)]),
                    other("ctx_x"),
                ],
            )
        },
        Scenario {
            policy: Some("# synthetic\nsilenzio_minuti: 45  # minutes\nblocco_minuti: 90\n"),
            ..Scenario::new(
                "thresholds-of-the-policy",
                "TestPolitica.test_le_due_soglie_dal_file; TestSegnaleDiBlocco.test_la_soglia_del_blocco_e_quella_data",
                &["60", "61", "62", "63", "64"],
                vec![
                    d("ctx_60", 60, vec![call(100 * M, "running", false)]),
                    d("ctx_61", 61, vec![text(60 * M)]),
                    d("ctx_62", 62, vec![text(95 * M)]),
                    d("ctx_63", 63, vec![call(90 * M, "running", false)]),
                    d("ctx_64", 64, vec![text(45 * M)]),
                ],
            )
        },
        Scenario::new(
            "boundaries",
            "TestSegnaleDiBlocco: test_il_confine_e_oltre_non_uguale, test_chiamata_fallita_senza_risultato_e_rimasta_a_meta, test_un_permesso_in_attesa_da_poco_non_cambia_il_silenzio, test_stato_running_ma_con_risultato_non_e_aperta",
            &["70", "71", "72", "73", "74", "75", "76", "77"],
            vec![
                d("ctx_70", 70, vec![call(180 * M, "running", false)]),
                d("ctx_71", 71, vec![call(181 * M, "running", false)]),
                d("ctx_72", 72, vec![text(30 * M)]),
                d("ctx_73", 73, vec![text(30 * M + 30)]),
                d("ctx_74", 74, vec![text(-5 * M)]),
                d("ctx_75", 75, vec![call(45 * M, "running", false), system(45 * M, PERMISSION)]),
                d("ctx_76", 76, vec![call(480 * M, "failed", false)]),
                d("ctx_77", 77, vec![call(480 * M, "running", true)]),
            ],
        ),
        Scenario {
            policy: Some("silenzio_minuti: 45\naltro: 3\n"),
            ..Scenario::new("policy-with-an-unknown-key", "TestPolitica.test_le_due_soglie_dal_file: una chiave sconosciuta", &["10"], vec![d("ctx_a", 10, vec![text(40 * M)])])
        },
    ]
}

// ---------- the runs ----------

/// A run's folder: the answers, the fake `orca` with its log, and the home of its policy.
struct Prepared {
    state: PathBuf,
    responses: PathBuf,
    orca: PathBuf,
    log: PathBuf,
    home: PathBuf,
}

impl Prepared {
    fn new(label: &str, scenario: &Scenario) -> Prepared {
        common::built(&BINARIES);
        let state = prepare_state_dir(label);
        let responses = state.join("orca-responses.json");
        fs::write(&responses, json!({"orca": scenario.orca()}).to_string()).expect("the answers");
        let dir = state.join("fake-orca");
        fs::create_dir_all(&dir).expect("the fake orca's folder");
        let orca = dir.join(format!("orca{}", std::env::consts::EXE_SUFFIX));
        place_fake(Path::new(FAKE_ORCA), &orca);
        let log = dir.join("calls.log");
        fs::write(
            dir.join("fixture.json"),
            json!({"responses": responses, "log": log}).to_string(),
        )
        .expect("the fake orca's fixture");
        let home = state.join("home");
        fs::create_dir_all(home.join("policies")).expect("the policies' folder");
        if let Some(text) = scenario.policy {
            fs::write(home.join("policies").join("worker.yaml"), text)
                .expect("the workers' policy");
        }
        Prepared {
            state,
            responses,
            orca,
            log,
            home,
        }
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// A run of either side, and the calls the fake `orca` saw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Ran {
    args: Vec<String>,
    exit: Option<i32>,
    stdout: String,
    stderr: String,
    calls: Vec<String>,
}

/// The product's `workers` on a run, in `language`: its `orca` is the fake, by its absolute path.
fn product(label: &str, scenario: &Scenario, args: &[String], language: &str) -> Ran {
    let run = Prepared::new(label, scenario);
    let argv = [vec!["workers".to_owned()], args.to_vec()].concat();
    let (launch, _) = fixture_for_rust(
        regia_exec::harness::launch(&regia_binary(), &argv, &run.state),
        &run.responses,
        &run.state,
    );
    let launch = launch
        .env(&Program::Orca.env_name(), &run.orca)
        .env(&prototype_env_var("HOME"), &run.home)
        .env(&env_var("LANG"), language);
    let output = launch.run().expect("run the product");
    let text = |bytes: &[u8]| lf(&String::from_utf8_lossy(bytes));
    Ran {
        args: args.to_vec(),
        exit: output.status.code(),
        stdout: text(&output.stdout),
        stderr: text(&output.stderr),
        calls: run.calls(),
    }
}

/// The copy of the prototype in `REGIA_HOME`.
fn prototype() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let home = home.unwrap_or_else(|| {
        panic!(
            "set {} to a copy of the prototype's tools/ and policies/",
            prototype_env_var("HOME")
        )
    });
    assert!(
        home.join("tools").join("worker_orca.py").is_file(),
        "no tools/worker_orca.py in {}",
        home.display()
    );
    home
}

/// The prototype's `main` on a run: the fake first on `PATH`, the run's policy home in `REGIA_HOME`. The fake answered,
/// and not the machine's Orca, when its log starts with the list.
fn prototype_side(copy: &Path, label: &str, scenario: &Scenario, args: &[String]) -> Ran {
    let run = Prepared::new(label, scenario);
    let script = run.state.join("prototype_side.py");
    fs::write(&script, PROTOTYPE_SIDE).expect("the prototype's side");
    let input = run.state.join("run.json");
    fs::write(
        &input,
        json!({"now": FIXTURE_NOW, "args": args}).to_string(),
    )
    .expect("the run");
    let interpreter = Pin::resolve(Program::Python).expect("a Python interpreter");
    let mut argv: Vec<String> = python::FLAGS.map(str::to_owned).to_vec();
    argv.extend([
        script.display().to_string(),
        copy.display().to_string(),
        input.display().to_string(),
    ]);
    let mut path = vec![run.orca.parent().expect("the fake's folder").to_path_buf()];
    path.extend(std::env::split_paths(&path_with_fake_gh()));
    let launch = fixture_for_python(
        regia_exec::harness::program(&interpreter, &argv, copy).limits(PROTOTYPE_LIMITS),
        &run.responses,
        &run.state,
    )
    .env("PATH", std::env::join_paths(path).expect("PATH"))
    .env(&prototype_env_var("HOME"), &run.home);
    let output = launch.run().expect("run the prototype");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "the prototype's side failed\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|error| panic!("{error}: {stdout}"));
    let exit = result["exit"]
        .as_i64()
        .and_then(|exit| i32::try_from(exit).ok());
    let calls = run.calls();
    if exit != Some(2) {
        assert!(
            calls
                .first()
                .is_some_and(|call| call.starts_with("orchestration worker-list")),
            "{}: the fake orca did not answer: {calls:?}",
            scenario.name
        );
    }
    let field = |key: &str| lf(result[key].as_str().unwrap_or_default());
    Ran {
        args: args.to_vec(),
        exit,
        stdout: field("stdout"),
        stderr: field("stderr"),
        calls,
    }
}

fn recording_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("workers-prototype.json")
}

fn recording() -> BTreeMap<String, Vec<Ran>> {
    regia_conformance::read_json(&recording_path())
}

// ---------- the comparison ----------

/// The keys of a task's entry and of a dispatch's, the prototype's with the product's.
const ENTRY: [(&str, &str); 9] = [
    ("task", "task"),
    ("worker", "worker"),
    ("in_corso", "in_progress"),
    ("minuti", "minutes"),
    ("attesa", "waiting"),
    ("oltre_soglia", "silent"),
    ("bloccato", "blocked"),
    ("causa", "cause"),
    ("dispatch", "dispatches"),
];
const DISPATCH: [(&str, &str); 10] = [
    ("n", "n"),
    ("leggibile", "readable"),
    ("minuti", "minutes"),
    ("attesa", "waiting"),
    ("attivita", "activity"),
    ("oltre_soglia", "silent"),
    ("bloccato", "blocked"),
    ("causa", "cause"),
    ("id_dispatch", "dispatch_id"),
    ("id_run", "run_id"),
];
/// The prototype's values, by the product's key, with the product's.
const VALUES: [(&str, &str, &str); 7] = [
    ("worker", "si", "read"),
    ("worker", "no", "none"),
    ("worker", "illeggibile", "unreadable"),
    ("blocked", "chiamata-aperta", "open-call"),
    ("blocked", "attesa-persona", "waiting-for-person"),
    ("cause", "limite-sessione", "session-limit"),
    ("cause", "fatturazione", "billing"),
];

/// An object of the prototype in the product's names; a key or a value the names do not know fails.
fn renamed(
    python: &Value,
    keys: &[(&str, &str)],
) -> Result<serde_json::Map<String, Value>, String> {
    let mut out = serde_json::Map::new();
    for (key, value) in python
        .as_object()
        .ok_or_else(|| format!("not an object: {python}"))?
    {
        let (_, name) = keys
            .iter()
            .find(|(from, _)| from == key)
            .ok_or_else(|| format!("a key the names do not know: {key}"))?;
        let value = match value.as_str() {
            Some(text) if VALUES.iter().any(|(at, _, _)| at == name) => {
                let found = VALUES
                    .iter()
                    .find(|(at, from, _)| at == name && *from == text);
                json!(
                    found
                        .ok_or_else(|| format!("a value the names do not know: {key} {text}"))?
                        .2
                )
            }
            _ => value.clone(),
        };
        out.insert((*name).to_owned(), value);
    }
    Ok(out)
}

/// The prototype's `--json` in the product's names.
fn product_json(python: &Value) -> Result<Value, String> {
    let mut top = renamed(
        python,
        &[
            ("soglia_minuti", "silence_minutes"),
            ("blocco_minuti", "block_minutes"),
            ("task", "tasks"),
        ],
    )?;
    let mut tasks = Vec::new();
    for task in top
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let mut entry = renamed(&task, &ENTRY)?;
        let dispatches = entry
            .get("dispatches")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let dispatches: Result<Vec<Value>, String> = dispatches
            .iter()
            .map(|dispatch| renamed(dispatch, &DISPATCH).map(Value::Object))
            .collect();
        entry.insert("dispatches".to_owned(), Value::Array(dispatches?));
        tasks.push(Value::Object(entry));
    }
    top.insert("tasks".to_owned(), Value::Array(tasks));
    Ok(Value::Object(top))
}

/// The prototype's text in the product's Italian: a block's case in the contract's word.
fn product_text(python: &str) -> String {
    python
        .replace("BLOCCATO (chiamata-aperta, ", "BLOCCATO (open-call, ")
        .replace(
            "BLOCCATO (attesa-persona, ",
            "BLOCCATO (waiting-for-person, ",
        )
}

/// Every way the product's runs, in Italian and in English, differ from the prototype's.
fn differences(python: &Ran, italian: &Ran, english: Option<&Ran>) -> Vec<String> {
    let mut out = Vec::new();
    let (json, ids) = (
        python.args.iter().any(|arg| arg == "--json"),
        python.args.iter().any(|arg| arg == "--dispatch"),
    );
    for (language, ran) in [("it", Some(italian)), ("en", english)] {
        let Some(ran) = ran else { continue };
        if ran.exit != python.exit {
            out.push(format!(
                "{language} exit: rust {:?}, python {:?}; stderr: {}",
                ran.exit, python.exit, ran.stderr
            ));
        }
        if ran.calls != python.calls {
            out.push(format!(
                "{language} calls:\n  rust   {:?}\n  python {:?}",
                ran.calls, python.calls
            ));
        }
        if !ids {
            for marker in ["ctx_", "run_", "task_", "dispatch="] {
                if ran.stdout.contains(marker) || ran.stderr.contains(marker) {
                    out.push(format!(
                        "{language}: an id of Orca without --dispatch ({marker})"
                    ));
                }
            }
        }
        if python.exit == Some(1) {
            if ran.stderr.lines().count() != 1 || python.stderr.lines().count() != 1 {
                out.push(format!(
                    "{language} stderr, one line each:\n  rust   {:?}\n  python {:?}",
                    ran.stderr, python.stderr
                ));
            }
            if language == "it" && !ran.stderr.starts_with("errore: Orca non raggiungibile (") {
                out.push(format!("it stderr: {}", ran.stderr));
            }
        } else if ran.stderr.is_empty() != python.stderr.is_empty() {
            out.push(format!(
                "{language} stderr:\n  rust   {:?}\n  python {:?}",
                ran.stderr, python.stderr
            ));
        }
        if python.exit != Some(0) {
            if !ran.stdout.is_empty() {
                out.push(format!("{language} stdout: {}", ran.stdout));
            }
            continue;
        }
        if json {
            let rust =
                serde_json::from_str::<Value>(&ran.stdout).map_err(|error| error.to_string());
            let prototype = serde_json::from_str::<Value>(&python.stdout)
                .map_err(|error| error.to_string())
                .and_then(|value| product_json(&value));
            if rust.is_err() || rust != prototype {
                out.push(format!(
                    "{language} json:\n  rust   {rust:?}\n  python {prototype:?}"
                ));
            }
        } else if language == "it" {
            let prototype = product_text(&python.stdout);
            if ran.stdout != prototype {
                out.push(format!(
                    "it text:\n  rust\n{}  python\n{prototype}",
                    ran.stdout
                ));
            }
        } else {
            let (rust, prototype): (Vec<&str>, Vec<&str>) = (
                ran.stdout.lines().collect(),
                python.stdout.lines().collect(),
            );
            let shape = |line: &str| {
                (
                    line.split(": ").next().unwrap_or_default().to_owned(),
                    line.split_once("  [").map(|(_, ids)| ids.to_owned()),
                )
            };
            let same_shape = rust.len() == prototype.len()
                && rust
                    .iter()
                    .zip(&prototype)
                    .all(|(rust, prototype)| shape(rust) == shape(prototype));
            if !same_shape || ran.stdout == python.stdout {
                out.push(format!(
                    "en text, the prototype's lines in English:\n  rust\n{}  python\n{}",
                    ran.stdout, python.stdout
                ));
            }
        }
    }
    out
}

// ---------- the tests ----------

/// Acceptance 1 of #744: on every scenario and form the product prints what the prototype printed, through the names,
/// and asks Orca what the prototype asked.
#[test]
fn the_product_prints_what_the_prototype_printed() {
    let recorded = recording();
    let scenarios = scenarios();
    let mut names: Vec<&str> = scenarios.iter().map(|scenario| scenario.name).collect();
    names.sort_unstable();
    assert_eq!(
        recorded.keys().map(String::as_str).collect::<Vec<_>>(),
        names,
        "the recording is of these scenarios: record it again"
    );
    let failures: Vec<String> = std::thread::scope(|scope| {
        let threads: Vec<_> = scenarios
            .iter()
            .map(|scenario| {
                let runs = &recorded[scenario.name];
                scope.spawn(move || {
                    let argvs = scenario.argvs();
                    assert_eq!(
                        runs.iter().map(|run| run.args.clone()).collect::<Vec<_>>(),
                        argvs,
                        "{}: the recording's runs are the scenario's",
                        scenario.name
                    );
                    let mut failures = Vec::new();
                    for (index, (args, python)) in argvs.iter().zip(runs).enumerate() {
                        let italian = product(
                            &format!("workers-{}-{index}-it", scenario.name),
                            scenario,
                            args,
                            "it",
                        );
                        let english = (!args.iter().any(|arg| arg == "--json")).then(|| {
                            product(
                                &format!("workers-{}-{index}-en", scenario.name),
                                scenario,
                                args,
                                "en",
                            )
                        });
                        let found = differences(python, &italian, english.as_ref());
                        if !found.is_empty() {
                            failures.push(format!(
                                "{} {args:?}:\n{}",
                                scenario.name,
                                found.join("\n")
                            ));
                        }
                    }
                    failures
                })
            })
            .collect();
        threads
            .into_iter()
            .flat_map(|thread| thread.join().expect("a scenario's runs"))
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The prototype prints the recording on every scenario and form, its calls of Orca included; with the variable set,
/// what it prints becomes the recording.
#[test]
#[ignore = "needs a copy of the prototype in REGIA_HOME"]
fn the_prototype_prints_the_recording() {
    let copy = prototype();
    let record = std::env::var_os(RECORD_VARIABLE).is_some_and(|value| value == "1");
    let recorded = if record { BTreeMap::new() } else { recording() };
    let mut fresh = BTreeMap::new();
    let mut failures = Vec::new();
    for scenario in scenarios() {
        let runs: Vec<Ran> = scenario
            .argvs()
            .iter()
            .enumerate()
            .map(|(index, args)| {
                prototype_side(
                    &copy,
                    &format!("workers-{}-{index}-python", scenario.name),
                    &scenario,
                    args,
                )
            })
            .collect();
        if !record && recorded.get(scenario.name) != Some(&runs) {
            failures.push(format!(
                "{}: the prototype printed\n{runs:#?}\nrecorded\n{:#?}",
                scenario.name,
                recorded.get(scenario.name)
            ));
        }
        fresh.insert(scenario.name.to_owned(), runs);
    }
    if record {
        fs::write(
            recording_path(),
            serde_json::to_string_pretty(&fresh).expect("JSON") + "\n",
        )
        .expect("the recording");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The prototype's `test_orca_assente_dal_path_exit_1`: with no `orca` where it is named, one line on stderr and exit 1.
#[test]
fn without_orca_the_command_exits_1_with_one_line() {
    common::built(&BINARIES);
    let state = prepare_state_dir("workers-no-orca");
    let argv = ["workers", "--task", "10"].map(str::to_owned);
    let (launch, _) = fixture_for_rust(
        regia_exec::harness::launch(&regia_binary(), &argv, &state),
        &state.join("no-responses.json"),
        &state,
    );
    let missing = state
        .join("no-orca")
        .join(format!("orca{}", std::env::consts::EXE_SUFFIX));
    let output = launch
        .env(&Program::Orca.env_name(), &missing)
        .env(&env_var("LANG"), "it")
        .run()
        .expect("run the product");
    let stderr = lf(&String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        (output.status.code(), output.stdout.is_empty()),
        (Some(1), true),
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.starts_with("errore: Orca non raggiungibile ("),
        "{stderr}"
    );
}
