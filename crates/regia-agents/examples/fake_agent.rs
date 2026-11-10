//! Stands in for `claude` and `codex` in the tests of the Cli surface: no network, no account.
//!
//! A test copies it under the name of the agent it plays. It records what it received (argv, stdin,
//! folder, environment) in `<name>.<n>.json` next to itself, prints the first event of that agent's
//! stream, and writes the session's transcript where the agent would, under the agent's folder
//! variable, which it requires: it never writes in a real home. A file `mode` next to it asks for
//! something else, because the allow-list keeps a test's own variables from reaching it:
//!
//! - `dies`: print an error and exit with 2 before any session;
//! - `slow`: sleep 8 s once the session is open, then write `<name>.done` next to itself.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

/// The session every fake opens.
const SESSION: &str = "11111111-2222-3333-4444-555555555555";

fn main() -> ExitCode {
    let exe = std::env::current_exe().expect("own path");
    let dir = exe.parent().expect("own folder").to_path_buf();
    let name = exe.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let mode = fs::read_to_string(dir.join("mode")).unwrap_or_default().trim().to_owned();
    let mut stdin = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut stdin);
    let prompt = String::from_utf8_lossy(&stdin).into_owned();
    record(&dir, &name, &prompt);
    let folder_variable = match name.as_str() {
        "claude" => "CLAUDE_CONFIG_DIR",
        "codex" => "CODEX_HOME",
        other => {
            eprintln!("the fake does not play {other}");
            return ExitCode::from(64);
        }
    };
    if mode == "dies" {
        say(&format!("Error: fake {name} cannot start"));
        return ExitCode::from(2);
    }
    let Some(home) = std::env::var_os(folder_variable).filter(|value| !value.is_empty()).map(PathBuf::from) else {
        eprintln!("the fake {name} needs {folder_variable}: it does not write in a real home");
        return ExitCode::from(65);
    };
    if name == "claude" {
        say(&serde_json::json!({"type": "system", "subtype": "init", "session_id": SESSION}).to_string());
        let cwd = std::env::current_dir().expect("own folder");
        let slug: String = cwd.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let folder = home.join("projects").join(slug);
        let line = serde_json::json!({"type": "user", "message": {"content": prompt}});
        write(&folder, &format!("{SESSION}.jsonl"), &format!("{line}\n"));
    } else {
        say(&serde_json::json!({"type": "thread.started", "thread_id": SESSION}).to_string());
        let folder = home.join("sessions").join("2026").join("10").join("21");
        write(&folder, &format!("rollout-2026-10-21T12-00-00-{SESSION}.jsonl"), "{}\n");
        say(&serde_json::json!({"type": "turn.completed"}).to_string());
    }
    if mode == "slow" {
        std::thread::sleep(Duration::from_secs(8));
    }
    fs::write(dir.join(format!("{name}.done")), "x").expect("the end mark");
    ExitCode::SUCCESS
}

/// One line on stdout, flushed: the surface reads the log while the fake runs.
fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn write(folder: &Path, file: &str, text: &str) {
    fs::create_dir_all(folder).expect("the transcript folder");
    fs::write(folder.join(file), text).expect("the transcript");
}

fn record(dir: &Path, name: &str, prompt: &str) {
    let count = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    let file = entry.file_name().to_string_lossy().into_owned();
                    file.starts_with(&format!("{name}.")) && file.ends_with(".json")
                })
                .count()
        })
        .unwrap_or(0);
    let env: BTreeMap<String, String> =
        std::env::vars_os().map(|(key, value)| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned())).collect();
    let report = serde_json::json!({
        "args": std::env::args_os().skip(1).map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "cwd": std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        "env": env,
        "stdin": prompt,
    });
    fs::write(dir.join(format!("{name}.{count}.json")), report.to_string()).expect("the record");
}
