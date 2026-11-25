//! The writes of `setup`, `project new` and `project set` through `gh`'s own subcommands, as the prototype calls
//! them (G3, G6 of the dossier's conformance study): a label created or updated in place, the body of an issue
//! replaced, the private repo of a dossier and its topic. Each value is a typed argument of `regia-exec`, as
//! `name=value`.

use regia_core::TrackerError;
use regia_exec::{Args, IssueNumber, RepoSlug, Text};

use crate::GhTracker;

/// The URL of the repo `full`, `<owner>/<name>`, on GitHub: the `origin` a dossier's folder gets (`repo_url`).
#[must_use]
pub fn repo_url(full: &str) -> String {
    format!("https://github.com/{full}.git")
}

impl GhTracker {
    /// `gh label create <name> --repo=<owner/repo> --color=<color> --description=<text> --force`: idempotent (G3).
    /// An empty description is passed as such, as the prototype passes it.
    pub(crate) fn label_create(&self, name: &str, color: &str, description: &str) -> Result<(), TrackerError> {
        let mut args = Args::new().word("label").word("create").value(&text(name)?).opt("--repo", &self.repo_slug()?).opt("--color", &text(color)?);
        args = if description.is_empty() { args.word("--description=") } else { args.opt("--description", &text(description)?) };
        self.run_gh("label create", args.word("--force"), None).map(|_| ())
    }

    /// `gh issue edit <n> --repo=<owner/repo> --body-file=-`, with the body on stdin.
    pub(crate) fn issue_edit_body(&self, number: u64, body: &str) -> Result<(), TrackerError> {
        let number = IssueNumber::new(number).map_err(invalid)?;
        let args = Args::new().word("issue").word("edit").value(&number).opt("--repo", &self.repo_slug()?).word("--body-file=-");
        self.run_gh("issue edit", args, Some(body)).map(|_| ())
    }

    /// `gh repo create <owner/name> --private --description=<text>`: the visibility is a literal of the code, and
    /// there is no other (G6).
    pub(crate) fn repo_create_private(&self, full: &str, description: &str) -> Result<(), TrackerError> {
        let slug = RepoSlug::parse(full).map_err(invalid)?;
        let args = Args::new().word("repo").word("create").value(&slug).word("--private");
        let args = if description.is_empty() { args.word("--description=") } else { args.opt("--description", &text(description)?) };
        self.run_gh("repo create", args, None).map(|_| ())
    }

    /// `gh repo edit <owner/name> --add-topic=<topic>`.
    pub(crate) fn repo_add_topic(&self, full: &str, topic: &str) -> Result<(), TrackerError> {
        let slug = RepoSlug::parse(full).map_err(invalid)?;
        let args = Args::new().word("repo").word("edit").value(&slug).opt("--add-topic", &text(topic)?);
        self.run_gh("repo edit", args, None).map(|_| ())
    }
}

fn text(value: &str) -> Result<Text, TrackerError> {
    Text::parse(value).map_err(invalid)
}

fn invalid(error: regia_exec::InvalidValue) -> TrackerError {
    TrackerError::Unavailable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repo_s_url_is_github_s() {
        assert_eq!(repo_url("example/proj-x"), "https://github.com/example/proj-x.git");
    }
}
