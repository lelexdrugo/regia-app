//! The dossier: which files a checkpoint commits, and the prototype's `dossier_sync` on them.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use regia_exec::{Args, DossierPath, ExecError, Text};
use serde::{Deserialize, Serialize};

use crate::git::{Git, GitDate, Run, call, last_line};

/// Format v1 of the dossier, as the prototype has it. Its names are data, not identifiers.
const FORMAT: &str = include_str!("../formats/dossier_v1.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Format {
    named: Named,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Named {
    files: Vec<String>,
    archives: Archives,
}

/// Where the archives of the decisions are and what their names look like.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archives {
    folder: String,
    /// `#` is one ASCII digit, any other character itself.
    name: String,
}

fn format() -> &'static Format {
    static FORMAT_V1: OnceLock<Format> = OnceLock::new();
    FORMAT_V1.get_or_init(|| toml::from_str(FORMAT).expect("formats/dossier_v1.toml is valid TOML of its shape"))
}

/// The files every checkpoint commits when they changed, before those it is given.
pub fn named_files() -> Vec<DossierPath> {
    let files = &format().named.files;
    files.iter().map(|file| DossierPath::parse(file).expect("a named file of format v1 is a dossier path")).collect()
}

/// Whether `name` is an archive's name by `model`: the whole of it, `#` for one ASCII digit and every other
/// character for itself, as the prototype's `fullmatch` of `decisioni-[0-9]{4}-[0-9]{2}\.md` does.
fn fits(model: &str, name: &str) -> bool {
    let (mut model, mut name) = (model.chars(), name.chars());
    loop {
        match (model.next(), name.next()) {
            (None, None) => return true,
            (Some('#'), Some(c)) if c.is_ascii_digit() => {}
            (Some(m), Some(c)) if m != '#' && m == c => {}
            _ => return false,
        }
    }
}

/// The archives of the decisions that exist in `root`, by name, each as a dossier path: the prototype's
/// `_archivi_decisioni`. One read of the folder and no call to git; a file only, not a folder that has the
/// name of one, and nothing from its subfolders. A folder that cannot be read has none.
fn archives(root: &Path) -> Vec<DossierPath> {
    let Archives { folder, name } = &format().named.archives;
    let Ok(entries) = std::fs::read_dir(root.join(folder)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|found| fits(name, found))
        .collect();
    names.sort();
    names.iter().map(|found| DossierPath::parse(&format!("{folder}/{found}")).expect("an archive of format v1 is a dossier path")).collect()
}

/// What happened to the dossier, with the contract's names (`dossier_outcome` of the facade table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Committed, or already committed, and pushed to the upstream.
    Pushed,
    /// Committed, and the push did not go: it failed, or a divergence keeps the commit here.
    LocalCommit,
    /// No named file changed, and no commit waits for a push.
    Nothing,
    /// Committed, with nowhere to push: no remote, or a branch with no upstream.
    NoRemote,
    /// No commit was tried.
    Unavailable,
}

/// The outcome of a checkpoint, with the keys of the contract (`docs/contract/commands.md`). Every key
/// is always there; one that does not apply is `null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DossierSync {
    pub outcome: Outcome,
    /// The folder as it was given; `null` when it could not be read.
    pub folder: Option<String>,
    pub branch: Option<String>,
    /// The commit pushed or left local.
    pub sha: Option<String>,
    /// The named files that changed, in git's order: tracked first, then untracked.
    pub files: Vec<String>,
    pub reason: Option<String>,
    /// How many commits the upstream lacks, when there is one.
    pub ahead: Option<u64>,
}

impl DossierSync {
    /// No commit tried, and why.
    pub fn unavailable(folder: Option<String>, reason: String) -> Self {
        DossierSync { reason: Some(reason), ..Self::of(folder) }
    }

    /// Nothing tried yet on `folder`.
    fn of(folder: Option<String>) -> Self {
        DossierSync { outcome: Outcome::Unavailable, folder, branch: None, sha: None, files: Vec::new(), reason: None, ahead: None }
    }

