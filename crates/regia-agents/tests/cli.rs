//! The Cli surface on the fake agent of `examples/`: argv, prompt, folder, log, session, transcript
//! and receipt, and the failures before and after the start. The cases are those of the prototype's
//! launcher without Orca; the report of #460 maps each to its test there.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{PROMPT, SESSION, Trial, args, env_of};
use regia_agents::{
    Cli, LaunchError, LaunchRequest, LaunchSurface, ServiceId, SurfaceKind, claude_slug,
};
#[cfg(windows)]
use regia_exec::Program;
use regia_exec::{Effort, ModelId, Search};

/// An argv as `(option, value)` pairs and positionals, whichever form it uses: `--name=value`, or
/// `--name value` and the short names the prototype gives codex. A flag has no value.
fn options(argv: &[String]) -> (Vec<(String, Option<String>)>, Vec<String>) {
    const VALUED: [&str; 6] = ["--output-format", "--model", "--effort", "--cd", "--output-last-message", "--config"];
    let long = |name: &str| match name {
        "-C" => "--cd".to_owned(),
        "-m" => "--model".to_owned(),
        "-c" => "--config".to_owned(),
        other => other.to_owned(),
    };
    let (mut pairs, mut positionals) = (Vec::new(), Vec::new());
    let mut tokens = argv.iter();
    while let Some(token) = tokens.next() {
        if token == "-" || !token.starts_with('-') {
            positionals.push(token.clone());
        } else if let Some((name, value)) = token.split_once('=').filter(|_| token.starts_with("--")) {
            pairs.push((name.to_owned(), Some(value.to_owned())));
        } else {
            let name = long(token);
            let value = VALUED.contains(&name.as_str()).then(|| tokens.next().cloned()).flatten();
            pairs.push((name, value));
        }
    }
    pairs.sort();
    (pairs, positionals)
}

/// A path as either side writes it: `/` for `\`.
fn slashes(path: &str) -> String {
    path.replace('\\', "/")
}

fn same_place(a: &Path, b: &Path) -> bool {
    fs::canonicalize(a).expect("a") == fs::canonicalize(b).expect("b")
}

fn model(name: &str) -> Option<ModelId> {
    Some(ModelId::parse(name).expect("a model"))
}

/// Acceptance 1 of #460: the fake claude gets the options and the prompt the prototype gives it, the
/// prompt on stdin and never in argv; the receipt says where the session, its transcript and its log
/// are, and how to resume it.
#[test]
fn claude_gets_the_prototype_argv_and_the_prompt_on_stdin() {
    let trial = Trial::new("claude");
    let request = trial.request(ServiceId::Claude).model(model("haiku")).effort(Some(Effort::Low));
    let receipt = trial.launch(&request).expect("the launch");
    assert_eq!((receipt.surface, receipt.service, receipt.model.as_deref(), receipt.effort), (SurfaceKind::Cli, ServiceId::Claude, Some("haiku"), Some("low")));
    assert_eq!(receipt.session.as_deref(), Some(SESSION));
    assert!(receipt.transcript_exists);
    let transcript = trial.claude_home().join("projects").join(claude_slug(&request.folder)).join(format!("{SESSION}.jsonl"));
    assert_eq!(receipt.transcript.as_deref().map(PathBuf::from), Some(transcript));
    assert!(receipt.pid.is_some());
    assert_eq!(receipt.resume.as_deref(), Some(format!("claude --resume {SESSION}").as_str()));
    let started = receipt.started.as_bytes();
    assert!(started.len() >= 11 && started[..4].iter().all(u8::is_ascii_digit) && started[10] == b'T', "{}", receipt.started);
    let log = PathBuf::from(receipt.log.clone().expect("a log"));
    assert_eq!(log.parent(), Some(trial.state.join("passi").join("trial").as_path()));
    let name = log.file_name().unwrap().to_string_lossy().into_owned();
    let stamp = name.strip_suffix(".log").expect("a .log");
    assert!(stamp.len() == 15 && stamp.chars().enumerate().all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() }), "{name}");
    let prompt_file = PathBuf::from(receipt.prompt_file.clone().expect("a prompt file"));
    assert_eq!(fs::read(&prompt_file).unwrap(), PROMPT.as_bytes(), "the prompt file is byte for byte");

    let record = trial.record("claude", 0);
    let argv = args(&record);
    let prototype: Vec<String> =
        ["-p", "--output-format", "stream-json", "--verbose", "--dangerously-skip-permissions", "--model", "haiku", "--effort", "low"]
            .map(str::to_owned)
            .to_vec();
    assert_eq!(options(&argv), options(&prototype), "{argv:?}");
    assert!(!argv.iter().any(|arg| arg.contains("do the step")), "the prompt is not an argument");
    assert_eq!(record["stdin"], PROMPT);
    assert!(same_place(Path::new(record["cwd"].as_str().unwrap()), &request.folder));
    trial.wait_for(|| fs::read_to_string(&log).is_ok_and(|text| text.contains(SESSION)), Duration::from_secs(10));
}

