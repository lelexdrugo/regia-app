//! The Orca surface and `Auto` on the fake Orca of `examples/`: the checks before any effect, the
//! prompt as an argument, the effort rules, the agent's session found by its marker, and the fallback
//! to `Cli`. The cases are those of the prototype's launcher with Orca; the report of #462 maps each
//! to its test here. Every surface is pinned to a fake, or searches an empty folder: the real `orca`
//! may be installed on the machine that runs the tests.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{PROMPT, SESSION, Trial};
use regia_agents::{Auto, LaunchSurface, Orca, PROMPT_IN_ARGV_MAX, ServiceId, SurfaceKind, launch_with_fallback};
use regia_exec::{Effort, ModelId, Search};

fn model(name: &str) -> Option<ModelId> {
    Some(ModelId::parse(name).expect("a model"))
}

/// The folder as Orca's selector names it, with `/`.
fn selector(folder: &Path) -> String {
    format!("--worktree=path:{}", folder.display().to_string().replace('\\', "/"))
}

/// Orca where it is not installed: a search of an empty folder, never this machine's `orca`.
fn missing_orca(trial: &Trial) -> Orca {
    let empty = trial.dir.join("empty");
    fs::create_dir_all(&empty).unwrap();
    Orca::new(trial.env()).with_state(trial.state.clone()).with_search(Search::from_parts(Some(empty.as_os_str()), Vec::new(), &[]))
}

/// Acceptance 1 of #462: without `orca`, a launch on the Orca surface fails before any effect, having
/// written nothing, and the caller falls back to `Cli`.
#[test]
fn without_orca_the_launch_fails_before_any_effect_and_the_caller_falls_back_to_cli() {
    let trial = Trial::new("orca-missing");
    let orca = missing_orca(&trial);
    let error = orca.launch(&trial.request(ServiceId::Claude)).unwrap_err();
    assert!(error.before_effect(), "{error}");
    assert!(error.to_string().contains("not available") && error.to_string().contains("not found"), "{error}");
    assert!(trial.steps().is_empty(), "nothing was written: {:?}", trial.steps());

    let receipt = launch_with_fallback(&orca, &trial.cli(), &trial.request(ServiceId::Claude)).expect("the fallback");
    assert_eq!((receipt.surface, receipt.session.as_deref()), (SurfaceKind::Cli, Some(SESSION)));
    assert!(receipt.warnings.iter().any(|warning| warning.starts_with("orca unavailable, used cli: Orca is not available")), "{:?}", receipt.warnings);
    let auto = Auto::new(missing_orca(&trial), trial.cli()).launch(&trial.request(ServiceId::Claude)).expect("auto");
    assert_eq!(auto.surface, SurfaceKind::Cli);
}