    fn done(mut self, outcome: Outcome, reason: Option<String>) -> Self {
        self.outcome = outcome;
        self.reason = reason;
        self
    }
}

/// A checkpoint to make.
#[derive(Debug, Clone)]
pub struct Checkpoint<'a> {
    /// The project's folder, as it was given: the outcome repeats it.
    pub folder: &'a str,
    pub message: &'a str,
    /// The files given besides the named ones, relative to the folder.
    pub files: &'a [String],
    /// The commit's date when the product's clock is fixed; without it, git's own clock.
    pub date: Option<GitDate>,
}

/// The named files of the dossier, committed and pushed with the user's git. Never an error: a failure
/// is in the outcome.
pub fn sync(request: &Checkpoint) -> DossierSync {
    sync_with(Git::resolve(), request)
}

/// [`sync`] with the git it is given, or the reason there is none.
pub fn sync_with(git: Result<Git, ExecError>, request: &Checkpoint) -> DossierSync {
    let mut out = DossierSync::of(Some(request.folder.to_owned()));
    let folder = Path::new(request.folder);
    if !folder.is_dir() {
        return out.done(Outcome::Unavailable, Some("the project's folder does not exist".to_owned()));
    }
    let git = match git {
        Ok(git) => git,
        Err(error) => return out.done(Outcome::Unavailable, Some(format!("git: {error}"))),
    };
    let at = |args: Args| git.run(folder, args, None);
    let top = at(call::show_toplevel());
    if !top.ok() || !same_folder(Path::new(&top.stdout), folder) {
        let reason = if top.ok() { "the folder is not the root of a git repo".to_owned() } else { format!("git: {}", last_line(&top.stderr)) };
        return out.done(Outcome::Unavailable, Some(reason));
    }
    let paths = match named_paths(folder, request.files) {
        Ok(paths) => paths,
        Err(reason) => return out.done(Outcome::Unavailable, Some(reason)),
    };
    let branch = at(call::branch());
    if !branch.ok() {
        return out.done(Outcome::Unavailable, Some("detached HEAD: no branch to commit on".to_owned()));
    }
    let branch = branch.stdout;
    out.branch = Some(branch.clone());
    let upstream = at(call::upstream());
    let upstream = upstream.ok() && !upstream.stdout.is_empty();
    let (mut warnings, mut diverged) = (Vec::new(), false);
    if upstream {
        let fetch = at(call::fetch());
        if !fetch.ok() {
            warnings.push(format!("fetch failed: {}", last_line(&fetch.stderr)));
        }
        let behind = at(call::behind());
        if behind.ok() && behind.stdout != "0" {
            let merge = at(call::fast_forward());
            if !merge.ok() {
                diverged = true;
                warnings.push(format!("fast-forward failed: {}", last_line(&merge.stderr)));
            }
        }
    }
    let status = at(call::status(&paths));
    if !status.ok() {
        return out.done(Outcome::Unavailable, Some(format!("git status: {}", last_line(&status.stderr))));
    }
    let changed = changed(&status.stdout);
    out.files = changed.iter().map(|(_, path)| path.clone()).collect();
    if upstream {
        let ahead = at(call::ahead());
        out.ahead = if ahead.ok() && !ahead.stdout.is_empty() && ahead.stdout.bytes().all(|b| b.is_ascii_digit()) {
            ahead.stdout.parse().ok()
        } else {
            None
        };
    }
    if changed.is_empty() {
        // Commits made before and never pushed: a push that failed, or another session's (#159).
        if upstream && !diverged && out.ahead.unwrap_or(0) > 0 {
            out.sha = Some(at(call::head()).stdout);
            let retried = match out.ahead {
                Some(1) => "retried: 1 commit already made".to_owned(),
                Some(n) => format!("retried: {n} commits already made"),
                None => unreachable!("ahead is above zero"),
            };
            return push(&git, folder, out, &branch, warnings, Some(retried));
        }
        let reason = joined(&warnings);
        return out.done(Outcome::Nothing, reason);
    }
    // Paths from git's output reach an argv again only typed (threat model §3.6, rule 7).
    // The reason names the path, escaped, and the rule it breaks: what to rename (#571/V5).
    let mut typed = Vec::new();
    for path in &out.files.clone() {
        match DossierPath::parse(path) {
            Ok(path) => typed.push(path),
            Err(error) => {
                let reason = format!("a changed file is not a dossier path: {path:?}: {error}");
                return out.done(Outcome::Unavailable, Some(reason));
            }
        }
    }
    // `commit --only` does not know new files: only the named ones are added.
    let new: Vec<DossierPath> = changed.iter().zip(&typed).filter(|((code, _), _)| code == "??").map(|(_, path)| path.clone()).collect();
    if !new.is_empty() {
        let add = at(call::add(&new));
        if !add.ok() {
            return out.done(Outcome::Unavailable, Some(format!("git add: {}", last_line(&add.stderr))));
        }
    }
    let commit = match Text::parse(request.message) {
        Ok(message) => git.run(folder, call::commit(&message, &typed), request.date),
        Err(error) => Run { code: 1, stdout: String::new(), stderr: error.to_string() },
    };
    if !commit.ok() {
        return out.done(Outcome::Unavailable, Some(format!("git commit: {}", last_line(&commit.stderr))));
    }
    out.sha = Some(at(call::head()).stdout);
    if !upstream {
        let reason = if at(call::remotes()).stdout.is_empty() {
            "no remote configured".to_owned()
        } else {
            format!("the branch {branch} has no upstream")
        };
        return out.done(Outcome::NoRemote, Some(reason));
    }
    out.ahead = Some(out.ahead.unwrap_or(0) + 1);
    if diverged {
        let reason = format!("{} (divergence: the owner resolves it)", warnings.join("; "));
        return out.done(Outcome::LocalCommit, Some(reason));
    }
    push(&git, folder, out, &branch, warnings, None)
}

