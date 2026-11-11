//! `sync` on real, temporary repos with a bare remote, as the prototype's `test_dossier.py` and
//! `test_dossier_avvisi.py` prove `dossier_sync` (#157 and #159 of the dossier). The repos live under
//! the build's own temporary folder, never the system's. Each has its own identity, no signing, no
//! hooks and no line-break conversion, whatever the machine's configuration says.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use regia_exec::{Args, Pin, Program, Text};
use regia_git::{Checkpoint, DossierSync, GitDate, Outcome, named_files, sync};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const MESSAGE: &str = "p: checkpoint 2026-01-10T09:00:00Z";

/// `git <words…> <values…>` in `dir`, which must succeed; its stdout, trimmed at the end.
fn git(dir: &Path, words: &[&'static str], values: &[&str]) -> String {
    let args = words.iter().fold(Args::new(), |args, word| args.word(word));
    let args = values.iter().fold(args, |args, value| args.value(&Text::parse(value).unwrap()));
    let output = Pin::resolve(Program::Git).expect("git").launch(args, dir).run().expect("git runs");
    assert!(output.status.success(), "git {words:?} {values:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim_end().to_owned()
}

fn write(dir: &Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn state_file() -> String {
    named_files()[0].as_str().to_owned()
}

fn decisions_file() -> String {
    named_files()[1].as_str().to_owned()
}

/// A dossier `work` with one commit pushed to `origin.git`, its upstream.
struct Repo {
    root: PathBuf,
    work: PathBuf,
    bare: PathBuf,
}

impl Repo {
    fn new() -> Repo {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("regia-git-sync")
            .join(format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&root);
        let (work, bare) = (root.join("work"), root.join("origin.git"));
        fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare", "-b", "main"], &[]);
        let repo = Repo { root, work, bare };
        repo.init(&repo.work);
        write(&repo.work, &state_file(), "v0\n");
        write(&repo.work, &decisions_file(), "d0\n");
        write(&repo.work, "README.md", "r0\n");
        git(&repo.work, &["add", "--all"], &[]);
        git(&repo.work, &["commit", "-q", "-m", "first"], &[]);
        git(&repo.work, &["remote", "add", "origin"], &[repo.bare.to_str().unwrap()]);
        git(&repo.work, &["push", "-q", "-u", "origin", "main"], &[]);
        repo
    }

    /// A repo in `dir` that reads nothing of the machine's configuration that would change a commit.
    fn init(&self, dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"], &[]);
        Repo::configure(dir);
    }

    fn configure(dir: &Path) {
        for (key, value) in [
            ("user.name", "Trial"),
            ("user.email", "trial@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
            ("core.hooksPath", ".git/no-hooks"),
        ] {
            git(dir, &["config"], &[key, value]);
        }
    }

    /// A second clone of the remote: whoever pushed before us.
    fn other(&self) -> PathBuf {
        let other = self.root.join(format!("other-{}", NEXT.fetch_add(1, Ordering::SeqCst)));
        // Checked out with no line-break conversion too, or its files would look changed.
        git(&self.root, &["clone", "-q", "--config=core.autocrlf=false"], &[self.bare.to_str().unwrap(), other.to_str().unwrap()]);
        Repo::configure(&other);
        other
    }

    fn push_from(&self, other: &Path, relative: &str, text: &str) {
        write(other, relative, text);
        git(other, &["add", "--"], &[relative]);
        git(other, &["commit", "-q", "-m", "other"], &[]);
        git(other, &["push", "-q", "origin", "main"], &[]);
    }

    fn commit_here(&self, relative: &str, text: &str) {
        write(&self.work, relative, text);
        git(&self.work, &["add", "--"], &[relative]);
        git(&self.work, &["commit", "-q", "-m", "here"], &[]);
    }

    fn sync(&self, files: &[&str]) -> DossierSync {
        sync_folder(&self.work, files)
    }

    fn heads(&self) -> (String, String) {
        (git(&self.work, &["rev-parse", "HEAD"], &[]), git(&self.bare, &["rev-parse", "main"], &[]))
    }

    fn committed(&self) -> Vec<String> {
        let mut files: Vec<String> = git(&self.work, &["show", "--name-only", "--format=", "HEAD"], &[]).lines().map(str::to_owned).collect();
        files.sort();
        files
    }

    fn status(&self) -> Vec<String> {
        let mut lines: Vec<String> = git(&self.work, &["status", "--porcelain"], &[]).lines().map(str::to_owned).collect();
        lines.sort();
        lines
    }

    fn count(&self) -> String {
        git(&self.work, &["rev-list", "--count", "HEAD"], &[])
    }

    fn break_remote(&self) {
        // Relative, so no reason names a path of the machine.
        git(&self.work, &["remote", "set-url", "origin", "../missing.git"], &[]);
    }

    fn mend_remote(&self) {
        git(&self.work, &["remote", "set-url", "origin"], &[self.bare.to_str().unwrap()]);
    }
}

fn sync_folder(folder: &Path, files: &[&str]) -> DossierSync {
    let files: Vec<String> = files.iter().map(|file| (*file).to_owned()).collect();
    sync(&Checkpoint { folder: folder.to_str().unwrap(), message: MESSAGE, files: &files, date: None })
}

fn reason(out: &DossierSync) -> &str {
    out.reason.as_deref().unwrap_or("")
}

#[test]
fn a_push_leaves_the_same_commit_here_and_on_the_remote() {
    let repo = Repo::new();
    write(&repo.work, &state_file(), "v1\n");
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, out.branch.as_deref(), out.files.clone(), out.ahead), (Outcome::Pushed, Some("main"), vec![state_file()], Some(0)));
    let (here, there) = repo.heads();
    assert_eq!((out.sha.as_deref(), there.as_str()), (Some(here.as_str()), here.as_str()));
    assert_eq!(git(&repo.work, &["log", "-1", "--format=%s"], &[]), MESSAGE);
    assert_eq!(repo.committed(), [state_file()]);
    assert!(repo.status().is_empty());
    assert_eq!(out.reason, None);
    assert_eq!(out.folder.as_deref(), repo.work.to_str(), "the folder as it was given");
}

/// Acceptance 2: what someone else staged stays staged and out of the commit.
#[test]
fn a_file_someone_else_staged_stays_out_of_the_commit() {
    let repo = Repo::new();
    write(&repo.work, "theirs.txt", "another session's work\n");
    git(&repo.work, &["add", "--", "theirs.txt"], &[]);
    write(&repo.work, "README.md", "r1: changed, not staged\n");
    write(&repo.work, &state_file(), "v1\n");
    let out = repo.sync(&[]);
    assert_eq!(out.outcome, Outcome::Pushed);
    assert_eq!(repo.committed(), [state_file()]);
    assert_eq!(git(&repo.work, &["diff", "--cached", "--name-only"], &[]), "theirs.txt", "still staged");
    assert_eq!(repo.status(), [" M README.md", "A  theirs.txt"]);
}

#[test]
fn a_new_named_file_goes_in_and_an_unnamed_one_stays_out() {
    let repo = Repo::new();
    write(&repo.work, ".regia/journal/2026-10-23.md", "new\n");
    write(&repo.work, ".regia/report/r.md", "a new report\n");
    write(&repo.work, "unnamed.txt", "stays out\n");
    let out = repo.sync(&[".regia/report/r.md"]);
    assert_eq!(out.outcome, Outcome::Pushed);
    assert_eq!(repo.committed(), [".regia/journal/2026-10-23.md", ".regia/report/r.md"]);
    assert_eq!(repo.status(), ["?? unnamed.txt"]);
    let (here, there) = repo.heads();
    assert_eq!(here, there);
}

#[test]
fn with_nothing_named_changed_nothing_is_committed() {
    let repo = Repo::new();
    let before = repo.heads();
    write(&repo.work, "README.md", "r1: not a named file\n");
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, out.sha.clone(), out.files.clone(), out.reason.clone(), out.ahead), (Outcome::Nothing, None, vec![], None, Some(0)));
    assert_eq!(repo.heads(), before);
    assert_eq!(repo.count(), "1");
}

