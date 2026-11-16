//! The conformance harness (ADR 0001, "Conformance").
//!
//! A fixture case holds what the tracker answered (`responses.json`) and what the Python prototype
//! printed for it (`python.it.json`, or `python.it.txt` for a command that prints text), recorded
//! locally because CI cannot read the private dossier. It also holds the exit code both sides must
//! end with, the rows of the state database before the run, and the writes the prototype made to the
//! tracker, in order (`calls.json`). A case can also name a dossier, a repo the run commits to, and
//! hold what the repo looked like after (`dossier.json`). Every case is recorded and checked at one
//! instant, [`FIXTURE_NOW`]. Cases live in families, one folder of `fixtures/` per group of commands,
//! so slices that add cases do not edit the same folder.
//!
//! A case of a ported command runs the real `regia` binary against the same responses through a fake
//! `gh`, passes the recorded Python output through the facade table, and diffs the two. A case of a
//! command that goes through the facade runs the binary too, on a stand-in interpreter that checks
//! the argv the table built and replays what the prototype printed: that proves the translation, the
//! exit code and the text in CI, with no Python. Its writes are proven where the prototype is, by a
//! local test that runs the binary on the real prototype (`tests/facade.rs`).
//!
//! The two sides reach the fake `gh` differently. The prototype looks `gh` up on `PATH`, so for it the
//! fake goes first there. The product starts only an absolute path (threat model T06), so it gets a
//! copy of the fake named by `REGIA_GH`, which reads its fixture from a file beside it: the product
//! does not pass the fixture's variables to `gh`, and should not.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regia_exec::{Launch, Program};
use regia_facade::{FacadeError, SCRIPT, Table};
use regia_identity::{env_var, prototype_env_var};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Read by the fake `gh`: the responses file to answer from.
pub fn responses_var() -> String {
    env_var("FAKE_GH_RESPONSES")
}

/// Read by the fake `gh`: where to log each call, to prove it was the one that answered.
pub fn log_var() -> String {
    env_var("FAKE_GH_LOG")
}

/// Read by the fake `gh`: where to append each write to the tracker, one JSON line per call.
pub fn writes_var() -> String {
    env_var("FAKE_GH_WRITES")
}

/// Beside a copy of the fake `gh`: `{"responses": …, "log": …, "writes": …}`, read when the variables
/// are unset.
pub const FAKE_GH_SIDECAR: &str = "fixture.json";

/// Beside a copy of the fake interpreter: the argv it expects and what it replays.
pub const FAKE_PYTHON_SIDECAR: &str = "replay.json";

/// The fake interpreter's exit code when it is started with an argv the case does not expect.
pub const FAKE_PYTHON_WRONG_ARGV: u8 = 99;

/// The day every fixture is recorded and checked on, so due dates mean the same on both sides: the
/// prototype gets it through `oracle/fixed_now.py`, the product through `REGIA_TODAY`.
pub const FIXTURE_TODAY: &str = "2026-01-10";

/// The instant every fixture is recorded and checked at, on [`FIXTURE_TODAY`], so a lock taken now
/// expires at the same time on both sides: the prototype gets it through `oracle/fixed_now.py`, the
/// product through `REGIA_NOW`. Local time is UTC on both.
pub const FIXTURE_NOW: &str = "2026-01-10T09:00:00Z";

/// What the prototype reads to sign and attribute a session (`CLAUDE_CODE_SESSION_ID` and the others
/// the allow-list of the Python passes), and the role of `REGIA_RUOLO`: a fixture run sees none of the
/// machine's, unless its case names one in `env`.
pub const SESSION_VARIABLES: [&str; 3] = ["CLAUDE_CODE_SESSION_ID", "ORCA_AGENT_SESSION_ID", "CODEX_THREAD_ID"];

/// Folders of `fixtures/` whose cases drive something other than the CLI: the controller's oracle, and
/// the precheck's, whose cases are in `fixtures/precheck/events/` and run the `precheck` binary.
const NOT_CLI: [&str; 2] = ["controller", "precheck"];

/// The prototype's driver that runs it at [`FIXTURE_NOW`].
pub fn fixed_now_driver() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle").join("fixed_now.py")
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().expect("workspace root")
}

pub fn fixtures_root() -> PathBuf {
    workspace_root().join("fixtures")
}

/// The family of the brief, which holds the synthetic `tracker.json` every run gets.
pub fn fixtures_dir() -> PathBuf {
    fixtures_root().join("brief")
}

/// The folder with the workspace's binaries, found from the running test or tool.
pub fn bin_dir() -> PathBuf {
    let exe = env::current_exe().expect("current executable");
    let wanted = format!("{}{}", regia_identity::BIN_NAME, env::consts::EXE_SUFFIX);
    exe.ancestors()
        .skip(1)
        // Cargo can leave a copy in `deps`, where the other binaries are not.
        .find(|dir| dir.file_name().is_some_and(|name| name != "deps") && dir.join(&wanted).is_file())
        .unwrap_or_else(|| panic!("{wanted} not found above {}: run `cargo build --workspace` first", exe.display()))
        .to_path_buf()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseSpec {
    #[serde(default, rename = "_note")]
    pub note: Option<String>,
    /// The name of a file in the family's `scenarios/`: the tracker the fake `gh` answers from.
    #[serde(default)]
    pub scenario: Option<String>,
    /// The English command line, without the binary.
    pub args: Vec<String>,
    /// The exit code both sides end with.
    #[serde(default)]
    pub exit: u8,
    /// For a command that goes through the facade, the prototype's argv, written by hand: the table
    /// must build exactly this, and the stand-in interpreter refuses anything else.
    #[serde(default)]
    pub python: Option<Vec<String>>,
    /// Rows of the state database before the run.
    #[serde(default)]
    pub state: Vec<StateRow>,
    /// The rows of the state database after the run are part of the case: `record` stores them in
    /// `state.json`, and a native run must leave the same.
    #[serde(default)]
    pub state_after: bool,
    /// Session variables the run sees, on both sides: only those of [`SESSION_VARIABLES`].
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The project's dossier before the run, which the arguments name as [`DOSSIER_PLACEHOLDER`].
    #[serde(default)]
    pub dossier: Option<DossierSpec>,
    /// The two sides fail in their own words: only the class is compared, the exit code and, where the
    /// output holds one, the outcome of the dossier, not the text of an error (#571/V8).
    #[serde(default)]
    pub same_class_only: bool,
    /// Answers of the fake `gh` the scenario does not give, by key of its responses (`cli`,
    /// `mutations`), merged over those `record` builds: the URL a comment gets, for one.
    #[serde(default)]
    pub gh: serde_json::Map<String, Value>,
    /// Files of the case's dossier, by their path in it, whose bytes after the run are part of the case: `record`
    /// stores them in `written.json`, and a native run must leave the same (the `tracker.json` of `init derive`).
    #[serde(default)]
    pub written: Vec<String>,
}

/// A case's dossier: a folder of files, by default a git repo whose `files` are committed and pushed to
/// a bare remote, its upstream, and whose `changes` are written after, not committed. The other fields
/// shape the repo in between, in the order they are listed here, each commit with its own files: the
/// paths of the dossier that one checkpoint cannot reach from a clean repo (#442/V4).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DossierSpec {
    #[serde(default = "yes")]
    pub git: bool,
    #[serde(default = "yes")]
    pub remote: bool,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// Commits another clone pushed to the remote, which the dossier has not fetched.
    #[serde(default)]
    pub remote_commits: Vec<BTreeMap<String, String>>,
    /// Branches besides `main`, each with a commit of its files, never pushed.
    #[serde(default)]
    pub branches: BTreeMap<String, BTreeMap<String, String>>,
    /// Commits made here and never pushed: a push that failed, or another session's work.
    #[serde(default)]
    pub local_commits: Vec<BTreeMap<String, String>>,
    /// `false`: `main` keeps its remote and loses its upstream.
    #[serde(default = "yes")]
    pub upstream: bool,
    /// The URL `origin` gets: a relative one that leads nowhere breaks every fetch and push, and names
    /// no path of the machine in what git says.
    #[serde(default)]
    pub origin_url: Option<String>,
    /// Hooks, by name, in the folder the harness gives the repo for them.
    #[serde(default)]
    pub hooks: BTreeMap<String, String>,
    /// Files someone else staged and did not commit.
    #[serde(default)]
    pub staged: BTreeMap<String, String>,
    /// HEAD detached at the commit of `main`.
    #[serde(default)]
    pub detached: bool,
    /// A stand-in where git should be, on both sides (#571/V8).
    #[serde(default)]
    pub git_double: Option<GitDouble>,
    #[serde(default)]
    pub changes: BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}

