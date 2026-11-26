//! A session the usage limit stopped, read from its transcript (#470, F28): the prototype's cases of
//! `tools/tests/test_sospese.py` on the Rust, 30 of its 35. The five that start the fake agent (`RipresaCli` and the
//! search of the app's index, `test_cerca_orca_mappa_le_chat_e_le_sessioni_cli`) are `regia-agents/tests/resume.rs`.
//!
//! The transcripts are synthetic, with the **shape** of the real ones of 2026-10-21 and no content: the limit followed by
//! the entries around it and a subagent's notice, the limit then "Continue where you left off." and an answer, a
//! subagent's limit (`subagents/agent-*.jsonl`), a rollout of Codex with `usage_limit_exceeded`. The agents' folders are
//! under `target/`, never the real ones.
//!
//! `test_senza_tzdata_il_testo_ripiega_su_resetsat_o_sulla_stima` keeps its second half only: the Rust bundles the time
//! zones' database, so a text with a known zone always reads; the structured field still wins over it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use chrono::DateTime;
use regia_agents::suspension::{self, Reading, Source, State, Zone, analyze, opted_in, reset_from_text, scan, suspended_subagents};
use regia_agents::{ChatHost, Env, Known, ServiceId, claude_slug, coordinated_workers, session_by_marker, worker_wake_command};
use regia_controller::engine::native::{StoreSteps, new_record};
use regia_controller::engine::{LaunchedStep, StepReceipt, Steps};
use regia_controller::resume::{OptIn, read_opt_in, resumable_from_here};
use regia_store::{CursorName, Holder, ProjectId, Store, Ttl};
use serde_json::{Map, Value, json};

const SID: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const CWD: &str = "C:\\Users\\prova\\progetto";
/// "11pm (Europe/Rome)" of 2026-10-21 = 21:00Z, and "5:30pm (Europe/Rome)" of that day = 15:30Z: real values.
const RESETS_23: f64 = 1_792_616_400.0;
const RESETS_1730: f64 = 1_792_596_600.0;

fn stamp(at: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    let utc = DateTime::from_timestamp_micros((at * 1e6) as i64).unwrap();
    utc.format("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

fn user(at: f64, text: &str, sid: &str, entrypoint: &str) -> Value {
    json!({"type": "user", "timestamp": stamp(at), "sessionId": sid, "cwd": CWD, "entrypoint": entrypoint,
           "message": {"role": "user", "content": [{"type": "text", "text": text}]}})
}

fn answer(at: f64, text: &str) -> Value {
    json!({"type": "assistant", "timestamp": stamp(at), "sessionId": SID, "cwd": CWD, "entrypoint": "sdk-ts",
           "message": {"role": "assistant", "model": "claude-test", "content": [{"type": "text", "text": text}]}})
}

/// The limit as Claude Code writes it; `field` names where its hour is, none for the text alone.
fn limit_with(at: f64, resets: Option<f64>, text: &str, entrypoint: &str, field: &str) -> Value {
    let mut entry = json!({"type": "assistant", "timestamp": stamp(at), "sessionId": SID, "cwd": CWD, "entrypoint": entrypoint,
                           "message": {"role": "assistant", "model": "<synthetic>", "stop_reason": "stop_sequence", "content": [{"type": "text", "text": text}]},
                           "apiError": "usage_limit_reached", "error": "rate_limit", "isApiErrorMessage": true, "apiErrorStatus": 429});
    if let Some(resets) = resets {
        if field == "quotaLimits" {
            entry["quotaLimits"] = json!({"status": "rejected", "resetsAt": resets, "rateLimitType": "five_hour", "isUsingOverage": false});
        }
        entry["apiErrorParams"] = json!({"rate_limit_info": {"status": "rejected", "resetsAt": resets}});
    }
    entry
}

const ELEVEN: &str = "You've hit your session limit \u{b7} resets 11pm (Europe/Rome)";

fn limit(at: f64, resets: f64) -> Value {
    limit_with(at, Some(resets), ELEVEN, "sdk-ts", "quotaLimits")
}

fn around(at: f64) -> Vec<Value> {
    ["queue-operation", "last-prompt", "mode", "cost-state", "attachment"].iter().map(|kind| json!({"type": kind, "timestamp": stamp(at)})).collect()
}

fn notice(at: f64) -> Value {
    user(at, "<task-notification>\n<task-id>abc</task-id>\n</task-notification>", SID, "sdk-ts")
}

fn write(path: &Path, entries: &[Value]) -> PathBuf {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, entries.iter().map(|entry| format!("{entry}\n")).collect::<String>()).unwrap();
    path.to_path_buf()
}

fn touch(path: &Path, at: f64) {
    fs::File::options().write(true).open(path).unwrap().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs_f64(at)).unwrap();
}

