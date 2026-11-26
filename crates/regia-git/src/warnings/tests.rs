//! The warnings on real, temporary repos with bare remotes, as the prototype's tests of the dossier's warnings,
//! of the workers' branches and of the sites of the products prove `avvisi_dossier`. The repos live under the build's
//! own folder, never the system's temporary one; each has its own identity, no signing, no hooks and no line-break
//! conversion. Where the prototype's tests replace its `_git` to make a call fail, these put a spy between the
//! warnings and git.

use std::cell::RefCell;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use regia_exec::{Pin, Program};

use super::*;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const OWNER: &str = "example";
const DOSSIER_REPO: &str = "example/dossier";
const PRODUCT: &str = "example/product";

/// A folder of its own under the build's folder.
fn scratch(tag: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test's path");
    let base = exe.parent().and_then(Path::parent).expect("the build's folder").join("regia-git-warnings");
    let dir = base.join(format!("{}-{}-{tag}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// `git <words…> <values…>` in `dir`, which must succeed; its stdout, trimmed at the end.
fn git(dir: &Path, words: &[&'static str], values: &[&str]) -> String {
    let found = git_or_none(dir, words, values);
    found.unwrap_or_else(|| panic!("git {words:?} {values:?} failed in {}", dir.display()))
}

/// `git <words…> <values…>` in `dir`: its stdout, trimmed at the end, when it succeeds.
fn git_or_none(dir: &Path, words: &[&'static str], values: &[&str]) -> Option<String> {
    let args = words.iter().fold(Args::new(), |args, word| args.word(word));
    let args = values.iter().fold(args, |args, value| args.value(&Text::parse(value).expect("a value")));
    let output = Pin::resolve(Program::Git).expect("git").launch(args, dir).run().expect("git runs");
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
}

fn write(dir: &Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().expect("a file's folder")).expect("the file's folder");
    fs::write(path, text).expect("a file");
}

/// A repo in `dir` on `main`, that reads nothing of the machine's configuration that would change a commit.
fn init(dir: &Path, main: &str) {
    fs::create_dir_all(dir).expect("the repo's folder");
    git(dir, &["init", "-q", "-b"], &[main]);
    for (key, value) in [("user.name", "Trial"), ("user.email", "trial@example.invalid"), ("commit.gpgsign", "false"), ("core.autocrlf", "false"), ("core.hooksPath", ".git/no-hooks")] {
        git(dir, &["config"], &[key, value]);
    }
}

fn commit(dir: &Path, relative: &str) {
    write(dir, relative, &format!("{relative}\n"));
    git(dir, &["add", "--"], &[relative]);
    git(dir, &["commit", "-q", "-m"], &[relative]);
}

/// A repo `dir` with one commit on `main`, pushed to the bare `bare`, its upstream.
fn pushed_repo(dir: &Path, bare: &Path, main: &str) {
    fs::create_dir_all(bare).expect("the bare's folder");
    git(bare, &["init", "-q", "--bare", "-b"], &[main]);
    init(dir, main);
    commit(dir, "first.txt");
    git(dir, &["remote", "add", "origin"], &[bare.to_str().expect("a path")]);
    git(dir, &["push", "-q", "-u", "origin"], &[main]);
}

/// The prototype's `Repo`: a dossier `work` with its bare `origin.git`, and beside it, for a product, its clone
/// `product` with a bare whose path ends as a real remote's URL does, `…/example/product.git`.
struct Fixture {
    root: PathBuf,
    work: PathBuf,
    main: &'static str,
}

impl Fixture {
    fn new(tag: &str) -> Fixture {
        Fixture::on(tag, "main")
    }

    fn on(tag: &str, main: &'static str) -> Fixture {
        let root = scratch(tag);
        let work = root.join("work");
        pushed_repo(&work, &root.join("origin.git"), main);
        Fixture { root, work, main }
    }

    fn clone_dir(&self) -> PathBuf {
        self.root.join("product")
    }

    fn with_product(self) -> Fixture {
        pushed_repo(&self.clone_dir(), &self.root.join("remotes").join(OWNER).join("product.git"), "main");
        self
    }

    /// A branch from `main` with `commits` commits, pushed or not; the repo goes back to `main`.
    fn branch_in(&self, dir: &Path, name: &str, commits: usize, push: bool) {
        git(dir, &["checkout", "-q", "-b"], &[name]);
        for at in 0..commits {
            commit(dir, &format!("{}-{at}.txt", name.replace('/', "_")));
        }
        if push {
            git(dir, &["push", "-q", "-u", "origin"], &[name]);
        }
        git(dir, &["checkout", "-q"], &[self.main_of(dir)]);
    }

    fn main_of(&self, dir: &Path) -> &'static str {
        if dir == self.work { self.main } else { "main" }
    }

    fn branch(&self, name: &str, commits: usize, push: bool) {
        self.branch_in(&self.work, name, commits, push);
    }

    /// A report of the dossier that names `branch` at its commit, as a worker writes it.
    fn report(&self, file: &str, branch: &str, kind: &str, task: u64, extra: &str) {
        let words = &format().reports;
        let sha: String = git_or_none(&self.work, &["rev-parse", "--verify", "-q"], &[&format!("refs/heads/{branch}")]).unwrap_or_else(|| "0".repeat(7)).chars().take(7).collect();
        let text = format!(
            "{fence}\n{t}: {kind}\n{k}: {task}\nsummary: \"x\"\n{extra}{b}: \"{branch}@{sha}\"  # name@sha\n{fence}\nbody\n",
            fence = words.fence,
            t = words.type_key,
            k = words.task_key,
            b = words.branch_key
        );
        write(&self.work, &format!("{}/{file}.md", words.folder), &text);
    }

    fn project(&self) -> DossierProject {
        DossierProject { id: Some("p".to_owned()), folder: Some(self.work.display().to_string()), repo: Some(DOSSIER_REPO.to_owned()), ..DossierProject::default() }
    }

    fn with(&self, products: &str) -> DossierProject {
        DossierProject { products: Some(products.to_owned()), ..self.project() }
    }

    fn warnings(&self) -> Vec<Warning> {
        run(&[self.project()], &BTreeSet::new())
    }

    fn of_products(&self, products: &str) -> Vec<Warning> {
        run(&[self.with(products)], &BTreeSet::new())
    }
}

fn request(projects: &[DossierProject], in_progress: &BTreeSet<u64>) -> WarningsRequest {
    WarningsRequest { projects: projects.to_vec(), deadline: None, in_progress: in_progress.clone(), owner: Some(OWNER.to_owned()) }
}

fn run(projects: &[DossierProject], in_progress: &BTreeSet<u64>) -> Vec<Warning> {
    dossier_warnings(&request(projects, in_progress))
}

fn problems(warnings: &[Warning]) -> Vec<Problem> {
    warnings.iter().map(|warning| warning.problem).collect()
}

fn one(warnings: Vec<Warning>) -> Warning {
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    warnings.into_iter().next().expect("one warning")
}

/// What a test answers in place of git for a call, by its folder and words: `None` lets git answer.
type Fake<'a> = dyn Fn(&Path, &[String]) -> Option<Run> + 'a;

/// The user's git, with the calls a test watches: every call seen, by its folder and words, and an answer in place
/// of git's for those `fake` picks.
struct Spy<'a> {
    real: Calls,
    seen: RefCell<Vec<(PathBuf, Vec<String>)>>,
    fake: Box<Fake<'a>>,
}

impl<'a> Spy<'a> {
    fn new(fake: impl Fn(&Path, &[String]) -> Option<Run> + 'a) -> Spy<'a> {
        Spy { real: Calls { git: Git::resolve().map_err(|error| error.to_string()), deadline: None }, seen: RefCell::default(), fake: Box::new(fake) }
    }

    fn watching() -> Spy<'a> {
        Spy::new(|_, _| None)
    }

    fn warnings(&self, projects: &[DossierProject], in_progress: &BTreeSet<u64>) -> Vec<Warning> {
        warnings(self, &request(projects, in_progress))
    }

    fn calls(&self, first: &str) -> usize {
        self.seen.borrow().iter().filter(|(_, words)| words.first().is_some_and(|word| word == first)).count()
    }

    fn any(&self, words: &[&str]) -> bool {
        self.seen.borrow().iter().any(|(_, seen)| seen.first().is_some_and(|first| words.contains(&first.as_str())))
    }
}

impl Runner for Spy<'_> {
    fn run(&self, folder: &Path, args: Args) -> Run {
        let words: Vec<String> = args.tokens().map(|token| token.to_string_lossy().into_owned()).collect();
        self.seen.borrow_mut().push((folder.to_path_buf(), words.clone()));
        (self.fake)(folder, &words).unwrap_or_else(|| self.real.run(folder, args))
    }
}

fn timed_out(said: &str) -> Run {
    Run { code: TIMED_OUT, stdout: String::new(), stderr: said.to_owned() }
}

fn worker(task: u64, slug: &str) -> String {
    format!("{OWNER}/{}{task}-{slug}", format().worker_branch.prefix)
}

// ---------- the dossier itself ----------

/// A dossier in step with its upstream has no warning: after a push, with changes not committed, with a remote that cannot be reached.
#[test]
fn a_dossier_in_order_has_no_warning() {
    let fixture = Fixture::new("in-order");
    assert_eq!(fixture.warnings(), []);
    commit(&fixture.work, "second.txt");
    git(&fixture.work, &["push", "-q"], &[]);
    assert_eq!(fixture.warnings(), [], "after a push");
    write(&fixture.work, "first.txt", "changed\n");
    write(&fixture.work, "new.txt", "new\n");
    assert_eq!(fixture.warnings(), [], "changes not committed are no commits not pushed");
    let gone = fixture.root.join("gone.git");
    git(&fixture.work, &["remote", "set-url", "origin"], &[gone.to_str().expect("a path")]);
    assert_eq!(fixture.warnings(), [], "only local git: nothing is known of the remote");
}

/// The dossier's own problems: commits not pushed with their count, no repo in the registry, no upstream, no `origin`, a detached `HEAD`.
#[test]
fn the_dossier_s_own_problems() {
    let fixture = Fixture::new("own");
    commit(&fixture.work, "two.txt");
    commit(&fixture.work, "three.txt");
    let ahead = one(fixture.warnings());
    assert_eq!((ahead.problem, ahead.branch.as_deref(), ahead.ahead), (Problem::NotPushed, Some("main"), Some(2)));
    assert_eq!((ahead.project.as_deref(), ahead.folder.as_str()), (Some("p"), fixture.work.display().to_string().as_str()));
    assert_eq!((ahead.detail, ahead.repo, ahead.task, ahead.never_pushed, ahead.expected, ahead.site), (None, None, None, None, false, false));
    let no_key = one(run(&[DossierProject { repo: Some(String::new()), ..fixture.project() }], &BTreeSet::new()));
    assert_eq!((no_key.problem, no_key.branch.as_deref()), (Problem::NoRemote, Some("main")));
    assert_eq!(no_key.detail.as_deref(), Some(format().details.no_repo_key.as_str()));
    git(&fixture.work, &["branch", "--unset-upstream"], &[]);
    let upstream = one(fixture.warnings());
    assert_eq!((upstream.problem, upstream.branch.as_deref(), upstream.ahead, upstream.detail), (Problem::NoUpstream, Some("main"), None, None));
    git(&fixture.work, &["remote", "remove", "origin"], &[]);
    let remote = one(fixture.warnings());
    assert_eq!((remote.problem, remote.branch.as_deref(), remote.ahead), (Problem::NoRemote, Some("main"), None));
    assert!(remote.detail.as_deref().is_some_and(|detail| detail.contains("origin")), "{remote:?}");
    git(&fixture.work, &["checkout", "-q", "--detach"], &[]);
    let detached = one(fixture.warnings());
    assert_eq!((detached.problem, detached.branch), (Problem::DetachedHead, None));
}

/// A folder missing, one that is no repo or below one, a project without a folder; several projects, a warning each only where it is due.
#[test]
fn a_folder_out_of_place_and_several_projects() {
    let fixture = Fixture::new("places");
    let at = |folder: PathBuf| DossierProject { folder: Some(folder.display().to_string()), ..fixture.project() };
    assert_eq!(problems(&run(&[at(fixture.root.join("missing"))], &BTreeSet::new())), [Problem::FolderMissing]);
    let empty = fixture.root.join("empty");
    fs::create_dir_all(&empty).expect("a folder");
    assert_eq!(problems(&run(&[at(empty)], &BTreeSet::new())), [Problem::NotARepo]);
    let below = fixture.work.join("below");
    fs::create_dir_all(&below).expect("a folder");
    assert_eq!(problems(&run(&[at(below)], &BTreeSet::new())), [Problem::NotARepo], "a repo further up is not the dossier");
    let blank = DossierProject { folder: Some("  ".to_owned()), ..fixture.project() };
    assert_eq!(run(&[blank, DossierProject { id: Some("q".to_owned()), ..DossierProject::default() }], &BTreeSet::new()), []);
    git(&fixture.work, &["branch", "--unset-upstream"], &[]);
    let fine = DossierProject { id: Some("fine".to_owned()), ..at(fixture.root.join("work2")) };
    let wrong = DossierProject { id: Some("wrong".to_owned()), ..fixture.project() };
    let found: Vec<(Option<String>, Problem)> = run(&[fine, wrong], &BTreeSet::new()).into_iter().map(|warning| (warning.project, warning.problem)).collect();
    assert_eq!(found, [(Some("fine".to_owned()), Problem::FolderMissing), (Some("wrong".to_owned()), Problem::NoUpstream)]);
}

/// The main branch's name is not taken for granted: `master`, and the remote's `HEAD` over the names of the defaults.
#[test]
fn the_main_branch_is_not_taken_for_granted() {
    let fixture = Fixture::on("master", "master");
    commit(&fixture.work, "two.txt");
    let ahead = one(fixture.warnings());
    assert_eq!((ahead.problem, ahead.branch.as_deref(), ahead.ahead), (Problem::NotPushed, Some("master"), Some(1)));
    git(&fixture.work, &["push", "-q"], &[]);
    let slice = worker(158, "trial");
    fixture.branch(&slice, 1, true);
    git(&fixture.work, &["merge", "-q", "--ff-only"], &[&slice]);
    git(&fixture.work, &["push", "-q"], &[]);
    assert_eq!(fixture.warnings(), [], "in origin/master");
    git(&fixture.work, &["checkout", "-q"], &[&slice]);
    commit(&fixture.work, "more.txt");
    git(&fixture.work, &["checkout", "-q", "master"], &[]);
    assert_eq!(one(fixture.warnings()).ahead, Some(1));

    // The remote's HEAD wins over the names of the defaults.
    let fixture = Fixture::on("trunk", "master");
    git(&fixture.work, &["push", "-q", "origin", "master:trunk"], &[]);
    git(&fixture.work, &["fetch", "-q"], &[]);
    git(&fixture.work, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/trunk"], &[]);
    fixture.branch(&slice, 1, false);
    git(&fixture.work, &["push", "-q", "origin"], &[&format!("{slice}:trunk")]);
    git(&fixture.work, &["fetch", "-q"], &[]);
    assert_eq!(fixture.warnings(), [], "in origin/trunk, the main branch indeed");
}

// ---------- git that fails ----------

/// A git that does not answer or cannot start is a warning, not an error; each call's deadline is short; a deadline spent starts no git.
#[test]
fn a_git_that_does_not_answer_or_cannot_start_is_a_warning() {
    let fixture = Fixture::new("broken");
    let spy = Spy::new(|_, _| Some(timed_out("git rev-parse: no answer within 10 s")));
    let silent = one(spy.warnings(&[fixture.project()], &BTreeSet::new()));
    assert_eq!((silent.problem, silent.branch.clone(), silent.ahead), (Problem::GitFailed, None, None));
    assert!(silent.detail.as_deref().is_some_and(|detail| detail.contains("no answer")), "{silent:?}");
    let missing = Err(ExecError::NotFound { program: Program::Git, variable: Program::Git.env_name() });
    let none = one(dossier_warnings_with(missing, &request(&[fixture.project()], &BTreeSet::new())));
    assert_eq!(none.problem, Problem::GitFailed);
    assert!(format().call_timeout_s <= 10, "the call's deadline is short");
    let past = WarningsRequest { deadline: Some(Instant::now()), ..request(&[fixture.project()], &BTreeSet::new()) };
    let spent = one(dossier_warnings(&past));
    assert_eq!(spent.problem, Problem::GitFailed);
    assert!(spent.detail.as_deref().is_some_and(|detail| detail.contains("deadline")), "no git started: {spent:?}");
}

/// The count failing on one project leaves the next one read as usual; a count that does not read is a failure too.
#[test]
fn a_failure_midway_stays_on_its_project() {
    let fixture = Fixture::new("midway");
    commit(&fixture.work, "two.txt");
    let spy = Spy::new(|_, words| (words[0] == "rev-list").then(|| timed_out("git rev-list: no answer")));
    let other = DossierProject { id: Some("q".to_owned()), folder: Some(fixture.root.join("no").display().to_string()), ..fixture.project() };
    let found: Vec<(Option<String>, Problem)> = spy.warnings(&[fixture.project(), other], &BTreeSet::new()).into_iter().map(|warning| (warning.project, warning.problem)).collect();
    assert_eq!(found, [(Some("p".to_owned()), Problem::GitFailed), (Some("q".to_owned()), Problem::FolderMissing)]);
    let garbled = Spy::new(|_, words| (words[0] == "rev-list").then(|| Run { code: 0, stdout: "many".to_owned(), stderr: String::new() }));
    let unreadable = one(garbled.warnings(&[fixture.project()], &BTreeSet::new()));
    assert_eq!(unreadable.problem, Problem::GitFailed);
    assert_eq!(unreadable.detail, Some(format().details.rev_list.replace("{git}", &format().details.no_message)));
}

/// No fetch and no network: of the remote, only the URL of `origin` is asked.
#[test]
fn no_fetch_and_no_network() {
    let fixture = Fixture::new("local");
    commit(&fixture.work, "two.txt");
    fixture.branch(&worker(158, "trial"), 1, false);
    let spy = Spy::watching();
    assert_eq!(problems(&spy.warnings(&[fixture.project()], &BTreeSet::new())), [Problem::NotPushed, Problem::BranchNotPushed]);
    assert!(!spy.any(&["fetch", "pull", "push", "ls-remote"]));
    let remotes: Vec<Vec<String>> = spy.seen.borrow().iter().filter(|(_, words)| words[0] == "remote").map(|(_, words)| words.clone()).collect();
    assert!(remotes.iter().all(|words| words[1] == "get-url"), "{remotes:?}");
}

// ---------- the workers' branches ----------

/// A worker's branch a commit ahead of its remote, or never pushed; nothing at the remote's commit.
#[test]
fn a_worker_s_branch_ahead_of_the_remote_or_never_pushed() {
    let fixture = Fixture::new("worker");
    let name = format!("{OWNER}/spike-158");
    fixture.branch(&name, 1, true);
    fixture.report("2026-01-05-158-trial", &name, "done", 158, "");
    assert_eq!(fixture.warnings(), [], "at the remote's commit");
    git(&fixture.work, &["checkout", "-q"], &[&name]);
    commit(&fixture.work, "more.txt");
    git(&fixture.work, &["checkout", "-q", "main"], &[]);
    let ahead = one(fixture.warnings());
    assert_eq!((ahead.problem, ahead.branch.as_deref(), ahead.ahead, ahead.task, ahead.never_pushed), (Problem::BranchNotPushed, Some(name.as_str()), Some(1), Some(158), Some(false)));
    assert_eq!((ahead.expected, ahead.project.as_deref(), ahead.folder.as_str(), ahead.repo.as_deref()), (false, Some("p"), fixture.work.display().to_string().as_str(), None));
    let fresh = format!("{OWNER}/spike-159");
    fixture.branch(&fresh, 2, false);
    fixture.report("2026-01-05-159-trial", &fresh, "done", 159, "");
    let both: Vec<(Option<String>, Option<u64>, Option<bool>)> = fixture.warnings().into_iter().map(|warning| (warning.branch, warning.ahead, warning.never_pushed)).collect();
    assert_eq!(both, [(Some(name), Some(1), Some(false)), (Some(fresh), Some(2), Some(true))], "one a branch, in the order of their names");
}

/// Nothing for a branch a report names that is not here, for a branch outside the convention without a report, nor for one the main branch has.
#[test]
fn nothing_for_a_branch_that_is_not_here_or_is_in_the_main_one() {
    let fixture = Fixture::new("contained");
    fixture.report("2026-01-05-1-other", "spike/of-another-repo", "done", 1, "");
    fixture.branch("another-branch-without-report", 1, false);
    assert_eq!(fixture.warnings(), [], "a branch no report names, outside the convention, is no worker's");
    let name = worker(158, "trial");
    fixture.branch(&name, 1, true);
    git(&fixture.work, &["checkout", "-q"], &[&name]);
    commit(&fixture.work, "more.txt");
    git(&fixture.work, &["checkout", "-q", "main"], &[]);
    git(&fixture.work, &["merge", "-q", "--ff-only"], &[&name]);
    git(&fixture.work, &["push", "-q"], &[]);
    assert_eq!(fixture.warnings(), [], "the main branch has everything, origin/<branch> stayed behind");
}

/// A worktree on the commit the main branch integrated: what keeps it quiet is the branch equal to its remote, not the
/// main branch; without the remote's ref, the commit delivered is not in the main one.
#[test]
fn a_worktree_on_the_integrated_commit() {
    let fixture = Fixture::new("worktree");
    let name = worker(158, "trial");
    fixture.branch(&name, 1, true);
    commit(&fixture.work, "other.txt");
    git(&fixture.work, &["cherry-pick"], &[&name]);
    git(&fixture.work, &["push", "-q"], &[]);
    let tree = fixture.root.join("tree");
    git(&fixture.work, &["worktree", "add", "-q", "--detach"], &[tree.to_str().expect("a path"), "main"]);
    assert_eq!(git(&tree, &["branch", "--show-current"], &[]), "");
    fixture.report("2026-01-05-158-trial", &name, "done", 158, "");
    assert_eq!(fixture.warnings(), []);
    git(&fixture.work, &["update-ref", "-d"], &[&format!("refs/remotes/origin/{name}")]);
    let delivered = one(fixture.warnings());
    assert_eq!((delivered.branch.as_deref(), delivered.ahead, delivered.never_pushed), (Some(name.as_str()), Some(1), Some(true)));
}

/// The dossier's current branch is its own warning's, not repeated; after a warning that keeps the remote the branches are
/// still read, whatever is in progress; without the remote, nothing to compare with.
#[test]
fn the_dossier_s_own_warning_and_its_branches() {
    let fixture = Fixture::new("current");
    let name = worker(158, "trial");
    commit(&fixture.work, "two.txt");
    git(&fixture.work, &["checkout", "-q", "-b"], &[&name]);
    commit(&fixture.work, "on-branch.txt");
    fixture.report("2026-01-05-158-trial", &name, "done", 158, "");
    assert_eq!(problems(&fixture.warnings()), [Problem::NoUpstream], "the current branch is the dossier's warning already");
    git(&fixture.work, &["checkout", "-q", "main"], &[]);
    git(&fixture.work, &["branch", "--unset-upstream", "main"], &[]);
    assert_eq!(problems(&fixture.warnings()), [Problem::NoUpstream, Problem::BranchNotPushed]);
    assert_eq!(problems(&run(&[fixture.project()], &BTreeSet::from([1, 2, 3]))), [Problem::NoUpstream, Problem::BranchNotPushed]);
    git(&fixture.work, &["remote", "remove", "origin"], &[]);
    assert_eq!(problems(&fixture.warnings()), [Problem::NoRemote], "without the remote, nothing to compare with");
}

/// What the reports say of a branch: a gate, a push held by a gate, the last report winning, a long sha, and the reports
/// that do not read skipped.
#[test]
fn what_the_reports_say_of_a_branch() {
    let fixture = Fixture::new("reports");
    let name = format!("{OWNER}/spike-158");
    fixture.branch(&name, 1, false);
    fixture.report("2026-01-04-158-first", &name, "gate", 158, "");
    assert!(one(fixture.warnings()).expected, "a gate holds it here on purpose");
    fixture.report("2026-01-05-158-second", &name, "done", 158, "");
    assert!(!one(fixture.warnings()).expected, "the last report of the branch wins");
    fixture.report("2026-01-06-158-third", &name, "done", 158, &format!("{}: {}\n", format().reports.push_key, format().reports.gate));
    assert!(one(fixture.warnings()).expected, "a push held by a gate");
    let words = &format().reports;
    let folder = fixture.work.join(&words.folder);
    let long = git(&fixture.work, &["rev-parse"], &[&name]);
    fs::write(folder.join("2026-01-07-158-long.md"), format!("{f}\n{t}: done\n{k}: 160\n{b}: {name}@{long}\n{f}\n", f = words.fence, t = words.type_key, k = words.task_key, b = words.branch_key)).expect("a report");
    assert_eq!(one(fixture.warnings()).task, Some(160), "a sha of 40 digits goes too");
    fs::write(folder.join("2026-01-08-1-bare.md"), format!("no front matter\n{}: {name}\n", words.branch_key)).expect("a report");
    fs::write(folder.join("2026-01-08-2-bytes.md"), b"\xff\xfe\x00---\n").expect("a report");
    fs::create_dir_all(folder.join("2026-01-08-3-folder.md")).expect("a folder");
    let kept = one(fixture.warnings());
    assert_eq!((kept.branch.as_deref(), kept.task), (Some(name.as_str()), Some(160)), "the reports that do not read are skipped");
}

/// One warning a branch, in the order of their names.
#[test]
fn one_warning_a_branch_in_the_order_of_their_names() {
    let fixture = Fixture::new("order");
    for (name, task) in [("example/b", 11), ("example/a", 10)] {
        fixture.branch(name, 1, false);
        fixture.report(&format!("2026-01-05-{task}"), name, "done", task, "");
    }
    let found: Vec<(Option<String>, Option<u64>)> = fixture.warnings().into_iter().map(|warning| (warning.branch, warning.task)).collect();
    assert_eq!(found, [(Some("example/a".to_owned()), Some(10)), (Some("example/b".to_owned()), Some(11))]);
}

/// One read of the refs and no count when everything is in order; a failing read of the refs, a failing count and a
/// broken `.git` are warnings; a deadline spent starts no git.
#[test]
fn the_cost_and_the_failures_of_the_branches() {
    let fixture = Fixture::new("cost");
    let name = worker(158, "trial");
    fixture.branch(&name, 1, true);
    fixture.report("2026-01-05-158-trial", &name, "done", 158, "");
    let spy = Spy::watching();
    assert_eq!(spy.warnings(&[fixture.project()], &BTreeSet::new()), []);
    assert_eq!(spy.calls("for-each-ref"), 1);
    assert!(!spy.seen.borrow().iter().any(|(_, words)| words[0] == "rev-list" && words.iter().any(|word| word == "--not")));
    let fresh = worker(159, "trial");
    fixture.branch(&fresh, 1, false);
    let refs = Spy::new(|_, words| (words[0] == "for-each-ref").then(|| Run { code: 128, stdout: String::new(), stderr: "fatal: unreadable refs".to_owned() }));
    let failed = one(refs.warnings(&[fixture.project()], &BTreeSet::new()));
    assert_eq!((failed.problem, failed.detail.as_deref()), (Problem::GitFailed, Some("fatal: unreadable refs")));
    let fresh_ref = format!("refs/heads/{fresh}");
    let count = Spy::new(move |_, words| (words[0] == "rev-list" && words.get(2) == Some(&fresh_ref)).then(|| timed_out("")));
    let failed = one(count.warnings(&[fixture.project()], &BTreeSet::new()));
    assert_eq!((failed.problem, failed.detail), (Problem::GitFailed, Some(format().details.rev_list_unreadable.clone())));
    let past = WarningsRequest { deadline: Some(Instant::now()), ..request(&[fixture.project()], &BTreeSet::new()) };
    assert_eq!(problems(&dossier_warnings(&past)), [Problem::GitFailed]);
    fs::write(fixture.work.join(".git").join("packed-refs"), "garbage\n").expect("a broken file");
    let found = problems(&fixture.warnings());
    assert!(found.contains(&Problem::GitFailed) && found.iter().all(|problem| matches!(problem, Problem::GitFailed | Problem::NoUpstream)), "{found:?}");
}

// ---------- the branches no report names, and the tasks in progress ----------

/// A branch by the workers' convention, with or without the owner, never pushed and with no report; pushed, nothing;
/// outside the convention, nothing.
#[test]
fn a_branch_by_the_convention_of_the_workers() {
    let fixture = Fixture::new("convention");
    let name = worker(549, "trial");
    fixture.branch(&name, 1, false);
    let found = one(fixture.warnings());
    assert_eq!((found.problem, found.branch.as_deref(), found.task, found.never_pushed, found.ahead), (Problem::BranchNotPushed, Some(name.as_str()), Some(549), Some(true), Some(1)));
    assert_eq!((found.expected, found.repo), (false, None), "the dossier's has no repo");
    git(&fixture.work, &["push", "-q", "origin"], &[&name]);
    assert_eq!(fixture.warnings(), [], "pushed");
    let prefix = &format().worker_branch.prefix;
    let bare = format!("{prefix}12-without-owner");
    fixture.branch(&bare, 1, false);
    assert_eq!(one(fixture.warnings()).task, Some(12));
    git(&fixture.work, &["branch", "-q", "-D"], &[&bare]);
    for outside in ["feature/something", &format!("{prefix}without-number"), &format!("{OWNER}/{prefix}12"), &format!("{OWNER}/{prefix}x-y"), &format!("a/b/{prefix}1-x"), &format!("{OWNER}/{prefix}1-x/y")] {
        fixture.branch(outside, 1, false);
    }
    assert_eq!(fixture.warnings(), [], "outside the convention, and no report");
    for (name, task) in [(&format!("{prefix}12-x"), Some(12)), (&format!("x/{prefix}007-y"), Some(7)), (&format!("{prefix}12-"), None), (&format!("/{prefix}1-x"), None), (&format!("{prefix}-1-x"), None)] {
        assert_eq!(worker_task(name), task, "{name}");
    }
}

/// The branches of the tasks in progress are no warning, in the dossier and in a product's clone, named by a report or
/// by the convention; a report wins over the name, and one without a task takes the name's.
#[test]
fn the_tasks_in_progress_and_the_report_against_the_name() {
    let fixture = Fixture::new("in-progress").with_product();
    let name = worker(549, "trial");
    fixture.branch(&name, 1, false);
    assert_eq!(run(&[fixture.project()], &BTreeSet::from([549])), []);
    assert_eq!(run(&[fixture.project()], &BTreeSet::from([550])).len(), 1);
    let sliced = worker(435, "slice");
    fixture.branch_in(&fixture.clone_dir(), &sliced, 1, false);
    let tasks = |in_progress: &[u64]| -> Vec<Option<u64>> { run(&[fixture.with(PRODUCT)], &in_progress.iter().copied().collect()).into_iter().map(|warning| warning.task).collect() };
    assert_eq!(tasks(&[549]), [Some(435)], "in the product's clone too");
    assert_eq!(tasks(&[435]), [Some(549)]);
    assert_eq!(tasks(&[435, 549]), []);
    assert_eq!(tasks(&[]).len(), 2);
    let named = format!("{OWNER}/spike-158");
    fixture.branch(&named, 1, false);
    fixture.report("2026-01-05-158-trial", &named, "done", 158, "");
    assert_eq!(run(&[fixture.project()], &BTreeSet::from([549, 158])), [], "a branch a report names");
    let five = worker(5, "x");
    fixture.branch(&five, 1, false);
    fixture.report("2026-01-05-9-x", &five, "gate", 9, "");
    let won = run(&[fixture.project()], &BTreeSet::from([549, 158]));
    assert_eq!(won.iter().map(|warning| (warning.task, warning.expected)).collect::<Vec<_>>(), [(Some(9), true)], "the report wins over the name");
    let words = &format().reports;
    let twelve = worker(12, "x");
    fixture.branch(&twelve, 1, false);
    write(&fixture.work, &format!("{}/2026-01-05-x.md", words.folder), &format!("{f}\n{t}: done\n{b}: \"{twelve}@abc1234\"\n{f}\n", f = words.fence, t = words.type_key, b = words.branch_key));
    let tasks: Vec<Option<u64>> = run(&[fixture.project()], &BTreeSet::from([549, 158, 9])).into_iter().map(|warning| warning.task).collect();
    assert_eq!(tasks, [Some(12)], "a report without a task takes the number of the name");
    assert_eq!(run(&[fixture.project()], &BTreeSet::from([549, 158, 9, 12])), []);
}

// ---------- the products and the sites of their clones ----------

/// A product's clone: a branch ahead of its remote or never pushed, nothing once pushed or integrated, its current branch a
/// candidate, read whatever the dossier says.
#[test]
fn a_product_s_branches() {
    let fixture = Fixture::new("product").with_product();
    let clone = fixture.clone_dir();
    let sliced = worker(435, "slice");
    fixture.branch_in(&clone, &sliced, 1, true);
    assert_eq!(fixture.of_products(PRODUCT), [], "pushed");
    git(&clone, &["checkout", "-q"], &[&sliced]);
    commit(&clone, "more.txt");
    git(&clone, &["checkout", "-q", "main"], &[]);
    let ahead = one(fixture.of_products(PRODUCT));
    assert_eq!((ahead.problem, ahead.branch.as_deref(), ahead.ahead, ahead.task, ahead.never_pushed, ahead.repo.as_deref()), (Problem::BranchNotPushed, Some(sliced.as_str()), Some(1), Some(435), Some(false), Some(PRODUCT)));
    assert_eq!((ahead.project.as_deref(), ahead.folder.clone(), ahead.site), (Some("p"), shown(&clone), false));
    git(&clone, &["push", "-q", "origin"], &[&sliced]);
    let integrated = worker(436, "integrated");
    fixture.branch_in(&clone, &integrated, 1, false);
    git(&clone, &["merge", "-q", "--ff-only"], &[&integrated]);
    git(&clone, &["push", "-q"], &[]);
    assert_eq!(fixture.of_products(PRODUCT), [], "integrated");
    let fresh = worker(437, "fresh");
    git(&clone, &["checkout", "-q", "-b"], &[&fresh]);
    commit(&clone, "fresh.txt");
    commit(&clone, "fresher.txt");
    fixture.branch(&worker(1, "in-the-dossier"), 1, true);
    let current = one(fixture.of_products(PRODUCT));
    assert_eq!((current.branch.as_deref(), current.ahead, current.never_pushed, current.repo.as_deref()), (Some(fresh.as_str()), Some(2), Some(true), Some(PRODUCT)), "the clone's current branch is a candidate");
}

/// A product without its clone: missing, with another `origin` (never printed), without one, a folder that is no repo;
/// several products, one missing and one with a branch.
#[test]
fn a_product_without_its_clone() {
    let fixture = Fixture::new("no-clone").with_product();
    let clone = fixture.clone_dir();
    let other = fixture.root.join("remotes").join(OWNER).join("other.git");
    git(&clone, &["remote", "set-url", "origin"], &[other.to_str().expect("a path")]);
    let wrong = one(fixture.of_products(PRODUCT));
    assert_eq!((wrong.problem, wrong.repo.as_deref(), wrong.detail.as_deref()), (Problem::ProductWithoutClone, Some(PRODUCT), Some(format().details.origin_not_the_repo.as_str())));
    assert!(!format!("{wrong:?}").contains("other"), "the remote's URL is never in a warning");
    git(&clone, &["remote", "remove", "origin"], &[]);
    assert_eq!(problems(&fixture.of_products(PRODUCT)), [Problem::ProductWithoutClone]);
    fs::remove_dir_all(&clone).expect("the clone goes");
    let gone = one(fixture.of_products(PRODUCT));
    assert_eq!((gone.problem, gone.repo.as_deref(), gone.branch, gone.project.as_deref(), gone.site), (Problem::ProductWithoutClone, Some(PRODUCT), None, Some("p"), false));
    assert_eq!((gone.folder, gone.detail.as_deref()), (shown(&clone), Some(format().details.clone_missing.as_str())));
    fs::create_dir_all(&clone).expect("a folder");
    assert_eq!(one(fixture.of_products(PRODUCT)).detail.as_deref(), Some(format().details.clone_not_a_root.as_str()), "the folder is there, but no repo");
    let fixture = Fixture::new("two-products").with_product();
    let sliced = worker(435, "slice");
    fixture.branch_in(&fixture.clone_dir(), &sliced, 1, false);
    let found: Vec<(Problem, Option<String>)> = fixture.of_products(&format!("{PRODUCT}, {OWNER}/missing")).into_iter().map(|warning| (warning.problem, warning.repo)).collect();
    assert_eq!(found, [(Problem::BranchNotPushed, Some(PRODUCT.to_owned())), (Problem::ProductWithoutClone, Some(format!("{OWNER}/missing")))]);
}

/// Which products are read: none for the words of none, never the dossier's own repo, a repo written twice once, an
/// owner taken from the tracker; none for a dossier out of place; a report of the dossier naming a product's branch.
#[test]
fn which_products_are_read() {
    let fixture = Fixture::new("which").with_product();
    for none in &format().registry.none {
        assert_eq!(fixture.of_products(none), [], "{none:?}");
    }
    assert_eq!(fixture.of_products(DOSSIER_REPO), []);
    assert_eq!(fixture.of_products(&DOSSIER_REPO.to_uppercase()), []);
    let sliced = worker(435, "slice");
    fixture.branch_in(&fixture.clone_dir(), &sliced, 1, false);
    let branches: Vec<Option<String>> = fixture.of_products(&format!("{PRODUCT}, {}, {PRODUCT}", PRODUCT.to_uppercase())).into_iter().map(|warning| warning.branch).collect();
    assert_eq!(branches, [Some(sliced.clone())], "once");
    assert_eq!(problems(&fixture.of_products("product")), [Problem::BranchNotPushed], "a product without an owner takes the tracker's");
    assert_eq!(fixture.of_products(&format!("{PRODUCT}, not a repo")), [], "one name that is not a repo's: none");
    for place in [fixture.root.join("missing").join("dossier"), fixture.root.join("plain")] {
        if place.ends_with("plain") {
            fs::create_dir_all(&place).expect("a folder");
        }
        let project = DossierProject { folder: Some(place.display().to_string()), ..fixture.with(PRODUCT) };
        let found = problems(&run(&[project], &BTreeSet::new()));
        assert!(matches!(found.as_slice(), [Problem::FolderMissing | Problem::NotARepo]), "{found:?}");
    }
    let spike = "spike/not-the-convention";
    fixture.branch_in(&fixture.clone_dir(), spike, 1, false);
    let words = &format().reports;
    write(&fixture.work, &format!("{}/2026-01-05-77.md", words.folder), &format!("{f}\n{t}: done\n{k}: 77\n{b}: {spike}\n{f}\n", f = words.fence, t = words.type_key, k = words.task_key, b = words.branch_key));
    let found: Vec<(Option<String>, Option<u64>)> = fixture.of_products(PRODUCT).into_iter().map(|warning| (warning.branch, warning.task)).collect();
    assert_eq!(found, [(Some(sliced), Some(435)), (Some(spike.to_owned()), Some(77))], "a report of the dossier names a branch of the product");
}

/// A product's git failing is a warning with its repo, never a missing clone; one read of the refs a clone and no count
/// when it is in order; a deadline spent starts no git.
#[test]
fn a_product_s_git_failing_and_its_cost() {
    let fixture = Fixture::new("product-git").with_product();
    let clone = fixture.clone_dir();
    fixture.branch_in(&clone, &worker(435, "slice"), 1, true);
    let spy = Spy::watching();
    assert_eq!(spy.warnings(&[fixture.with(PRODUCT)], &BTreeSet::new()), []);
    let in_clone = spy.seen.borrow().iter().filter(|(folder, words)| *folder == clone && words[0] == "for-each-ref").count();
    assert_eq!(in_clone, 1);
    assert!(!spy.seen.borrow().iter().any(|(_, words)| words[0] == "rev-list" && words.iter().any(|word| word == "--not")));
    for call in ["for-each-ref", "rev-parse"] {
        let target = clone.clone();
        let spy = Spy::new(move |folder, words| (folder == target && words[0] == call).then(|| timed_out("git: no answer")));
        let failed = one(spy.warnings(&[fixture.with(PRODUCT)], &BTreeSet::new()));
        assert_eq!((failed.problem, failed.repo.as_deref()), (Problem::GitFailed, Some(PRODUCT)), "{call}");
    }
    let past = WarningsRequest { deadline: Some(Instant::now()), ..request(&[fixture.with(PRODUCT)], &BTreeSet::new()) };
    let found = dossier_warnings(&past);
    assert!(!found.is_empty() && found.iter().all(|warning| warning.problem == Problem::GitFailed), "{found:?}");
}

/// A remote's URL, https, ssh or a local path, is the repo's without regard to case or suffix.
#[test]
fn a_remote_s_url_is_of_the_repo() {
    for url in ["https://github.com/example/product.git", "https://github.com/example/product", "git@github.com:example/product.git", "C:\\x\\example\\product.git", "https://x:y@github.com/Example/Product.GIT", " https://h/example/product/ "] {
        assert!(url_is_of(url, PRODUCT), "{url}");
    }
    for url in ["https://github.com/example/product-old.git", "https://github.com/another/product.git", "", "product"] {
        assert!(!url_is_of(url, PRODUCT), "{url}");
    }
}

/// The site of a product's clone: without one the sibling folder; a site that resolves, without regard to the repo's case;
/// the roots of the aliases; an unknown alias, a path that leaves the root or is absolute, a site without a path.
#[test]
fn the_site_of_a_product_s_clone() {
    let fixture = Fixture::new("site").with_product();
    let aliases = &format().registry.aliases;
    let (one_up, two_up) = (aliases.iter().find(|alias| alias.up == 1).expect("an alias"), aliases.iter().find(|alias| alias.up == 2).expect("an alias"));
    assert_eq!(fixture.of_products(PRODUCT), []);
    let moved = fixture.root.join("elsewhere").join("here");
    fs::create_dir_all(moved.parent().expect("a parent")).expect("a folder");
    fs::rename(fixture.clone_dir(), &moved).expect("the clone moves");
    let gone = one(fixture.of_products(PRODUCT));
    assert_eq!((gone.problem, gone.site), (Problem::ProductWithoutClone, false));
    let sited = |sites: String| run(&[DossierProject { sites: Some(sites), ..fixture.with(PRODUCT) }], &BTreeSet::new());
    assert_eq!(sited(format!("{PRODUCT}={}/elsewhere/here", one_up.alias)), []);
    assert_eq!(sited(format!("{}={}/elsewhere/here", PRODUCT.to_uppercase(), one_up.alias)), [], "the repo without regard to case");
    let sliced = worker(435, "slice");
    fixture.branch_in(&moved, &sliced, 1, false);
    let seen = one(sited(format!("{PRODUCT}={}/elsewhere/here", one_up.alias)));
    assert_eq!((seen.problem, seen.repo.as_deref(), seen.site), (Problem::BranchNotPushed, Some(PRODUCT), false), "a branch's warning says nothing of the site");
    let work = fixture.work.display().to_string();
    assert_eq!(clone_place(&work, PRODUCT, Some(&format!("{}/x/y", two_up.alias))), Ok(fixture.work.parent().and_then(Path::parent).expect("two up").join("x").join("y")));
    assert_eq!(clone_place(&work, PRODUCT, Some(&format!("{}/x", one_up.alias))), Ok(fixture.root.join("x")));
    assert_eq!(clone_place(&work, PRODUCT, None), Ok(fixture.root.join("product")));
    let refused = |site: String| {
        let found = one(sited(format!("{PRODUCT}={site}")));
        assert_eq!((found.problem, found.repo.as_deref(), found.site), (Problem::ProductWithoutClone, Some(PRODUCT), true), "{site}");
        found.detail.expect("a detail")
    };
    let details = &format().details;
    assert_eq!(refused("home/product".to_owned()), details.site_unknown_alias.replace("{alias}", "home"));
    assert_eq!(refused(format!("{}/../../out", one_up.alias)), details.site_leaves_root);
    assert_eq!(refused(format!("{}/..", one_up.alias)), details.site_leaves_root, "`..` alone leaves the root too");
    for absolute in ["/tmp/product", "C:/x/product", "C:\\x\\product", "\\\\server\\x"] {
        assert_eq!(refused(absolute.to_owned()), details.site_absolute, "{absolute}");
    }
    assert_eq!(refused(format!("{}/", one_up.alias)), details.site_without_path);
    assert_eq!(sites_of(Some("without separator, , =x")), BTreeMap::new());
    assert_eq!(sites_of(Some(" A/B = x/y , a/b=z ")), BTreeMap::from([("a/b".to_owned(), "z".to_owned())]), "the last one written wins");
}

/// A value of a report's front matter, a branch without its sha, `normpath`, the last line, a repo's name.
#[test]
fn the_small_readings() {
    assert_eq!(yaml_value(" \"a b\" # c"), "a b");
    assert_eq!(yaml_value("'a\" b' x"), "a\" b");
    assert_eq!(yaml_value("\"open"), "open");
    assert_eq!(yaml_value("plain # comment"), "plain");
    assert_eq!(yaml_value("plain#not"), "plain#not");
    assert_eq!(yaml_value("   "), "");
    assert_eq!(without_sha("x@abc1234"), "x");
    assert_eq!(without_sha("x@abc123"), "x@abc123", "six digits are no sha");
    assert_eq!(without_sha("x@ABC1234"), "x@ABC1234", "uppercase is no sha");
    assert_eq!(without_sha(&format!("x@{}", "a".repeat(41))), format!("x@{}", "a".repeat(41)));
    assert_eq!(without_sha("@abc1234"), "");
    let sep = MAIN_SEPARATOR_STR;
    assert_eq!(normpath("a/./b//c"), format!("a{sep}b{sep}c"));
    assert_eq!(normpath("a/../../b"), format!("..{sep}b"));
    assert_eq!(normpath("a/.."), ".");
    assert_eq!(normpath("/a/../../b"), format!("{sep}b"));
    assert_eq!(last("one\n\n  two  \n\n"), "two");
    assert_eq!(last(" \n "), format().details.no_message);
    assert!(is_repo("a/b") && is_repo("A1/b.c_d-e") && !is_repo("a") && !is_repo("a/b/c") && !is_repo(".a/b") && !is_repo("a/"));
}

/// The words of the problems are the format's, one a problem.
#[test]
fn every_problem_has_its_own_word() {
    let words: BTreeSet<&str> = Problem::ALL.iter().map(|problem| problem.word()).collect();
    assert_eq!(words.len(), Problem::ALL.len());
    assert!(words.iter().all(|word| !word.is_empty() && word.is_ascii()));
}