/// What stands where git should be, for the paths a real git cannot be made to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitDouble {
    /// A git that answers every call with the exit code 124, as the prototype's deadline gives.
    Timeout,
    /// No git where the run looks for it.
    Missing,
}

/// The variables that put `double` in place of git for a run: `REGIA_GIT` for the product, which starts
/// only an absolute path, and `PATH` for the prototype, which looks git up there. For [`GitDouble::Missing`]
/// `REGIA_GIT` names a file that is not there and `PATH` keeps the fake `gh` and the interpreter, and no git.
pub fn git_double_env(state: &Path, double: GitDouble) -> Vec<(String, OsString)> {
    let dir = state.join("fake-git");
    let git = dir.join(format!("git{}", env::consts::EXE_SUFFIX));
    let mut paths = vec![fake_gh_dir()];
    match double {
        GitDouble::Timeout => {
            fs::create_dir_all(&dir).expect("fake git folder");
            let source = bin_dir().join(format!("fake-git{}", env::consts::EXE_SUFFIX));
            place_fake(&source, &git);
            paths.insert(0, dir);
            paths.extend(env::var_os("PATH").iter().flat_map(env::split_paths));
        }
        GitDouble::Missing => {
            let interpreter = regia_exec::Pin::resolve(Program::Python).ok();
            paths.extend(interpreter.and_then(|pin| pin.path().parent().map(Path::to_path_buf)));
        }
    }
    vec![(Program::Git.env_name(), git.into()), ("PATH".to_owned(), env::join_paths(paths).expect("PATH"))]
}

/// Where a case's arguments, the prototype's argv and what it printed name the dossier's folder,
/// which differs from run to run and from machine to machine. The one normalization of output besides
/// line breaks, declared: a recorded file never holds a path of the machine.
pub const DOSSIER_PLACEHOLDER: &str = "{dossier}";

/// When a case's dossier was committed, a day before [`FIXTURE_NOW`], in seconds since the epoch: its
/// commit has the same id on every run.
const DOSSIER_COMMIT_EPOCH: u64 = 1_767_949_200;

/// The modification time of the files of a case's dossier, an hour before [`FIXTURE_NOW`]: the
/// prototype stores a report's in the `events` cursor.
const DOSSIER_FILE_EPOCH: u64 = 1_768_032_000;

/// The modification time of its changes, half an hour later: with the time of the commit, a change of
/// the same size would look unchanged to `git status`.
const DOSSIER_CHANGE_EPOCH: u64 = 1_768_033_800;

/// The folder of the dossier of a case run in `state`.
pub fn dossier_dir(state: &Path) -> PathBuf {
    state.join("project-dossier")
}

fn remote_dir(state: &Path) -> PathBuf {
    state.join("dossier-remote.git")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let pin = regia_exec::Pin::resolve(Program::Git).expect("git");
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let date = format!("@{DOSSIER_COMMIT_EPOCH} +0000");
    let output = regia_exec::harness::program(&pin, &args, dir)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .run()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn write_files(dir: &Path, files: &BTreeMap<String, String>, epoch: u64) {
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch);
    for (name, text) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().expect("a file's folder")).expect("the file's folder");
        fs::write(&path, text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let file = fs::File::options().write(true).open(&path).expect("the file, to date it");
        file.set_modified(time).expect("the file's modification time");
    }
}

/// The identity, signing, hooks and line breaks of a repo of the harness: its own, never the machine's.
fn configure_repo(dir: &Path, name: &str) {
    let email = format!("{}@example.invalid", name.to_lowercase());
    for (key, value) in [
        ("user.name", name),
        ("user.email", email.as_str()),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("core.hooksPath", ".git/no-hooks"),
    ] {
        git(dir, &["config", key, value]);
    }
}

/// `files` written and committed in `dir` with `message`, and nothing else.
fn commit_files(dir: &Path, files: &BTreeMap<String, String>, message: &str) {
    write_files(dir, files, DOSSIER_FILE_EPOCH);
    let mut args = vec!["add", "--"];
    args.extend(files.keys().map(String::as_str));
    git(dir, &args);
    git(dir, &["commit", "-q", "-m", message]);
}