fn now() -> f64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs_f64()
}

/// A scratch folder of a case, with the agents' folders inside it.
struct Base {
    dir: PathBuf,
}

impl Base {
    fn new(label: &str) -> Base {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("suspended").join(label);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Base { dir }
    }

    fn env(&self) -> Env {
        Env::from_pairs([("CLAUDE_CONFIG_DIR", self.dir.join("claude")), ("CODEX_HOME", self.dir.join("codex"))])
    }

    fn analyze(&self, entries: &[Value], now: f64) -> suspension::Analysis {
        let path = write(&self.dir.join("t.jsonl"), entries);
        analyze(&path, ServiceId::Claude, &Reading::at(now))
    }

    fn analyze_codex(&self, entries: &[Value]) -> suspension::Analysis {
        let path = write(&self.dir.join("t.jsonl"), entries);
        analyze(&path, ServiceId::Codex, &Reading::at(now()))
    }

    fn project(&self, folder: &Path) -> PathBuf {
        self.dir.join("claude").join("projects").join(claude_slug(folder))
    }
}

// ---------- Riconoscimento ----------

/// `test_limite_con_voci_di_contorno_e_notifica_di_sottoagente_e_ancora_sospesa`: the shape of a6473, the limit is not
/// the last line; the entries around it and a subagent's notice follow.
#[test]
fn a_limit_followed_by_the_entries_around_it_and_a_subagent_s_notice_is_still_suspended() {
    let t = 1_792_600_000.0;
    let mut entries = vec![user(t - 60.0, "lavora", SID, "sdk-ts"), answer(t - 30.0, "fatto"), limit(t, RESETS_23)];
    entries.extend(around(t + 1.0));
    entries.push(notice(t + 120.0));
    entries.extend(around(t + 121.0));
    let a = Base::new("around").analyze(&entries, now());
    assert_eq!(a.state, State::Suspended);
    assert_eq!(a.resets, Some(RESETS_23));
    assert_eq!(a.resets.and_then(suspension::iso).as_deref(), Some("2026-10-21T21:00:00Z"));
    assert_eq!(a.source, Some(Source::QuotaLimits));
    assert_eq!(a.zone, Some(Zone::Named("Europe/Rome".to_owned())));
    assert_eq!(a.entrypoint.as_deref(), Some("sdk-ts"));
    assert_eq!(a.estimated, Some(false));
}

/// `test_ripresa_con_continue_e_risposta_non_e_piu_sospesa`: the shape of f6b7.
#[test]
fn a_resume_with_continue_and_an_answer_is_no_longer_suspended() {
    let t = 1_792_600_000.0;
    let mut entries = vec![answer(t - 30.0, "fatto"), limit(t, RESETS_1730)];
    entries.extend(around(t + 1.0));
    entries.extend([user(t + 9000.0, "Continue where you left off.", SID, "sdk-ts"), answer(t + 9010.0, "riprendo")]);
    assert_eq!(Base::new("continued").analyze(&entries, now()).state, State::Active);
}

/// `test_continue_senza_risposta_e_ripresa_in_corso_poi_di_nuovo_sospesa`.
#[test]
fn continue_without_an_answer_is_a_resume_under_way_then_suspended_again() {
    let t = 1_792_600_000.0;
    let base = Base::new("under-way");
    let entries = [answer(t - 30.0, "fatto"), limit(t, RESETS_1730), user(t + 9000.0, "Continue where you left off.", SID, "sdk-ts")];
    assert_eq!(base.analyze(&entries, t + 9060.0).state, State::Resuming);
    let a = base.analyze(&entries, t + 9000.0 + suspension::RESUME_GRACE_S + 5.0);
    assert_eq!(a.state, State::Suspended, "no answer: the resume did not go");
    assert_eq!(a.user_after.as_deref(), Some("Continue where you left off."));
}

