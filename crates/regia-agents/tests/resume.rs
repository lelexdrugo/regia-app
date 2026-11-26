//! The resume of a session the usage limit stopped (#470), on the fake agent of `examples/`: the prototype's cases of
//! `RipresaCli` in `tools/tests/test_sospese.py` on the Cli surface, and the search of Orca's index of its chats
//! (`test_cerca_orca_mappa_le_chat_e_le_sessioni_cli`) on the fake Orca. Every program is the fake, never a real agent
//! or a real Orca.

mod common;

use std::fs;
use std::path::Path;
use std::time::Duration;

use common::{SESSION, Trial, args, env_of};
use regia_agents::suspension::CONTINUE;
use regia_agents::{ChatHost, Known, OrcaChats, ResumeRequest, ServiceId};
use regia_exec::{Effort, ModelId, Program};
use serde_json::json;

fn same_place(a: &Path, b: &Path) -> bool {
    fs::canonicalize(a).expect("a") == fs::canonicalize(b).expect("b")
}

fn request(trial: &Trial, service: ServiceId) -> ResumeRequest {
    ResumeRequest::new(service, "trial", &trial.folder, SESSION).expect("a valid request")
}

/// The first case of `RipresaCli`, the argv of a resume: what each agent gets, the session joined to `--resume` for claude and a
/// positional before stdin's `-` for codex, with the model and the effort the registry has.
#[test]
fn claude_and_codex_get_the_resume_s_argv() {
    let trial = Trial::new("resume-argv");
    let haiku = Some(ModelId::parse("haiku").expect("a model"));
    trial.cli().resume(&request(&trial, ServiceId::Claude).model(haiku.clone()).effort(Some(Effort::Low))).expect("the resume of claude");
    assert_eq!(
        args(&trial.record("claude", 0)),
        ["-p", &format!("--resume={SESSION}"), "--output-format=stream-json", "--verbose", "--dangerously-skip-permissions", "--model=haiku", "--effort=low"]
    );
    trial.cli().resume(&request(&trial, ServiceId::Codex)).expect("the resume of codex");
    let codex = args(&trial.record("codex", 0));
    assert_eq!(codex[..3], ["exec", "resume", "--json"]);
    assert_eq!(codex[codex.len() - 2..], [SESSION, "-"], "the session, then what it is told from stdin");
}

/// The second case of `RipresaCli`: claude resumed on its own, with its log, told to go on on stdin.
#[test]
fn claude_is_resumed_on_its_own_with_its_log_and_continue_on_stdin() {
    let trial = Trial::new("resume-claude");
    let resumed = trial.cli().resume(&request(&trial, ServiceId::Claude)).expect("the resume");
    assert_eq!(resumed.session, SESSION);
    assert!(resumed.pid > 0);
    trial.wait_for(|| trial.started("claude"), Duration::from_secs(10));
    let record = trial.record("claude", 0);
    assert_eq!(args(&record)[..2], ["-p".to_owned(), format!("--resume={SESSION}")]);
    assert_eq!(record["stdin"], CONTINUE);
    assert!(same_place(Path::new(record["cwd"].as_str().expect("a folder")), &trial.folder));
    let env = env_of(&record);
    assert!(!env.keys().any(|name| name.starts_with("ORCA_") || name.starts_with("CLAUDE_CODE_")), "the environment is scrubbed: {env:?}");
    assert!(resumed.log.ends_with("-ripresa.log"), "{}", resumed.log);
    trial.wait_for(|| fs::read_to_string(&resumed.log).is_ok_and(|log| log.contains("session_id")), Duration::from_secs(10));
    assert!(resumed.transcript.as_deref().is_some_and(|transcript| Path::new(transcript).is_file()), "{:?}", resumed.transcript);
}

/// The third case of `RipresaCli`: a resume that forks says its new session.
#[test]
fn a_resume_that_forks_names_its_new_session() {
    let trial = Trial::new("resume-fork");
    trial.mode("fork");
    let resumed = trial.cli().resume(&request(&trial, ServiceId::Claude)).expect("the resume");
    assert_eq!(resumed.session, "bbbbbbbb-1111-2222-3333-444444444444");
}

/// The fourth case of `RipresaCli`, a folder that does not exist, and an id that is not an agent's: refused before any
/// effect.
#[test]
fn a_session_without_its_folder_or_with_a_strange_id_is_not_resumed() {
    let trial = Trial::new("resume-refused");
    let missing = ResumeRequest::new(ServiceId::Claude, "trial", &trial.dir.join("no"), SESSION).expect_err("no folder");
    assert!(missing.before_effect() && missing.to_string().contains("does not exist"), "{missing}");
    for session in ["--resume=x", "a b", "", "x;y", &"a".repeat(129)] {
        let refused = ResumeRequest::new(ServiceId::Claude, "trial", &trial.folder, session).expect_err(session);
        assert!(refused.before_effect(), "{session}: {refused}");
    }
    assert!(!trial.started("claude"));
}

/// `test_cerca_orca_mappa_le_chat_e_le_sessioni_cli`: the hits of claude, page after page, each with its chat when Orca
/// has one; an answer that is not `ok` is no answer.
#[test]
fn orca_s_index_maps_its_chats_and_the_sessions_of_the_command_line() {
    let trial = Trial::new("orca-search");
    let pages = json!([
        {"hits": [{"agent": "claude", "sessionId": "u1", "structuredSession": {"sessionId": "claude_a_b"}},
                  {"agent": "claude", "sessionId": "u2"}, {"agent": "codex", "sessionId": "u3"}],
         "page": {"hasMore": true, "cursor": "c1"}},
        {"hits": [{"agent": "claude", "sessionId": "u4"}], "page": {"hasMore": false}},
    ]);
    fs::write(trial.bin.join("orca-search.json"), pages.to_string()).expect("the pages");
    let chats = OrcaChats::new().with_pin(trial.pin(Program::Orca, "orca"));
    let known = chats.known(&trial.folder.display().to_string()).expect("an answer");
    assert_eq!(known, Known::from([("u1".to_owned(), Some("claude_a_b".to_owned())), ("u2".to_owned(), None), ("u4".to_owned(), None)]));
    let calls = trial.orca_calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].contains(&"--cursor=c1".to_owned()), "{:?}", calls[1]);
    assert!(calls[0].contains(&"--query=hit your session limit".to_owned()) && calls[0].contains(&"--json".to_owned()), "{:?}", calls[0]);
    trial.orca_mode("down");
    assert_eq!(chats.known(&trial.folder.display().to_string()), None, "Orca down is no answer");
}