/// The dossier of `spec` in `state`: its files, and with `git` a repo with the harness's own identity
/// and no signing, hooks or line-break conversion of the machine, its upstream a bare repo beside it,
/// then shaped as `spec` says. Every commit has the same date, so it has the same id on every run.
pub fn prepare_dossier(state: &Path, spec: &DossierSpec) -> PathBuf {
    let dir = dossier_dir(state);
    fs::create_dir_all(&dir).expect("the dossier");
    write_files(&dir, &spec.files, DOSSIER_FILE_EPOCH);
    if spec.git {
        git(&dir, &["init", "-q", "-b", "main"]);
        configure_repo(&dir, "Fixture");
        git(&dir, &["add", "--all"]);
        git(&dir, &["commit", "-q", "-m", "fixture: the dossier before the run"]);
        let remote = remote_dir(state).display().to_string();
        if spec.remote {
            git(state, &["init", "-q", "--bare", "-b", "main", &remote]);
            git(&dir, &["remote", "add", "origin", &remote]);
            git(&dir, &["push", "-q", "-u", "origin", "main"]);
        }
        if !spec.remote_commits.is_empty() {
            let other = state.join("dossier-other-clone");
            // Checked out with no line-break conversion of the machine, or its files would look changed.
            git(state, &["clone", "-q", "--config=core.autocrlf=false", &remote, &other.display().to_string()]);
            configure_repo(&other, "Other");
            for files in &spec.remote_commits {
                commit_files(&other, files, "fixture: another clone's commit");
            }
            git(&other, &["push", "-q", "origin", "main"]);
        }
        for (branch, files) in &spec.branches {
            git(&dir, &["checkout", "-q", "-b", branch]);
            commit_files(&dir, files, "fixture: a commit on another branch");
            git(&dir, &["checkout", "-q", "main"]);
        }
        for files in &spec.local_commits {
            commit_files(&dir, files, "fixture: a commit not pushed");
        }
        if !spec.upstream {
            git(&dir, &["branch", "--unset-upstream"]);
        }
        if let Some(url) = &spec.origin_url {
            git(&dir, &["remote", "set-url", "origin", url]);
        }
        for (name, text) in &spec.hooks {
            let hook = dir.join(".git").join("no-hooks").join(name);
            write_files(&dir, &BTreeMap::from([(format!(".git/no-hooks/{name}"), text.clone())]), DOSSIER_FILE_EPOCH);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("the hook runs");
            }
            assert!(hook.is_file(), "the hook {name}");
        }
        if !spec.staged.is_empty() {
            write_files(&dir, &spec.staged, DOSSIER_FILE_EPOCH);
            let mut args = vec!["add", "--"];
            args.extend(spec.staged.keys().map(String::as_str));
            git(&dir, &args);
        }
        if spec.detached {
            git(&dir, &["checkout", "-q", "--detach"]);
        }
    }
    write_files(&dir, &spec.changes, DOSSIER_CHANGE_EPOCH);
    dir
}

/// The dossier's repo after a run: the commits of `main` and of its remote, newest first, and what is
/// left uncommitted; for a repo with no remote before the run, the `origin` the run gave it, if any;
/// for a repo with other branches, the refs of the remote, which a push must leave alone. `None` for a
/// dossier that is not a repo.
pub fn dossier_after(state: &Path, spec: &DossierSpec) -> Option<Value> {
    let lines = |dir: &Path, args: &[&str]| -> Vec<String> { git(dir, args).lines().map(str::to_owned).collect() };
    spec.git.then(|| {
        let dir = dossier_dir(state);
        let mut after = json!({
            "log": lines(&dir, &["log", "--format=%H %s", "main"]),
            "remote": spec.remote.then(|| lines(&remote_dir(state), &["log", "--format=%H %s", "main"])),
            "status": lines(&dir, &["status", "--porcelain=v1", "--untracked-files=all"]),
        });
        if !spec.remote && lines(&dir, &["remote"]).iter().any(|name| name == "origin") {
            after["origin"] = json!(git(&dir, &["remote", "get-url", "origin"]).trim());
        }
        if spec.remote && !spec.branches.is_empty() {
            after["remote_refs"] = json!(lines(&remote_dir(state), &["for-each-ref", "--format=%(refname)"]));
        }
        after
    })
}