/// `test_nuovo_limite_dopo_la_ripresa_vale_il_nuovo`.
#[test]
fn a_new_limit_after_the_resume_is_the_one_that_counts() {
    let t = 1_792_600_000.0;
    let entries = [limit(t, RESETS_1730), user(t + 9000.0, "Continue where you left off.", SID, "sdk-ts"), answer(t + 9010.0, "fatto"), limit(t + 9500.0, RESETS_23)];
    let a = Base::new("new-limit").analyze(&entries, now());
    assert_eq!((a.state, a.resets), (State::Suspended, Some(RESETS_23)));
}

/// `test_messaggio_normale_che_cita_la_frase_non_e_un_limite`.
#[test]
fn an_ordinary_message_that_quotes_the_sentence_is_no_limit() {
    let t = 1_792_600_000.0;
    let quoting = "Ho visto che \"You've hit your session limit \u{b7} resets 5pm (Europe/Rome)\" e' comparso: ora riprendo il lavoro dal punto in cui ero.";
    let entries = [limit(t, RESETS_1730), user(t + 9000.0, "continua", SID, "sdk-ts"), answer(t + 9010.0, quoting)];
    assert_eq!(Base::new("quoting").analyze(&entries, now()).state, State::Active);
}

/// `test_testo_senza_campo_strutturato_con_ora_e_fuso`: "5:30pm (Europe/Rome)" written at 13:21Z of 2026-10-21, 15:21 in
/// Rome: the 17:30 in Rome of that same day.
#[test]
fn a_text_without_the_structured_field_reads_its_hour_and_zone() {
    let t = 1_792_588_910.0;
    let text = "You've hit your session limit \u{b7} resets 5:30pm (Europe/Rome)";
    let a = Base::new("text").analyze(&[answer(t - 5.0, "fatto"), limit_with(t, None, text, "sdk-ts", "quotaLimits")], now());
    assert_eq!((a.state, a.source, a.zone.clone()), (State::Suspended, Some(Source::Text), Some(Zone::Named("Europe/Rome".to_owned()))));
    assert_eq!(a.resets, Some(RESETS_1730));
}

/// `test_testo_ora_gia_passata_vuol_dire_il_giorno_dopo`: "2am" written at 23:00 in Rome (21:00Z of 2026-10-21) is 02:00 in
/// Rome of the 22nd, 00:00Z; "12am" is midnight, "12pm" noon.
#[test]
fn an_hour_already_past_in_the_text_is_the_next_day_s() {
    let at = |hour: &str| reset_from_text(&format!("You've hit your session limit \u{b7} resets {hour} (Europe/Rome)"), Some(RESETS_23)).unwrap();
    let two = at("2am");
    assert_eq!((two.at, two.zone.as_str()), (RESETS_23 + 3.0 * 3600.0, "Europe/Rome"));
    assert_eq!(at("12am").at, RESETS_23 + 3.0 * 3600.0 - 2.0 * 3600.0);
    assert_eq!(at("12pm").at, RESETS_23 + 13.0 * 3600.0);
}

/// `test_testo_con_data_del_limite_settimanale`: the day after the summer time ends.
#[test]
fn a_weekly_limit_s_text_has_its_day() {
    let found = reset_from_text("You've hit your weekly limit \u{b7} resets Oct 27, 9am (Europe/Rome)", Some(RESETS_23)).unwrap();
    assert_eq!(suspension::iso(found.at).as_deref(), Some("2026-10-27T08:00:00Z"));
}

/// `test_fuso_sconosciuto_senza_resetsat_ora_stimata_a_cinque_ore`.
#[test]
fn an_unknown_zone_without_a_field_estimates_five_hours() {
    let t = 1_792_588_910.0;
    let text = "You've hit your session limit \u{b7} resets 5:30pm (Mars/Olympus)";
    let a = Base::new("unknown-zone").analyze(&[limit_with(t, None, text, "sdk-ts", "quotaLimits")], now());
    assert_eq!(a.state, State::Suspended);
    assert_eq!(a.estimated, Some(true));
    assert_eq!(a.resets, Some(t + suspension::UNKNOWN_WAIT_S));
}

