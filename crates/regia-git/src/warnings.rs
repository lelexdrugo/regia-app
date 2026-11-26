//! The dossier's warnings: the prototype's `avvisi_dossier` (#159, #158, #549 and #646 of the dossier), which the
//! brief lists under the dossiers to fix and of which the precheck keeps a compact copy in its line. For each project,
//! in the order given:
//!
//! - the dossier itself: its folder missing, no repo at its root, a detached `HEAD`, no remote (the registry names no
//!   repo, or the repo has no `origin`), no upstream, or commits the upstream lacks;
//! - then, unless the dossier is missing, is no repo, has no remote or git failed on it, the branches of its workers:
//!   local branches with commits that neither their own branch on the remote nor the remote's main branch has, each
//!   named by a report's `branch` or by the convention of a worker's branch, except those of a task in progress;
//! - then, unless the dossier is missing or is no repo, each of its products: the same branches in the product's
//!   clone, or one warning that the clone is not where the registry says.
//!
//! Only local git, no fetch and no network: "not pushed" is against what the remote looked like last. Every call has
//! a deadline, and none starts past the caller's. A failure of git is a warning of the project it happened on, never
//! an error.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regia_exec::{Args, ExecError, Text};
use serde::Deserialize;

use crate::dossier::same_folder;
use crate::git::{Git, NOT_RUN, Run, TIMED_OUT, call, is_line_break};

/// Format v1 of the warnings, as the prototype has it. Its words are data, not identifiers.
const FORMAT: &str = include_str!("../formats/warnings_v1.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Format {
    call_timeout_s: u64,
    problems: Problems,
    details: Details,
    remote: Remote,
    reports: Reports,
    worker_branch: WorkerBranch,
    registry: RegistryWords,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Problems {
    folder_missing: String,
    not_a_repo: String,
    detached_head: String,
    no_remote: String,
    no_upstream: String,
    not_pushed: String,
    git_failed: String,
    branch_not_pushed: String,
    product_without_clone: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Details {
    no_repo_key: String,
    no_origin: String,
    rev_list: String,
    rev_list_unreadable: String,
    no_message: String,
    main_branch_unknown: String,
    clone_missing: String,
    clone_not_a_root: String,
    origin_not_the_repo: String,
    site_absolute: String,
    site_unknown_alias: String,
    site_without_path: String,
    site_leaves_root: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Remote {
    main_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reports {
    folder: String,
    extension: String,
    fence: String,
    type_key: String,
    task_key: String,
    branch_key: String,
    push_key: String,
    gate: String,
    sha_mark: String,
    sha_min: usize,
    sha_max: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerBranch {
    prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryWords {
    none: Vec<String>,
    products_separator: String,
    site_separator: String,
    repo_suffix: String,
    aliases: Vec<Alias>,
}

/// A site's alias, and how many levels above the dossier's folder its root is.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Alias {
    alias: String,
    up: usize,
}

fn format() -> &'static Format {
    static FORMAT_V1: OnceLock<Format> = OnceLock::new();
    FORMAT_V1.get_or_init(|| toml::from_str(FORMAT).expect("formats/warnings_v1.toml is valid TOML of its shape"))
}

/// What is wrong, in the prototype's order of the checks of a dossier, then a worker's branch and a product's clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Problem {
    /// The registry's folder is not one.
    FolderMissing,
    /// The folder is not the root of a git repo.
    NotARepo,
    /// No branch is checked out.
    DetachedHead,
    /// The registry names no repo, or the repo has no remote `origin`.
    NoRemote,
    /// The branch has no upstream.
    NoUpstream,
    /// The upstream lacks commits of the branch.
    NotPushed,
    /// Git did not answer, could not start, or said what the warnings cannot read.
    GitFailed,
    /// A worker's branch has commits neither the remote nor its main branch has.
    BranchNotPushed,
    /// A product has no clone where the registry says, or that clone is not the product's.
    ProductWithoutClone,
}

impl Problem {
    pub const ALL: [Problem; 9] = [
        Problem::FolderMissing,
        Problem::NotARepo,
        Problem::DetachedHead,
        Problem::NoRemote,
        Problem::NoUpstream,
        Problem::NotPushed,
        Problem::GitFailed,
        Problem::BranchNotPushed,
        Problem::ProductWithoutClone,
    ];

    /// The prototype's word for the problem, which the brief and the precheck print.
    pub fn word(self) -> &'static str {
        let words = &format().problems;
        let word = match self {
            Problem::FolderMissing => &words.folder_missing,
            Problem::NotARepo => &words.not_a_repo,
            Problem::DetachedHead => &words.detached_head,
            Problem::NoRemote => &words.no_remote,
            Problem::NoUpstream => &words.no_upstream,
            Problem::NotPushed => &words.not_pushed,
            Problem::GitFailed => &words.git_failed,
            Problem::BranchNotPushed => &words.branch_not_pushed,
            Problem::ProductWithoutClone => &words.product_without_clone,
        };
        word.as_str()
    }

    /// After this problem of the dossier there is still a remote to compare its workers' branches with.
    fn keeps_the_remote(self) -> bool {
        matches!(self, Problem::NotPushed | Problem::NoUpstream | Problem::DetachedHead)
    }
}

/// A warning, with the prototype's fields (`docs/contract` names them for the brief): the project's id, `None` when
/// the caller gives none, as the precheck; the folder it is about, the dossier's or a product's clone; the problem,
/// the branch and how many commits it is ahead; a detail, in the prototype's words or in git's. A product's warning
/// has the product's repo, and says whether the registry gives its clone a site. A worker's branch has the task it
/// belongs to, whether it was ever pushed (only such a warning has it), and whether a gate holds it here on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub project: Option<String>,
    pub folder: String,
    pub problem: Problem,
    pub branch: Option<String>,
    pub ahead: Option<u64>,
    pub detail: Option<String>,
    pub repo: Option<String>,
    pub task: Option<u64>,
    pub never_pushed: Option<bool>,
    pub expected: bool,
    pub site: bool,
}

impl Warning {
    fn of(project: Option<&str>, folder: &str, problem: Problem) -> Warning {
        Warning {
            project: project.map(str::to_owned),
            folder: folder.to_owned(),
            problem,
            branch: None,
            ahead: None,
            detail: None,
            repo: None,
            task: None,
            never_pushed: None,
            expected: false,
            site: false,
        }
    }
}

/// A project as its registry writes it: its id, its folder, the dossier's repo, its products and the sites of their
/// clones, each as the registry's text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DossierProject {
    pub id: Option<String>,
    pub folder: Option<String>,
    pub repo: Option<String>,
    pub products: Option<String>,
    pub sites: Option<String>,
}

