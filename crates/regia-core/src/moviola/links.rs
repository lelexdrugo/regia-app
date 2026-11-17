//! A milestone's links (§8 of the dossier's `docs/moviola/CONTRATTO.md`, the prototype's `Rimandi` of `vista.py`
//! and the helpers of `fonti.py`): issues to the tracker's repo, commits to the web page of the project's repo,
//! reports resolved by the dossier's one resolver of references. What the links need comes from the caller: the
//! tracker's configuration and the project's remote are read by adapters, not here.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

use super::records::Refs;
use crate::reference::{ReferenceKind, resolve};
use crate::text::normalize_title;

/// What the links of a project come from. An empty value is no value, as the prototype's truth test reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkSettings {
    /// The tracker's owner and repo: an issue has a URL only with both.
    pub owner: Option<String>,
    pub repo: Option<String>,
    /// The Project of the tracker, as its configuration gives it.
    pub project_url: Option<String>,
    /// The web page of the project's repo, as [`normalize_remote`] gives it: a commit has a URL only with it.
    pub remote: Option<String>,
}

fn given(value: Option<&String>) -> Option<&str> {
    value.map(String::as_str).filter(|value| !value.is_empty())
}

impl LinkSettings {
    /// As the configuration gives it, an empty one included: the prototype passes it on as it is.
    pub fn project_url(&self) -> Option<String> {
        self.project_url.clone()
    }

    /// `https://github.com/<owner>/<repo>/issues/<n>` (`url_issue`).
    pub fn issue_url(&self, number: u64) -> Option<String> {
        let (owner, repo) = (given(self.owner.as_ref())?, given(self.repo.as_ref())?);
        Some(format!("https://github.com/{owner}/{repo}/issues/{number}"))
    }

    pub fn commit_url(&self, hash: &str) -> Option<String> {
        given(self.remote.as_ref()).map(|remote| format!("{remote}/commit/{hash}"))
    }
}

/// A milestone's links, in the order of its references.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Links {
    pub issue: Vec<IssueLink>,
    pub commit: Vec<CommitLink>,
    pub report: Vec<ReportLink>,
}

/// `#<n>`, and its URL when the tracker is known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct IssueLink {
    pub text: String,
    pub url: Option<String>,
}

/// The hash's first seven characters, its URL when the remote is known, and the hash as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommitLink {
    pub text: String,
    pub url: Option<String>,
    pub hash: String,
}

/// A report's reference as the resolver reads it: never opened, and never followed out of the dossier. `path`
/// is on this machine, `null` when the reference does not resolve; `exists` says a file is there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ReportLink {
    pub text: String,
    pub path: Option<String>,
    pub kind: ReportKind,
    pub exists: bool,
}

/// The resolver's kinds of a reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    Relative,
    Data,
    Absolute,
    Outside,
    NotAPath,
}

impl From<ReferenceKind> for ReportKind {
    fn from(kind: ReferenceKind) -> Self {
        match kind {
            ReferenceKind::Relative => ReportKind::Relative,
            ReferenceKind::Data => ReportKind::Data,
            ReferenceKind::Absolute => ReportKind::Absolute,
            ReferenceKind::Outside => ReportKind::Outside,
            ReferenceKind::NotAPath(_) => ReportKind::NotAPath,
        }
    }
}

/// The links of `refs`, for the project whose dossier is `folder`.
pub fn links(refs: &Refs, folder: &Path, settings: &LinkSettings) -> Links {
    Links {
        issue: refs.issue.iter().map(|&number| IssueLink { text: format!("#{number}"), url: settings.issue_url(number) }).collect(),
        commit: refs
            .commit
            .iter()
            .map(|hash| CommitLink { text: hash.chars().take(7).collect(), url: settings.commit_url(hash), hash: hash.clone() })
            .collect(),
        report: refs
            .report
            .iter()
            .map(|reference| {
                let resolved = resolve(reference, folder);
                ReportLink {
                    text: normalize_title(reference).text,
                    path: resolved.path().map(|path| path.display().to_string()),
                    kind: resolved.kind().into(),
                    exists: resolved.exists(),
                }
            })
            .collect(),
    }
}

/// The web URL of a git remote, without credentials or `.git`, as the prototype's `normalizza_remote` gives it:
/// `user@host:path` and `ssh://user@host[:port]/path` become `https://host/path`; `http` and `https` keep their
/// scheme and lose what precedes an `@`. `None` for any other form.
pub fn normalize_remote(url: &str) -> Option<String> {
    static SSH: OnceLock<Regex> = OnceLock::new();
    static WEB: OnceLock<Regex> = OnceLock::new();
    // Python's `$` also matches before one line feed at the end.
    let ssh = SSH.get_or_init(|| Regex::new(r"^(?:ssh://)?[\w.-]+@([\w.-]+)[:/](?:\d+/)?(.+?)(?:\.git)?/?\n?\z").expect("a valid pattern"));
    let web = WEB.get_or_init(|| Regex::new(r"^(https?)://(?:[^@/]*@)?([^/]+)/(.+?)(?:\.git)?/?\n?\z").expect("a valid pattern"));
    if let Some(found) = ssh.captures(url).filter(|_| !url.starts_with("http://") && !url.starts_with("https://")) {
        return Some(format!("https://{}/{}", &found[1], &found[2]));
    }
    web.captures(url).map(|found| format!("{}://{}/{}", &found[1], &found[2], &found[3]))
}
