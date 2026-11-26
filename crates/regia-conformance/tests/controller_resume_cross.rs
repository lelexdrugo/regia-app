//! Local only: Acceptance 2 of #470, the sessions to resume by hand that the Rust resident wrote, read by the prototype's
//! brief on one copy of the state. CI cannot read the dossier, so the test is ignored there:
//!
//!     REGIA_HOME=<dossier> cargo test -p regia-conformance --test controller_resume_cross -- --ignored --test-threads=1
//!
//! One round of the Rust resident finds a step on the app stopped by the limit, past its hour: it does not resume it,
//! and writes it in `da-riprendere.json`. The prototype's `avvisi_controller` and brief (`brief_oracle.py` of #468) read
//! the file the Rust wrote, with the twelve keys the facade knows, and the brief's JSON goes through the facade's own
//! translation. The state folder and the agents' folder are fresh ones under `target/`; the transcript is synthetic.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use regia_agents::{Dry, Env, NoChatHost, ProbeConsent, claude_slug};
use regia_controller::engine::native::new_record;
use regia_controller::engine::{Folders, LaunchedStep, Liveness, Precheck, PrecheckRun, StepReceipt};
use regia_controller::resident::{Config, Console, Exit, HostPorts, Notice, Resident, Sleep};
use regia_controller::resume::{Resume, ResumeReceipt, Resumer};
use regia_exec::harness;
use regia_exec::{Pin, Program};
use regia_facade::Table;
use regia_identity::prototype_env_var;
use regia_store::{CursorName, ProjectId, Store, SystemClock};
use serde_json::{Map, Value, json};

fn dossier() -> PathBuf {
    let home = std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = home.unwrap_or_else(|| panic!("set {} to the dossier: this test runs its brief", prototype_env_var("HOME")));
    assert!(home.join("tools").join("regia.py").is_file(), "no tools/regia.py in {}", home.display());
    home
}

fn folder(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("controller-resume-cross").join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    }
    fs::create_dir_all(&dir).expect("state folder");
    dir
}

struct Idle;

impl Precheck for Idle {
    fn run(&self, _projects: &[String], _timeout_s: f64) -> PrecheckRun {
        PrecheckRun::from_output(Some(3), &json!({"v": 1, "outcome": "idle", "exit": 3, "motivo": "nessun-evento", "payload": {"progetti": []}}).to_string())
    }
}

struct NoFolders;

impl Folders for NoFolders {
    fn folders(&self) -> Result<BTreeMap<String, Option<String>>, String> {
        Ok(BTreeMap::new())
    }
}

struct Nobody;

impl Liveness for Nobody {
    fn alive(&self, _pid: u32) -> bool {
        false
    }
}

struct Quiet;

impl Console for Quiet {
    fn notice(&self, _notice: &Notice) {}
}

struct NoSleep;

impl Sleep for NoSleep {
    fn sleep(&self, _secs: f64) {}
}

/// A resume that must not happen: the app's session is never resumed from outside.
struct Never;

impl Resumer for Never {
    fn resume(&self, _record: &Map<String, Value>) -> Result<ResumeReceipt, String> {
        panic!("the app's session was resumed from outside");
    }
}