/// The prototype's `test_orca`: Orca gets the agent, the folder, the model and the effort, and the
/// prompt as one argument; the receipt has the agent's session, found by the marker, and Orca's.
#[test]
fn orca_launches_the_agent_and_the_receipt_finds_its_session() {
    let trial = Trial::new("orca");
    let request = trial.request(ServiceId::Claude).model(model("haiku")).effort(Some(Effort::Low));
    let receipt = trial.orca().launch(&request).expect("the launch");
    assert_eq!((receipt.surface, receipt.service, receipt.model.as_deref(), receipt.effort), (SurfaceKind::Orca, ServiceId::Claude, Some("haiku"), Some("low")));
    assert_eq!(receipt.orca_session.as_deref(), Some("claude_orca_1"));
    assert_eq!(receipt.session.as_deref(), Some(SESSION), "the agent's own session, from the new session file");
    assert!(receipt.transcript.as_deref().is_some_and(|path| path.ends_with(&format!("{SESSION}.jsonl"))), "{:?}", receipt.transcript);
    assert!(receipt.transcript_exists);
    assert_eq!(receipt.pid, None);
    assert!(PathBuf::from(receipt.log.clone().unwrap()).is_file());
    assert_eq!(receipt.visible.as_deref(), Some("structured"));
    assert_eq!(receipt.resume.as_deref(), Some(format!("claude --resume {SESSION}").as_str()));
    assert_eq!(receipt.workspace, Some(format!("repo::path:{}", request.folder.display().to_string().replace('\\', "/"))));
    assert_eq!(receipt.marker.as_deref(), Some(PROMPT.chars().take(60).collect::<String>().as_str()));
    assert!(receipt.warnings.is_empty(), "{:?}", receipt.warnings);

    let calls = trial.orca_calls();
    assert_eq!(calls[0], ["status", "--json"]);
    assert_eq!(calls[1], ["worktree", "show", &selector(&request.folder), "--json"]);
    let launches = trial.agent_launches();
    assert_eq!(launches.len(), 1);
    let expected = ["agent", "launch", "--agent=claude", &selector(&request.folder), &format!("--prompt={PROMPT}"), "--model=haiku", "--effort=low", "--json"];
    assert_eq!(launches[0], expected, "the prompt is the one argument the exception allows");
    let log: serde_json::Value = serde_json::from_slice(&fs::read(receipt.log.unwrap()).unwrap()).unwrap();
    assert_eq!(log["launch"]["outcome"]["sessionId"], "claude_orca_1");
}

#[test]
fn an_unreachable_orca_fails_before_any_effect() {
    let trial = Trial::new("orca-unreachable");
    trial.orca_mode("unreachable");
    let error = trial.orca().launch(&trial.request(ServiceId::Claude)).unwrap_err();
    assert!(error.before_effect() && error.to_string().contains("reachable"), "{error}");
    assert_eq!(trial.orca_calls().len(), 2, "two tries of `orca status`");
    assert!(trial.agent_launches().is_empty() && trial.steps().is_empty());
}

#[test]
fn a_folder_that_is_not_a_workspace_of_orca_fails_before_any_effect() {
    let trial = Trial::new("orca-not-a-workspace");
    trial.orca_mode("not-a-workspace");
    let error = trial.orca().launch(&trial.request(ServiceId::Claude)).unwrap_err();
    assert!(error.before_effect() && error.to_string().contains("workspace"), "{error}");
    assert!(error.to_string().contains("selector_not_found"), "Orca's own reason: {error}");
    assert!(trial.agent_launches().is_empty() && trial.steps().is_empty());
}

/// Once `orca agent launch` has run, a refusal is after an effect: the session may exist.
#[test]
fn a_refused_launch_is_after_an_effect() {
    let trial = Trial::new("orca-launch-fails");
    trial.orca_mode("launch-fails");
    let error = trial.orca().launch(&trial.request(ServiceId::Claude)).unwrap_err();
    assert!(!error.before_effect() && error.to_string().contains("launch refused"), "{error}");
}

/// A model that takes no effort: Orca refuses before it creates anything, and the surface launches
/// once more without it.
#[test]
fn an_effort_the_model_does_not_take_is_dropped_and_launched_again() {
    let trial = Trial::new("orca-no-effort");
    trial.orca_mode("no-effort");
    let receipt = trial.orca().launch(&trial.request(ServiceId::Claude).model(model("haiku")).effort(Some(Effort::Low))).expect("the launch");
    assert_eq!(receipt.effort, None);
    assert!(receipt.warnings.iter().any(|warning| warning.contains("effort")), "{:?}", receipt.warnings);
    let launches = trial.agent_launches();
    assert_eq!(launches.len(), 2);
    assert!(launches[0].contains(&"--effort=low".to_owned()) && !launches[1].iter().any(|arg| arg.starts_with("--effort")));
}

