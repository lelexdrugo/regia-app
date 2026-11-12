//! The moviola's commands (threat model §3.4): the portfolio, a project's timeline and one sidecar, in place of the
//! routes of the prototype's local viewer, which the product does not port (ADR 0001 §9). The core reads and builds
//! the answers in its own types, made for the view; this module finds the projects and the settings of their
//! links. The commands' names and their arguments' are provisional until the gate of the names (#223 of the
//! dossier); `get_snapshot` is the route of a sidecar that #400 of the dossier turned into a command.
//!
//! The registry is read here, through the tracker, as `projects` of the CLI reads it: that composition belongs
//! to `regia-runtime`, where it can move. The project's remote is not read yet, so a commit has no URL.

use std::sync::{Mutex, MutexGuard, PoisonError};

use regia_core::moviola::{self, LinkSettings, Project, ProjectSummary, Range, Snapshot, Timeline};
use regia_core::{Registry, Tracker, TrackerBinding, TrackerConfigV1, project_entry};
use regia_github::GhTracker;
use regia_identity::state_dir;
use regia_runtime::dossier_dir;
use serde_json::Value;
use tauri::State;

/// The longest id the webview may send, as the threat model's opaque tokens; an instant of a range is shorter.
const ID_LIMIT: usize = 64;

/// The projects and the links' settings of the last read. An id from the webview is compared with these
/// projects, never joined to a path (threat model T05).
#[derive(Debug, Default)]
pub struct Moviola {
    projects: Mutex<Option<Vec<Project>>>,
    links: Mutex<Option<LinkSettings>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Moviola {
    /// A state that never reads the tracker: for the tests.
    #[cfg(test)]
    pub fn with(projects: Vec<Project>, links: LinkSettings) -> Moviola {
        Moviola { projects: Mutex::new(Some(projects)), links: Mutex::new(Some(links)) }
    }

    /// The project `id` names among those known, the registry read first if none is.
    fn project(&self, id: &str) -> Result<Project, String> {
        let valid = (1..=ID_LIMIT).contains(&id.len())
            && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        if !valid {
            return Err("not a project id".to_owned());
        }
        let mut known = lock(&self.projects);
        if known.is_none() {
            *known = Some(read_projects()?);
        }
        known.iter().flatten().find(|project| project.id == id).cloned().ok_or_else(|| format!("unknown project: {id}"))
    }

    fn links(&self) -> Result<LinkSettings, String> {
        let mut known = lock(&self.links);
        if known.is_none() {
            *known = Some(read_link_settings()?);
        }
        Ok(known.clone().unwrap_or_default())
    }
}

/// The projects of the registry whose folder is a folder on this machine, as the prototype's `moviola serve --tutti`
/// takes them, with the title of their entry: the entry of each open Objective as `projects` builds it, so the
/// registry's own `id` and title win over the Objective's, and the first of two entries with one id is kept.
fn read_projects() -> Result<Vec<Project>, String> {
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref()).map_err(|error| error.to_string())?;
    let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
    let mut projects: Vec<Project> = Vec::new();
    for mut issue in GhTracker::new(&config).open_issues().map_err(|error| error.to_string())? {
        let body = std::mem::take(&mut issue.body);
        // Only the Objectives count, and their status plays no part: an option the configuration does not bind
        // on some issue does not stop the list.
        issue.status_option_id = None;
        let task = binding.resolve(issue).map_err(|error| error.to_string())?;
        if !task.objective {
            continue;
        }
        let entry = project_entry(&task, &Registry::parse(&body), &config.owner).map_err(|error| error.to_string())?;
        let text = |key: &str| entry.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned);
        let (Some(id), Some(folder)) = (text("id"), text("folder")) else { continue };
        if std::path::Path::new(&folder).is_dir() && !projects.iter().any(|project| project.id == id) {
            projects.push(Project { id, folder: folder.into(), title: text("title") });
        }
    }
    Ok(projects)
}

/// The tracker's owner, repo and Project, by the prototype's rule: a local configuration that does not read is an
/// error, which names it; with none, or the dossier's unreadable, the links have no URL.
fn read_link_settings() -> Result<LinkSettings, String> {
    let config = TrackerConfigV1::load_for_links(state_dir().as_deref(), dossier_dir().as_deref()).map_err(|error| error.to_string())?;
    Ok(config.map_or_else(LinkSettings::default, |config| LinkSettings {
        owner: Some(config.owner),
        repo: Some(config.repo),
        project_url: config.project_url,
        remote: None,
    }))
}

/// L0: the projects with a folder on this machine, by id, with the health and the last milestone of each.
/// Reads the tracker, so it runs off the main thread, and keeps the projects for the two commands below.
#[tauri::command(async)]
pub fn list_projects(state: State<'_, Moviola>) -> Result<Vec<ProjectSummary>, String> {
    let projects = read_projects()?;
    let portfolio = moviola::portfolio(&projects).map_err(|error| error.to_string())?;
    *lock(&state.projects) = Some(projects);
    Ok(portfolio)
}

/// L1 to L3 of a project, its sidecars left out: the milestones from `from`, included, to `to`, excluded, each a
/// date or an ISO 8601 instant, as `moviola export` reads them; directions and aggregates whole.
#[tauri::command(async, rename_all = "snake_case")]
pub fn get_timeline(state: State<'_, Moviola>, project_id: String, from: Option<String>, to: Option<String>) -> Result<Timeline, String> {
    let project = state.project(&project_id)?;
    if [&from, &to].into_iter().flatten().any(|instant| instant.len() > ID_LIMIT) {
        return Err("not a date or an instant".to_owned());
    }
    let range = Range::parse(from.as_deref(), to.as_deref()).map_err(|error| error.to_string())?;
    moviola::timeline(&project, &state.links()?, &range).map_err(|error| error.to_string())
}

/// The sidecar of a project's milestone, on demand: `null` when the project has no milestone with that id, or the
/// milestone has no sidecar.
#[tauri::command(async, rename_all = "snake_case")]
pub fn get_snapshot(state: State<'_, Moviola>, project_id: String, milestone_id: String) -> Result<Option<Snapshot>, String> {
    let project = state.project(&project_id)?;
    let valid = (1..=ID_LIMIT).contains(&milestone_id.len())
        && milestone_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !valid {
        return Err("not a milestone id".to_owned());
    }
    moviola::snapshot(&project.folder, &milestone_id).map_err(|error| error.to_string())
}