/// `test_senza_tzdata_il_testo_ripiega_su_resetsat_o_sulla_stima`, its second half: with the structured field the text's
/// hour is not read. The first half has no case on the Rust, whose time zones are bundled.
#[test]
fn the_structured_field_wins_over_the_text() {
    let a = Base::new("field-wins").analyze(&[limit(1_792_588_910.0, RESETS_1730)], now());
    assert_eq!((a.resets, a.source), (Some(RESETS_1730), Some(Source::QuotaLimits)));
}

/// `test_campo_strutturato_alternativo_apiErrorParams`.
#[test]
fn the_other_structured_field_api_error_params_gives_the_hour() {
    let a = Base::new("api-error-params").analyze(&[limit_with(1_792_600_000.0, Some(RESETS_23), ELEVEN, "sdk-ts", "apiErrorParams")], now());
    assert_eq!((a.source, a.resets), (Some(Source::ApiErrorParams), Some(RESETS_23)));
}

/// `test_resetsat_in_millisecondi`.
#[test]
fn an_hour_in_milliseconds_is_read() {
    assert_eq!(Base::new("millis").analyze(&[limit(1_792_600_000.0, RESETS_23 * 1000.0)], now()).resets, Some(RESETS_23));
}

/// `test_senza_limite_e_senza_assistente`.
#[test]
fn without_a_limit_and_without_the_assistant() {
    let base = Base::new("no-limit");
    assert_eq!(base.analyze(&[user(1_792_600_000.0, "ciao", SID, "sdk-ts"), answer(1_792_600_001.0, "fatto")], now()).state, State::Active);
    assert_eq!(base.analyze(&[user(1_792_600_000.0, "ciao", SID, "sdk-ts")], now()).state, State::Empty);
    assert_eq!(analyze(&base.dir.join("non-esiste.jsonl"), ServiceId::Claude, &Reading::at(now())).state, State::Unreadable);
}

/// `test_righe_non_json_e_file_lungo`: the end of the file (512 KB) holds no assistant entry, so more is read; a line cut
/// in half is skipped.
#[test]
fn lines_that_are_not_json_and_a_long_file() {
    let t = 1_792_600_000.0;
    let base = Base::new("long");
    let mut entries = vec![answer(t - 5.0, "fatto"), limit(t, RESETS_23)];
    entries.extend((0..1700).map(|i| json!({"type": "attachment", "timestamp": stamp(t + f64::from(i)), "x": "y".repeat(400)})));
    let path = write(&base.dir.join("t.jsonl"), &entries);
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("{\"type\": \"attachment\", \"troncata");
    fs::write(&path, text).unwrap();
    assert!(fs::metadata(&path).unwrap().len() > 512 * 1024);
    let a = analyze(&path, ServiceId::Claude, &Reading::at(now()));
    assert_eq!((a.state, a.resets), (State::Suspended, Some(RESETS_23)));
}

/// `test_sottoagenti_sospesi_non_sospendono_la_madre`.
#[test]
fn suspended_subagents_do_not_suspend_their_parent() {
    let t = 1_792_600_000.0;
    let base = Base::new("subagents");
    let parent = write(&base.dir.join("claude").join("projects").join("p").join(format!("{SID}.jsonl")), &[answer(t, "fatto"), user(t + 5.0, "ok", SID, "sdk-ts")]);
    let folder = parent.with_extension("").join("subagents");
    write(&folder.join("agent-a1.jsonl"), &[answer(t - 10.0, "fatto"), limit(t, RESETS_23)]);
    write(&folder.join("agent-a2.jsonl"), &[answer(t - 10.0, "fatto"), answer(t, "fatto")]);
    assert_eq!(analyze(&parent, ServiceId::Claude, &Reading::at(now())).state, State::Active);
    let stopped = suspended_subagents(&parent, &Reading::at(now()));
    assert_eq!(stopped.iter().map(|subagent| subagent.id.as_str()).collect::<Vec<_>>(), ["a1"]);
    assert_eq!(stopped[0].resets.and_then(suspension::iso).as_deref(), Some("2026-10-21T21:00:00Z"));
}