/// The prototype's `_push_su_upstream`: `HEAD` to the upstream of `branch`, never elsewhere and never
/// with force. `retried` opens the reason of a push of commits made before.
fn push(git: &Git, folder: &Path, out: DossierSync, branch: &str, warnings: Vec<String>, retried: Option<String>) -> DossierSync {
    let config = |key: String| {
        let run = Text::parse(&key).map(|key| git.run(folder, call::config(&key), None));
        run.ok().filter(Run::ok).and_then(|run| Text::parse(&run.stdout).ok())
    };
    let remote = config(format!("branch.{branch}.remote"));
    let merge = config(format!("branch.{branch}.merge"));
    let (Some(remote), Some(merge)) = (remote, merge) else {
        return out.done(Outcome::LocalCommit, Some("upstream with no readable remote or ref".to_owned()));
    };
    let Ok(refspec) = Text::parse(&format!("HEAD:{}", merge.as_str())) else {
        return out.done(Outcome::LocalCommit, Some("upstream with no readable remote or ref".to_owned()));
    };
    let push = git.run(folder, call::push(&remote, &refspec), None);
    if !push.ok() {
        let mut reasons = warnings;
        reasons.push(format!("push: {}", last_line(&push.stderr)));
        return out.done(Outcome::LocalCommit, Some(reasons.join("; ")));
    }
    let mut out = out;
    out.ahead = Some(0);
    let reasons: Vec<String> = retried.into_iter().chain(warnings).collect();
    out.done(Outcome::Pushed, joined(&reasons))
}

fn joined(reasons: &[String]) -> Option<String> {
    (!reasons.is_empty()).then(|| reasons.join("; "))
}

/// `(code, path)` of each entry of `git status --porcelain=v1 -z`, as the prototype's `_cambiati`
/// reads them: a rename or a copy is followed by its old path, which is skipped.
fn changed(stdout: &str) -> Vec<(String, String)> {
    let fields: Vec<&str> = stdout.split('\0').collect();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < fields.len() {
        let field = fields[i];
        if field.chars().count() > 3 {
            out.push((field.chars().take(2).collect(), field.chars().skip(3).collect()));
            if field.starts_with(['R', 'C']) {
                i += 1;
            }
        }
        i += 1;
    }
    out
}