/// What the warnings are asked of: the projects; the instant past which no call of git starts; the tasks in progress,
/// whose branches are no warning because their workers are at work; the tracker's owner, which a product written
/// without one takes.
#[derive(Debug, Clone, Default)]
pub struct WarningsRequest {
    pub projects: Vec<DossierProject>,
    pub deadline: Option<Instant>,
    pub in_progress: BTreeSet<u64>,
    pub owner: Option<String>,
}

/// The warnings of the request's projects, with the user's git: none when every dossier is in order.
pub fn dossier_warnings(request: &WarningsRequest) -> Vec<Warning> {
    dossier_warnings_with(Git::resolve(), request)
}

/// [`dossier_warnings`] with the git it is given, or the reason there is none: then every call fails as a git that
/// cannot start would, and each project gets its warning.
pub fn dossier_warnings_with(git: Result<Git, ExecError>, request: &WarningsRequest) -> Vec<Warning> {
    let calls = Calls { git: git.map_err(|error| error.to_string()), deadline: request.deadline };
    warnings(&calls, request)
}

/// Where the warnings' calls of git go: the user's git within the deadline, or a stand-in in a test.
trait Runner {
    fn run(&self, folder: &Path, args: Args) -> Run;
}

/// The user's git, each call within the format's limit and the caller's deadline, the prototype's `_git_con_scadenza`.
struct Calls {
    git: Result<Git, String>,
    deadline: Option<Instant>,
}

impl Runner for Calls {
    fn run(&self, folder: &Path, args: Args) -> Run {
        let limit = Duration::from_secs(format().call_timeout_s);
        let limit = self.deadline.map_or(limit, |deadline| limit.min(deadline.saturating_duration_since(Instant::now())));
        if limit.is_zero() {
            return Run { code: TIMED_OUT, stdout: String::new(), stderr: "the deadline ran out before git started".to_owned() };
        }
        match &self.git {
            Ok(git) => git.run_within(folder, args, None, limit),
            Err(error) => Run { code: NOT_RUN, stdout: String::new(), stderr: format!("git cannot run: {error}") },
        }
    }
}

