//! The cleaning of the tasks' work files (#471; R16 of the dossier's inventory, #269/V12): the class `passaggio`
//! of the dossier's ADR 0019, in `<cache>/<work>/<project>/<task>/`, goes once its task has been closed for the
//! format's days.
//!
//! A file removed by mistake does not come back, so the cleaning fails closed:
//! - only the folders of that one place: a project's id, then a task's number, each a plain folder; any other entry,
//!   and a link or a junction at any level, is neither followed nor removed;
//! - only a task the tracker says closed, and closed long enough: an open task, one the tracker does not know, and
//!   a tracker that does not read keep their folders;
//! - inside a task's folder a link is removed as a link, its target untouched; a folder that holds any other
//!   reparse point (a file of a cloud folder) stays whole;
//! - a dry run counts and removes nothing.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::compaction::is_link;
use crate::time::WallTime;
use crate::v1::format;

/// How a cleaning runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkPolicy {
    pub now: WallTime,
    /// A task closed at least this many days ago loses its folder.
    pub after_days: i64,
    pub dry_run: bool,
}

impl WorkPolicy {
    /// The format's days, at `now`.
    #[must_use]
    pub fn at(now: WallTime, dry_run: bool) -> WorkPolicy {
        WorkPolicy {
            now,
            after_days: format().work.after_days,
            dry_run,
        }
    }
}

/// What a cleaning did, or would do on a dry run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkCleaned {
    pub dry_run: bool,
    /// The task folders removed, as `<project>/<task>`.
    pub removed: Vec<String>,
    /// Task folders of a task the tracker does not say closed.
    pub kept_open: usize,
    /// Task folders of a task closed less than the days ago.
    pub kept_recent: usize,
    /// Task folders that hold a reparse point that is not a link.
    pub kept_reparse: usize,
    /// Entries that are not a project's or a task's folder, or that are links.
    pub skipped: usize,
}

/// Why a cleaning stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WorkError {
    #[error("{} is a link: the work folder is not followed", folder.display())]
    Link { folder: PathBuf },
    #[error("{} is not a work folder: its name is not `{name}`", folder.display())]
    NotWork { folder: PathBuf, name: String },
    #[error("the closed tasks do not read: {0}")]
    Closures(String),
    #[error("cannot {action} {}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The work folder under the cache folder `cache`.
#[must_use]
pub fn work_folder(cache: &Path) -> PathBuf {
    cache.join(&format().work.folder)
}

/// A project's id, as the prototype checks it: a lowercase letter or a digit, then lowercase letters, digits, dashes.
fn project_id(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A task's number: digits, no leading zero.
fn task_number(name: &str) -> Option<u64> {
    (!name.is_empty() && !name.starts_with('0') && name.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| name.parse().ok())
        .flatten()
}

/// The plain folders of `folder`, by name, and how many entries are not one.
fn plain_folders(folder: &Path) -> Result<(Vec<(String, PathBuf)>, usize), WorkError> {
    let entries = fs::read_dir(folder).map_err(|source| WorkError::Io {
        action: "list",
        path: folder.to_path_buf(),
        source,
    })?;
    let mut out = Vec::new();
    let mut other = 0;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(meta) if !is_link(&meta) && meta.is_dir() => {
                out.push((entry.file_name().to_string_lossy().into_owned(), path))
            }
            _ => other += 1,
        }
    }
    out.sort();
    Ok((out, other))
}

/// Whether the tree under `folder` holds a reparse point that is not a link: a link is not followed.
fn holds_other_reparse(folder: &Path) -> Result<bool, std::io::Error> {
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if is_link(&meta) || (meta.is_dir() && holds_other_reparse(&path)?) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Cleans the work folder `root` (see [`work_folder`]): `closed` gives the closed tasks with when they closed, and is
/// asked only when there is a task folder to look at. No folder, nothing to do.
///
/// # Errors
///
/// `root` is a link, or not named as the format's work folder; the closed tasks do not read; a folder cannot be
/// listed or removed. A task folder removed before the error stays removed, and the error says which.
pub fn clean_work(
    root: &Path,
    closed: &dyn Fn() -> Result<BTreeMap<u64, WallTime>, String>,
    policy: &WorkPolicy,
) -> Result<WorkCleaned, WorkError> {
    let mut out = WorkCleaned {
        dry_run: policy.dry_run,
        ..WorkCleaned::default()
    };
    let name = &format().work.folder;
    if root
        .file_name()
        .is_none_or(|own| own.to_string_lossy() != *name)
    {
        return Err(WorkError::NotWork {
            folder: root.to_path_buf(),
            name: name.clone(),
        });
    }
    let meta = match fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(source) => {
            return Err(WorkError::Io {
                action: "read",
                path: root.to_path_buf(),
                source,
            });
        }
    };
    if is_link(&meta) {
        return Err(WorkError::Link {
            folder: root.to_path_buf(),
        });
    }
    let mut tasks = Vec::new();
    let (projects, other) = plain_folders(root)?;
    out.skipped += other;
    for (project, path) in projects {
        if !project_id(&project) {
            out.skipped += 1;
            continue;
        }
        let (folders, other) = plain_folders(&path)?;
        out.skipped += other;
        for (task, folder) in folders {
            match task_number(&task) {
                Some(number) => tasks.push((project.clone(), number, folder)),
                None => out.skipped += 1,
            }
        }
    }
    if tasks.is_empty() {
        return Ok(out);
    }
    let closed = closed().map_err(WorkError::Closures)?;
    let after = policy.after_days.saturating_mul(86_400_000_000);
    for (project, number, folder) in tasks {
        let Some(at) = closed.get(&number) else {
            out.kept_open += 1;
            continue;
        };
        if policy.now.unix_micros() - at.unix_micros() < after {
            out.kept_recent += 1;
            continue;
        }
        if holds_other_reparse(&folder).map_err(|source| WorkError::Io {
            action: "read",
            path: folder.clone(),
            source,
        })? {
            out.kept_reparse += 1;
            continue;
        }
        if !policy.dry_run {
            // std's removal opens every entry as a reparse point and removes a link as a link, never its target.
            fs::remove_dir_all(&folder).map_err(|source| WorkError::Io {
                action: "remove",
                path: folder.clone(),
                source,
            })?;
        }
        out.removed.push(format!("{project}/{number}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_project_s_id_and_a_task_s_number_are_looked_at() {
        assert!(
            project_id("a-1")
                && project_id("1a")
                && !project_id("A")
                && !project_id("-a")
                && !project_id("")
        );
        assert_eq!(task_number("471"), Some(471));
        for name in ["0", "012", "4a", "", "-1"] {
            assert_eq!(task_number(name), None, "{name}");
        }
    }
}
