//! #571/V4, the proof of rule 8 for the writes: the hooks of the user, which `commit` and `push` run, do
//! not see `GIT_OPTIONAL_LOCKS`, `GIT_LITERAL_PATHSPECS` or the configuration that turns fsmonitor off,
//! and they see the two variables a push over SSH and a signed commit need, `SSH_AUTH_SOCK` and
//! `GNUPGHOME`, when the process has them. A token still never gets through.
//!
//! The test sets variables of its process, so it is alone in its file (its own process) and has one
//! test. The repos live under the build's own temporary folder, never the system's. The reads are
//! proved at the level of the environment, in `regia-exec`: they have no hook to show.

use std::fs;
use std::path::{Path, PathBuf};

use regia_exec::{Args, Pin, Program, Text};
use regia_git::{Checkpoint, Outcome, named_files, sync};

fn git(dir: &Path, words: &[&'static str], values: &[&str]) {
    let args = words.iter().fold(Args::new(), |args, word| args.word(word));
    let args = values.iter().fold(args, |args, value| args.value(&Text::parse(value).unwrap()));
    let output = Pin::resolve(Program::Git).expect("git").launch(args, dir).run().expect("git runs");
    assert!(output.status.success(), "git {words:?} {values:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn write(dir: &Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The path as a shell reads it, in the hook's own script: `/` on Windows too.
fn for_the_shell(path: &Path) -> String {
    path.to_str().unwrap().replace('\\', "/")
}

/// A hook that writes its whole environment to `out/<name>.env`.
fn recording_hook(hooks: &Path, out: &Path, name: &str) {
    let script = format!("#!/bin/sh\nenv > \"{}/{name}.env\"\nexit 0\n", for_the_shell(out));
    write(hooks, name, &script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hooks.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn seen_by(out: &Path, name: &str) -> Vec<String> {
    let text = fs::read_to_string(out.join(format!("{name}.env"))).unwrap_or_else(|_| panic!("the {name} hook did not run"));
    text.lines().map(str::to_owned).collect()
}

#[test]
fn the_hooks_of_a_write_see_the_agent_and_the_gpg_folder_and_none_of_the_settings_of_a_read() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("regia-git-hooks").join(std::process::id().to_string());
    let _ = fs::remove_dir_all(&root);
    let (work, bare, hooks, out) = (root.join("work"), root.join("origin.git"), root.join("hooks"), root.join("out"));
    for dir in [&work, &bare, &hooks, &out] {
        fs::create_dir_all(dir).unwrap();
    }
    git(&bare, &["init", "-q", "--bare", "-b", "main"], &[]);
    git(&work, &["init", "-q", "-b", "main"], &[]);
    for (key, value) in [
        ("user.name", "Trial"),
        ("user.email", "trial@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("core.hooksPath", ".git/no-hooks"),
    ] {
        git(&work, &["config"], &[key, value]);
    }
    let state = named_files()[0].as_str().to_owned();
    write(&work, &state, "v0\n");
    git(&work, &["add", "--all"], &[]);
    git(&work, &["commit", "-q", "-m", "first"], &[]);
    git(&work, &["remote", "add", "origin"], &[bare.to_str().unwrap()]);
    git(&work, &["push", "-q", "-u", "origin", "main"], &[]);

    // Only now the hooks, so that nothing of the setup is recorded.
    recording_hook(&hooks, &out, "pre-commit");
    recording_hook(&hooks, &out, "pre-push");
    git(&work, &["config", "core.hooksPath"], &[&for_the_shell(&hooks)]);
    write(&work, &state, "v1\n");

    // Values that are not paths, which a shell on Windows would convert.
    // SAFETY: the only test of this process, and nothing else runs while it sets them.
    unsafe {
        std::env::set_var("SSH_AUTH_SOCK", "agent-socket-canary");
        std::env::set_var("GNUPGHOME", "gpg-folder-canary");
        std::env::set_var("REGIA_TRIAL_TOKEN", "token-canary");
    }
    let done = sync(&Checkpoint { folder: work.to_str().unwrap(), message: "p: checkpoint 2026-01-10T09:00:00Z", files: &[], date: None });
    assert_eq!(done.outcome, Outcome::Pushed, "{:?}", done.reason);

    for hook in ["pre-commit", "pre-push"] {
        let env = seen_by(&out, hook);
        let has = |line: &str| env.iter().any(|seen| seen == line);
        assert!(has("SSH_AUTH_SOCK=agent-socket-canary"), "{hook} lacks the agent");
        assert!(has("GNUPGHOME=gpg-folder-canary"), "{hook} lacks the gpg folder");
        for name in ["GIT_OPTIONAL_LOCKS", "GIT_LITERAL_PATHSPECS", "GIT_CONFIG_COUNT"] {
            assert!(!env.iter().any(|seen| seen.starts_with(&format!("{name}="))), "{hook} sees {name}");
        }
        assert!(!env.iter().any(|seen| seen.contains("core.fsmonitor")), "{hook} sees the fsmonitor setting");
        assert!(!env.iter().any(|seen| seen.contains("token-canary")), "{hook} sees a token");
    }
}