/// The prototype's `avvisi_dossier`.
fn warnings(runner: &dyn Runner, request: &WarningsRequest) -> Vec<Warning> {
    let mut out = Vec::new();
    for project in &request.projects {
        let Some(folder) = project.folder.as_deref().map(py_strip).filter(|folder| !folder.is_empty()) else {
            continue;
        };
        let own = dossier(runner, project, folder);
        let branches = match &own {
            Some(warning) if !warning.problem.keeps_the_remote() => Vec::new(),
            _ => {
                let place = Place { project: project.id.as_deref(), dir: PathBuf::from(folder), shown: folder.to_owned(), repo: None, reports: Path::new(folder) };
                branches(runner, &place, &request.in_progress)
            }
        };
        // A dossier out of place says nothing of where its clones are: their warnings would only be noise.
        let out_of_place = own.as_ref().is_some_and(|warning| matches!(warning.problem, Problem::FolderMissing | Problem::NotARepo));
        out.extend(own);
        out.extend(branches);
        if out_of_place {
            continue;
        }
        let own_repo = registry_repo(project.repo.as_deref());
        let sites = sites_of(project.sites.as_deref());
        for repo in products_of(project.products.as_deref(), request.owner.as_deref()) {
            if own_repo.is_some_and(|own| own.eq_ignore_ascii_case(&repo)) {
                continue;
            }
            out.extend(product(runner, project, folder, &repo, &sites, &request.in_progress));
        }
    }
    out
}

/// Whether a call's code is a git that did not answer in time or could not start: the prototype's `_GIT_GUASTO`.
fn broken(run: &Run) -> bool {
    run.code == TIMED_OUT || run.code == NOT_RUN
}

/// The prototype's `_avviso_dossier`: the dossier's own warning, `None` when it is in order.
fn dossier(runner: &dyn Runner, project: &DossierProject, folder: &str) -> Option<Warning> {
    let words = &format().details;
    let id = project.id.as_deref();
    let warning = |problem, branch: Option<&str>, detail: Option<String>| {
        Some(Warning { branch: branch.map(str::to_owned), detail, ..Warning::of(id, folder, problem) })
    };
    let root = Path::new(folder);
    if !root.is_dir() {
        return warning(Problem::FolderMissing, None, None);
    }
    let top = runner.run(root, call::show_toplevel());
    if !top.ok() {
        let problem = if broken(&top) { Problem::GitFailed } else { Problem::NotARepo };
        return warning(problem, None, Some(last(&top.stderr)));
    }
    // A repo further up is not the dossier.
    if !same_folder(Path::new(&top.stdout), root) {
        return warning(Problem::NotARepo, None, None);
    }
    let current = runner.run(root, call::show_current());
    if !current.ok() {
        return warning(Problem::GitFailed, None, Some(last(&current.stderr)));
    }
    let Some(branch) = Some(current.stdout.as_str()).filter(|branch| !branch.is_empty()) else {
        return warning(Problem::DetachedHead, None, None);
    };
    if registry_repo(project.repo.as_deref()).is_none() {
        return warning(Problem::NoRemote, Some(branch), Some(words.no_repo_key.clone()));
    }
    let origin = runner.run(root, call::origin_url());
    if !origin.ok() {
        return if broken(&origin) {
            warning(Problem::GitFailed, Some(branch), Some(last(&origin.stderr)))
        } else {
            warning(Problem::NoRemote, Some(branch), Some(words.no_origin.clone()))
        };
    }
    let upstream = runner.run(root, call::upstream());
    if !upstream.ok() {
        return if broken(&upstream) { warning(Problem::GitFailed, Some(branch), Some(last(&upstream.stderr))) } else { warning(Problem::NoUpstream, Some(branch), None) };
    }
    let ahead = runner.run(root, call::ahead());
    let count = ahead.ok().then_some(ahead.stdout.as_str()).and_then(count_of);
    let Some(count) = count else {
        let detail = if broken(&ahead) { last(&ahead.stderr) } else { words.rev_list.replace("{git}", &last(&ahead.stderr)) };
        return warning(Problem::GitFailed, Some(branch), Some(detail));
    };
    (count > 0).then(|| Warning { branch: Some(branch.to_owned()), ahead: Some(count), ..Warning::of(id, folder, Problem::NotPushed) })
}