#[test]
fn a_remote_ahead_fast_forwards_then_pushes_without_a_merge_commit() {
    let repo = Repo::new();
    let other = repo.other();
    repo.push_from(&other, "x.txt", "from the other\n");
    write(&repo.work, &state_file(), "v1\n");
    let out = repo.sync(&[]);
    assert_eq!(out.outcome, Outcome::Pushed, "{out:?}");
    let (here, there) = repo.heads();
    assert_eq!(here, there);
    assert!(repo.work.join("x.txt").exists(), "the fast-forward brought the other's commit");
    assert_eq!(repo.count(), "3");
    assert_eq!(git(&repo.work, &["rev-list", "--merges", "--count", "HEAD"], &[]), "0");
    assert_eq!(repo.committed(), [state_file()]);
}

#[test]
fn a_divergence_keeps_the_commit_local_and_pushes_nothing() {
    let repo = Repo::new();
    let other = repo.other();
    repo.push_from(&other, "x.txt", "from the other\n");
    let remote_before = git(&repo.bare, &["rev-parse", "main"], &[]);
    repo.commit_here("y.txt", "a local commit, not pushed\n");
    write(&repo.work, &state_file(), "v1\n");
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, out.files.clone()), (Outcome::LocalCommit, vec![state_file()]));
    assert!(reason(&out).starts_with("fast-forward failed: ") && reason(&out).ends_with(" (divergence: the owner resolves it)"), "{out:?}");
    assert_eq!(out.sha.as_deref(), Some(repo.heads().0.as_str()));
    assert_eq!(git(&repo.bare, &["rev-parse", "main"], &[]), remote_before, "no push, no force");
    assert_eq!(out.ahead, Some(2), "the local commit and the new one");
}

