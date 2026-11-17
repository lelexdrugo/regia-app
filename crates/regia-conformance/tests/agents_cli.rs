//! The agent commands on the built binary (#460/V12): `launch` that stops with exit 1 and a line on
//! stderr, and `agents show` with a service, as a process answers them: exit code, stdout and stderr.
//! The unit tests of `regia-cli` build the request and the assignment; these start `regia` itself.
//!
//! Nothing here starts an agent: `launch` runs on the `dry` surface or refuses before any surface.

use std::fs;
use std::path::PathBuf;

use regia_conformance::{prepare_state_dir, regia_binary};
use regia_exec::{Launch, harness};
use regia_identity::{config_dir, prototype_env_var};
use serde_json::Value;

/// The binary on `args`, in a folder of its own under `target/`, with the user's configuration folder
/// moved there too where the system lets an environment variable do it (it does not on Windows, see
/// [`settings_may_apply`]).
fn regia(label: &str, args: &[&str]) -> (Launch, PathBuf) {
    let state = prepare_state_dir(label);
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let launch = harness::launch(&regia_binary(), &args, &state)
        .env(&prototype_env_var("RUNTIME"), &state)
        .env("HOME", &state)
        .env("USERPROFILE", &state)
        .env("XDG_CONFIG_HOME", state.join("config"));
    (launch, state)
}

/// Whether the user's own `agents.toml` can change what the binary answers: on Windows the configuration
/// folder is a known folder, which no variable moves. `agents.toml` is the name `regia-agents` gives it.
fn settings_may_apply() -> bool {
    cfg!(windows) && config_dir().is_some_and(|dir| dir.join("agents.toml").exists())
}

fn run(label: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let (launch, _) = regia(label, args);
    let output = launch.run().expect("run regia");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// `launch` that refuses stops with exit 1, a line `regia: <reason>` on stderr, and nothing on stdout:
/// no receipt, since nothing started.
#[test]
fn launch_that_refuses_exits_1_with_its_reason_on_stderr() {
    let missing = prepare_state_dir("agents-launch-missing").join("no-such-folder");
    let missing = missing.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run(
        "agents-launch-folder",
        &[
            "launch",
            "--project",
            "trial",
            "--folder",
            &missing,
            "--prompt",
            "p",
            "--surface",
            "dry",
        ],
    );
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stdout.is_empty()
            && stderr.contains("the folder does not exist")
            && !stderr.contains("panicked"),
        "{stdout} | {stderr}"
    );

    let folder = prepare_state_dir("agents-launch-work");
    let folder = folder.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run(
        "agents-launch-label",
        &[
            "launch",
            "--project",
            "Trial X",
            "--folder",
            &folder,
            "--prompt",
            "p",
            "--surface",
            "dry",
        ],
    );
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stdout.is_empty() && stderr.contains("invalid project id"),
        "{stdout} | {stderr}"
    );

    let (code, stdout, stderr) = run(
        "agents-launch-blank",
        &[
            "launch",
            "--project",
            "trial",
            "--folder",
            &folder,
            "--prompt",
            "  ",
            "--surface",
            "dry",
        ],
    );
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stdout.is_empty() && stderr.contains("empty prompt"),
        "{stdout} | {stderr}"
    );

    let absent = format!("{folder}/no-prompt.md");
    let (code, stdout, stderr) = run(
        "agents-launch-file",
        &[
            "launch",
            "--project",
            "trial",
            "--folder",
            &folder,
            "--prompt-file",
            &absent,
            "--surface",
            "dry",
        ],
    );
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stdout.is_empty() && stderr.contains("no-prompt.md"),
        "{stdout} | {stderr}"
    );
}