// ---------- Codex ----------

fn rollout(at: f64, resumed: bool) -> Vec<Value> {
    let mut entries = vec![
        json!({"type": "session_meta", "timestamp": stamp(at - 100.0), "payload": {"id": "cdx-1"}}),
        json!({"type": "event_msg", "timestamp": stamp(at - 50.0), "payload": {"type": "task_started", "turn_id": "t1"}}),
        json!({"type": "event_msg", "timestamp": stamp(at - 10.0), "payload": {"type": "token_count", "rate_limits": {"primary": {"used_percent": 100.0, "window_minutes": 300, "resets_at": RESETS_1730}}}}),
        json!({"type": "event_msg", "timestamp": stamp(at), "payload": {"type": "task_complete", "last_agent_message": null,
               "error": {"message": "You've hit your usage limit. Upgrade to Pro or try again at 5:30 PM.", "codex_error_info": "usage_limit_exceeded"}}}),
    ];
    if resumed {
        entries.push(json!({"type": "event_msg", "timestamp": stamp(at + 9000.0), "payload": {"type": "task_started", "turn_id": "t2"}}));
        entries.push(json!({"type": "event_msg", "timestamp": stamp(at + 9100.0), "payload": {"type": "task_complete", "last_agent_message": "ok"}}));
    }
    entries
}

/// `test_codex_sospeso_con_resets_at_del_token_count`.
#[test]
fn codex_suspended_with_the_hour_of_its_token_count() {
    let a = Base::new("codex-suspended").analyze_codex(&rollout(1_792_590_000.0, false));
    assert_eq!((a.state, a.resets, a.source), (State::Suspended, Some(RESETS_1730), Some(Source::TaskError)));
    assert_eq!(a.session.as_deref(), Some("cdx-1"));
}

/// `test_codex_ripreso`.
#[test]
fn codex_resumed() {
    assert_eq!(Base::new("codex-resumed").analyze_codex(&rollout(1_792_590_000.0, true)).state, State::Active);
}

// ---------- Guardia: never the resume of an app's session from outside ----------

/// The prototype's `cerca_orca` for the guard: what the app answers, and how many times it was asked.
struct Host {
    answer: Option<Known>,
    asked: Mutex<usize>,
}

impl Host {
    fn answers(answer: Option<Known>) -> Host {
        Host { answer, asked: Mutex::new(0) }
    }
}

impl ChatHost for Host {
    fn known(&self, _folder: &str) -> Option<Known> {
        *self.asked.lock().unwrap() += 1;
        self.answer.clone()
    }
}

fn record(changes: &[(&str, Value)]) -> Map<String, Value> {
    let mut record = json!({"progetto": "p", "cartella": "C:\\work", "origine": "controller", "adapter": "cli", "agente": "claude",
                            "sessione": SID, "sessione_resume": SID, "sessione_orca": null})
    .as_object()
    .cloned()
    .unwrap();
    for (key, value) in changes {
        record.insert((*key).to_owned(), value.clone());
    }
    record
}

/// `test_adapter_orca_mai_via_cli`.
#[test]
fn a_step_on_the_app_is_never_resumed_on_the_cli() {
    let (ok, why) = resumable_from_here(&record(&[("adapter", json!("orca"))]), &Host::answers(Some(Known::new())));
    assert!(!ok);
    assert!(why.contains("Orca"));
}

/// `test_id_orca_mai_via_cli`.
#[test]
fn a_record_with_the_app_s_id_is_never_resumed_on_the_cli() {
    assert!(!resumable_from_here(&record(&[("sessione_orca", json!("claude_x_1"))]), &Host::answers(Some(Known::new()))).0);
}

/// `test_transcript_di_chat_orca_mai_via_cli`.
#[test]
fn a_transcript_of_the_app_s_chat_is_never_resumed_on_the_cli() {
    assert!(!resumable_from_here(&record(&[("analisi", json!({"entrypoint": "sdk-ts"}))]), &Host::answers(Some(Known::new()))).0);
}