#[test]
fn a_local_file_in_conflict_with_the_remote_is_not_pushed_nor_lost() {
    let repo = Repo::new();
    let other = repo.other();
    repo.push_from(&other, &state_file(), "from the other\n");
    let remote_before = git(&repo.bare, &["rev-parse", "main"], &[]);
    write(&repo.work, &state_file(), "mine\n");
    let out = repo.sync(&[]);
    assert_eq!(out.outcome, Outcome::LocalCommit);
    assert_eq!(git(&repo.bare, &["rev-parse", "main"], &[]), remote_before);
    assert_eq!(fs::read_to_string(repo.work.join(state_file())).unwrap(), "mine\n", "no work lost");
}

/// #442/V4: the push that fails is a local commit, with git's own words in the reason.
#[test]
fn a_push_that_fails_is_a_local_commit() {
    let repo = Repo::new();
    write(&repo.work, &state_file(), "v1\n");
    repo.break_remote();
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, out.ahead), (Outcome::LocalCommit, Some(1)));
    assert_eq!(out.sha.as_deref(), Some(repo.heads().0.as_str()));
    assert!(reason(&out).starts_with("fetch failed: ") && reason(&out).contains("; push: "), "{out:?}");
    assert!(!reason(&out).contains(repo.root.to_str().unwrap()), "no path of the machine: {out:?}");
}

#[test]
fn with_no_remote_the_commit_stays_here() {
    let repo = Repo::new();
    let alone = repo.root.join("alone");
    repo.init(&alone);
    write(&alone, &state_file(), "v0\n");
    git(&alone, &["add", "--all"], &[]);
    git(&alone, &["commit", "-q", "-m", "first"], &[]);
    write(&alone, &state_file(), "v1\n");
    let out = sync_folder(&alone, &[]);
    assert_eq!((out.outcome, reason(&out), out.ahead), (Outcome::NoRemote, "no remote configured", None));
    assert_eq!(out.sha.as_deref(), Some(git(&alone, &["rev-parse", "HEAD"], &[]).as_str()));
}

#[test]
fn a_remote_without_an_upstream_is_no_remote() {
    let repo = Repo::new();
    git(&repo.work, &["branch", "--unset-upstream"], &[]);
    write(&repo.work, &state_file(), "v1\n");
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, reason(&out)), (Outcome::NoRemote, "the branch main has no upstream"));
    let (here, there) = repo.heads();
    assert_ne!(here, there);
}

#[test]
fn a_folder_that_is_not_the_root_of_its_repo_is_not_the_dossier() {
    let repo = Repo::new();
    let below = repo.work.join("below");
    write(&below, &state_file(), "v1\n");
    let before = repo.heads();
    let out = sync_folder(&below, &[]);
    assert_eq!((out.outcome, reason(&out)), (Outcome::Unavailable, "the folder is not the root of a git repo"));
    assert_eq!(repo.heads(), before);
    let missing = sync_folder(&repo.root.join("missing"), &[]);
    assert_eq!((missing.outcome, reason(&missing)), (Outcome::Unavailable, "the project's folder does not exist"));
    assert_eq!(missing.folder.as_deref(), repo.root.join("missing").to_str());
}