/// The files `paths` of `dossier` after a run: each one's text, kept whole inside a JSON string so that no checkout
/// changes its line breaks, or null for one that is not there.
pub fn written_files(dossier: &Path, paths: &[String]) -> Value {
    let text = |path: &String| match fs::read(dossier.join(path)) {
        Ok(bytes) => Value::String(String::from_utf8(bytes).unwrap_or_else(|_| panic!("{path} is not UTF-8"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(error) => panic!("{path}: {error}"),
    };
    Value::Object(paths.iter().map(|path| (path.clone(), text(path))).collect())
}

/// `args` with the dossier's folder in place of [`DOSSIER_PLACEHOLDER`].
pub fn place_args(args: &[String], dossier: &Path) -> Vec<String> {
    args.iter().map(|arg| arg.replace(DOSSIER_PLACEHOLDER, &dossier.display().to_string())).collect()
}

/// `text` with the placeholder in place of the dossier's folder.
pub fn unplace_text(text: &str, dossier: &Path) -> String {
    text.replace(&dossier.display().to_string(), DOSSIER_PLACEHOLDER)
}

/// Every string of `value` with the placeholder in place of the dossier's folder.
pub fn unplace_json(value: &Value, dossier: &Path) -> Value {
    match value {
        Value::String(text) => Value::String(unplace_text(text, dossier)),
        Value::Array(items) => Value::Array(items.iter().map(|item| unplace_json(item, dossier)).collect()),
        Value::Object(object) => {
            Value::Object(object.iter().map(|(key, inner)| (key.clone(), unplace_json(inner, dossier))).collect())
        }
        other => other.clone(),
    }
}

/// Every string of `value` with the dossier's folder in place of the placeholder.
fn place_json(value: &Value, dossier: &Path) -> Value {
    let folder = dossier.display().to_string();
    match value {
        Value::String(text) => Value::String(text.replace(DOSSIER_PLACEHOLDER, &folder)),
        Value::Array(items) => Value::Array(items.iter().map(|item| place_json(item, dossier)).collect()),
        Value::Object(object) => Value::Object(object.iter().map(|(key, inner)| (key.clone(), place_json(inner, dossier))).collect()),
        other => other.clone(),
    }
}

impl CaseSpec {
    /// The variables of `env`, checked: a case never passes anything of the machine's but a session id.
    pub fn session_env(&self) -> Vec<(String, String)> {
        for name in self.env.keys() {
            assert!(SESSION_VARIABLES.contains(&name.as_str()), "case variable {name} is not a session variable");
        }
        self.env.iter().map(|(name, value)| (name.clone(), value.clone())).collect()
    }
}

/// A row of the documents table, as the prototype and `regia-store` write it: the document is data.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StateRow {
    pub kind: String,
    pub key: String,
    pub doc: Value,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub family: String,
    pub name: String,
    pub dir: PathBuf,
    pub spec: CaseSpec,
}

/// What the prototype printed for a case.
#[derive(Debug, Clone, PartialEq)]
pub enum Recorded {
    Json(Value),
    Text(String),
}

impl Case {
    /// Every case of every family of the CLI, sorted by family and name.
    pub fn all() -> Vec<Case> {
        let mut families: Vec<String> = fs::read_dir(fixtures_root())
            .expect("the fixtures folder")
            .map(|entry| entry.expect("a fixtures entry").path())
            .filter(|path| path.join("cases").is_dir())
            .map(|path| path.file_name().expect("a family name").to_string_lossy().into_owned())
            .filter(|family| !NOT_CLI.contains(&family.as_str()))
            .collect();
        families.sort();
        let out: Vec<Case> = families.iter().flat_map(|family| Case::of_family(family)).collect();
        assert!(!out.is_empty(), "no fixture cases in {}", fixtures_root().display());
        out
    }

    pub fn of_family(family: &str) -> Vec<Case> {
        let cases = fixtures_root().join(family).join("cases");
        let mut out: Vec<Case> = fs::read_dir(&cases)
            .unwrap_or_else(|error| panic!("{}: {error}", cases.display()))
            .map(|entry| entry.expect("case entry").path())
            .filter(|path| path.is_dir())
            .map(|dir| Case {
                family: family.to_owned(),
                name: dir.file_name().expect("case name").to_string_lossy().into_owned(),
                spec: read_json(&dir.join("case.json")),
                dir,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The case of that name, in whichever family: names are unique across families.
    pub fn named(name: &str) -> Case {
        let mut found: Vec<Case> = Case::all().into_iter().filter(|case| case.name == name).collect();
        assert_eq!(found.len(), 1, "case {name} must exist exactly once");
        found.remove(0)
    }

    pub fn family_dir(&self) -> PathBuf {
        fixtures_root().join(&self.family)
    }

    pub fn responses_path(&self) -> PathBuf {
        self.dir.join("responses.json")
    }

    pub fn python_output(&self) -> Value {
        read_json(&self.dir.join("python.it.json"))
    }

    /// The prototype's stdout: JSON if it printed JSON, text otherwise.
    pub fn recorded(&self) -> Recorded {
        let text = self.dir.join("python.it.txt");
        if text.is_file() {
            Recorded::Text(fs::read_to_string(&text).unwrap_or_else(|error| panic!("{}: {error}", text.display())))
        } else {
            Recorded::Json(self.python_output())
        }
    }

    /// What the prototype wrote on stderr, empty if nothing.
    pub fn recorded_stderr(&self) -> String {
        fs::read_to_string(self.dir.join("python.it.stderr.txt")).unwrap_or_default()
    }

    /// The writes the prototype made to the tracker, in order; none without `calls.json`.
    pub fn writes(&self) -> Vec<Value> {
        let path = self.dir.join("calls.json");
        if path.is_file() { read_json(&path) } else { Vec::new() }
    }

    /// The dossier's repo after the prototype's run, from `dossier.json`; `None` without a repo.
    pub fn dossier_after(&self) -> Option<Value> {
        let path = self.dir.join("dossier.json");
        path.is_file().then(|| read_json(&path))
    }

    /// The rows of the state database after the prototype's run, from `state.json`, as [`state_rows`]
    /// reads them; `None` for a case without [`CaseSpec::state_after`].
    pub fn state_after(&self) -> Option<Value> {
        let path = self.dir.join("state.json");
        path.is_file().then(|| read_json(&path))
    }

    /// The files of [`CaseSpec::written`] as the prototype's run left them, from `written.json`, as
    /// [`written_files`] reads them; `None` for a case without such files.
    pub fn written(&self) -> Option<Value> {
        let path = self.dir.join("written.json");
        path.is_file().then(|| read_json(&path))
    }

    /// The command the case runs, by the facade table's longest name.
    pub fn command_name(&self) -> String {
        Table::embedded().split_command(&self.spec.args).expect("the table knows the case's command").0
    }
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    let text = fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A state folder with only the synthetic `tracker.json`, outside the temp folder (the prototype
/// refuses it) and under `target/`, so nothing from the machine leaks in and nothing dirties the repo.
pub fn prepare_state_dir(label: &str) -> PathBuf {
    let dir = bin_dir().join("conformance-state").join(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean state folder");
    }
    fs::create_dir_all(&dir).expect("state folder");
    fs::copy(fixtures_dir().join("tracker.json"), dir.join("tracker.json")).expect("copy tracker.json");
    dir
}

/// The state folder of a case: `tracker.json`, and the case's rows in the state database, which both
/// implementations read.
pub fn prepare_case_state(label: &str, case: &Case) -> PathBuf {
    let dir = prepare_state_dir(label);
    write_state(&dir, &case.spec.state);
    dir
}

/// `rows` in the state database of `dir`, in format v1: the table and its columns come from
/// `regia-store`'s description of the format, each row with generation 1.
pub fn write_state(dir: &Path, rows: &[StateRow]) {
    if rows.is_empty() {
        return;
    }
    let format = workspace_root().join("crates").join("regia-store").join("formats").join("store_v1.toml");
    let format: toml::Table = toml::from_str(&fs::read_to_string(&format).expect("the v1 state format")).expect("TOML");
    let column = |name: &str| format["table"][name].as_str().expect("a column of the v1 table").to_owned();
    let [table, kind, key, generation, document] = ["name", "kind", "key", "generation", "document"].map(column);
    let connection = rusqlite::Connection::open(dir.join(regia_store::database_name())).expect("the state database");
    connection
        .execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {table} ({kind} TEXT NOT NULL, {key} TEXT NOT NULL, {generation} INTEGER NOT NULL, \
             {document} TEXT NOT NULL, PRIMARY KEY ({kind}, {key}))"
        ))
        .expect("the documents table");
    for row in rows {
        connection
            .execute(
                &format!("INSERT INTO {table} ({kind}, {key}, {generation}, {document}) VALUES (?1, ?2, 1, ?3)"),
                (&row.kind, &row.key, row.doc.to_string()),
            )
            .expect("a state row");
    }
}

/// The rows of the state database of `dir`, in kind and key order: `{kind, key, generation, doc, stamped}`,
/// the document parsed when it is JSON and as its text otherwise. The monotonic stamp is taken out of the
/// document, because its clock and values are each implementation's own; `stamped` says it was there.
pub fn state_rows(dir: &Path) -> Value {
    let rows = regia_store::Store::at(dir).and_then(|store| store.rows()).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    let format = workspace_root().join("crates").join("regia-store").join("formats").join("store_v1.toml");
    let format: toml::Table = toml::from_str(&fs::read_to_string(&format).expect("the v1 state format")).expect("TOML");
    let stamp = format["monotonic"]["key"].as_str().expect("the key of the monotonic stamp").to_owned();
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let mut doc = serde_json::from_str::<Value>(&row.document).unwrap_or(Value::String(row.document));
            let stamped = doc.as_object_mut().is_some_and(|doc| doc.remove(&stamp).is_some());
            json!({"kind": row.kind, "key": row.key, "generation": row.generation, "doc": doc, "stamped": stamped})
        })
        .collect();
    Value::Array(rows)
}

/// What a generated holder reads as in a comparison: its 12 random hex digits are each run's own.
pub const GENERATED_HOLDER: &str = "<generated>";

/// `text` with every generated holder's digits as [`GENERATED_HOLDER`], so two runs that each generated
/// one compare equal, and a holder given by hand stays as it is.
pub fn generated_holders_as_one(text: &str) -> String {
    let prefix = regia_store::generated_holder_prefix();
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(prefix) {
        let after = &rest[at + prefix.len()..];
        out.push_str(&rest[..at + prefix.len()]);
        let digits = after.bytes().take_while(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()).count();
        if digits == 12 {
            out.push_str(GENERATED_HOLDER);
            rest = &after[12..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// [`generated_holders_as_one`] in every string of `value`.
pub fn generated_holders_as_one_in(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(generated_holders_as_one(text)),
        Value::Array(items) => Value::Array(items.iter().map(generated_holders_as_one_in).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, value)| (key.clone(), generated_holders_as_one_in(value))).collect()),
        other => other.clone(),
    }
}

/// A folder holding the fake as `gh`. It must not sit next to `regia`: Windows looks in the folder of
/// the running executable before PATH, and a `gh` there would shadow the real one in `live-check`.
fn fake_gh_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let suffix = env::consts::EXE_SUFFIX;
        let dir = bin_dir().join("conformance-state").join("fake-gh-bin");
        fs::create_dir_all(&dir).expect("fake gh folder");
        let source = bin_dir().join(format!("fake-gh{suffix}"));
        let scratch = dir.join(format!("gh-{}{suffix}", std::process::id()));
        fs::copy(&source, &scratch).unwrap_or_else(|error| panic!("{}: {error}", source.display()));
        // Another process may be running the current copy; that copy is just as good.
        if fs::rename(&scratch, dir.join(format!("gh{suffix}"))).is_err() {
            let _ = fs::remove_file(&scratch);
        }
        dir
    })
    .clone()
}

/// PATH with the fake `gh` first.
pub fn path_with_fake_gh() -> OsString {
    let mut paths = vec![fake_gh_dir()];
    if let Some(current) = env::var_os("PATH") {
        paths.extend(env::split_paths(&current));
    }
    env::join_paths(paths).expect("PATH")
}

/// Where the fake `gh` of a run in `state` appends the writes, on either side.
pub fn writes_log(state: &Path) -> PathBuf {
    state.join("gh-writes.jsonl")
}

/// The writes the fake `gh` saw in a run in `state`, in order. The body a write read on stdin has its
/// line breaks as LF, the declared normalization of [`lf`]: the prototype writes CRLF there on Windows.
pub fn writes_of_run(state: &Path) -> Vec<Value> {
    let text = fs::read_to_string(writes_log(state)).unwrap_or_default();
    text.lines()
        .map(|line| {
            let mut write: Value = serde_json::from_str(line).expect("a JSON line of the writes log");
            if let Some(Value::String(body)) = write.get_mut("stdin") {
                *body = lf(body);
            }
            write
        })
        .collect()
}

/// What every run against a fixture needs, on either side: its state folder, the fixed day and
/// instant, and no engine, dossier or session of the machine. The credentials of Claude and Codex
/// point at files that do not exist: the prototype's brief reads Codex's for a warning, its budget
/// reads both and would ask the providers with them, and the machine's own must not leak in. The home
/// is the state folder too: `init` looks for the skills there, and git for its global configuration.
fn fixture_env(launch: Launch, state: &Path) -> Launch {
    let launch = SESSION_VARIABLES.iter().fold(launch, |launch, name| launch.without(name));
    launch
        .env("HOME", state)
        .env("USERPROFILE", state)
        .env(&prototype_env_var("RUNTIME"), state)
        .env(&prototype_env_var("CODEX_AUTH"), state.join("no-codex-auth.json"))
        .env(&prototype_env_var("CLAUDE_CREDENTIALS"), state.join("no-claude-credentials.json"))
        .env(&env_var("TODAY"), FIXTURE_TODAY)
        .env(&env_var("NOW"), FIXTURE_NOW)
        .without(&env_var("ENGINE"))
        .without(&prototype_env_var("HOME"))
        .without(&prototype_env_var("RUOLO"))
}

/// The prototype against a fixture: the fake first on PATH, the fixture in its variables.
pub fn fixture_for_python(launch: Launch, responses: &Path, state: &Path) -> Launch {
    fixture_env(launch, state)
        .env("PATH", path_with_fake_gh())
        .env(&responses_var(), responses)
        .env(&writes_var(), writes_log(state))
}

/// The fake `source` at `target`, the path a run starts it by: the fake reads its fixture from the file
/// beside that path. On Linux it is a hard link, so the state folders of a test run, some four hundred,
/// share one file on disk: a copy in each filled the Linux runner's disk (#610). Elsewhere it is a copy,
/// as before.
fn place_fake(source: &Path, target: &Path) {
    // A link left by an earlier run in the same folder: a copy over it would empty the file it shares.
    let _ = fs::remove_file(target);
    #[cfg(target_os = "linux")]
    match fs::hard_link(source, target) {
        Ok(()) => return,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => panic!("{}: {error}", target.display()),
        // A file system without hard links gets a copy.
        Err(_) => {}
    }
    fs::copy(source, target).unwrap_or_else(|error| panic!("{}: {error}", source.display()));
}

/// The fake `gh` in `state/fake-gh/`, with its fixture beside it. Returns its path and the log of its
/// calls.
pub fn fake_gh_for_run(state: &Path, responses: &Path) -> (PathBuf, PathBuf) {
    let suffix = env::consts::EXE_SUFFIX;
    let dir = state.join("fake-gh");
    fs::create_dir_all(&dir).expect("fake gh folder");
    let source = bin_dir().join(format!("fake-gh{suffix}"));
    let copy = dir.join(format!("gh{suffix}"));
    place_fake(&source, &copy);
    let log = dir.join("calls.log");
    let sidecar = json!({"responses": responses, "log": log, "writes": writes_log(state)});
    fs::write(dir.join(FAKE_GH_SIDECAR), sidecar.to_string()).expect("fake gh fixture");
    (copy, log)
}

/// The product, or the app, against a fixture: its `gh` is the copy at an absolute path, and the
/// fake is nowhere on PATH.
pub fn fixture_for_rust(launch: Launch, responses: &Path, state: &Path) -> (Launch, PathBuf) {
    let (gh, log) = fake_gh_for_run(state, responses);
    (fixture_env(launch, state).env(&Program::Gh.env_name(), gh), log)
}

pub fn regia_binary() -> PathBuf {
    bin_dir().join(format!("{}{}", regia_identity::BIN_NAME, env::consts::EXE_SUFFIX))
}

/// Runs the real binary on `responses` and returns its parsed stdout. The fake's log proves that the
/// copy named by its absolute path answered.
pub fn run_rust(label: &str, args: &[String], responses: &Path) -> Result<Value, String> {
    let state = prepare_state_dir(label);
    let launch = regia_exec::harness::launch(&regia_binary(), args, &state);
    let (launch, log) = fixture_for_rust(launch, responses, &state);
    let output = launch.run().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("regia failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    if fs::read_to_string(&log).unwrap_or_default().is_empty() {
        return Err("the fake gh at its absolute path was not called".to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("regia printed invalid JSON: {error}"))
}

/// A run of the binary on a case.
pub struct CliRun {
    /// `None` if it ended without an exit code.
    pub exit: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
    /// The writes the fake `gh` saw, in order, with the placeholder in place of the dossier's folder.
    pub writes: Vec<Value>,
    /// The folder of the case's dossier, if it has one: its arguments named it.
    pub dossier: Option<PathBuf>,
    /// The dossier's repo after the run, when the run had a repo to work on.
    pub dossier_after: Option<Value>,
    /// The run's state folder, with its state database.
    pub state: PathBuf,
}

/// What the stand-in interpreter replays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replay {
    pub argv: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit: u8,
    /// The stdout is JSON, with the dossier's placeholder inside its strings.
    #[serde(default)]
    pub json: bool,
}

impl Replay {
    /// What the prototype did for `case`, started as the facade starts it: the interpreter's flags,
    /// the script, and the argv the case expects.
    pub fn of(case: &Case) -> Replay {
        let python = case.spec.python.clone().unwrap_or_else(|| panic!("case {} does not go through the facade", case.name));
        let (stdout, json) = match case.recorded() {
            Recorded::Json(value) => (format!("{}\n", serde_json::to_string_pretty(&value).expect("JSON")), true),
            Recorded::Text(text) => (text, false),
        };
        let argv = regia_exec::python::FLAGS.iter().map(|flag| (*flag).to_owned()).chain([SCRIPT.to_owned()]).chain(python).collect();
        Replay { argv, stdout, stderr: case.recorded_stderr(), exit: case.spec.exit, json }
    }

    /// The replay of a run whose dossier is `dossier`: its folder where the recording has the placeholder.
    fn placed(&self, dossier: &Path) -> Replay {
        let folder = dossier.display().to_string();
        let stdout = if self.json {
            let value: Value = serde_json::from_str(&self.stdout).expect("the replayed JSON");
            format!("{}\n", serde_json::to_string_pretty(&place_json(&value, dossier)).expect("JSON"))
        } else {
            self.stdout.replace(DOSSIER_PLACEHOLDER, &folder)
        };
        Replay {
            argv: place_args(&self.argv, dossier),
            stdout,
            stderr: self.stderr.replace(DOSSIER_PLACEHOLDER, &folder),
            ..self.clone()
        }
    }
}

/// A dossier with only the prototype's entry point in it, and a copy of the stand-in interpreter that
/// replays `replay`, with the folder of the run's own case dossier in place of the placeholder.
/// Returns the variables that point the binary at them.
pub fn replay_prototype(state: &Path, replay: &Replay) -> Vec<(String, PathBuf)> {
    let replay = &replay.placed(&dossier_dir(state));
    let dossier = state.join("dossier");
    let script = dossier.join(SCRIPT);
    fs::create_dir_all(script.parent().expect("the script's folder")).expect("the dossier");
    fs::write(&script, "# The stand-in interpreter replays a recording and never reads this file.\n").expect("the script");
    let dir = state.join("fake-python");
    fs::create_dir_all(&dir).expect("fake interpreter folder");
    let suffix = env::consts::EXE_SUFFIX;
    let source = bin_dir().join(format!("fake-python{suffix}"));
    let copy = dir.join(format!("python{suffix}"));
    place_fake(&source, &copy);
    fs::write(dir.join(FAKE_PYTHON_SIDECAR), serde_json::to_string(replay).expect("JSON")).expect("the replay");
    vec![(prototype_env_var("HOME"), dossier), (env_var("PYTHON"), copy)]
}

/// A dossier whose entry point is the real prototype of `dossier` at [`FIXTURE_NOW`], for the binary to
/// start through the facade as it starts the prototype: the clock is the one place the two runs could
/// differ, and the facade passes no driver. Its `tools/regia.py` points `REGIA_HOME` back at `dossier`
/// and hands its arguments to `oracle/fixed_now.py`. Returns the folder to name in `REGIA_HOME`.
pub fn prototype_at_fixture_now(state: &Path, dossier: &Path) -> PathBuf {
    let shim = state.join("dossier-at-fixture-now");
    let script = shim.join(SCRIPT);
    fs::create_dir_all(script.parent().expect("the script's folder")).expect("the shim dossier");
    // JSON string literals are Python string literals too, backslashes of Windows paths included.
    let literal = |path: &Path| serde_json::to_string(&path.display().to_string()).expect("JSON");
    let text = format!(
        "# Written by the conformance harness: the prototype of the dossier at the fixtures' instant.\n\
         import os, runpy, sys\n\
         os.environ[{home}] = {dossier}\n\
         sys.argv = [{driver}, {script}, {now}, *sys.argv[1:]]\n\
         runpy.run_path({driver}, run_name=\"__main__\")\n",
        home = serde_json::to_string(&prototype_env_var("HOME")).expect("JSON"),
        dossier = literal(dossier),
        driver = literal(&fixed_now_driver()),
        script = literal(&dossier.join(SCRIPT)),
        now = serde_json::to_string(FIXTURE_NOW).expect("JSON"),
    );
    fs::write(&script, text).expect("the shim's entry point");
    shim
}

/// The real binary on `case`, with the fake `gh` and the case's state, plus the variables in `extra`.
/// Its arguments name the folder of the case's dossier, which stays empty: a run on the stand-in
/// interpreter never reads it.
pub fn run_case(label: &str, case: &Case, extra: impl FnOnce(&Path) -> Vec<(String, OsString)>) -> CliRun {
    run_case_in(label, case, false, extra)
}

/// [`run_case`] on the case's dossier, built as the recording built it, for a run that reaches the
/// prototype; the run carries the dossier's repo after it.
pub fn run_case_on_dossier(label: &str, case: &Case, extra: impl FnOnce(&Path) -> Vec<(String, OsString)>) -> CliRun {
    run_case_in(label, case, true, extra)
}

fn run_case_in(label: &str, case: &Case, build: bool, extra: impl FnOnce(&Path) -> Vec<(String, OsString)>) -> CliRun {
    let state = prepare_case_state(label, case);
    let responses = if case.responses_path().is_file() {
        case.responses_path()
    } else {
        let empty = state.join("no-responses.json");
        fs::write(&empty, "{}").expect("empty responses");
        empty
    };
    let dossier = case.spec.dossier.as_ref().map(|spec| if build { prepare_dossier(&state, spec) } else { dossier_dir(&state) });
    let args = dossier.as_ref().map_or_else(|| case.spec.args.clone(), |dossier| place_args(&case.spec.args, dossier));
    let launch = regia_exec::harness::launch(&regia_binary(), &args, &state);
    let (mut launch, _) = fixture_for_rust(launch, &responses, &state);
    for (name, value) in case.spec.session_env() {
        launch = launch.env(&name, value);
    }
    if let Some(double) = case.spec.dossier.as_ref().and_then(|spec| spec.git_double) {
        for (name, value) in git_double_env(&state, double) {
            launch = launch.env(&name, value);
        }
    }
    for (name, value) in extra(&state) {
        launch = launch.env(&name, value);
    }
    let output = launch.run().expect("run regia");
    let dossier_after = case.spec.dossier.as_ref().filter(|_| build).and_then(|spec| dossier_after(&state, spec));
    let writes = writes_of_run(&state);
    CliRun {
        exit: output.status.code(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        writes: dossier.as_ref().map_or(writes.clone(), |dossier| writes.iter().map(|write| unplace_json(write, dossier)).collect()),
        dossier,
        dossier_after,
        state,
    }
}

/// Whether a comparison counts the writes. Only a run that reaches a real implementation makes them:
/// the stand-in interpreter does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writes {
    Compare,
    Skip,
}

/// Text with its line breaks as LF. The one normalization of text, declared: the prototype writes
/// CRLF on Windows, the repo stores LF, and the Rust side writes LF (ADR 0001 §11).
pub fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Every way `run` differs from what the prototype did for `case`: exit code, stdout through the
/// table or as text, stderr, and with [`Writes::Compare`] the writes in order and the dossier's repo
/// after the run. The run's dossier reads as the placeholder, as in the recording, and a generated
/// holder as [`GENERATED_HOLDER`]. Empty when they agree.
pub fn differences(case: &Case, run: &CliRun, writes: Writes) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    let text = |text: &str| run.dossier.as_ref().map_or_else(|| text.to_owned(), |dossier| unplace_text(text, dossier));
    if case.spec.same_class_only {
        out.extend(class_differences(case, run));
    } else {
        out.extend(output_differences(case, run, &text));
    }
    if writes == Writes::Compare {
        let diff = json_diff(&Value::Array(run.writes.clone()), &Value::Array(case.writes()));
        out.extend(diff.into_iter().map(|line| format!("writes {line}")));
        let after = |value: Option<Value>| value.unwrap_or(Value::Null);
        let diff = json_diff(&after(run.dossier_after.clone()), &after(case.dossier_after()));
        out.extend(diff.into_iter().map(|line| format!("dossier {line}")));
    }
    out
}