/// A whole number as git prints it: ASCII digits only.
fn count_of(text: &str) -> Option<u64> {
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())).then(|| text.parse().ok()).flatten()
}

/// Where the workers' branches are read: the dossier, whose current branch has its own warning already, or a product's
/// clone, whose current branch is a branch like the others. Each warning shows `shown` as its folder and has `repo`
/// for a product; the reports that name the branches are the dossier's, at `reports`.
struct Place<'a> {
    project: Option<&'a str>,
    dir: PathBuf,
    shown: String,
    repo: Option<&'a str>,
    reports: &'a Path,
}

/// One line of `for-each-ref`, as the prototype splits it: a field that is not there is empty.
fn fields(line: &str) -> [&str; 4] {
    let mut out = [""; 4];
    for (slot, field) in out.iter_mut().zip(line.split('\t')) {
        *slot = field;
    }
    out
}

/// The prototype's `_avvisi_branch`.
fn branches(runner: &dyn Runner, place: &Place<'_>, in_progress: &BTreeSet<u64>) -> Vec<Warning> {
    let words = &format().details;
    let base = Warning { repo: place.repo.map(str::to_owned), ..Warning::of(place.project, &place.shown, Problem::BranchNotPushed) };
    let failed = |detail: &str| vec![Warning { problem: Problem::GitFailed, detail: Some(last(detail)), ..base.clone() }];
    let refs = runner.run(&place.dir, call::refs());
    if !refs.ok() {
        return failed(&refs.stderr);
    }
    let (heads, remotes, remote_head) = ("refs/heads/", "refs/remotes/origin/", "refs/remotes/origin/HEAD");
    let (mut local, mut remote, mut current, mut head) = (BTreeMap::new(), BTreeMap::new(), None, String::new());
    for line in refs.stdout.split(is_line_break) {
        let [mark, name, sha, target] = fields(line);
        if let Some(branch) = name.strip_prefix(heads) {
            local.insert(branch, sha);
            if mark == "*" {
                current = Some(branch);
            }
        } else if name == remote_head {
            // The prototype cuts the prefix's length off the target, whatever the target starts with.
            head = target.chars().skip(remotes.chars().count()).collect();
        } else if let Some(branch) = name.strip_prefix(remotes) {
            remote.insert(branch, sha);
        }
    }
    // Nothing to compare with: `no_remote` says it already, or the remote was never read.
    if remote.is_empty() {
        return Vec::new();
    }
    let names = std::iter::once(head.as_str()).chain(format().remote.main_names.iter().map(String::as_str));
    let main = names.into_iter().find(|name| !name.is_empty() && remote.contains_key(name));
    let excluded = [if place.repo.is_some() { None } else { current }, main];
    let candidates: BTreeSet<&str> =
        local.iter().filter(|(name, sha)| !excluded.contains(&Some(**name)) && remote.get(*name) != Some(*sha)).map(|(name, _)| *name).collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let reported = report_branches(place.reports);
    let mut out = Vec::new();
    for name in candidates {
        let report = reported.get(name);
        let by_name = worker_task(name);
        if report.is_none() && by_name.is_none() {
            continue;
        }
        // The report wins; without a task in it, the name's.
        let task = report.and_then(|report| report.task).or(by_name);
        if task.is_some_and(|task| in_progress.contains(&task)) {
            continue;
        }
        let never_pushed = !remote.contains_key(name);
        let against: Vec<String> = [(!never_pushed).then_some(name), main].into_iter().flatten().map(|branch| format!("{remotes}{branch}")).collect();
        let mut ahead = None;
        if !against.is_empty() {
            // A name from git's output reaches an argv again only typed (threat model §3.6, rule 7).
            let typed = Text::parse(&format!("{heads}{name}")).and_then(|branch| {
                let against = against.iter().map(|other| Text::parse(other)).collect::<Result<Vec<_>, _>>()?;
                Ok((branch, against))
            });
            let (branch, against) = match typed {
                Ok(typed) => typed,
                Err(error) => return failed(&error.to_string()),
            };
            let count = runner.run(&place.dir, call::count_not(&branch, &against));
            match count.ok().then_some(count.stdout.as_str()).and_then(count_of) {
                // As the prototype, the failure is the only warning of this folder: those found before it go.
                None => return failed(if count.stderr.is_empty() { &words.rev_list_unreadable } else { &count.stderr }),
                // Everything there is, the remote or its main branch has already.
                Some(0) => continue,
                Some(count) => ahead = Some(count),
            }
        }
        let expected = report.is_some_and(|report| report.gated);
        let detail = main.is_none().then(|| words.main_branch_unknown.clone());
        out.push(Warning { branch: Some(name.to_owned()), ahead, task, never_pushed: Some(never_pushed), expected, detail, ..base.clone() });
    }
    out
}

