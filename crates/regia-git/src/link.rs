//! A project's dossier linked to its repo, as `project new --repo` links it (`_controlla_origin`,
//! `_collega_dossier` and `_primo_push` of the prototype): a folder that is a repo with another `origin` is left
//! alone, before the repo exists; a folder that is no repo becomes one on `main`, with the repo as its `origin`;
//! and only a folder made a repo here gets a first commit of the scaffold, pushed with its upstream, so that the
//! first step has somewhere to push. A failure of that commit or push is an outcome, never an error: the project
//! exists by then.

use std::path::Path;

use regia_exec::{Args, DossierPath, Text};

use crate::git::{Git, GitDate, last_line};

/// Whether the folder was a repo before: the prototype's `init` and `esistente`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Linked {
    /// Made a repo here.
    Init,
    /// A repo already.
    Existing,
}

/// A step of the link that failed, with the last line git wrote: the project's repo exists by then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    Init(String),
    Remote(String),
}

/// The outcome of the first commit and push of a dossier made a repo here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstPush {
    /// Pushed: the commit's short id.
    Pushed(String),
    /// No file to commit.
    Nothing,
    /// HEAD names no branch: nothing was committed.
    NoBranch(String),
    /// A step failed; after the commit, the commit stays local.
    Failed { step: PushStep, reason: String },
}

/// The steps of the first push, in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushStep {
    Add,
    Commit,
    Push,
}

/// Whether `folder` holds a repo of its own, as the prototype tests it: a `.git` there, file or folder.
#[must_use]
pub fn is_repo(folder: &Path) -> bool {
    folder.join(".git").exists()
}

/// The `origin` that keeps `folder` from being linked to `url`: the one it has when it is a repo whose `origin` is
/// another URL. `None` when the folder can be linked.
#[must_use]
pub fn other_origin(git: &Git, folder: &Path, url: &str) -> Option<String> {
    if !is_repo(folder) {
        return None;
    }
    git.origin_url(folder).filter(|origin| origin != url)
}

/// `folder` made a repo on `main` when it is not one, then given `url` as its `origin` when it has none.
pub fn link(git: &Git, folder: &Path, url: &str) -> Result<Linked, LinkError> {
    let mut linked = Linked::Existing;
    if !is_repo(folder) {
        let run = git.run(folder, call::init(), None);
        if !run.ok() {
            return Err(LinkError::Init(last_line(&run.stderr)));
        }
        linked = Linked::Init;
    }
    if git.origin_url(folder).is_none() {
        let url = Text::parse(url).map_err(|error| LinkError::Remote(error.to_string()))?;
        let run = git.run(folder, call::add_origin(&url), None);
        if !run.ok() {
            return Err(LinkError::Remote(last_line(&run.stderr)));
        }
    }
    Ok(linked)
}

/// The first commit of `folder`, with `files` alone and `message`, dated `date` when given, then pushed to
/// `origin` with its upstream: the branch HEAD names, as git names it.
#[must_use]
pub fn first_push(git: &Git, folder: &Path, files: &[DossierPath], message: &Text, date: Option<GitDate>) -> FirstPush {
    let branch = git.run(folder, call::branch(), None);
    if !branch.ok() {
        return FirstPush::NoBranch(last_line(&branch.stderr));
    }
    if files.is_empty() {
        return FirstPush::Nothing;
    }
    let branch = match Text::parse(&branch.stdout) {
        Ok(branch) => branch,
        Err(error) => return FirstPush::NoBranch(error.to_string()),
    };
    let origin = Text::parse("origin").expect("a valid text");
    let steps = [(PushStep::Add, call::add(files)), (PushStep::Commit, call::commit(message)), (PushStep::Push, call::push(&origin, &branch))];
    for (step, args) in steps {
        let run = git.run(folder, args, (step == PushStep::Commit).then_some(date).flatten());
        if !run.ok() {
            return FirstPush::Failed { step, reason: last_line(&run.stderr) };
        }
    }
    FirstPush::Pushed(git.run(folder, call::short_head(), None).stdout)
}

/// The argv of each call, in the prototype's order.
mod call {
    use super::*;

    pub fn init() -> Args {
        Args::new().word("init").word("--quiet").word("--initial-branch=main")
    }

    pub fn add_origin(url: &Text) -> Args {
        Args::new().word("remote").word("add").word("--").word("origin").value(url)
    }

    /// The branch HEAD names, short; git says why when it names none.
    pub fn branch() -> Args {
        Args::new().word("symbolic-ref").word("--short").word("HEAD")
    }

    /// A write: each pathspec says it is literal, as the dossier's other writes do.
    pub fn add(paths: &[DossierPath]) -> Args {
        paths.iter().fold(Args::new().word("add").word("--"), Args::pathspec)
    }

    /// What is staged, which in a repo just made is what was added.
    pub fn commit(message: &Text) -> Args {
        Args::new().word("commit").word("--quiet").opt("--message", message)
    }

    /// Never `--force`: the branch to the same name on `remote`, which becomes its upstream.
    pub fn push(remote: &Text, branch: &Text) -> Args {
        Args::new().word("push").word("--quiet").word("--set-upstream").word("--end-of-options").value(remote).value(branch)
    }

    pub fn short_head() -> Args {
        Args::new().word("rev-parse").word("--short").word("HEAD")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(args: &Args) -> Vec<String> {
        args.tokens().map(|token| token.to_string_lossy().into_owned()).collect()
    }

    /// The push never forces, and the add names its paths literally.
    #[test]
    fn the_first_push_never_forces_and_names_only_its_paths() {
        let paths = [DossierPath::parse(".regia/notes.md").unwrap()];
        let push = tokens(&call::push(&Text::parse("origin").unwrap(), &Text::parse("main").unwrap()));
        assert_eq!(push, ["push", "--quiet", "--set-upstream", "--end-of-options", "origin", "main"]);
        assert!(!push.iter().any(|token| token.contains("force") || token.starts_with('+')));
        assert_eq!(tokens(&call::add(&paths)), ["add", "--", ":(literal).regia/notes.md"]);
    }
}