#[test]
fn a_file_outside_the_folder_or_naming_all_of_it_is_refused() {
    let repo = Repo::new();
    write(&repo.work, &state_file(), "v1\n");
    let before = repo.heads();
    let absolute = repo.root.join("absolute.txt");
    for (bad, why) in [
        ("../outside.txt", "leaves the project's folder"),
        (".", "names the whole folder or .git"),
        ("docs/..", "names the whole folder or .git"),
        (".git/config", "names the whole folder or .git"),
        (".GIT/config", "names the whole folder or .git"),
        (absolute.to_str().unwrap(), "must be relative to the project's folder"),
    ] {
        let out = repo.sync(&[bad]);
        assert_eq!(out.outcome, Outcome::Unavailable, "{bad}");
        assert_eq!(reason(&out), format!("--file '{bad}': {why}"), "{bad}");
        assert_eq!((repo.heads(), repo.count()), (before.clone(), "1".to_owned()), "{bad}");
    }
}

/// A pattern is never one: the prototype finds nothing for `a*.txt`, and the product refuses the name,
/// which a dossier path cannot spell. A path with `.` and `..` inside the folder is the file it names.
#[test]
fn paths_are_literal_and_a_dotted_path_is_resolved() {
    let repo = Repo::new();
    write(&repo.work, "a1.txt", "1\n");
    let glob = repo.sync(&["a*.txt"]);
    assert_eq!((glob.outcome, reason(&glob)), (Outcome::Unavailable, "--file 'a*.txt': invalid dossier path: has a character outside its alphabet"));
    assert_eq!(repo.sync(&["a.txt"]).outcome, Outcome::Nothing, "a name that matches nothing");
    write(&repo.work, "docs/plan.md", "plan\n");
    let out = repo.sync(&["docs/./x/../plan.md"]);
    assert_eq!((out.outcome, out.files.clone()), (Outcome::Pushed, vec!["docs/plan.md".to_owned()]));
}

#[test]
fn a_detached_head_commits_nothing() {
    let repo = Repo::new();
    git(&repo.work, &["checkout", "-q", "--detach"], &[]);
    write(&repo.work, &state_file(), "v1\n");
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, reason(&out), out.branch.clone()), (Outcome::Unavailable, "detached HEAD: no branch to commit on", None));
    assert_eq!(repo.count(), "1");
}

#[test]
fn a_hook_that_refuses_the_commit_is_an_outcome() {
    let repo = Repo::new();
    let hooks = repo.root.join("hooks");
    write(&hooks, "pre-commit", "#!/bin/sh\necho no >&2\nexit 1\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hooks.join("pre-commit"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(&repo.work, &["config", "core.hooksPath"], &[hooks.to_str().unwrap()]);
    write(&repo.work, &state_file(), "v1\n");
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, reason(&out)), (Outcome::Unavailable, "git commit: no"));
    assert_eq!(repo.count(), "1");
}

/// #159 of the dossier: after a push that failed, the next checkpoint pushes the commit left behind,
/// and makes none of its own.
#[test]
fn after_a_failed_push_the_next_one_pushes_what_was_left() {
    let repo = Repo::new();
    repo.break_remote();
    write(&repo.work, &state_file(), "v1\n");
    let left = repo.sync(&[]);
    assert_eq!(left.outcome, Outcome::LocalCommit);
    repo.mend_remote();
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, reason(&out), out.ahead, out.files.clone()), (Outcome::Pushed, "retried: 1 commit already made", Some(0), vec![]));
    let (here, there) = repo.heads();
    assert_eq!((out.sha.clone(), there.clone()), (left.sha.clone(), here.clone()));
    assert_eq!(left.sha.as_deref(), Some(here.as_str()));
    assert_eq!(repo.count(), "2", "no new commit");
}