/// The same command, accepted, on `dry`: exit 0 and one receipt on stdout that says nothing started.
/// The prompt file with CRLF is read as the prototype reads it, so the command takes it as it takes any.
#[test]
fn launch_on_the_dry_surface_prints_its_receipt() {
    let folder = prepare_state_dir("agents-launch-dry");
    let prompt = folder.join("prompt.md");
    fs::write(&prompt, "do the step\r\nline two\r\n").unwrap();
    let (folder, prompt) = (
        folder.to_string_lossy().into_owned(),
        prompt.to_string_lossy().into_owned(),
    );
    let (code, stdout, stderr) = run(
        "agents-launch-dry-run",
        &[
            "launch",
            "--project",
            "trial",
            "--folder",
            &folder,
            "--prompt-file",
            &prompt,
            "--surface",
            "dry",
            "--model",
            "haiku",
            "--effort",
            "low",
        ],
    );
    assert_eq!(code, Some(0), "{stderr}");
    let receipt: Value =
        serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{error}: {stdout}"));
    assert_eq!(
        (
            receipt["surface"].as_str(),
            receipt["model"].as_str(),
            receipt["effort"].as_str(),
            receipt["pid"].is_null()
        ),
        (Some("dry"), Some("haiku"), Some("low"), true),
        "{receipt}"
    );
}

/// `agents show` for a role and the service of the work's writer: exit 0, the contract's keys in JSON, and
/// the role asked for. With the settings as they come, the reviewer of work by codex is another service
/// than codex; with a settings file of the user's own on Windows the keys are what is proved.
#[test]
fn agents_show_with_a_writer_answers_with_the_contract_keys() {
    let (code, stdout, stderr) = run(
        "agents-show-json",
        &[
            "agents",
            "show",
            "--role",
            "adversarial_review",
            "--writer",
            "codex",
            "--json",
        ],
    );
    assert_eq!(code, Some(0), "{stderr}");
    let assignment: Value =
        serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{error}: {stdout}"));
    for key in [
        "role",
        "service",
        "tier",
        "model",
        "effort",
        "surface",
        "deadline_minutes",
        "clean_context",
        "diversity",
        "fallback",
    ] {
        assert!(assignment.get(key).is_some(), "{key}: {assignment}");
    }
    assert_eq!(assignment["role"], "adversarial_review");
    // Whatever the user's settings: the role asks for another service than the writer's, so the
    // diversity is met (and the service is not the writer's) or degraded; never "not required", which is
    // what an assignment made without the writer says.
    let diversity = assignment["diversity"].as_str().unwrap_or_default();
    assert!(matches!(diversity, "met" | "degraded"), "{assignment}");
    assert!(
        diversity != "met" || assignment["service"] != "codex",
        "{assignment}"
    );
    if !settings_may_apply() {
        assert_eq!(
            (
                assignment["service"].as_str(),
                assignment["diversity"].as_str()
            ),
            (Some("claude"), Some("met")),
            "{assignment}"
        );
    }

    let (code, stdout, stderr) = run(
        "agents-show-text",
        &[
            "agents",
            "show",
            "--role",
            "adversarial_review",
            "--writer",
            "claude",
        ],
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stdout
            .lines()
            .any(|line| line == "role: adversarial_review")
            && stdout.lines().any(|line| line.starts_with("service: ")),
        "{stdout}"
    );
    if !settings_may_apply() {
        assert!(
            stdout.lines().any(|line| line == "service: codex"),
            "{stdout}"
        );
    }
}

/// A service the product does not know, or a role it does not have, is refused by the arguments before
/// anything is answered: exit 2, the reason on stderr, nothing on stdout.
#[test]
fn agents_show_refuses_an_unknown_service_or_role() {
    for (label, args) in [
        (
            "agents-show-service",
            ["agents", "show", "--role", "code", "--writer", "gemini"],
        ),
        (
            "agents-show-role",
            ["agents", "show", "--role", "nobody", "--writer", "codex"],
        ),
    ] {
        let (code, stdout, stderr) = run(label, &args);
        assert_eq!(code, Some(2), "{args:?}: {stderr}");
        assert!(
            stdout.is_empty() && !stderr.is_empty() && !stderr.contains("panicked"),
            "{args:?}: {stdout} | {stderr}"
        );
    }
}
