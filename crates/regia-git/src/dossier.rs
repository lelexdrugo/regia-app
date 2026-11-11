//! The dossier: which files a checkpoint commits, where the registry names the folder, and the
//! prototype's `dossier_sync` on them.

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
    registry: Registry,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Named {
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    marker: String,
    folder: String,
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

/// The project's folder as the registry block of its objective's body names it, as the prototype's
/// `registro` reads it: the first `<!-- <marker> … -->`, its lines `key: value`, the last line of the key.
/// `None` without a block, without the key, or with the key empty.
pub fn registry_folder(body: &str) -> Option<String> {
    let registry = &format().registry;
    let block = registry_block(body, &registry.marker)?;
    let mut folder = None;
    for line in block.split(is_line_break) {
        if let Some((key, value)) = line.split_once(':')
            && key.trim() == registry.folder
        {
            folder = Some(value.trim().to_owned());
        }
    }
    folder.filter(|folder| !folder.is_empty())
}

/// The text of the first `<!--\s*<marker>\s*(.*?)-->` in `body`.
fn registry_block<'a>(body: &'a str, marker: &str) -> Option<&'a str> {
    let mut rest = body;
    while let Some(start) = rest.find("<!--") {
        rest = &rest[start + 4..];
        if let Some(inner) = rest.trim_start().strip_prefix(marker) {
            let inner = inner.trim_start();
            return inner.find("-->").map(|end| &inner[..end]);
        }
    }
    None
}

fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}')
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
    let typed: Result<Vec<DossierPath>, _> = out.files.iter().map(|path| DossierPath::parse(path)).collect();
    let typed = match typed {
        Ok(typed) => typed,
        Err(error) => return out.done(Outcome::Unavailable, Some(format!("a changed file is not a dossier path: {error}"))),
    };
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
fn same_folder(top: &Path, folder: &Path) -> bool {
    match (canonical(top), canonical(folder)) {
        (Ok(top), Ok(folder)) => top == folder,
        _ => false,
    }
}

/// The named files, then each of `files` as the prototype's `_percorsi_nominati` takes it: relative to
/// the folder, inside it once resolved, and neither the folder itself nor `.git`. Each once, in order.
/// The product also types each one as a [`DossierPath`], which is narrower than the prototype: no space,
/// no letter outside ASCII, no name Windows reserves.
pub fn named_paths(folder: &Path, files: &[String]) -> Result<Vec<DossierPath>, String> {
    let root = canonical(folder).map_err(|error| format!("the project's folder cannot be read: {error}"))?;
    let mut out = named_files();
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

    /// The registry of the prototype's objectives: the block with its marker, `key: value` lines, the
    /// last line of a key winning, an empty value being none.
    #[test]
    fn the_registry_names_the_folder() {
        let marker = &format().registry.marker;
        let key = &format().registry.folder;
        let body = format!("intro\n<!-- other -->\n<!--\n  {marker}\nid: alpha\n{key}: C:\\work\\alpha \n-->\n\n## Goal\n");
        assert_eq!(registry_folder(&body).as_deref(), Some("C:\\work\\alpha"));
        let twice = format!("<!-- {marker}\n{key}: /a\n{key}:  /b:c \n-->");
        assert_eq!(registry_folder(&twice).as_deref(), Some("/b:c"), "the last line wins; a colon in the value stays");
        assert_eq!(registry_folder(&format!("<!-- {marker}\nid: alpha\n-->")), None);
        assert_eq!(registry_folder(&format!("<!-- {marker}\n{key}:\n-->")), None, "an empty folder is none");
        assert_eq!(registry_folder(&format!("<!-- {marker}\n{key}: /a\n")), None, "a block that does not close");
        assert_eq!(registry_folder(&format!("<!-- {marker}-->")), None);
        assert_eq!(registry_folder("no block"), None);
    }

    #[test]
    fn porcelain_entries_skip_the_old_path_of_a_rename() {
        let out = changed(" M a.md\0R  new.md\0old.md\0?? n/é.md\0\0");
        let codes: Vec<(&str, &str)> = out.iter().map(|(code, path)| (code.as_str(), path.as_str())).collect();
        assert_eq!(codes, [(" M", "a.md"), ("R ", "new.md"), ("??", "n/é.md")]);
        assert!(changed("").is_empty());
    }
}