/// A case of [`CaseSpec::same_class_only`]: the outcome of the dossier, where the output holds one. The
/// exit code is compared by the caller; what an error says is each side's own.
pub fn class_differences(case: &Case, run: &CliRun) -> Vec<String> {
    let outcome = |value: &Value| value["dossier"]["outcome"].as_str().map(str::to_owned);
    let rust = serde_json::from_slice::<Value>(&run.stdout).ok().and_then(|value| outcome(&value));
    let python = match case.recorded() {
        Recorded::Json(python) => translate_python(&case.command_name(), &python).ok().and_then(|value| outcome(&value)),
        Recorded::Text(_) => None,
    };
    if rust == python { Vec::new() } else { vec![format!("outcome: rust {rust:?}, python {python:?}")] }
}

fn output_differences(case: &Case, run: &CliRun, text: &dyn Fn(&str) -> String) -> Vec<String> {
    let mut out = Vec::new();
    match case.recorded() {
        Recorded::Json(python) => match (serde_json::from_slice::<Value>(&run.stdout), translate_python(&case.command_name(), &python)) {
            (Ok(rust), Ok(python)) => {
                let rust = run.dossier.as_ref().map_or_else(|| rust.clone(), |dossier| unplace_json(&rust, dossier));
                let (rust, python) = (generated_holders_as_one_in(&rust), generated_holders_as_one_in(&python));
                out.extend(json_diff(&rust, &python).into_iter().map(|line| format!("stdout {line}")));
            }
            (Err(error), _) => out.push(format!("stdout: rust printed no JSON ({error}): {}", String::from_utf8_lossy(&run.stdout))),
            (_, Err(error)) => out.push(format!("stdout: the table does not cover the prototype's output: {error}")),
        },
        Recorded::Text(python) => {
            let rust = generated_holders_as_one(&text(&lf(&String::from_utf8_lossy(&run.stdout))));
            let python = generated_holders_as_one(&python);
            if rust != python {
                out.push(format!("stdout: rust {rust:?}, python {python:?}"));
            }
        }
    }
    let stderr = case.recorded_stderr();
    if case.spec.python.is_some() && text(&lf(&run.stderr)) != stderr {
        out.push(format!("stderr: rust {:?}, python {stderr:?}", run.stderr));
    }
    out
}