/// `test_passo_cli_del_controller_si_riprende_senza_chiedere_a_orca`.
#[test]
fn a_step_the_controller_launched_on_the_cli_is_resumed_without_asking_the_app() {
    let host = Host::answers(None);
    let (ok, _) = resumable_from_here(&record(&[("analisi", json!({"entrypoint": "sdk-cli"}))]), &host);
    assert!(ok);
    assert_eq!(*host.asked.lock().unwrap(), 0);
}

/// `test_optin_conosciuta_a_orca_non_si_riprende_e_orca_muta_e_fail_closed`.
#[test]
fn an_allowed_session_the_app_knows_is_not_resumed_and_a_silent_app_fails_closed() {
    let allowed = record(&[("origine", json!("optin"))]);
    assert!(!resumable_from_here(&allowed, &Host::answers(Some(Known::from([(SID.to_owned(), Some("claude_abc_1".to_owned()))])))).0);
    let (ok, why) = resumable_from_here(&allowed, &Host::answers(None));
    assert!(!ok);
    assert!(why.contains("non risponde"));
    assert!(resumable_from_here(&allowed, &Host::answers(Some(Known::from([("altra".to_owned(), Some("claude_x".to_owned()))])))).0);
    assert!(resumable_from_here(&allowed, &Host::answers(Some(Known::from([(SID.to_owned(), None)])))).0, "indexed but no chat: a session of the command line");
}

// ---------- Registro ----------

fn reserved() -> ProjectId {
    ProjectId::new("controller").unwrap()
}

/// `test_registro_nel_runtime_con_api_pubblica_e_ritrovato_da_un_altro_processo`: the registry in the state, found again
/// by another controller; the reserved project's cursor of the events is left alone.
#[test]
fn the_registry_is_in_the_state_and_another_controller_finds_it() {
    let folder = regia_conformance::prepare_state_dir("suspended-registry");
    let store = Store::at(&folder).unwrap();
    let holder = Holder::new("controller-test-1").unwrap();
    assert!(store.acquire_lock(&reserved(), &holder, Ttl::from_secs(600).unwrap()).unwrap().is_ok());
    let steps = StoreSteps::new(store.clone(), holder.clone());
    steps.load().unwrap();
    let receipt = StepReceipt {
        adapter: "cli".to_owned(),
        agent: "claude".to_owned(),
        model: None,
        effort: None,
        session: Some(SID.to_owned()),
        transcript: Some("x.jsonl".to_owned()),
        pid: Some(123),
        log: Some("l".to_owned()),
        started: true,
        surface_session: None,
        marker: None,
    };
    let here = folder.display().to_string();
    let key = steps.launched(&LaunchedStep { project: "p", folder: &here, prompt: "prompt del passo", receipt: &receipt }, 1_792_000_000.0).unwrap();
    let stored = store.cursor(&reserved(), &CursorName::new("passi").unwrap()).unwrap();
    assert_eq!(stored["passi"][&key]["sessione_resume"], SID);
    let other = StoreSteps::new(store.clone(), holder);
    other.load().unwrap();
    assert_eq!(Steps::records(&other)[&key]["pid"], 123);
    assert_eq!(store.cursor(&reserved(), &CursorName::new("eventi").unwrap()).unwrap(), Value::Null, "the cursor of the events is not touched");
}

/// `test_passo_orca_senza_file_ritrovato_non_ha_sessione_resume`.
#[test]
fn a_step_on_the_app_without_its_file_found_has_no_session_to_resume() {
    let receipt = |session: &str, transcript: Option<&str>| StepReceipt {
        adapter: "orca".to_owned(),
        agent: "claude".to_owned(),
        model: None,
        effort: None,
        session: Some(session.to_owned()),
        transcript: transcript.map(str::to_owned),
        pid: None,
        log: None,
        started: true,
        surface_session: Some("claude_o_1".to_owned()),
        marker: Some("x".to_owned()),
    };
    let (_, record) = new_record(&LaunchedStep { project: "p", folder: "w", prompt: "x", receipt: &receipt("claude_o_1", None) }, 5.0);
    assert_eq!(record["sessione_resume"], Value::Null);
    assert_eq!(record["marcatore"], "x", "the marker of the receipt (#462/V7)");
    let (_, found) = new_record(&LaunchedStep { project: "p", folder: "w", prompt: "x", receipt: &receipt(SID, Some("t.jsonl")) }, 5.0);
    assert_eq!(found["sessione_resume"], SID);
}