/// What a report says of a worker's branch: its task, and whether a gate holds the branch here on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReportBranch {
    task: Option<u64>,
    gated: bool,
}

/// Whether `name` is a report's: it ends in the format's extension, without regard to case where the system's names
/// have none, as the prototype's pattern matches.
fn is_report(name: &str) -> bool {
    let extension = &format().reports.extension;
    if cfg!(windows) { name.to_lowercase().ends_with(&extension.to_lowercase()) } else { name.ends_with(extension.as_str()) }
}

/// The prototype's `_branch_dei_report`: of each branch a report names, what its last report says, the reports read in
/// the order of their names (without regard to case where the system's names have none). A report that does not read,
/// or has no front matter, is skipped; so is a folder that cannot be listed.
fn report_branches(dossier: &Path) -> BTreeMap<String, ReportBranch> {
    let words = &format().reports;
    let mut files: Vec<(String, PathBuf)> = match std::fs::read_dir(dossier.join(&words.folder)) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok().filter(|name| is_report(name)).map(|name| (name, entry.path())))
            .map(|(name, path)| (if cfg!(windows) { name.to_lowercase() } else { name }, path))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    let mut out = BTreeMap::new();
    for (_, path) in files {
        let Some(fields) = std::fs::read(&path).ok().and_then(|bytes| front_matter(&bytes)) else {
            continue;
        };
        let field = |key: &String| fields.get(key.as_str()).map(String::as_str);
        let name = without_sha(field(&words.branch_key).unwrap_or_default());
        if name.is_empty() {
            continue;
        }
        let task = field(&words.task_key).and_then(count_of);
        let gated = [&words.type_key, &words.push_key].into_iter().any(|key| field(key) == Some(words.gate.as_str()));
        out.insert(name.to_owned(), ReportBranch { task, gated });
    }
    out
}

/// The keys of a report's front matter the warnings read, each with its last value: the text decoded as Python's
/// `utf-8-sig` with errors replaced, its line breaks read as `\n`. `None` when the first line is not the fence.
fn front_matter(bytes: &[u8]) -> Option<BTreeMap<&'static str, String>> {
    let words = &format().reports;
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text).replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = text.split('\n');
    if lines.next().map(py_strip) != Some(words.fence.as_str()) {
        return None;
    }
    let keys = [&words.type_key, &words.task_key, &words.branch_key, &words.push_key];
    let mut out = BTreeMap::new();
    for line in lines {
        if py_strip(line) == words.fence {
            break;
        }
        // A key at the very start of the line, then a colon: the prototype's `re.match`.
        if let Some((key, rest)) = keys.iter().find_map(|key| line.strip_prefix(key.as_str()).and_then(|rest| rest.strip_prefix(':')).map(|rest| (*key, rest))) {
            out.insert(key.as_str(), yaml_value(rest));
        }
    }
    Some(out)
}

/// A value of the front matter as the prototype reads it: in quotes, up to the next one; else up to a ` #` comment, stripped.
fn yaml_value(raw: &str) -> String {
    let value = py_strip(raw);
    match value.chars().next() {
        Some(quote @ ('\'' | '"')) => {
            let rest = &value[quote.len_utf8()..];
            rest.find(quote).map_or(rest, |end| &rest[..end]).to_owned()
        }
        _ => py_strip(value.split(" #").next().unwrap_or_default()).to_owned(),
    }
}

/// A report's `branch` without the `@<sha>` at its end, the sha of the format's lowercase hex digits.
fn without_sha(value: &str) -> &str {
    let words = &format().reports;
    match value.rsplit_once(words.sha_mark.as_str()) {
        Some((name, sha)) if (words.sha_min..=words.sha_max).contains(&sha.len()) && sha.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) => name,
        _ => value,
    }
}