fn stamp(at: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    let utc = chrono::DateTime::from_timestamp_micros((at * 1e6) as i64).unwrap();
    utc.format("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

/// The test of conformance of Acceptance 2: `da-riprendere.json` written by the Rust appears in the Python brief.
#[test]
#[ignore = "needs the dossier and python: REGIA_HOME"]
fn the_prototype_s_brief_shows_the_session_to_resume_the_rust_wrote() {
    let (dossier, dir) = (dossier(), folder("to-resume"));
    let work = dir.join("work");
    let claude = dir.join("claude");
    fs::create_dir_all(&work).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    let (limit_at, resets) = (now - 3600.0, now - 600.0);
    // A chat of the app a step was launched in, stopped by the limit an hour ago, its hour ten minutes past.
    let transcript = claude.join("projects").join(claude_slug(&work)).join("s-app.jsonl");
    fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    let entries = [
        json!({"type": "user", "timestamp": stamp(limit_at - 60.0), "sessionId": "s-app", "entrypoint": "sdk-ts", "message": {"role": "user", "content": "passo"}}),
        json!({"type": "assistant", "timestamp": stamp(limit_at), "sessionId": "s-app", "entrypoint": "sdk-ts",
               "message": {"role": "assistant", "model": "<synthetic>", "content": [{"type": "text", "text": "You've hit your session limit \u{b7} resets 11pm (Europe/Rome)"}]},
               "quotaLimits": {"status": "rejected", "resetsAt": resets}}),
    ];
    fs::write(&transcript, entries.iter().map(|entry| format!("{entry}\n")).collect::<String>()).unwrap();
    let receipt = StepReceipt {
        adapter: "orca".to_owned(),
        agent: "claude".to_owned(),
        model: None,
        effort: None,
        session: Some("s-app".to_owned()),
        transcript: Some(transcript.display().to_string()),
        pid: None,
        log: None,
        started: true,
        surface_session: Some("claude_cross_1".to_owned()),
        marker: Some("passo".to_owned()),
    };
    let work_text = work.display().to_string();
    let (key, record) = new_record(&LaunchedStep { project: "a", folder: &work_text, prompt: "passo", receipt: &receipt }, limit_at - 120.0);
    let store = Store::at(&dir).unwrap();
    store.write_cursor(&ProjectId::new("controller").unwrap(), &CursorName::new("passi").unwrap(), json!({"v": 1, "passi": {key: record}})).unwrap();

    let mut config = Config::new(&dir);
    config.rounds = 1;
    let ports = HostPorts {
        store,
        clock: Arc::new(SystemClock),
        sleep: Arc::new(NoSleep),
        precheck: Arc::new(Idle),
        folders: Arc::new(NoFolders),
        liveness: Arc::new(Nobody),
        surface: Arc::new(Dry::new()),
        ids: Arc::new(|| "id-cross".to_owned()),
        consent: ProbeConsent::Refused,
        token_expiry: None,
        console: Arc::new(Quiet),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let env = Env::from_pairs([("CLAUDE_CONFIG_DIR", claude.as_os_str())]);
    let resume = Resume::new(&dir, env, Arc::new(Never), Arc::new(NoChatHost));
    assert_eq!(Resident::new(config, ports).with_before_precheck(Box::new(resume)).run(), Exit::Done);
    let log = fs::read_to_string(dir.join("controller.log")).unwrap();
    let line: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(line["decisione"], "attesa-da-riprendere", "{line}");

    // The prototype's brief on the same folder.
    let python = Pin::resolve(Program::Python).unwrap_or_else(|error| panic!("{error}"));
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle").join("brief_oracle.py");
    let argv: Vec<String> = ["-X", "utf8", "-I", "-B"].iter().map(|word| (*word).to_owned()).chain([oracle.display().to_string(), "--dossier".to_owned(), dossier.display().to_string(), "--dir".to_owned(), dir.display().to_string()]).collect();
    let output = harness::program(&python, &argv, &dir).run().expect("run the prototype");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the prototype failed: {}\n{stdout}", String::from_utf8_lossy(&output.stderr));
    let brief: Value = serde_json::from_str(stdout.lines().last().unwrap_or("null")).unwrap();
    let voices = brief["warnings"]["da_riprendere"].as_array().cloned().unwrap_or_default();
    assert_eq!(voices.len(), 1, "{brief}");
    let voice = &voices[0];
    let mut keys: Vec<&str> = voice.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["adapter", "agente", "cartella", "comando_orca", "come", "motivo", "origine", "progetto", "riprende", "sessione", "sessione_orca", "transcript"]);
    assert_eq!((&voice["progetto"], &voice["sessione"], &voice["adapter"]), (&json!("a"), &json!("s-app"), &json!("orca")));
    assert!(voice["comando_orca"].as_str().unwrap_or_default().contains("--to orca_session_id:claude_cross_1"), "{voice}");
    let text = brief["text"].as_str().unwrap_or_default();
    assert!(text.contains("Da riprendere a mano: a, sessione s-app"), "the brief's text names it: {text}");
    // The brief's JSON, as the facade translates it: every key of the voice is one it knows. The prototype's controller
    // block also has `precedente_aperta` (#209), which the facade's table does not name yet: that key, not this slice's,
    // is left out here (a finding of #470 for the gate of names).
    let mut json = brief["json"].clone();
    if let Some(Value::Object(controller)) = json.get_mut("controller") {
        controller.remove("precedente_aperta");
    }
    let translated = Table::embedded().translate_output("brief", &json).unwrap_or_else(|error| panic!("{error}"));
    let to_resume = &translated["controller"]["to_resume"][0];
    assert_eq!((&to_resume["project"], &to_resume["session"], &to_resume["orca_session"]), (&json!("a"), &json!("s-app"), &json!("claude_cross_1")), "{translated}");
}