#[test]
fn claude_without_model_and_effort_gets_neither() {
    let trial = Trial::new("claude-plain");
    trial.launch(&trial.request(ServiceId::Claude)).expect("the launch");
    let argv = args(&trial.record("claude", 0));
    assert!(!argv.iter().any(|arg| arg.starts_with("--model") || arg.starts_with("--effort")), "{argv:?}");
}

/// The child does not get what binds the core to its own session; it gets the state folder, which
/// the prototype's tools read, and Python in UTF-8 mode even where the parent turned it off.
#[test]
fn the_child_does_not_inherit_the_parent_session() {
    let trial = Trial::new("parent-session");
    let env = trial.env().with("PYTHONUTF8", "0");
    trial.cli_over(env).launch(&trial.request(ServiceId::Claude)).expect("the launch");
    let env = env_of(&trial.record("claude", 0));
    for absent in ["ORCA_TERMINAL_HANDLE", "CLAUDE_CODE_SESSION_ID", "CLAUDECODE"] {
        assert!(!env.contains_key(absent), "the child sees {absent}");
    }
    assert!(env.contains_key("REGIA_RUNTIME"), "the rest passes");
    assert_eq!(env.get("PYTHONUTF8").map(String::as_str), Some("1"));
    assert_eq!(env.get("CODEX_HOME").map(PathBuf::from), Some(trial.codex_home()), "a session may start the other agent");
}

#[test]
fn codex_gets_the_prototype_argv_and_the_prompt_on_stdin() {
    let trial = Trial::new("codex");
    let request = trial.request(ServiceId::Codex).model(model("gpt-x")).effort(Some(Effort::High));
    let receipt = trial.launch(&request).expect("the launch");
    assert_eq!((receipt.surface, receipt.service, receipt.session.as_deref()), (SurfaceKind::Cli, ServiceId::Codex, Some(SESSION)));
    assert!(receipt.transcript_exists);
    assert!(receipt.transcript.as_deref().unwrap().ends_with(&format!("rollout-2026-10-21T12-00-00-{SESSION}.jsonl")));
    assert_eq!(receipt.resume.as_deref(), Some(format!("codex resume {SESSION}").as_str()));

    let record = trial.record("codex", 0);
    let argv = args(&record);
    let log = receipt.log.clone().unwrap();
    let last = format!("{}.ultimo.txt", log.strip_suffix(".log").unwrap());
    let folder = request.folder.display().to_string();
    let prototype: Vec<String> = [
        "exec",
        "--json",
        "--dangerously-bypass-approvals-and-sandbox",
        "--skip-git-repo-check",
        "-C",
        folder.as_str(),
        "--output-last-message",
        last.as_str(),
        "-m",
        "gpt-x",
        "-c",
        "model_reasoning_effort=high",
        "-",
    ]
    .map(slashes)
    .to_vec();
    let ours: Vec<String> = argv.iter().map(|arg| slashes(arg)).collect();
    assert_eq!(options(&ours), options(&prototype), "{argv:?}");
    assert_eq!(argv.first().map(String::as_str), Some("exec"));
    assert_eq!(argv.last().map(String::as_str), Some("-"), "the prompt comes from stdin");
    assert_eq!(record["stdin"], PROMPT);
}