/// Orca refuses an effort without a model (#383): the surface does not pass it.
#[test]
fn an_effort_without_a_model_is_not_passed_to_orca() {
    let trial = Trial::new("orca-effort-alone");
    let receipt = trial.orca().launch(&trial.request(ServiceId::Claude).effort(Some(Effort::Low))).expect("the launch");
    assert_eq!(receipt.effort, None);
    assert!(receipt.warnings.iter().any(|warning| warning.contains("without a model")), "{:?}", receipt.warnings);
    let launches = trial.agent_launches();
    assert_eq!(launches.len(), 1);
    assert!(!launches[0].iter().any(|arg| arg.starts_with("--effort")), "{:?}", launches[0]);
}

/// The limit of the exception: a long prompt goes in a file next to the log, byte for byte, and the
/// argument points to it.
#[test]
fn a_long_prompt_goes_in_a_file() {
    let trial = Trial::new("orca-long");
    let long = "x".repeat(PROMPT_IN_ARGV_MAX + 10);
    let request = regia_agents::LaunchRequest::new(ServiceId::Claude, "trial", &trial.folder, &long).unwrap();
    let receipt = trial.orca().launch(&request).expect("the launch");
    let launch = &trial.agent_launches()[0];
    let text = launch.iter().find_map(|arg| arg.strip_prefix("--prompt=")).expect("the prompt");
    assert!(text.len() < 400, "{text}");
    let file = PathBuf::from(text.strip_prefix("Read the file ").and_then(|rest| rest.split(" and follow").next()).expect("the file"));
    assert_eq!(fs::read_to_string(&file).unwrap(), long);
    assert_eq!(file.parent(), PathBuf::from(receipt.log.unwrap()).parent());
    assert_eq!(receipt.marker.as_deref(), Some(text.chars().take(60).collect::<String>().as_str()), "the marker is the start of what the agent got");
    assert_eq!(receipt.session.as_deref(), Some(SESSION), "found by the pointer's start");
}

/// #509: Orca first starts a short session that names the chat and quotes the prompt; the surface
/// takes the one that goes on, whose first message starts with the prompt.
#[test]
fn with_orca_naming_session_the_one_that_goes_on_is_taken() {
    let trial = Trial::new("orca-naming");
    trial.orca_mode("naming");
    let receipt = trial.orca().launch(&trial.request(ServiceId::Claude)).expect("the launch");
    assert_eq!(receipt.session.as_deref(), Some(SESSION));
    assert!(receipt.transcript.as_deref().is_some_and(|path| path.ends_with(&format!("{SESSION}.jsonl"))));
    assert_eq!(receipt.resume.as_deref(), Some(format!("claude --resume {SESSION}").as_str()));
    assert!(receipt.warnings.is_empty(), "{:?}", receipt.warnings);
}

/// A file that only quotes the prompt is not the session: the receipt keeps Orca's id, and says so.
#[test]
fn only_the_naming_session_is_not_certain() {
    let trial = Trial::new("orca-naming-only");
    trial.orca_mode("naming-only");
    let receipt = trial.orca_waiting(Duration::from_millis(300)).launch(&trial.request(ServiceId::Claude)).expect("the launch");
    assert_eq!(receipt.session.as_deref(), Some("claude_orca_1"));
    assert_eq!((receipt.transcript, receipt.transcript_exists, receipt.resume), (None, false, None));
    assert!(receipt.warnings.iter().any(|warning| warning.contains("without starting with it")), "{:?}", receipt.warnings);
}

#[test]
fn without_a_session_file_the_receipt_keeps_orca_id() {
    let trial = Trial::new("orca-no-session");
    trial.orca_mode("no-session");
    let receipt = trial.orca_waiting(Duration::from_millis(300)).launch(&trial.request(ServiceId::Claude)).expect("the launch");
    assert_eq!(receipt.session.as_deref(), Some("claude_orca_1"));
    assert_eq!(receipt.transcript, None);
    assert!(receipt.warnings.iter().any(|warning| warning.contains("no session file found")), "{:?}", receipt.warnings);
}