/// The Python output in English, through the table the CLI's passthrough uses.
pub fn translate_python(command: &str, python: &Value) -> Result<Value, FacadeError> {
    Table::embedded().translate_output(command, python)
}

/// The Rust brief marks the sections it does not produce; the prototype has no such key. The harness
/// checks the marker and removes it, so an unported section is declared, never silently absent.
pub fn take_not_ported(rust: &mut Value) -> Result<Vec<String>, String> {
    let marker = rust
        .as_object_mut()
        .and_then(|object| object.remove("not_ported"))
        .ok_or("the Rust brief has no not_ported marker")?;
    serde_json::from_value(marker).map_err(|error| error.to_string())
}

/// The first field of a mutation, which names it, and the text of its arguments with no blank outside
/// a string: `closeIssue` and `input:{issueId:$i,stateReason:$r}` in
/// `mutation($i:ID!,$r:…){closeIssue(input:{issueId:$i, stateReason:$r}){…}}`. The arguments carry what
/// the variables do not: a value written in the query, such as `replaceParent:true`. `None` for a query.
pub fn mutation_call(query: &str) -> Option<(String, String)> {
    let text: Vec<char> = query.chars().collect();
    let mut at = 0;
    skip_blank(&text, &mut at);
    if !text[at..].starts_with(&['m', 'u', 't', 'a', 't', 'i', 'o', 'n']) {
        return None;
    }
    at += 8;
    skip_blank(&text, &mut at);
    if text.get(at) == Some(&'(') {
        skip_arguments(&text, &mut at).ok()?;
        skip_blank(&text, &mut at);
    }
    if text.get(at) != Some(&'{') {
        return None;
    }
    at += 1;
    skip_blank(&text, &mut at);
    let name = Some(read_name(&text, &mut at)).filter(|name| !name.is_empty())?;
    skip_blank(&text, &mut at);
    let arguments = if text.get(at) == Some(&'(') { skip_arguments(&text, &mut at).ok()? } else { String::new() };
    Some((name, compact(&arguments)))
}