/// The launch returns once the session is open: the fake still sleeps, and goes on by itself.
#[test]
fn the_launch_does_not_wait_for_the_end() {
    let trial = Trial::new("slow");
    trial.mode("slow");
    let receipt = trial.launch(&trial.request(ServiceId::Claude)).expect("the launch");
    assert!(!trial.bin.join("claude.done").exists(), "the launch waited for the end");
    assert!(receipt.pid.is_some());
    trial.wait_for(|| trial.bin.join("claude.done").exists(), Duration::from_secs(40));
}

/// A process that ends without opening a session did not start, and something ran: the error is
/// after an effect, and carries the tail of the log.
#[test]
fn a_process_that_dies_at_once_did_not_start() {
    let trial = Trial::new("dies");
    trial.mode("dies");
    for (service, name) in [(ServiceId::Claude, "claude"), (ServiceId::Codex, "codex")] {
        let error = trial.launch(&trial.request(service)).unwrap_err();
        assert!(!error.before_effect(), "{error}");
        let text = error.to_string();
        assert!(text.contains("exited with code 2"), "{text}");
        assert!(text.contains(&format!("fake {name} cannot start")), "the tail of the log is in the error: {text}");
    }
}

#[test]
fn a_missing_program_fails_before_any_effect() {
    let trial = Trial::new("missing");
    let empty = trial.dir.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let cli = Cli::new(trial.env()).with_state(trial.state.clone()).with_search(Search::from_parts(Some(empty.as_os_str()), Vec::new(), &[]));
    let error = cli.launch(&trial.request(ServiceId::Claude)).unwrap_err();
    assert!(error.before_effect() && error.to_string().contains("not found"), "{error}");
    assert!(!trial.started("claude"));
}

/// A program replaced after it was pinned does not start, and nothing was launched.
#[test]
fn a_changed_program_fails_before_any_effect() {
    let trial = Trial::new("changed");
    let cli = trial.cli();
    fs::write(trial.bin.join(common::exe("claude")), b"not the program that was pinned").unwrap();
    let error = cli.launch(&trial.request(ServiceId::Claude)).unwrap_err();
    assert!(error.before_effect(), "{error}");
    assert!(!trial.started("claude"));
}

/// The state folder is never in the temporary folder, which Storage Sense empties; the refusal
/// creates nothing there.
#[test]
fn a_state_folder_in_the_temporary_folder_is_refused_before_anything_is_created() {
    let trial = Trial::new("temporary");
    let state = std::env::temp_dir().join(format!("agents-trial-state-{}", std::process::id()));
    assert!(!state.exists());
    let error = trial.cli().with_state(state.clone()).launch(&trial.request(ServiceId::Claude)).unwrap_err();
    assert!(error.before_effect() && error.to_string().contains("temporary"), "{error}");
    assert!(!state.exists(), "the refusal created the folder");
    assert!(!trial.started("claude"));
}

#[test]
fn two_launches_do_not_share_a_log() {
    let trial = Trial::new("twice");
    let request = trial.request(ServiceId::Claude);
    let (first, second) = (trial.launch(&request).unwrap(), trial.launch(&request).unwrap());
    assert_ne!(first.log, second.log);
    assert_ne!(first.prompt_file, second.prompt_file);
}

