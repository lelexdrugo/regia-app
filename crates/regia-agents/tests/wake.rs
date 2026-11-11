//! The token wake-up on the fake claude of `examples/`: the smallest session, in the state folder,
//! with claude's allow-list, its answer read from the last JSON line, and every failure a value. The
//! cases are the prototype's `TestSvegliaClaude` and the fake wake-up of an injected launcher; the
//! report of #462 maps each to its test here. No surface here can reach the real claude: each is
//! pinned to the fake or searches an empty folder.

mod common;

use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use common::{Trial, args, env_of};
use regia_agents::{Cli, Env, LaunchSurface, ProbeConsent, SurfaceKind, WAKE_PROMPT, WakeUp, Waits, surface};

/// The argv of the wake-up: `claude::wake` of regia-exec, every token as the fake received it.
const ARGV: [&str; 9] = [
    "-p",
    "--model=haiku",
    "--effort=low",
    "--safe-mode",
    "--tools=",
    "--system-prompt=Reply-with-one-word.",
    "--no-session-persistence",
    "--max-turns=1",
    "--output-format=json",
];

fn same_place(a: &Path, b: &Path) -> bool {
    fs::canonicalize(a).expect("a") == fs::canonicalize(b).expect("b")
}

/// The prototype's `test_comando_minimo_e_esito_ok`: the fixed argv and the prompt on stdin, in the
/// state folder, with claude's folder as the surface has it and nothing of the session that started
/// the core; the answer gives ok, the exit and the cost. Nothing is kept, and no log is written.
#[test]
fn the_wake_up_is_the_smallest_session_and_its_answer_is_read() {
    let trial = Trial::new("wake");
    let outcome = trial.cli().wake();
    assert!(outcome.ok && !outcome.fake, "{outcome:?}");
    assert_eq!((outcome.exit, outcome.cost_usd, outcome.error.as_deref()), (Some(0), Some(0.00015), None));
    assert!(outcome.duration_s.is_some_and(|seconds| seconds >= 0.0));

    let record = trial.record("claude", 0);
    assert_eq!(args(&record), ARGV);
    assert_eq!(record["stdin"], WAKE_PROMPT, "the prompt is on stdin, not in argv");
    assert!(same_place(Path::new(record["cwd"].as_str().unwrap()), &trial.state), "{}", record["cwd"]);
    let env = env_of(&record);
    assert_eq!(env.get("CLAUDE_CONFIG_DIR").map(Path::new), Some(trial.claude_home().as_path()));
    // This process may run under an agent: those variables are really there to leave out.
    for name in env.keys() {
        assert!(!["CLAUDECODE", "CLAUDE_CODE_", "ORCA_"].iter().any(|prefix| name.starts_with(prefix)), "the wake-up sees {name}");
    }
    assert!(!env.contains_key("REGIA_RUNTIME") && !env.contains_key("PYTHONUTF8"), "claude's allow-list only: {env:?}");
    assert!(!trial.claude_home().join("projects").exists(), "--no-session-persistence keeps nothing");
    assert!(trial.steps().is_empty(), "a wake-up is not a step: {:?}", trial.steps());
}

/// The prototype's `test_errore_di_claude`.
#[test]
fn an_error_of_claude_is_in_the_outcome() {
    let trial = Trial::new("wake-expired");
    trial.mode("expired");
    let outcome = trial.cli().wake();
    assert!(!outcome.ok && outcome.exit == Some(1), "{outcome:?}");
    assert_eq!(outcome.error.as_deref(), Some("OAuth token has expired"));
}

/// The prototype's `test_timeout_e_eseguibile_mancante_non_sollevano`: a wake-up that does not end in
/// time is killed, and a claude that is not there starts nothing; both are values.
#[test]
fn a_timeout_and_a_missing_program_are_values() {
    let trial = Trial::new("wake-late");
    trial.mode("slow");
    let waits = Waits { session: Duration::from_secs(20), transcript: Duration::from_secs(4), wake: Duration::from_secs(1), tick: Duration::from_millis(50) };
    let outcome = trial.cli().with_waits(waits).wake();
    assert!(!outcome.ok && outcome.exit.is_none(), "{outcome:?}");
    assert!(outcome.error.as_deref().is_some_and(|error| error.contains("did not finish within 1 s")), "{outcome:?}");
    assert!(outcome.duration_s.is_some_and(|seconds| seconds < 8.0), "killed at the deadline, not at the end: {outcome:?}");

    let missing = Trial::new("wake-missing");
    let outcome = Cli::new(missing.env()).with_state(missing.state.clone()).with_search(missing.nowhere()).wake();
    assert!(!outcome.ok && outcome.exit.is_none(), "{outcome:?}");
    assert!(outcome.error.as_deref().is_some_and(|error| error.contains("not found")), "{outcome:?}");
    assert!(!missing.started("claude"));
}

/// The prototype's `test_con_un_lanciatore_iniettato_la_sveglia_e_finta`: the wake-up follows the
/// launches, so a round on a surface that starts nothing records a fake wake-up and starts nothing.
#[test]
fn with_an_injected_surface_the_wake_up_is_fake() {
    let dry = surface(SurfaceKind::Dry, Env::default());
    let mut wake_up = WakeUp::new(ProbeConsent::Granted);
    let now = Utc.with_ymd_and_hms(2026, 10, 23, 0, 30, 0).unwrap();
    let attempt = wake_up.round(true, now, || dry.wake()).cloned().expect("a wake-up");
    assert!(attempt.outcome.fake && !attempt.outcome.ok, "{attempt:?}");
    assert_eq!(serde_json::to_value(&attempt).unwrap()["fake"], true, "the round's log says so");

    let trial = Trial::new("wake-injected");
    assert!(!trial.cli().wake().fake, "the Cli surface wakes for real");
    assert!(!trial.started("orca") && trial.started("claude"));
}

/// npm installs claude as `claude.cmd`, and `cmd.exe` reads its arguments again: every argument of
/// the wake-up reaches the program whole, and the prompt still comes on stdin.
#[cfg(windows)]
#[test]
fn the_wake_up_through_a_cmd_shim_gets_every_argument_whole() {
    let trial = Trial::new("wake-cmd-shim");
    let shim = trial.dir.join("shim");
    fs::create_dir_all(&shim).unwrap();
    let script = shim.join("claude.cmd");
    fs::write(&script, format!("@\"{}\" %*\r\n", trial.bin.join(common::exe("claude")).display())).unwrap();
    let pin = regia_exec::Pin::at(regia_exec::Program::Claude, &script).unwrap();
    let outcome = trial.cli().with_pin(regia_agents::ServiceId::Claude, pin).wake();
    assert!(outcome.ok, "{outcome:?}");
    let record = trial.record("claude", 0);
    assert_eq!(args(&record), ARGV);
    assert_eq!(record["stdin"], WAKE_PROMPT);
}
