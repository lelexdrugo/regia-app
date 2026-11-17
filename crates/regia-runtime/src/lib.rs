//! The composition root shared by the CLI and the app. Only this crate knows which adapter serves which
//! port, so the two front ends cannot drift apart.

use std::path::PathBuf;

use regia_core::{
    Brief, BriefOptions, Registry, Tracker, TrackerBinding, TrackerConfigV1, TrackerError, V1Error, build_brief, parse_date,
};
use regia_github::GhTracker;
use regia_identity::{env_var, prototype_env_var, state_dir};

mod moviola;

pub use moviola::{MoviolaError, moviola_projects, project_links, project_remote, registry_projects, tracker_links};

#[derive(Debug, thiserror::Error)]
pub enum BriefError {
    #[error(transparent)]
    Format(#[from] V1Error),
    #[error(transparent)]
    Tracker(#[from] TrackerError),
    #[error("{name} is `{value}`, not a date YYYY-MM-DD")]
    Today { name: String, value: String },
}

/// Why the folder of a project was not read. The reasons of `checkpoint` and `step finish` repeat it.
#[derive(Debug, thiserror::Error)]
pub enum FolderError {
    #[error(transparent)]
    Format(#[from] V1Error),
    #[error(transparent)]
    Tracker(#[from] TrackerError),
    #[error("the registry of '{0}' has no folder")]
    NoFolder(String),
    #[error("project '{0}' is unknown")]
    Unknown(String),
}

/// The dossier of the prototype, while the prototype exists.
pub fn dossier_dir() -> Option<PathBuf> {
    std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// The folder the registry of the project's objective names, read from the tracker as the prototype
/// reads it: the first open objective with the project's label. Its status plays no part, so an option
/// the binding does not know on some issue does not hide the folder.
pub fn project_folder(project: &str) -> Result<String, FolderError> {
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref())?;
    let binding = TrackerBinding::new(&config)?;
    for mut issue in GhTracker::new(&config).open_issues()? {
        let body = std::mem::take(&mut issue.body);
        issue.status_option_id = None;
        let task = binding.resolve(issue)?;
        if task.objective && task.project.as_deref() == Some(project) {
            let folder = Registry::parse(&body).folder().map(str::to_owned);
            return folder.ok_or_else(|| FolderError::NoFolder(project.to_owned()));
        }
    }
    Err(FolderError::Unknown(project.to_owned()))
}

pub fn load_brief(options: &BriefOptions) -> Result<Brief, BriefError> {
    // `REGIA_TODAY` makes the brief count due dates from another day: the conformance fixtures are recorded
    // on a fixed one. A value that is not a date is an error, never the clock in its place. Without it the
    // day is the system's clock in the system's zone, and `TZ` does not change it on Windows (`BriefOptions::today`).
    let mut options = options.clone();
    let name = env_var("TODAY");
    if let (None, Ok(value)) = (options.today, std::env::var(&name)) {
        options.today = Some(parse_date(&value).ok_or(BriefError::Today { name, value })?);
    }
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref())?;
    let binding = TrackerBinding::new(&config)?;
    let raw = GhTracker::new(&config).open_issues()?;
    let tasks = raw
        .into_iter()
        .map(|issue| binding.resolve(issue))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(build_brief(tasks, &binding, &options))
}