/// Whether git's top level is the folder itself: a repo further up is not the dossier.
pub(crate) fn same_folder(top: &Path, folder: &Path) -> bool {
    match (canonical(top), canonical(folder)) {
        (Ok(top), Ok(folder)) => top == folder,
        _ => false,
    }
}

/// The named files, then the archives of the decisions that exist, then each of `files` as the prototype's
/// `_percorsi_nominati` takes it: relative to the folder, inside it once resolved, and neither the folder itself nor `.git`. Each once, in order.
/// The product also types each one as a [`DossierPath`], which is narrower than the prototype: no space,
/// no letter outside ASCII, no name Windows reserves.
pub fn named_paths(folder: &Path, files: &[String]) -> Result<Vec<DossierPath>, String> {
    let root = canonical(folder).map_err(|error| format!("the project's folder cannot be read: {error}"))?;
    let mut out = named_files();
    out.extend(archives(&root));
    for given in files {
        let path = Path::new(given);
        if path.is_absolute() {
            return Err(format!("--file '{given}': must be relative to the project's folder"));
        }
        let resolved = resolve(&root.join(path));
        let Ok(relative) = resolved.strip_prefix(&root) else {
            return Err(format!("--file '{given}': leaves the project's folder"));
        };
        let parts: Vec<String> = relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
        if parts.first().is_none_or(|first| first.to_lowercase() == ".git") {
            return Err(format!("--file '{given}': names the whole folder or .git"));
        }
        let typed = DossierPath::parse(&parts.join("/")).map_err(|error| format!("--file '{given}': {error}"))?;
        if !out.contains(&typed) {
            out.push(typed);
        }
    }
    Ok(out)
}

/// `path` as Python's non-strict `resolve` gives it: `.` and `..` removed, then the longest part that
/// exists replaced by its real path, symbolic links followed, and the rest kept as it is.
fn resolve(path: &Path) -> PathBuf {
    let mut lexical = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(lexical.components().next_back(), Some(Component::Normal(_))) {
                    lexical.pop();
                }
            }
            other => lexical.push(other.as_os_str()),
        }
    }
    let mut tail = Vec::new();
    let mut head = lexical.as_path();
    loop {
        if let Ok(real) = canonical(head) {
            return tail.iter().rev().fold(real, |path, part| path.join(part));
        }
        match (head.file_name(), head.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                head = parent;
            }
            _ => return lexical,
        }
    }
}