/// `text` without the blanks outside its string literals: two queries that differ only in layout read the
/// same. Commas stay, so no two tokens run together.
fn compact(text: &str) -> String {
    let mut out = String::new();
    let (mut quoted, mut escaped) = (false, false);
    for c in text.chars() {
        if quoted {
            out.push(c);
            (escaped, quoted) = (!escaped && c == '\\', escaped || c != '"');
        } else if c == '"' {
            out.push(c);
            quoted = true;
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

#[derive(Debug)]
struct Selection {
    name: String,
    /// The key of the answer, when it is not the field's name.
    alias: Option<String>,
    /// A `name: "…"` argument written in the query, for the fields that pick a value by name.
    by_name: Option<String>,
    fragment: bool,
    children: Vec<Selection>,
}

/// The one field the fake evaluates with its argument: the fixture holds its values in a map keyed by
/// field name, and a name with no value answers null, as on the real API.
const BY_NAME_FIELD: &str = "fieldValueByName";

fn skip_blank(text: &[char], at: &mut usize) {
    while text.get(*at).is_some_and(|c| c.is_whitespace() || *c == ',') {
        *at += 1;
    }
}

fn read_name(text: &[char], at: &mut usize) -> String {
    let start = *at;
    while text.get(*at).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
        *at += 1;
    }
    text[start..*at].iter().collect()
}

/// Skips `( … )`, which can hold braces and quoted strings, and returns what was inside.
fn skip_arguments(text: &[char], at: &mut usize) -> Result<String, String> {
    let start = *at + 1;
    let mut depth = 0;
    let mut quoted = false;
    while let Some(&c) = text.get(*at) {
        *at += 1;
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Ok(text[start..*at - 1].iter().collect());
                }
            }
            _ => {}
        }
    }
    Err("unbalanced parentheses in the query".to_owned())
}