/// `test_i_passi_conclusi_si_dimenticano_dopo_una_settimana_gli_aperti_mai`.
#[test]
fn the_finished_steps_are_forgotten_after_a_week_the_open_ones_never() {
    let folder = regia_conformance::prepare_state_dir("suspended-week");
    let store = Store::at(&folder).unwrap();
    let holder = Holder::new("controller-test-1").unwrap();
    assert!(store.acquire_lock(&reserved(), &holder, Ttl::from_secs(600).unwrap()).unwrap().is_ok());
    let steps = StoreSteps::new(store, holder);
    steps.load().unwrap();
    let old = json!({"progetto": "a", "stato": "finito", "aggiornato_t": 1000.0});
    let open = json!({"progetto": "b", "stato": "sospesa", "aggiornato_t": 1000.0});
    steps.save_records(Map::from_iter([("a/1".to_owned(), old), ("b/1".to_owned(), open)]), 1000.0 + 7.0 * 86_400.0 + 1.0).unwrap();
    assert_eq!(Steps::records(&steps).keys().cloned().collect::<Vec<_>>(), ["b/1"]);
}

/// `test_ritrova_sessione_orca_dal_prompt`.
#[test]
fn the_session_of_a_step_on_the_app_is_found_by_its_prompt() {
    let base = Base::new("by-marker");
    let folder = base.dir.join("prog");
    fs::create_dir_all(&folder).unwrap();
    let prompt = "Sei un passo di orchestratore della regia sul progetto";
    write(&base.project(&folder).join("vecchia.jsonl"), &[user(1.0, "altro", SID, "sdk-ts")]);
    write(&base.project(&folder).join("nuova.jsonl"), &[user(2.0, prompt, SID, "sdk-ts")]);
    let marker: String = prompt.chars().take(60).collect();
    let (session, path) = session_by_marker(ServiceId::Claude, &base.env(), &folder, 0.0, &marker).unwrap();
    assert_eq!((session.as_str(), path.file_name().unwrap().to_str().unwrap()), ("nuova", "nuova.jsonl"));
    assert_eq!(session_by_marker(ServiceId::Claude, &base.env(), &folder, 0.0, "mai scritto"), None);
}

/// `test_worker_di_orca_ritrovati_dal_coordinatore_nel_preambolo`.
#[test]
fn the_workers_of_the_app_are_found_by_their_coordinator_in_the_preamble() {
    let base = Base::new("workers");
    let folder = base.dir.join("prog");
    fs::create_dir_all(&folder).unwrap();
    let preamble = "You are a dispatched worker.\nYour coordinator's Orca session ID is: orca_session_id:claude_ABC_1\n\
                    Your task ID is: task_0123456789ab\n  orca orchestration send --type heartbeat --task-id task_0123456789ab \
                    --dispatch-id ctx_0123456789ab --phase x";
    let project = base.project(&folder);
    write(&project.join("w1.jsonl"), &[user(1.0, preamble, "w1", "sdk-ts")]);
    write(&project.join("w2.jsonl"), &[user(1.0, &preamble.replace("claude_ABC_1", "claude_ALTRO_9"), "w2", "sdk-ts")]);
    let found = coordinated_workers(&base.env(), &folder.display().to_string(), "claude_ABC_1", &[SID], None);
    assert_eq!(found.iter().map(|(session, _)| session.as_str()).collect::<Vec<_>>(), ["w1"]);
    let command = worker_wake_command(&project.join("w1.jsonl")).unwrap();
    assert!(command.contains("--to dispatch:ctx_0123456789ab"));
    assert!(command.contains("--task-id task_0123456789ab"));
    assert!(command.contains("Continue where you left off"));
    assert_eq!(worker_wake_command(&project.join("non-esiste.jsonl")), None);
}

