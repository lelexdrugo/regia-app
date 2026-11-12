//! Reader of the v1 formats, as the prototype leaves them on disk and in the tracker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::due::DueRule;
use crate::model::{Maturity, Status, Task};
use crate::tracker::RawIssue;

pub(crate) const VOCABULARY: &str = include_str!("../formats/tracker_v1.toml");

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
    #[error("invalid local tracker config {path}: {problem}")]
    Local { path: PathBuf, problem: String },
}

/// The keys a local `tracker.json` must have, as the prototype checks them (`CHIAVI_CONFIG`).
const LOCAL_KEYS: [&str; 7] = ["owner", "repo", "project_number", "project_id", "project_url", "status_field_id", "status_options"];

#[derive(Debug, Deserialize)]
struct Vocabulary {
    labels: Labels,
    fields: Fields,
    body: Body,
    status_labels: BTreeMap<String, Status>,
}

#[derive(Debug, Deserialize)]
struct Fields {
    project: String,
}

#[derive(Debug, Deserialize)]
struct Body {
    due_key: String,
    due_placeholder: String,
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
    /// Same rule as the prototype (`config_tracker`): `tracker.json` in the state folder, otherwise
    /// `config/tracker.json` of the dossier. A local file is checked as the prototype checks it, every key
    /// there and an option for every status, and an invalid one is an error, never a fallback on the
    /// dossier's, which is the owner's tracker.
    pub fn load(state_dir: Option<&Path>, dossier: Option<&Path>) -> Result<Self, V1Error> {
        let local = state_dir.map(|dir| dir.join("tracker.json"));
        if let Some(path) = local.as_ref().filter(|path| path.is_file()) {
            return Self::read_local(path);
        }
        let shared = dossier.map(|dir| dir.join("config").join("tracker.json"));
        if let Some(path) = shared.as_ref().filter(|path| path.is_file()) {
            return Self::read(path);
        }
        let tried: Vec<String> = [local, shared].into_iter().flatten().map(|p| p.display().to_string()).collect();
        Err(V1Error::Missing(tried.join(", ")))
    }

    fn read_local(path: &Path) -> Result<Self, V1Error> {
        let local = |problem: String| V1Error::Local { path: path.to_path_buf(), problem };
        let text = std::fs::read_to_string(path).map_err(|source| V1Error::Read { path: path.to_path_buf(), source })?;
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|source| V1Error::Parse { path: path.to_path_buf(), source })?;
        let object = value.as_object().ok_or_else(|| local("not a JSON object".to_owned()))?;
        let missing: Vec<&str> = LOCAL_KEYS.into_iter().filter(|key| !object.contains_key(*key)).collect();
        if !missing.is_empty() {
            return Err(local(format!("missing the keys {}", missing.join(", "))));
        }
        let options = object["status_options"].as_object().ok_or_else(|| local("status_options is not an object".to_owned()))?;
        let without: Vec<String> = vocabulary().status_labels.into_keys().filter(|label| !options.contains_key(label)).collect();
        if !without.is_empty() {
            return Err(local(format!("status_options without {}", without.join(", "))));
        }
        serde_json::from_value(value).map_err(|source| V1Error::Parse { path: path.to_path_buf(), source })
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
    project_field: String,
    due: DueRule,
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
            project_field: vocabulary.fields.project,
            due: DueRule::new(&vocabulary.body.due_key, &vocabulary.body.due_placeholder),
        })
    }

    /// The tracker label that marks a task as needing a working session with the owner.
    pub fn is_session_label(&self, label: &str) -> bool {
        label == self.session
    }

    /// The name of the Project field that mirrors the project label.
    pub fn project_field_name(&self) -> &str {
        &self.project_field
    }

    /// The status of an option id of issue `number`; an option the config does not bind is an error.
    pub fn status(&self, number: u64, option: Option<&str>) -> Result<Option<Status>, V1Error> {
        option
            .map(|option| {
                self.by_option.get(option).copied().ok_or_else(|| V1Error::UnknownOption { number, option: option.to_owned() })
            })
            .transpose()
    }

    pub fn resolve(&self, raw: RawIssue) -> Result<Task, V1Error> {
        let status = self.status(raw.number, raw.status_option_id.as_deref())?;
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
        // The last value wins, as for the status.
        let project_field = raw
            .field_values
            .iter()
            .rev()
            .find(|(field, _)| *field == self.project_field)
            .map(|(_, option)| option.clone());
        Ok(Task {
            number: raw.number,
            title: raw.title,
            state: raw.state,
            state_reason: raw.state_reason,
            status,
            project,
            project_field,
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
            due: self.due.read(&raw.body),
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
            body: String::new(),
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
            field_values: vec![],
        }
    }

    #[test]
    fn the_project_field_is_read_by_its_name_and_the_last_value_wins() {
        let binding = TrackerBinding::new(&config()).unwrap();
        let name = binding.project_field_name().to_owned();
        let mut issue = raw(None);
        issue.field_values = vec![(name.clone(), "alpha".into()), ("Priority".into(), "High".into()), (name, "beta".into())];
        assert_eq!(binding.resolve(issue).unwrap().project_field.as_deref(), Some("beta"));
        let mut other = raw(None);
        other.field_values = vec![("Priority".into(), "High".into())];
        assert_eq!(binding.resolve(other).unwrap().project_field, None);
    }

    #[test]
    fn the_due_date_comes_from_the_spec_in_the_body() {
        let binding = TrackerBinding::new(&config()).unwrap();
        let vocabulary = vocabulary();
        let mut issue = raw(None);
        issue.body = format!("```yaml\n{}: 2026-11-05\n```\n", vocabulary.body.due_key);
        let expected = chrono::NaiveDate::from_ymd_opt(2026, 11, 5).map(crate::model::DueDate::Date);
        assert_eq!(binding.resolve(issue).unwrap().due, expected);
        let mut placeholder = raw(None);
        placeholder.body = format!("```yaml\n{}: {}\n```\n", vocabulary.body.due_key, vocabulary.body.due_placeholder);
        assert_eq!(binding.resolve(placeholder).unwrap().due, None);
    }

    #[test]
    fn status_comes_from_the_option_id() {
        let binding = TrackerBinding::new(&config()).unwrap();
        let task = binding.resolve(raw(Some("opt-in_progress"))).unwrap();
        assert_eq!(task.status, Some(Status::InProgress));
    }

    /// T1 of the dossier's conformance study: the first label with the project's prefix wins, in the order
    /// the tracker gives the labels; the same holds for maturity.
    #[test]
    fn the_first_project_label_wins() {
        let binding = TrackerBinding::new(&config()).unwrap();
        let labels = vocabulary().labels;
        let mut issue = raw(None);
        issue.labels = vec![
            "other".to_owned(),
            format!("{}beta", labels.project_prefix),
            labels.maturity.ready.clone(),
            format!("{}alpha", labels.project_prefix),
            labels.maturity.draft.clone(),
        ];
        let task = binding.resolve(issue).unwrap();
        assert_eq!((task.project.as_deref(), task.maturity), (Some("beta"), Some(Maturity::Ready)));
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