/// The value of a `name: "…"` argument, if the arguments have one written as a string.
fn name_argument(arguments: &str) -> Option<String> {
    arguments.split(',').find_map(|argument| {
        let value = argument.trim().strip_prefix("name")?.trim_start().strip_prefix(':')?;
        let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
        Some(value.to_owned())
    })
}

/// Reads a selection set; `at` is just after its `{`.
fn parse_selection(text: &[char], at: &mut usize) -> Result<Vec<Selection>, String> {
    let mut out = Vec::new();
    loop {
        skip_blank(text, at);
        match text.get(*at) {
            None => return Err("unbalanced braces in the query".to_owned()),
            Some('}') => {
                *at += 1;
                return Ok(out);
            }
            Some('.') => {
                *at += 3;
                skip_blank(text, at);
                if read_name(text, at) != "on" {
                    return Err("expected an inline fragment".to_owned());
                }
                skip_blank(text, at);
                let name = read_name(text, at);
                skip_blank(text, at);
                if text.get(*at) != Some(&'{') {
                    return Err(format!("fragment on {name} has no selection"));
                }
                *at += 1;
                let children = parse_selection(text, at)?;
                out.push(Selection { name, alias: None, by_name: None, fragment: true, children });
            }
            Some(_) => {
                let mut name = read_name(text, at);
                if name.is_empty() {
                    return Err(format!("unexpected text in the query at {at}"));
                }
                skip_blank(text, at);
                let mut alias = None;
                if text.get(*at) == Some(&':') {
                    *at += 1;
                    skip_blank(text, at);
                    alias = Some(std::mem::replace(&mut name, read_name(text, at)));
                    if name.is_empty() {
                        return Err(format!("alias {alias:?} names no field"));
                    }
                    skip_blank(text, at);
                }
                let mut by_name = None;
                if text.get(*at) == Some(&'(') {
                    by_name = name_argument(&skip_arguments(text, at)?);
                    skip_blank(text, at);
                }
                let mut children = Vec::new();
                if text.get(*at) == Some(&'{') {
                    *at += 1;
                    children = parse_selection(text, at)?;
                }
                out.push(Selection { name, alias, by_name, fragment: false, children });
            }
        }
    }
}

fn project(selections: &[Selection], value: &Value, optional: bool) -> Result<Value, String> {
    match value {
        Value::Array(items) => {
            items.iter().map(|item| project(selections, item, optional)).collect::<Result<Vec<_>, _>>().map(Value::Array)
        }
        Value::Object(object) => {
            let mut out = serde_json::Map::new();
            for selection in selections {
                if selection.fragment {
                    // A fragment on another type selects nothing, as on the real API.
                    if let Value::Object(part) = project(&selection.children, value, true)? {
                        out.extend(part);
                    }
                    continue;
                }
                let key = selection.alias.clone().unwrap_or_else(|| selection.name.clone());
                let found = match (&selection.by_name, object.get(&selection.name)) {
                    (Some(field), Some(values)) if selection.name == BY_NAME_FIELD => {
                        Some(values.get(field).cloned().unwrap_or(Value::Null))
                    }
                    (_, found) => found.cloned(),
                };
                match found {
                    Some(inner) if selection.children.is_empty() => {
                        out.insert(key, inner);
                    }
                    Some(inner) => {
                        out.insert(key, project(&selection.children, &inner, false)?);
                    }
                    None if optional => {}
                    None => return Err(format!("the query selects `{}`, unknown to the fixture", selection.name)),
                }
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

/// What a GraphQL server would return for `query` on `value`: only the selected fields, and an error
/// on a field that does not exist.
pub fn project_query(query: &str, value: &Value) -> Result<Value, String> {
    let text: Vec<char> = query.chars().collect();
    let mut at = 0;
    skip_blank(&text, &mut at);
    for keyword in ["query", "mutation"] {
        let word: Vec<char> = keyword.chars().collect();
        if text[at..].starts_with(&word) {
            at += word.len();
            skip_blank(&text, &mut at);
            if text.get(at) == Some(&'(') {
                skip_arguments(&text, &mut at)?;
                skip_blank(&text, &mut at);
            }
            break;
        }
    }
    if text.get(at) != Some(&'{') {
        return Err("the query has no selection set".to_owned());
    }
    at += 1;
    project(&parse_selection(&text, &mut at)?, value, false)
}

/// Differences as JSON paths. Object key order does not count; array order does, because the order
/// of the sections is the logic under test.
pub fn json_diff(left: &Value, right: &Value) -> Vec<String> {
    let mut out = Vec::new();
    diff_at("$", left, right, &mut out);
    out
}

fn diff_at(path: &str, left: &Value, right: &Value, out: &mut Vec<String>) {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                match b.get(key) {
                    Some(other) => diff_at(&format!("{path}.{key}"), value, other, out),
                    None => out.push(format!("{path}.{key}: only in rust")),
                }
            }
            for key in b.keys().filter(|key| !a.contains_key(*key)) {
                out.push(format!("{path}.{key}: only in python"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                out.push(format!("{path}: rust has {} items, python {}", a.len(), b.len()));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff_at(&format!("{path}[{i}]"), x, y, out);
            }
        }
        (a, b) if a != b => out.push(format!("{path}: rust {a}, python {b}")),
        _ => {}
    }
}