/// The real path, without the `\\?\` that Windows adds to a path on a drive.
fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let real = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if let Some(Component::Prefix(prefix)) = real.components().next()
            && let Prefix::VerbatimDisk(_) = prefix.kind()
            && let Some(text) = real.to_str()
        {
            return Ok(PathBuf::from(&text[4..]));
        }
    }
    Ok(real)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_named_files_are_dossier_paths() {
        let named = named_files();
        assert_eq!(named.len(), 3);
        assert!(named.iter().all(|path| path.as_str().starts_with(".regia/")));
    }

    #[test]
    fn the_model_of_an_archive_is_the_prototype_s_whole_name() {
        let model = &format().named.archives.name;
        for name in ["decisioni-2026-11.md", "decisioni-0000-00.md", "decisioni-9999-99.md"] {
            assert!(fits(model, name), "{name}");
        }
        for name in [
            "decisioni-2026-1.md",
            "decisioni-2026-111.md",
            "decisioni-vecchie.md",
            "decisioni-2026-11.md.bak",
            "xdecisioni-2026-11.md",
            "decisioni-2026-11.MD",
            "Decisioni-2026-11.md",
            "decisioni-2026-11.mdx",
            "decisioni-20a6-11.md",
            "decisioni-202\u{666}-11.md",
            "decisioni-2026_11.md",
            "decisioni.md",
            "",
        ] {
            assert!(!fits(model, name), "{name}");
        }
    }

    /// A project's folder with a `.regia/` that holds `entries`: a path ending in `/` is a folder.
    fn dossier_with(tag: &str, entries: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("regia-git-archives-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for entry in entries {
            let path = root.join(entry);
            match entry.strip_suffix('/') {
                Some(_) => std::fs::create_dir_all(&path).unwrap(),
                None => {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, "text\n").unwrap();
                }
            }
        }
        std::fs::create_dir_all(root.join(".regia")).unwrap();
        root
    }

    fn names(paths: &[DossierPath]) -> Vec<&str> {
        paths.iter().map(DossierPath::as_str).collect()
    }

    #[test]
    fn the_archives_that_exist_follow_the_named_files_in_name_order() {
        let root = dossier_with("present", &[".regia/decisioni-2026-11.md", ".regia/decisioni-2025-12.md", ".regia/notes.md"]);
        let paths = named_paths(&root, &["docs/plan.md".to_owned()]).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        let fixed = named_files();
        assert_eq!(names(&paths)[..3], names(&fixed)[..]);
        assert_eq!(names(&paths)[3..], [".regia/decisioni-2025-12.md", ".regia/decisioni-2026-11.md", "docs/plan.md"]);
    }

    #[test]
    fn a_dossier_without_archives_has_the_named_files_only() {
        let root = dossier_with("none", &[".regia/notes.md"]);
        let paths = named_paths(&root, &[]).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(paths, named_files());
        let bare = std::env::temp_dir().join(format!("regia-git-archives-{}-bare", std::process::id()));
        std::fs::create_dir_all(&bare).unwrap();
        let paths = named_paths(&bare, &[]).unwrap();
        let _ = std::fs::remove_dir_all(&bare);
        assert_eq!(paths, named_files(), "a folder with no .regia has no archive, and that is no error");
    }

    #[test]
    fn a_name_outside_the_model_is_not_an_archive() {
        let root = dossier_with(
            "outside",
            &[
                ".regia/decisioni-2026-1.md",
                ".regia/decisioni-vecchie.md",
                ".regia/decisioni-2026-11.md.bak",
                ".regia/notes/decisioni-2026-11.md",
                "decisioni-2026-11.md",
                ".regia/decisioni-2026-03.md",
            ],
        );
        let paths = named_paths(&root, &[]).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(paths.len(), named_files().len() + 1);
        assert_eq!(paths.last().unwrap().as_str(), ".regia/decisioni-2026-03.md");
    }

    #[test]
    fn a_folder_with_the_name_of_an_archive_is_not_one() {
        let root = dossier_with("folder", &[".regia/decisioni-2026-02.md/inside.md", ".regia/decisioni-2026-03.md"]);
        let paths = named_paths(&root, &[]).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(names(&paths)[3..], [".regia/decisioni-2026-03.md"]);
    }

    #[test]
    fn an_archive_given_with_file_is_kept_once() {
        let root = dossier_with("twice", &[".regia/decisioni-2026-11.md", ".regia/decisioni-2026-12.md"]);
        let given = [".regia/decisioni-2026-11.md".to_owned(), ".regia/./decisioni-2026-11.md".to_owned(), "docs/a.md".to_owned()];
        let paths = named_paths(&root, &given).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            names(&paths)[3..],
            [".regia/decisioni-2026-11.md", ".regia/decisioni-2026-12.md", "docs/a.md"],
            "it keeps its place among the archives, and the --file adds nothing"
        );
    }

    #[test]
    fn porcelain_entries_skip_the_old_path_of_a_rename() {
        let out = changed(" M a.md\0R  new.md\0old.md\0?? n/é.md\0\0");
        let codes: Vec<(&str, &str)> = out.iter().map(|(code, path)| (code.as_str(), path.as_str())).collect();
        assert_eq!(codes, [(" M", "a.md"), ("R ", "new.md"), ("??", "n/é.md")]);
        assert!(changed("").is_empty());
    }
}