#[test]
fn every_commit_left_is_counted_and_a_second_failure_stays_local() {
    let repo = Repo::new();
    repo.break_remote();
    for n in 1..=2 {
        write(&repo.work, &state_file(), &format!("v{n}\n"));
        assert_eq!(repo.sync(&[]).outcome, Outcome::LocalCommit);
    }
    let again = repo.sync(&[]);
    assert_eq!((again.outcome, again.ahead, again.sha.as_deref()), (Outcome::LocalCommit, Some(2), Some(repo.heads().0.as_str())));
    assert!(reason(&again).contains("push: ") && !reason(&again).contains("retried"), "{again:?}");
    assert_eq!(git(&repo.bare, &["rev-list", "--count", "main"], &[]), "1");
    repo.mend_remote();
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, reason(&out)), (Outcome::Pushed, "retried: 2 commits already made"));
    let (here, there) = repo.heads();
    assert_eq!(here, there);
    let idle = repo.sync(&[]);
    assert_eq!((idle.outcome, idle.reason.clone(), idle.sha.clone()), (Outcome::Nothing, None, None), "again, nothing");
}

#[test]
fn with_a_divergence_and_nothing_new_nothing_is_pushed() {
    let repo = Repo::new();
    let other = repo.other();
    repo.push_from(&other, "x.txt", "from the other\n");
    repo.commit_here("y.txt", "a local commit\n");
    let before = (git(&repo.bare, &["rev-parse", "main"], &[]), repo.heads().0);
    let out = repo.sync(&[]);
    assert_eq!(out.outcome, Outcome::Nothing);
    assert!(reason(&out).starts_with("fast-forward failed: "), "{out:?}");
    assert_eq!((git(&repo.bare, &["rev-parse", "main"], &[]), repo.heads().0), before);
}

#[test]
fn another_session_s_local_commit_goes_and_nothing_else_does() {
    let repo = Repo::new();
    repo.commit_here("theirs.txt", "a commit of another session\n");
    write(&repo.work, "README.md", "r1: changed, not committed\n");
    write(&repo.work, "new.txt", "untracked\n");
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, reason(&out)), (Outcome::Pushed, "retried: 1 commit already made"));
    let (here, there) = repo.heads();
    assert_eq!(here, there);
    assert_eq!(repo.status(), [" M README.md", "?? new.txt"]);
}

#[test]
fn never_another_branch() {
    let repo = Repo::new();
    git(&repo.work, &["branch", "other"], &[]);
    git(&repo.work, &["checkout", "-q", "other"], &[]);
    repo.commit_here("z.txt", "z\n");
    git(&repo.work, &["checkout", "-q", "main"], &[]);
    repo.commit_here("y.txt", "y\n");
    assert_eq!(repo.sync(&[]).outcome, Outcome::Pushed);
    assert_eq!(git(&repo.bare, &["for-each-ref", "--format=%(refname)"], &[]), "refs/heads/main");
}

#[test]
fn without_an_upstream_nothing_is_retried() {
    let repo = Repo::new();
    git(&repo.work, &["branch", "--unset-upstream"], &[]);
    repo.commit_here("y.txt", "y\n");
    let out = repo.sync(&[]);
    assert_eq!((out.outcome, out.ahead), (Outcome::Nothing, None));
    let (here, there) = repo.heads();
    assert_ne!(here, there);
}

/// With the product's clock fixed, the commit has that date, so two runs give the same id.
#[test]
fn a_fixed_date_gives_the_commit_its_date() {
    let repo = Repo::new();
    write(&repo.work, &state_file(), "v1\n");
    let files: Vec<String> = Vec::new();
    let date = GitDate::new(1_768_035_600, 0);
    let out = sync(&Checkpoint { folder: repo.work.to_str().unwrap(), message: MESSAGE, files: &files, date: Some(date) });
    assert_eq!(out.outcome, Outcome::Pushed);
    assert_eq!(git(&repo.work, &["log", "-1", "--format=%at %ct %ai"], &[]), "1768035600 1768035600 2026-01-10 09:00:00 +0000");
}

#[test]
fn the_outcome_has_every_key_of_the_contract() {
    let repo = Repo::new();
    let json = serde_json::to_value(repo.sync(&[])).unwrap();
    let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["ahead", "branch", "files", "folder", "outcome", "reason", "sha"]);
    assert_eq!((&json["outcome"], &json["sha"], &json["reason"]), (&serde_json::json!("nothing"), &serde_json::Value::Null, &serde_json::Value::Null));
    for (outcome, name) in [(Outcome::Pushed, "pushed"), (Outcome::LocalCommit, "local_commit"), (Outcome::NoRemote, "no_remote"), (Outcome::Unavailable, "unavailable")] {
        assert_eq!(serde_json::to_value(outcome).unwrap(), name);
    }
}
