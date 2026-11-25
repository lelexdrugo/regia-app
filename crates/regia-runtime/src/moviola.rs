//! The moviola's registry and the settings of its links, composed here as the other reads are (the app and the
//! CLI call this crate, neither composes it): the projects of the registry as the prototype's `moviola serve
//! --tutti` takes them, and for each project the remote of its own repo, read through `regia-git` (the only
//! crate that starts `git`) and reduced to its web page before it goes anywhere.

use std::path::Path;

use regia_core::moviola::{LinkSettings, Project, normalize_remote};
use regia_core::{EntryError, Registry, Tracker, TrackerBinding, TrackerConfigV1, TrackerError, V1Error, project_entry};
use regia_git::Git;
use regia_github::GhTracker;
use regia_identity::{config_dir, state_dir};

use crate::dossier_dir;

#[derive(Debug, thiserror::Error)]
pub enum MoviolaError {
    #[error(transparent)]
    Format(#[from] V1Error),
    #[error(transparent)]
    Tracker(#[from] TrackerError),
    #[error(transparent)]
    Entry(#[from] EntryError),
}

/// The projects of the registry whose folder is a folder on this machine, read from the tracker: see
/// [`registry_projects`].
pub fn moviola_projects() -> Result<Vec<Project>, MoviolaError> {
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref(), config_dir().as_deref())?;
    let binding = TrackerBinding::new(&config)?;
    registry_projects(&GhTracker::new(&config), &binding, &config.owner)
}

/// The projects of the registry with a folder on this machine, with the title of their entry: the entry of each
/// open Objective as `projects` builds it, so the registry's own `id` and title win over the Objective's, and the
/// first of two entries with one id is kept. Only the Objectives count, and their status plays no part: an option
/// the configuration does not bind on some issue does not stop the list.
pub fn registry_projects(tracker: &impl Tracker, binding: &TrackerBinding, owner: &str) -> Result<Vec<Project>, MoviolaError> {
    let mut projects: Vec<Project> = Vec::new();
    for mut issue in tracker.open_issues()? {
        let body = std::mem::take(&mut issue.body);
        issue.status_option_id = None;
        let task = binding.resolve(issue)?;
        if !task.objective {
            continue;
        }
        let entry = project_entry(&task, &Registry::parse(&body), owner)?;
        let text = |key: &str| entry.get(key).and_then(|value| value.as_str()).filter(|value| !value.is_empty()).map(str::to_owned);
        let (Some(id), Some(folder)) = (text("id"), text("folder")) else { continue };
        if Path::new(&folder).is_dir() && !projects.iter().any(|project| project.id == id) {
            projects.push(Project { id, folder: folder.into(), title: text("title") });
        }
    }
    Ok(projects)
}

/// What the links of every project share: the tracker's owner, repo and Project, by the prototype's rule. A local
/// configuration that does not read is an error, which names it; with none, or the dossier's unreadable, the links
/// have no URL. The remote is not here: it is each project's own ([`project_links`]).
pub fn tracker_links() -> Result<LinkSettings, MoviolaError> {
    let config = TrackerConfigV1::load_for_links(state_dir().as_deref(), dossier_dir().as_deref(), config_dir().as_deref())?;
    Ok(config.map_or_else(LinkSettings::default, |config| LinkSettings {
        owner: Some(config.owner),
        repo: Some(config.repo),
        project_url: config.project_url,
        remote: None,
    }))
}

/// The web page of the repo in `folder`, from the URL of its `origin`: the prototype's `url_remote`. `None` when
/// there is no `origin`, when git does not answer, or when the URL is not a web one. The URL as git gives it may
/// carry credentials, and it does not leave this function: `normalize_remote` drops them.
pub fn project_remote(folder: &Path) -> Option<String> {
    let url = Git::resolve().ok()?.origin_url(folder)?;
    normalize_remote(&url)
}

/// The settings of the links of the project in `folder`: those the tracker gives, with the remote of this
/// project's repo and no other.
pub fn project_links(tracker: &LinkSettings, folder: &Path) -> LinkSettings {
    LinkSettings { remote: project_remote(folder), ..tracker.clone() }
}
