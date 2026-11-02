//! The composition root shared by the CLI and the app. Only this crate knows which adapter serves
//! which port, so the two front ends cannot drift apart.

use std::path::PathBuf;

use regia_core::{Brief, BriefOptions, Tracker, TrackerBinding, TrackerConfigV1, TrackerError, V1Error, build_brief};
use regia_github::GhTracker;
use regia_identity::{env_var, state_dir};

#[derive(Debug, thiserror::Error)]
pub enum BriefError {
    #[error(transparent)]
    Format(#[from] V1Error),
    #[error(transparent)]
    Tracker(#[from] TrackerError),
}

/// The dossier of the prototype, while the prototype exists.
pub fn dossier_dir() -> Option<PathBuf> {
    std::env::var_os(env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from)
}

pub fn load_brief(options: &BriefOptions) -> Result<Brief, BriefError> {
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref())?;
    let binding = TrackerBinding::new(&config)?;
    let raw = GhTracker::new(&config).open_issues()?;
    let tasks = raw
        .into_iter()
        .map(|issue| binding.resolve(issue))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(build_brief(tasks, &binding, options))
}
