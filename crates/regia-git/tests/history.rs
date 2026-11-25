//! #448: the two reads of the checks, on a synthetic repo under the build's own temporary folder. `file_at` gives a
//! file as a commit has it, its line breaks read as Python's text mode reads them, and a failure for a commit or a
//! path that is not there; `last_two_commits_of` gives the last two commits that touched a path, newest first.

use std::fs;
use std::path::{Path, PathBuf};

use regia_exec::{Args, DossierPath, GitRef, Pin, Program, Text};
use regia_git::Git;

fn git(dir: &Path, words: &[&'static str], values: &[&str]) -> String {
    let args = words.iter().fold(Args::new(), |args, word| args.word(word));
    let args = values.iter().fold(args, |args, value| args.value(&Text::parse(value).unwrap()));
    let output = Pin::resolve(Program::Git).expect("git").launch(args, dir).run().expect("git runs");
    assert!(output.status.success(), "git {words:?} {values:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(dir: &Path, relative: &str, bytes: &[u8], message: &str) -> String {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    git(dir, &["add", "--"], &[relative]);
    git(dir, &["commit", "-q", "-m"], &[message]);
    git(dir, &["rev-parse", "HEAD"], &[])
}

#[test]
fn a_file_at_a_commit_and_the_last_two_commits_of_a_path() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("regia-git-history").join(std::process::id().to_string());
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"], &[]);
    for (key, value) in [("user.name", "Fixture"), ("user.email", "fixture@example.invalid"), ("commit.gpgsign", "false"), ("core.autocrlf", "false")] {
        git(&dir, &["config"], &[key, value]);
    }
    let state = DossierPath::parse(".regia/state.md").unwrap();
    let git_port = Git::resolve().expect("git");
    let none = git_port.last_two_commits_of(&dir, &state);
    assert!(!none.ok(), "no commit yet: git fails");
    let first = commit(&dir, ".regia/state.md", b"- to do: x\r\nlone\rcr\n\n", "first");
    commit(&dir, "other.md", b"y\n", "another file");
    let one = git_port.last_two_commits_of(&dir, &state);
    assert_eq!((one.ok(), one.stdout.as_str()), (true, first.as_str()), "one version: one hash");
    let second = commit(&dir, ".regia/state.md", b"- to do: y\n", "second");
    let two = git_port.last_two_commits_of(&dir, &state);
    assert_eq!(two.stdout.split_whitespace().collect::<Vec<_>>(), [second.as_str(), first.as_str()]);

    let old = git_port.file_at(&dir, &GitRef::parse(&first).unwrap(), &state);
    assert_eq!((old.ok(), old.stdout.as_str()), (true, "- to do: x\nlone\ncr"), "text mode: CRLF and a bare CR as LF, no break at the end");
    assert_eq!(git_port.file_at(&dir, &GitRef::head(), &state).stdout, "- to do: y");
    let missing = git_port.file_at(&dir, &GitRef::head(), &DossierPath::parse("docs/none.md").unwrap());
    assert!(!missing.ok() && !missing.stderr.is_empty(), "a path the commit does not have: {missing:?}");
    let inside = git_port.file_at(&dir.join(".regia"), &GitRef::head(), &state);
    assert_eq!(inside.stdout, "- to do: y", "from a folder inside the repo, the path is the root's, as for the prototype");
}