/// The prototype's checks before any effect: none of these reaches a surface.
#[test]
fn invalid_requests_launch_nothing() {
    let trial = Trial::new("invalid");
    for (label, folder, prompt) in [
        ("Trial X", trial.folder.clone(), "p"),
        ("", trial.folder.clone(), "p"),
        ("trial", trial.folder.clone(), "  "),
        ("trial", trial.dir.join("does-not-exist"), "p"),
    ] {
        let refused = LaunchRequest::new(ServiceId::Claude, label, &folder, prompt).unwrap_err();
        assert!(matches!(refused, LaunchError::BeforeEffect(_)), "{label:?} {prompt:?}");
    }
    assert!(Effort::parse("alto; rm").is_err());
    assert!(ServiceId::parse("gemini").is_none());
    assert!(serde_json::from_str::<SurfaceKind>("\"ssh\"").is_err());
    assert!(!trial.started("claude"));
}

/// On Windows codex is npm's `codex.cmd`, and `cmd.exe` reads its arguments again: the folder,
/// written with `/`, reaches the program whole.
#[cfg(windows)]
#[test]
fn codex_through_a_cmd_shim_gets_its_folder_whole() {
    let trial = Trial::new("cmd-shim");
    let shim = trial.dir.join("shim");
    fs::create_dir_all(&shim).unwrap();
    let script = shim.join("codex.cmd");
    fs::write(&script, format!("@\"{}\" %*\r\n", trial.bin.join(common::exe("codex")).display())).unwrap();
    let pin = regia_exec::Pin::at(Program::Codex, &script).unwrap();
    let cli = trial.cli().with_pin(ServiceId::Codex, pin);
    let request = trial.request(ServiceId::Codex).effort(Some(Effort::High));
    let receipt = cli.launch(&request).expect("the launch through the shim");
    assert_eq!(receipt.session.as_deref(), Some(SESSION));
    let argv = args(&trial.record("codex", 0));
    let folder = slashes(&request.folder.display().to_string());
    assert!(argv.contains(&format!("--cd={folder}")), "{argv:?}");
    assert!(argv.contains(&"--config=model_reasoning_effort=high".to_owned()), "{argv:?}");
    assert_eq!(trial.record("codex", 0)["stdin"], PROMPT);
}

/// V14 of #460: the prompt read from a file is what the prototype's text-mode read gives, on every
/// platform: `\r\n` and a lone `\r` are `\n`. What the agent then receives on stdin is that text, byte
/// for byte.
#[test]
fn a_prompt_file_is_read_with_universal_newlines() {
    let trial = Trial::new("prompt-file");
    for (written, read) in [("do the step\r\nline two: è l'ora\r\n", "do the step\nline two: è l'ora\n"), ("one\rtwo\r\r\nthree", "one\ntwo\n\nthree"), ("already\nlf", "already\nlf")] {
        let file = trial.dir.join("prompt.md");
        fs::write(&file, written).unwrap();
        let prompt = regia_agents::read_prompt_file(&file).expect("the prompt");
        assert_eq!(prompt, read, "{written:?}");
        let receipt = trial.launch(&LaunchRequest::new(ServiceId::Claude, "trial", &trial.folder, &prompt).unwrap()).expect("the launch");
        assert_eq!(fs::read(receipt.prompt_file.expect("a prompt file")).unwrap(), read.as_bytes(), "{written:?}");
    }
    assert!(regia_agents::read_prompt_file(&trial.dir.join("missing.md")).is_err());
    fs::write(trial.dir.join("latin1.md"), b"caff\xe8").unwrap();
    assert!(regia_agents::read_prompt_file(&trial.dir.join("latin1.md")).is_err(), "not UTF-8");
}

