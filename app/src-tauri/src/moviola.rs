//! The moviola's commands (threat model §3.4): the portfolio, a project's timeline and one sidecar, in place of the
//! routes of the prototype's local viewer, which the product does not port (ADR 0001 §9). The core reads and builds
//! the answers in its own types, made for the view; this module asks `regia-runtime` for the projects and for the
//! settings of their links, and starts nothing and reads no tracker itself. The registry is composed there, and so
//! is the remote of each project's repo, which gives a commit its URL. The commands' names and their arguments' are
//! provisional until the gate of the names (#223 of the dossier); `get_snapshot` is the route of a sidecar that
//! #400 of the dossier turned into a command.

use std::sync::{Mutex, MutexGuard, PoisonError};

use regia_core::moviola::{self, LinkSettings, Project, ProjectSummary, Range, Snapshot, Timeline};
use regia_runtime::{moviola_projects, project_links, tracker_links};
use tauri::State;

/// The longest id the webview may send, as the threat model's opaque tokens; an instant of a range is shorter.
const ID_LIMIT: usize = 64;

/// The projects and the tracker's part of the links' settings of the last read. An id from the webview is compared with these
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
            *known = Some(moviola_projects().map_err(|error| error.to_string())?);
        }
        known.iter().flatten().find(|project| project.id == id).cloned().ok_or_else(|| format!("unknown project: {id}"))
    }

    fn links(&self) -> Result<LinkSettings, String> {
        let mut known = lock(&self.links);
        if known.is_none() {
            *known = Some(tracker_links().map_err(|error| error.to_string())?);
        }
        Ok(known.clone().unwrap_or_default())
    }
}

/// L0: the projects with a folder on this machine, by id, with the health and the last milestone of each.
/// Reads the tracker, so it runs off the main thread, and keeps the projects for the two commands below.
#[tauri::command(async)]
pub fn list_projects(state: State<'_, Moviola>) -> Result<Vec<ProjectSummary>, String> {
    let projects = moviola_projects().map_err(|error| error.to_string())?;
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
    // The remote is the project's own: read in its folder at each call, as the prototype's view reads it.
    let settings = project_links(&state.links()?, &project.folder);
    moviola::timeline(&project, &settings, &range).map_err(|error| error.to_string())
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
