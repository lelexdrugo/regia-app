//! Reader of the v1 formats, as the prototype leaves them on disk and in the tracker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::{Maturity, Status, Task};
use crate::tracker::RawIssue;

const VOCABULARY: &str = include_str!("../formats/tracker_v1.toml");

#[derive(Debug, thiserror::Error)]
pub enum V1Error {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("invalid tracker config {path}: {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
    #[error("no tracker config: looked in {0}")]
    Missing(String),
    #[error("tracker config has no option for status `{0}`")]
    MissingStatus(&'static str),
    #[error("issue #{number} has status option `{option}`, which the tracker config does not bind")]
    UnknownOption { number: u64, option: String },
}

#[derive(Debug, Deserialize)]
struct Vocabulary {
    labels: Labels,
    status_labels: BTreeMap<String, Status>,
}

#[derive(Debug, Deserialize)]
struct Labels {
    project_prefix: String,
    objective: String,
    session: String,
    maturity: MaturityLabels,
}

#[derive(Debug, Deserialize)]
struct MaturityLabels {
    draft: String,
    ready: String,
}

fn vocabulary() -> Vocabulary {
    toml::from_str(VOCABULARY).expect("the embedded v1 vocabulary is valid")
}

/// `tracker.json`, in the v1 format. Only the fields this slice reads.
#[derive(Debug, Clone, Deserialize)]
pub struct TrackerConfigV1 {
    pub owner: String,
    pub repo: String,
    pub project_id: String,
    pub status_field_id: String,
    pub status_options: BTreeMap<String, String>,
}

impl TrackerConfigV1 {
    /// Same rule as the prototype: `tracker.json` in the state folder, otherwise
    /// `config/tracker.json` of the dossier. An invalid local file is an error, never a fallback.
    pub fn load(state_dir: Option<&Path>, dossier: Option<&Path>) -> Result<Self, V1Error> {
        let candidates: Vec<PathBuf> = [
            state_dir.map(|dir| dir.join("tracker.json")),
            dossier.map(|dir| dir.join("config").join("tracker.json")),
        ]
        .into_iter()
        .flatten()
        .collect();
        for path in &candidates {
            if path.is_file() {
                return Self::read(path);
            }
        }
        let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
        Err(V1Error::Missing(tried.join(", ")))
    }

    pub fn read(path: &Path) -> Result<Self, V1Error> {
        let text = std::fs::read_to_string(path)
            .map_err(|source| V1Error::Read { path: path.to_path_buf(), source })?;
        serde_json::from_str(&text)
            .map_err(|source| V1Error::Parse { path: path.to_path_buf(), source })
    }
}

/// Binds option ids to canonical statuses and gives labels their meaning.
#[derive(Debug, Clone)]
pub struct TrackerBinding {
    by_option: BTreeMap<String, Status>,
    project_prefix: String,
    objective: String,
    session: String,
    draft: String,
    ready: String,
}

impl TrackerBinding {
    pub fn new(config: &TrackerConfigV1) -> Result<Self, V1Error> {
        let vocabulary = vocabulary();
        let mut by_option = BTreeMap::new();
        for (label, status) in &vocabulary.status_labels {
            let option = config
                .status_options
                .get(label)
                .ok_or(V1Error::MissingStatus(status.id()))?;
            by_option.insert(option.clone(), *status);
        }
        let labels = vocabulary.labels;
        Ok(Self {
            by_option,
            project_prefix: labels.project_prefix,
            objective: labels.objective,
            session: labels.session,
            draft: labels.maturity.draft,
            ready: labels.maturity.ready,
        })
    }

    /// The tracker label that marks a task as needing a working session with the owner.
    pub fn is_session_label(&self, label: &str) -> bool {
        label == self.session
    }

    pub fn resolve(&self, raw: RawIssue) -> Result<Task, V1Error> {
        let status = match &raw.status_option_id {
            None => None,
            Some(option) => Some(*self.by_option.get(option).ok_or_else(|| {
                V1Error::UnknownOption { number: raw.number, option: option.clone() }
            })?),
        };
        let project = raw
            .labels
            .iter()
            .find_map(|label| label.strip_prefix(self.project_prefix.as_str()))
            .map(str::to_owned);
        let maturity = raw.labels.iter().find_map(|label| {
            if *label == self.draft {
                Some(Maturity::Draft)
            } else if *label == self.ready {
                Some(Maturity::Ready)
            } else {
                None
            }
        });
        let objective = raw.labels.contains(&self.objective);
        Ok(Task {
            number: raw.number,
            title: raw.title,
            state: raw.state,
            state_reason: raw.state_reason,
            status,
            project,
            maturity,
            objective,
            labels: raw.labels,
            parent: raw.parent,
            sub_issues: raw.sub_issues,
            blocked_by: raw.blocked_by,
            blocked_by_total: raw.blocked_by_total,
            url: raw.url,
            updated_at: raw.updated_at,
            created_at: raw.created_at,
            closed_at: raw.closed_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::IssueState;

    fn config() -> TrackerConfigV1 {
        let vocabulary = vocabulary();
        let status_options = vocabulary
            .status_labels
            .iter()
            .map(|(label, status)| (label.clone(), format!("opt-{}", status.id())))
            .collect();
        TrackerConfigV1 {
            owner: "o".into(),
            repo: "r".into(),
            project_id: "p".into(),
            status_field_id: "f".into(),
            status_options,
        }
    }

    fn raw(option: Option<&str>) -> RawIssue {
        RawIssue {
            number: 1,
            title: "t".into(),
            state: IssueState::Open,
            state_reason: None,
            url: "u".into(),
            updated_at: "d".into(),
            created_at: None,
            closed_at: None,
            labels: vec![],
            parent: None,
            sub_issues: vec![],
            blocked_by: vec![],
            blocked_by_total: 0,
            status_option_id: option.map(str::to_owned),
        }
    }

    #[test]
    fn status_comes_from_the_option_id() {
        let binding = TrackerBinding::new(&config()).unwrap();
        let task = binding.resolve(raw(Some("opt-in_progress"))).unwrap();
        assert_eq!(task.status, Some(Status::InProgress));
    }

    #[test]
    fn an_unknown_option_is_an_error_not_a_silent_guess() {
        let binding = TrackerBinding::new(&config()).unwrap();
        assert!(matches!(binding.resolve(raw(Some("opt-x"))), Err(V1Error::UnknownOption { .. })));
    }

    #[test]
    fn a_config_without_an_option_is_rejected() {
        let mut config = config();
        config.status_options.pop_first();
        assert!(matches!(TrackerBinding::new(&config), Err(V1Error::MissingStatus(_))));
    }
}