/// A folder with a space and an accent in its name, as two registered projects have.
fn awkward_folder(trial: &Trial) -> PathBuf {
    let folder = trial.dir.join("Tesi di dottorato è qui");
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// V11 of #460, where the program is a real executable: the folder is an argument as the prototype
/// gives it, spaces and all, and it is the child's working folder too.
#[test]
fn codex_as_an_executable_gets_a_folder_with_spaces_as_its_argument() {
    let trial = Trial::new("awkward-exe");
    let folder = awkward_folder(&trial);
    let request = LaunchRequest::new(ServiceId::Codex, "trial", &folder, PROMPT).unwrap();
    trial.launch(&request).expect("the launch");
    let record = trial.record("codex", 0);
    let argv = args(&record);
    assert!(argv.contains(&format!("--cd={}", slashes(&request.folder.display().to_string()))), "{argv:?}");
    assert!(same_place(Path::new(record["cwd"].as_str().unwrap()), &folder));
}

/// V11 of #460, on Windows, where codex is npm's `codex.cmd` and `cmd.exe` reads its arguments again:
/// the alphabet of threat model §3.6 rule 9 does not take a space or an accent, and is not widened.
/// The folder is the working folder of the child instead, which codex uses when it has no `--cd`; one
/// that the alphabet takes stays an argument, as the prototype passes it.
#[cfg(windows)]
#[test]
fn codex_through_a_cmd_shim_gets_a_folder_with_spaces_as_its_working_folder() {
    let trial = Trial::new("awkward-shim");
    let shim = trial.dir.join("shim");
    fs::create_dir_all(&shim).unwrap();
    let script = shim.join("codex.cmd");
    fs::write(&script, format!("@\"{}\" %*\r\n", trial.bin.join(common::exe("codex")).display())).unwrap();
    let cli = trial.cli().with_pin(ServiceId::Codex, regia_exec::Pin::at(Program::Codex, &script).unwrap());

    let folder = awkward_folder(&trial);
    let request = LaunchRequest::new(ServiceId::Codex, "trial", &folder, PROMPT).unwrap().effort(Some(Effort::High));
    let receipt = cli.launch(&request).expect("the launch through the shim, from a folder with spaces");
    assert_eq!(receipt.session.as_deref(), Some(SESSION));
    let record = trial.record("codex", 0);
    let argv = args(&record);
    assert!(!argv.iter().any(|arg| arg.starts_with("--cd")), "{argv:?}");
    assert!(argv.iter().any(|arg| arg.starts_with("--output-last-message=")) && argv.contains(&"--config=model_reasoning_effort=high".to_owned()), "{argv:?}");
    assert!(same_place(Path::new(record["cwd"].as_str().unwrap()), &folder), "{record}");
    assert_eq!(record["stdin"], PROMPT);

    // A folder inside the alphabet is an argument still: the prototype's argv, byte for byte.
    cli.launch(&trial.request(ServiceId::Codex)).expect("the second launch");
    assert!(args(&trial.record("codex", 1)).iter().any(|arg| arg.starts_with("--cd=")));
}

/// V11 of #460, the limit that stays: a state folder the alphabet does not take cannot carry codex's
/// `--output-last-message` through a `.cmd`, so the launch refuses before anything starts or is written.
#[cfg(windows)]
#[test]
fn codex_through_a_cmd_shim_refuses_a_state_folder_outside_the_alphabet_before_any_effect() {
    let trial = Trial::new("awkward-state");
    let shim = trial.dir.join("shim");
    fs::create_dir_all(&shim).unwrap();
    let script = shim.join("codex.cmd");
    fs::write(&script, format!("@\"{}\" %*\r\n", trial.bin.join(common::exe("codex")).display())).unwrap();
    let state = trial.dir.join("state con spazi");
    let cli = trial.cli().with_state(state.clone()).with_pin(ServiceId::Codex, regia_exec::Pin::at(Program::Codex, &script).unwrap());
    let error = cli.launch(&trial.request(ServiceId::Codex)).unwrap_err();
    assert!(error.before_effect(), "{error}");
    assert!(error.to_string().contains(".cmd") && error.to_string().contains("RUNTIME"), "{error}");
    assert!(!trial.started("codex"));
    assert!(fs::read_dir(state.join("passi").join("trial")).unwrap().next().is_none(), "nothing was written");
    // Claude has no such limit: its prompt is on stdin and it names no file.
    let claude = trial.cli().with_state(state).launch(&trial.request(ServiceId::Claude));
    assert!(claude.is_ok(), "{claude:?}");
}
