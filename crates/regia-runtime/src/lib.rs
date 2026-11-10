//! The composition root shared by the CLI and the app. Only this crate knows which adapter serves which
//! port, so the two front ends cannot drift apart.

use std::path::PathBuf;

use regia_core::{
    Brief, BriefOptions, Tracker, TrackerBinding, TrackerConfigV1, TrackerError, V1Error, build_brief, parse_date,
};
use regia_github::GhTracker;
use regia_identity::{env_var, prototype_env_var, state_dir};

#[derive(Debug, thiserror::Error)]
pub enum BriefError {
    #[error(transparent)]
    Format(#[from] V1Error),
    #[error(transparent)]
    Tracker(#[from] TrackerError),
    #[error("{name} is `{value}`, not a date YYYY-MM-DD")]
    Today { name: String, value: String },
}

/// The dossier of the prototype, while the prototype exists.
pub fn dossier_dir() -> Option<PathBuf> {
    std::env::var_os(prototype_env_var("HOME")).filter(|value| !value.is_empty()).map(PathBuf::from)
}

pub fn load_brief(options: &BriefOptions) -> Result<Brief, BriefError> {
    // `REGIA_TODAY` makes the brief count due dates from another day: the conformance fixtures are recorded
    // on a fixed one. A value that is not a date is an error, never the clock in its place.
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