/// The task of a worker's branch by its name, `[<owner>/]<prefix><N>-<slug>`: the prototype's `_BRANCH_DI_WORKER`.
fn worker_task(name: &str) -> Option<u64> {
    let rest = match name.split_once('/') {
        Some((owner, rest)) if !owner.is_empty() => rest,
        Some(_) => return None,
        None => name,
    };
    let after = rest.strip_prefix(format().worker_branch.prefix.as_str())?;
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    let slug = after[digits..].strip_prefix('-')?;
    if digits == 0 || slug.is_empty() || slug.contains('/') {
        return None;
    }
    after[..digits].parse().ok()
}

/// The prototype's `_avvisi_prodotto`: the warnings of a product's clone, or the one that says it has none.
fn product(runner: &dyn Runner, project: &DossierProject, folder: &str, repo: &str, sites: &BTreeMap<String, String>, in_progress: &BTreeSet<u64>) -> Vec<Warning> {
    let words = &format().details;
    let site = sites.get(&repo.to_lowercase()).map(String::as_str).filter(|site| !site.is_empty());
    let clone = clone_place(folder, repo, site);
    let shown = clone.as_ref().map_or_else(|_| folder.to_owned(), |clone| shown(clone));
    let without = Warning { repo: Some(repo.to_owned()), site: site.is_some(), ..Warning::of(project.id.as_deref(), &shown, Problem::ProductWithoutClone) };
    // The URL of the remote is never in a warning: it can carry credentials.
    let missing = |detail: &str| vec![Warning { detail: Some(detail.to_owned()), ..without.clone() }];
    let failed = |run: &Run| vec![Warning { problem: Problem::GitFailed, detail: Some(last(&run.stderr)), ..without.clone() }];
    let clone = match clone {
        Ok(clone) => clone,
        Err(detail) => return missing(&detail),
    };
    if !clone.is_dir() {
        return missing(&words.clone_missing);
    }
    let top = runner.run(&clone, call::show_toplevel());
    if broken(&top) {
        return failed(&top);
    }
    if !top.ok() || !same_folder(Path::new(&top.stdout), &clone) {
        return missing(&words.clone_not_a_root);
    }
    let origin = runner.run(&clone, call::origin_url());
    if broken(&origin) {
        return failed(&origin);
    }
    if !origin.ok() || !url_is_of(&origin.stdout, repo) {
        return missing(&words.origin_not_the_repo);
    }
    let place = Place { project: project.id.as_deref(), dir: clone, shown, repo: Some(repo), reports: Path::new(folder) };
    branches(runner, &place, in_progress)
}

/// The prototype's `_url_e_del_repo`: whether a remote's URL (https, ssh or a local path) ends with `<owner>/<name>`, with
/// or without the repo suffix, without regard to case.
fn url_is_of(url: &str, repo: &str) -> bool {
    let url = py_strip(url).replace('\\', "/");
    let url = url.trim_end_matches('/').to_lowercase();
    let suffix = &format().registry.repo_suffix;
    let url = url.strip_suffix(suffix.as_str()).unwrap_or(&url);
    let repo = repo.to_lowercase();
    url == repo || url.ends_with(&format!("/{repo}")) || url.ends_with(&format!(":{repo}"))
}

/// The prototype's `_sede_del_clone`: where a product's clone is, or why the site the registry gives it does not say.
/// Without a site, the dossier's sibling folder with the repo's name; with one, its path under the root its alias
/// names, never above it. Only lexical: nothing is read.
fn clone_place(dossier: &str, repo: &str, site: Option<&str>) -> Result<PathBuf, String> {
    let words = &format().details;
    let dossier = Path::new(dossier);
    let name = repo.split_once('/').map_or(repo, |(_, name)| name);
    let Some(site) = site else {
        return Ok(parent(dossier).join(name));
    };
    let site = site.replace('\\', "/");
    let trimmed = py_strip(&site);
    if trimmed.starts_with('/') || trimmed.split('/').next().is_some_and(|first| first.contains(':')) {
        return Err(words.site_absolute.clone());
    }
    let (alias, rest) = site.split_once('/').unwrap_or((site.as_str(), ""));
    let Some(up) = format().registry.aliases.iter().find(|known| known.alias == alias).map(|known| known.up) else {
        return Err(words.site_unknown_alias.replace("{alias}", alias));
    };
    let relative = if rest.trim_matches('/').is_empty() { String::new() } else { normpath(rest) };
    if relative.is_empty() || relative == "." {
        return Err(words.site_without_path.clone());
    }
    if relative.split(MAIN_SEPARATOR_STR).next() == Some("..") {
        return Err(words.site_leaves_root.clone());
    }
    let root = (0..up).fold(dossier.to_path_buf(), |root, _| parent(&root));
    Ok(root.join(relative))
}