// ---------- OptIn ----------

/// `test_file_assente_o_vuoto_nessuna_sessione`.
#[test]
fn a_missing_or_empty_opt_in_allows_no_session() {
    let base = Base::new("opt-in-empty");
    assert_eq!(read_opt_in(&base.dir.join("non-c-e.json")), OptIn::default());
    let file = base.dir.join("o.json");
    fs::write(&file, "{}").unwrap();
    assert_eq!(read_opt_in(&file), OptIn::default());
    fs::write(&file, "non json").unwrap();
    assert_eq!(read_opt_in(&file), OptIn::default());
    assert!(opted_in(&base.env(), &[], &[], 14.0, now()).is_empty());
}

/// `test_optin_per_id_e_per_cartella`.
#[test]
fn an_opt_in_by_id_and_by_folder() {
    let base = Base::new("opt-in");
    let folder = base.dir.join("prog");
    fs::create_dir_all(&folder).unwrap();
    let project = base.project(&folder);
    let s1 = write(&project.join("s1.jsonl"), &[limit_with(1_792_600_000.0, Some(RESETS_23), ELEVEN, "cli", "quotaLimits")]);
    let s2 = write(&project.join("s2.jsonl"), &[answer(1_792_600_000.0, "fatto")]);
    let (at, env) = (now(), base.env());
    touch(&s1, at);
    touch(&s2, at);
    let by_id = opted_in(&env, &["s1".to_owned()], &[], 14.0, at);
    assert_eq!(by_id.iter().map(|found| found.session.as_str()).collect::<Vec<_>>(), ["s1"]);
    let mut by_folder: Vec<String> = opted_in(&env, &[], &[folder.display().to_string()], 14.0, at).into_iter().map(|found| found.session).collect();
    by_folder.sort();
    assert_eq!(by_folder, ["s1", "s2"]);
    assert_eq!(by_id[0].cwd, CWD, "the working folder the transcript names");
}

// ---------- Scansione ----------

/// `test_scansiona_elenca_le_sospese_e_i_sottoagenti_con_ora_di_ripresa`.
#[test]
fn the_scan_lists_the_suspended_sessions_and_the_subagents_with_their_hour() {
    let base = Base::new("scan");
    let folder = base.dir.join("prog");
    fs::create_dir_all(&folder).unwrap();
    let project = base.project(&folder);
    let t = 1_792_600_000.0;
    write(&project.join("sospesa.jsonl"), &[answer(t, "fatto"), limit(t + 1.0, RESETS_23)]);
    write(&project.join("ripresa.jsonl"), &[limit(t, RESETS_23), user(t + 5.0, "Continue where you left off.", SID, "sdk-ts"), answer(t + 9.0, "fatto")]);
    let parent = write(&project.join("madre.jsonl"), &[answer(t, "fatto")]);
    write(&parent.with_extension("").join("subagents").join("agent-x.jsonl"), &[limit(t, RESETS_1730)]);
    let at = now();
    for file in [project.join("sospesa.jsonl"), project.join("ripresa.jsonl"), parent.clone(), parent.with_extension("").join("subagents").join("agent-x.jsonl")] {
        touch(&file, at);
    }
    let chats = Known::from([("sospesa".to_owned(), Some("claude_o_1".to_owned()))]);
    let found = scan(&base.env(), &[folder.display().to_string()], 14.0, &Reading::at(RESETS_23 + 10.0), &chats, false);
    let mut names: Vec<&str> = found.iter().map(|found| found.session.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["madre", "sospesa"]);
    let suspended = found.iter().find(|found| found.session == "sospesa").unwrap();
    assert_eq!((suspended.analysis.state, suspended.analysis.resets.and_then(suspension::iso).as_deref(), suspended.due), (State::Suspended, Some("2026-10-21T21:00:00Z"), true));
    assert_eq!(suspended.chat.as_deref(), Some("claude_o_1"));
    let parent = found.iter().find(|found| found.session == "madre").unwrap();
    assert_eq!(parent.subagents.len(), 1);
    assert_eq!(parent.analysis.state, State::Active);
}