/// Codex has no first message to read: its session is the new file that holds the marker.
#[test]
fn a_codex_session_is_found_by_its_marker_too() {
    let trial = Trial::new("orca-codex");
    let receipt = trial.orca().launch(&trial.request(ServiceId::Codex)).expect("the launch");
    assert_eq!(receipt.session.as_deref(), Some(SESSION));
    assert_eq!(receipt.resume.as_deref(), Some(format!("codex resume {SESSION}").as_str()));
    assert!(trial.agent_launches()[0].contains(&"--agent=codex".to_owned()));
}

/// The prototype's `Auto`: Orca when it is reachable and the folder is one of its workspaces;
/// otherwise `Cli`, before any effect, with the reason in the receipt.
#[test]
fn auto_takes_orca_when_it_can_and_cli_otherwise() {
    let trial = Trial::new("auto");
    let auto = || Auto::new(trial.orca(), trial.cli());
    assert_eq!(auto().launch(&trial.request(ServiceId::Claude)).unwrap().surface, SurfaceKind::Orca);
    assert!(!trial.started("claude"));
    for (modes, reason) in [("unreachable", "reachable"), ("down", "not JSON"), ("not-a-workspace", "workspace")] {
        trial.orca_mode(modes);
        let receipt = auto().launch(&trial.request(ServiceId::Claude)).expect("the fallback");
        assert_eq!(receipt.surface, SurfaceKind::Cli, "{modes}");
        assert!(receipt.warnings.iter().any(|warning| warning.starts_with("orca unavailable, used cli") && warning.contains(reason)), "{modes}: {:?}", receipt.warnings);
    }
}

/// A launch that Orca refused after it ran is not launched again on `Cli`: it may exist.
#[test]
fn a_failed_orca_launch_does_not_fall_back_to_cli() {
    let trial = Trial::new("auto-launch-fails");
    trial.orca_mode("launch-fails");
    let error = Auto::new(trial.orca(), trial.cli()).launch(&trial.request(ServiceId::Claude)).unwrap_err();
    assert!(!error.before_effect(), "{error}");
    assert!(!trial.started("claude"), "the Cli surface did not run");
}

/// Everything about Orca stays in this crate and in the builders of regia-exec: no other product
/// source names it. Outside the scan, on purpose: regia-agents (the adapter); regia-exec (its closed
/// list of programs and their argv, threat model §3.6, rule 1); regia-conformance (the harness names
/// the prototype's variables); the facade's unit tests (the prototype's keys); the catalogs, the
/// facade table, fixtures and documents, which are not sources.
#[test]
fn no_other_product_source_names_orca() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("the workspace").to_path_buf();
    let skipped = [root.join("crates").join("regia-agents"), root.join("crates").join("regia-exec"), root.join("crates").join("regia-conformance")];
    let facade = root.join("crates").join("regia-facade").join("src").join("lib.rs");
    let mut sources = Vec::new();
    rust_sources(&root.join("crates"), &mut sources);
    rust_sources(&root.join("app"), &mut sources);
    assert!(sources.iter().any(|path| path.ends_with(Path::new("regia-cli").join("src").join("main.rs"))), "the scan reads the sources");
    let offenders: Vec<String> = sources
        .iter()
        .filter(|path| !skipped.iter().any(|skipped| path.starts_with(skipped)))
        .filter(|path| path.components().any(|part| part.as_os_str() == "src"))
        .filter(|path| {
            let text = fs::read_to_string(path).expect("a source").to_lowercase();
            let code = if **path == facade { text.split("#[cfg(test)]").next().unwrap_or_default().to_owned() } else { text };
            code.contains("orca")
        })
        .map(|path| path.display().to_string())
        .collect();
    assert!(offenders.is_empty(), "these name Orca outside its adapter: {offenders:?}");
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !["target", "gen", "node_modules"].iter().any(|skip| name == *skip) {
                rust_sources(&path, out);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}