/// Python's `os.path.normpath` of the path of a site, `/` between its parts: `.` and empty parts dropped, `..` taking
/// the part before it away and kept at the start; a leading `/` is a root, above which `..` goes nowhere. Joined with
/// the system's separator.
fn normpath(path: &str) -> String {
    let rooted = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if rooted => {}
            other => parts.push(other),
        }
    }
    let joined = parts.join(MAIN_SEPARATOR_STR);
    if rooted {
        format!("{MAIN_SEPARATOR_STR}{joined}")
    } else if joined.is_empty() {
        ".".to_owned()
    } else {
        joined
    }
}

/// Python's `Path.parent`: the path without its last part, the path itself at a root, and nothing for one part alone.
fn parent(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) => parent.to_path_buf(),
        None => path.to_path_buf(),
    }
}

/// A clone's folder as the prototype shows it: with the system's separator.
fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) { text.replace('/', "\\") } else { text }
}

/// The prototype's `sedi_del_registro` of the registry's text: each product's site by its repo without regard to case,
/// the last one written winning; an entry without the separator, or without a repo, is no site.
fn sites_of(raw: Option<&str>) -> BTreeMap<String, String> {
    let words = &format().registry;
    let mut out = BTreeMap::new();
    for entry in raw.unwrap_or_default().split(words.products_separator.as_str()) {
        if let Some((repo, site)) = entry.split_once(words.site_separator.as_str())
            && !py_strip(repo).is_empty()
        {
            out.insert(py_strip(repo).to_lowercase(), py_strip(site).to_owned());
        }
    }
    out
}

/// The prototype's `_prodotti_di` of the registry's text: each product as `<owner>/<name>`, the tracker's owner for one
/// written without; none for the words of none, and none at all when one of them is not a repo's name. A repo written
/// twice, in any case, counts once, as first written.
fn products_of(raw: Option<&str>, owner: Option<&str>) -> Vec<String> {
    let words = &format().registry;
    let value = py_strip(raw.unwrap_or_default());
    if words.none.iter().any(|none| *none == value.to_lowercase()) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for entry in value.split(words.products_separator.as_str()).map(py_strip).filter(|entry| !entry.is_empty()) {
        let full = if entry.contains('/') { entry.to_owned() } else { format!("{}/{entry}", owner.unwrap_or_default()) };
        if !is_repo(&full) || full.ends_with(words.repo_suffix.as_str()) {
            return Vec::new();
        }
        if !out.iter().any(|seen| seen.eq_ignore_ascii_case(&full)) {
            out.push(full);
        }
    }
    out
}

/// The prototype's `repo_del_registro`: the dossier's repo when the registry writes a repo's name.
fn registry_repo(raw: Option<&str>) -> Option<&str> {
    raw.map(py_strip).filter(|repo| is_repo(repo))
}

/// `<owner>/<name>` as the prototype's `REPO_RE` has it: each part ASCII letters, digits, `.`, `_` and `-`, starting with a
/// letter or a digit.
fn is_repo(text: &str) -> bool {
    let part = |part: &str| {
        part.bytes().next().is_some_and(|first| first.is_ascii_alphanumeric()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    };
    text.split_once('/').is_some_and(|(owner, name)| part(owner) && part(name))
}

/// Python's `str.strip()`: the blanks of Rust and the separators `\x1c` to `\x1f`.
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// The prototype's `_ultima`: the last line of `text` that is not blank, stripped; the format's words without one.
fn last(text: &str) -> String {
    text.split(is_line_break).map(py_strip).rfind(|line| !line.is_empty()).map_or_else(|| format().details.no_message.clone(), str::to_owned)
}

#[cfg(test)]
mod tests;
