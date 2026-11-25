//! Reader of the v1 formats, as the prototype leaves them on disk and in the tracker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    #[error("invalid tracker anchor {path}: {problem}")]
    InvalidAnchor { path: PathBuf, problem: String },
    #[error("{file} names the tracker {found}, but this app is anchored to {anchored} by {anchor}")]
    AnchorMismatch { file: PathBuf, anchor: PathBuf, found: Box<TrackerNames>, anchored: Box<TrackerNames> },
}

/// The file of the tracker's anchor in the configuration folder.
pub const ANCHOR_FILE: &str = "anchor.toml";

/// The anchor of a configuration folder: the one place its path is built.
#[must_use]
pub fn anchor_file(config_dir: &Path) -> PathBuf {
    config_dir.join(ANCHOR_FILE)
}

/// The tracker this machine's app follows, kept in the app's configuration folder, outside the dossier (threat model
/// T08, V17): `init derive` writes it beside the configuration it derives, and [`TrackerConfigV1::load`] stops on a
/// `tracker.json` that names another repo or another Project. Not format v1: the prototype has no anchor. The
/// Project is anchored by its id too, since every read and write of the tracker goes by the id and none by the number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackerAnchor {
    pub owner: String,
    pub repo: String,
    pub project_number: u64,
    pub project_id: String,
}

/// What names a tracker, as an anchor holds it or a `tracker.json` says it: `owner/repo #number id`. A value that is
/// missing reads `?`, one that is not a string its JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerNames {
    pub owner: String,
    pub repo: String,
    pub project_number: String,
    pub project_id: String,
}

impl std::fmt::Display for TrackerNames {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{} #{} {}", self.owner, self.repo, self.project_number, self.project_id)
    }
}

impl TrackerNames {
    fn of(config: &serde_json::Value) -> Self {
        let text = |key: &str| match config.get(key) {
            None | Some(serde_json::Value::Null) => "?".to_owned(),
            Some(serde_json::Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
        };
        Self { owner: text("owner"), repo: text("repo"), project_number: text("project_number"), project_id: text("project_id") }
    }
}

impl TrackerAnchor {
    /// The file as it is written: TOML, as the other files of the configuration folder.
    #[must_use]
    pub fn file_text(&self) -> String {
        toml::to_string(self).expect("four plain values serialize")
    }

    /// The anchor at `path`: `None` when there is no file. A file that is there and does not read, or does not hold
    /// exactly the four values, is an error, never no anchor.
    pub fn read(path: &Path) -> Result<Option<Self>, V1Error> {
        let invalid = |problem: String| V1Error::InvalidAnchor { path: path.to_path_buf(), problem };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(invalid(error.to_string())),
        };
        toml::from_str(&text).map(Some).map_err(|error| invalid(error.message().to_owned()))
    }

    /// Whether `config`, a `tracker.json` as JSON, names this tracker: the same owner and repo, which GitHub compares
    /// without case, the same number as Python compares numbers (`7 == 7.0`), and the same id.
    fn names(&self, config: &serde_json::Value) -> bool {
        let text = |key: &str| config.get(key).and_then(serde_json::Value::as_str);
        text("owner").is_some_and(|owner| owner.eq_ignore_ascii_case(&self.owner))
            && text("repo").is_some_and(|repo| repo.eq_ignore_ascii_case(&self.repo))
            && config.get("project_number").is_some_and(|number| crate::init::python_equal(number, &serde_json::Value::from(self.project_number)))
            && text("project_id") == Some(self.project_id.as_str())
    }

    fn tracker_names(&self) -> TrackerNames {
        TrackerNames {
            owner: self.owner.clone(),
            repo: self.repo.clone(),
            project_number: self.project_number.to_string(),
            project_id: self.project_id.clone(),
        }
    }
}

/// Stops on the `tracker.json` at `file`, read as `config`, when the anchor at `anchor` names another tracker. No
/// configuration folder, or no anchor in it, checks nothing.
fn check_anchor(anchor: Option<&Path>, file: &Path, config: &serde_json::Value) -> Result<(), V1Error> {
    let Some(path) = anchor else { return Ok(()) };
    match TrackerAnchor::read(path)? {
        Some(anchored) if !anchored.names(config) => Err(V1Error::AnchorMismatch {
            file: file.to_path_buf(),
            anchor: path.to_path_buf(),
            found: Box::new(TrackerNames::of(config)),
            anchored: Box::new(anchored.tracker_names()),
        }),
        _ => Ok(()),
    }
}

/// The keys a local `tracker.json` must have, as the prototype checks them (`CHIAVI_CONFIG`).
const LOCAL_KEYS: [&str; 7] = ["owner", "repo", "project_number", "project_id", "project_url", "status_field_id", "status_options"];

#[derive(Debug, Deserialize)]
struct Vocabulary {
    labels: Labels,
    fields: Fields,
    body: Body,
    status_labels: BTreeMap<String, Status>,
    base_labels: Vec<BaseLabel>,
}

/// A label `setup` and `init` create on the tracker, with its color and description.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BaseLabel {
    pub name: String,
    pub color: String,
    pub description: String,
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

/// The labels `setup` and `init` create, in their order (T6 of the dossier's conformance study).
#[must_use]
pub fn base_labels() -> Vec<BaseLabel> {
    vocabulary().base_labels
}

/// The names of the options of the Project's status field, in the order of the statuses.
#[must_use]
pub fn status_option_names() -> Vec<String> {
    let by_name = vocabulary().status_labels;
    Status::ALL
        .into_iter()
        .filter_map(|status| by_name.iter().find(|(_, bound)| **bound == status).map(|(name, _)| name.clone()))
        .collect()
}

/// The name of the Project's field that mirrors the project label.
#[must_use]
pub fn project_field_name() -> String {
    vocabulary().fields.project
}

/// The prefix of a project's label, which the project's id follows.
#[must_use]
pub fn project_label_prefix() -> String {
    vocabulary().labels.project_prefix
}

/// The local `tracker.json` of a state folder, which wins over the dossier's: the one place its path is built.
#[must_use]
pub fn local_config_file(state_dir: &Path) -> PathBuf {
    state_dir.join("tracker.json")
}

/// The project an issue's labels name, as a comment reads it (`_contesto_commento`): what follows the
/// project's prefix in the first label that has it; `None` without one, or with nothing after the prefix.
#[must_use]
pub fn project_of_labels(labels: &[String]) -> Option<String> {
    let prefix = vocabulary().labels.project_prefix;
    labels.iter().find_map(|label| label.strip_prefix(prefix.as_str())).filter(|project| !project.is_empty()).map(str::to_owned)
}

/// `tracker.json`, in the v1 format. Only the fields this slice reads.
#[derive(Debug, Clone, Deserialize)]
pub struct TrackerConfigV1 {
    pub owner: String,
    pub repo: String,
    pub project_id: String,
    pub status_field_id: String,
    pub status_options: BTreeMap<String, String>,
    /// The Project's page, for the moviola's link (§8 of its contract): `None` when it is missing or not a
    /// string, which the prototype passes on as it is and no other reader needs.
    #[serde(default, deserialize_with = "string_or_none")]
    pub project_url: Option<String>,
}

fn string_or_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(serde_json::Value::deserialize(deserializer)?.as_str().map(str::to_owned))
}

impl TrackerConfigV1 {
    /// Same rule as the prototype (`config_tracker`): `tracker.json` in the state folder, otherwise
    /// `config/tracker.json` of the dossier. A local file is checked as the prototype checks it, every key
    /// there and an option for every status, and an invalid one is an error, never a fallback on the
    /// dossier's, which is the owner's tracker.
    ///
    /// The file read is then checked against the anchor of `config_dir`, the app's configuration folder, which the
    /// prototype does not have (T08, V17): one that names another repo or Project is an error, whichever of the two
    /// it is. With no folder, or no anchor in it, nothing changes.
    pub fn load(state_dir: Option<&Path>, dossier: Option<&Path>, config_dir: Option<&Path>) -> Result<Self, V1Error> {
        let anchor = config_dir.map(anchor_file);
        let local = state_dir.map(local_config_file);
        if let Some(path) = local.as_ref().filter(|path| path.is_file()) {
            let (config, value) = Self::read_local(path)?;
            check_anchor(anchor.as_deref(), path, &value)?;
            return Ok(config);
        }
        let shared = dossier.map(|dir| dir.join("config").join("tracker.json"));
        if let Some(path) = shared.as_ref().filter(|path| path.is_file()) {
            let (config, value) = Self::read_with_value(path)?;
            check_anchor(anchor.as_deref(), path, &value)?;
            return Ok(config);
        }
        let tried: Vec<String> = [local, shared].into_iter().flatten().map(|p| p.display().to_string()).collect();
        Err(V1Error::Missing(tried.join(", ")))
    }

    /// The configuration as the prototype's moviola takes it for its links (`config_tracker` of its `fonti.py`): a
    /// local file that does not read is an error, as in [`load`](Self::load), since no configuration would drop
    /// the URLs in silence and the dossier's would name another tracker; with no local file, a configuration of
    /// the dossier that is missing or does not read is none. The anchor is checked as `load` checks it, and a file
    /// that names another tracker, or an anchor that does not read, is an error whichever file was read: no links
    /// would hide it.
    pub fn load_for_links(state_dir: Option<&Path>, dossier: Option<&Path>, config_dir: Option<&Path>) -> Result<Option<Self>, V1Error> {
        match Self::load(state_dir, dossier, config_dir) {
            Ok(config) => Ok(Some(config)),
            Err(error @ (V1Error::AnchorMismatch { .. } | V1Error::InvalidAnchor { .. })) => Err(error),
            Err(error) if state_dir.is_some_and(|dir| local_config_file(dir).is_file()) => Err(error),
            Err(_) => Ok(None),
        }
    }

    /// The local file, checked as the prototype checks it, and the JSON it holds.
    fn read_local(path: &Path) -> Result<(Self, serde_json::Value), V1Error> {
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
        let config = serde_json::from_value(value.clone()).map_err(|source| V1Error::Parse { path: path.to_path_buf(), source })?;
        Ok((config, value))
    }

    /// The file at `path`, with no anchor and no check of the keys: for the tests and the recorder. The commands read
    /// through [`load`](Self::load).
    pub fn read(path: &Path) -> Result<Self, V1Error> {
        Self::read_with_value(path).map(|(config, _)| config)
    }

    /// The file and the JSON it holds; the errors of the configuration are those of the file read as it.
    fn read_with_value(path: &Path) -> Result<(Self, serde_json::Value), V1Error> {
        let parse = |source| V1Error::Parse { path: path.to_path_buf(), source };
        let text = std::fs::read_to_string(path).map_err(|source| V1Error::Read { path: path.to_path_buf(), source })?;
        let config = serde_json::from_str(&text).map_err(parse)?;
        Ok((config, serde_json::from_str(&text).map_err(parse)?))
    }
}

/// Binds option ids to canonical statuses and gives labels their meaning.
#[derive(Debug, Clone)]
pub struct TrackerBinding {
    by_option: BTreeMap<String, Status>,
    /// The options' names, as a change of Status writes them.
    by_name: BTreeMap<String, Status>,
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
            by_name: vocabulary.status_labels,
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

    /// The label of the issues of `project`.
    pub fn project_label(&self, project: &str) -> String {
        format!("{}{project}", self.project_prefix)
    }

    /// The label of an Objective.
    pub fn objective_label(&self) -> &str {
        &self.objective
    }

    /// The label of `maturity`.
    pub fn maturity_label(&self, maturity: Maturity) -> &str {
        match maturity {
            Maturity::Draft => &self.draft,
            Maturity::Ready => &self.ready,
        }
    }

    /// The status an option stands for, by its name, as a change of Status names it; `None` for a name
    /// the format does not know.
    pub fn status_of_name(&self, name: &str) -> Option<Status> {
        self.by_name.get(name).copied()
    }

    /// The name of the option of `status`.
    pub fn status_name(&self, status: Status) -> Option<&str> {
        self.by_name.iter().find(|(_, bound)| **bound == status).map(|(name, _)| name.as_str())
    }

    /// The id of the option the config binds to `status`.
    pub fn option_id(&self, status: Status) -> Option<&str> {
        self.by_option.iter().find(|(_, bound)| **bound == status).map(|(option, _)| option.as_str())
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
            project_url: None,
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

    /// The first label with the prefix names the project; one with nothing after the prefix names none.
    #[test]
    fn a_comment_reads_its_issue_s_project_from_the_labels() {
        let prefix = vocabulary().labels.project_prefix;
        let labels = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        let (alpha, beta) = (format!("{prefix}alpha"), format!("{prefix}beta"));
        assert_eq!(project_of_labels(&labels(&["bug", alpha.as_str(), beta.as_str()])).as_deref(), Some("alpha"));
        assert_eq!(project_of_labels(&labels(&["bug"])), None);
        assert_eq!(project_of_labels(&labels(&[prefix.as_str()])), None);
        assert_eq!(project_of_labels(&[]), None);
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

    /// The labels `setup` and `init` create are those the rest of the format reads, and one more, in the
    /// prototype's order: the objective, the two maturities, the parked one, the session.
    #[test]
    fn the_base_labels_are_the_labels_of_the_format() {
        let labels = vocabulary().labels;
        let names: Vec<String> = base_labels().into_iter().map(|label| label.name).collect();
        assert_eq!(names.len(), 5);
        assert_eq!([&names[0], &names[1], &names[2], &names[4]], [&labels.objective, &labels.maturity.draft, &labels.maturity.ready, &labels.session]);
        assert!(base_labels().iter().all(|label| label.color.len() == 6 && label.color.chars().all(|c| c.is_ascii_hexdigit())));
    }

    /// A folder of its own under `target/`, beside the test's executable.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::current_exe().unwrap().parent().unwrap().join(format!("v1-anchor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// A valid local `tracker.json` in `state`, naming `Acme/tasks`, Project #7 `PVT_7`.
    fn write_local(state: &Path) {
        let options: serde_json::Map<String, serde_json::Value> =
            vocabulary().status_labels.into_keys().map(|label| (label.clone(), serde_json::Value::String(format!("opt-{label}")))).collect();
        let file = serde_json::json!({"owner": "Acme", "repo": "tasks", "project_number": 7, "project_id": "PVT_7", "project_url": "u",
                                      "status_field_id": "f", "status_options": options});
        std::fs::create_dir_all(state).unwrap();
        std::fs::write(local_config_file(state), file.to_string()).unwrap();
    }

    fn anchored_to(owner: &str, repo: &str, project_number: u64, project_id: &str) -> TrackerAnchor {
        TrackerAnchor { owner: owner.into(), repo: repo.into(), project_number, project_id: project_id.into() }
    }

    fn write_anchor(config: &Path, anchor: &TrackerAnchor) {
        std::fs::create_dir_all(config).unwrap();
        std::fs::write(anchor_file(config), anchor.file_text()).unwrap();
    }

    /// T08, V17: with an anchor, the same tracker loads, owner and repo in any case; another owner, repo, number or id
    /// is an error that names both files and shows both trackers. Without an anchor, or without a configuration
    /// folder, the file loads as before.
    #[test]
    fn the_anchor_stops_a_tracker_json_that_names_another_tracker() {
        let root = scratch("cases");
        let (state, config) = (root.join("state"), root.join("config"));
        write_local(&state);
        let load = || TrackerConfigV1::load(Some(&state), None, Some(&config));
        assert_eq!(load().unwrap().project_id, "PVT_7", "no anchor: as before");
        write_anchor(&config, &anchored_to("acme", "Tasks", 7, "PVT_7"));
        assert_eq!(load().unwrap().project_id, "PVT_7", "the same tracker");
        assert_eq!(TrackerConfigV1::load(Some(&state), None, None).unwrap().project_id, "PVT_7", "no folder: as before");
        let others = [
            ("owner", anchored_to("other", "tasks", 7, "PVT_7")),
            ("repo", anchored_to("acme", "other", 7, "PVT_7")),
            ("number", anchored_to("acme", "tasks", 8, "PVT_7")),
            ("id", anchored_to("acme", "tasks", 7, "PVT_8")),
        ];
        for (what, other) in others {
            write_anchor(&config, &other);
            match load() {
                Err(V1Error::AnchorMismatch { file, anchor, found, anchored }) => {
                    assert_eq!((file, anchor), (local_config_file(&state), anchor_file(&config)), "{what}");
                    assert_eq!((found.to_string(), *anchored), ("Acme/tasks #7 PVT_7".to_owned(), other.tracker_names()), "{what}");
                }
                result => panic!("another {what} loads: {result:?}"),
            }
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The dossier's file is checked too, its number compared as Python compares it, and a number it does not have is
    /// another tracker; for the links too, where a file that does not read is no links but another tracker is an
    /// error. An anchor that does not read, or holds a key more or less, is an error, never no anchor.
    #[test]
    fn the_dossier_s_file_is_anchored_too_and_a_broken_anchor_is_an_error() {
        let root = scratch("dossier");
        let (dossier, config) = (root.join("dossier"), root.join("config"));
        let shared = dossier.join("config").join("tracker.json");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        let mut file = serde_json::json!({"owner": "acme", "repo": "tasks", "project_number": 7.0, "project_id": "PVT_7", "status_field_id": "f", "status_options": {}});
        std::fs::write(&shared, file.to_string()).unwrap();
        let load = || TrackerConfigV1::load(None, Some(&dossier), Some(&config));
        let links = || TrackerConfigV1::load_for_links(None, Some(&dossier), Some(&config));
        write_anchor(&config, &anchored_to("acme", "tasks", 7, "PVT_7"));
        assert!(load().is_ok() && links().unwrap().is_some());
        file.as_object_mut().unwrap().remove("project_number");
        std::fs::write(&shared, file.to_string()).unwrap();
        assert!(matches!(load(), Err(V1Error::AnchorMismatch { ref found, .. }) if found.project_number == "?"));
        assert!(matches!(links(), Err(V1Error::AnchorMismatch { .. })), "another tracker is not just no links");
        let anchor = anchored_to("acme", "tasks", 7, "PVT_7");
        assert_eq!(toml::from_str::<TrackerAnchor>(&anchor.file_text()).unwrap(), anchor);
        let more = format!("{}more = 1\n", anchor.file_text());
        for text in ["owner = ", "owner = \"acme\"\n", more.as_str(), "owner = \"acme\"\nrepo = \"tasks\"\nproject_number = -7\nproject_id = \"PVT_7\"\n"] {
            std::fs::write(anchor_file(&config), text).unwrap();
            assert!(matches!(load(), Err(V1Error::InvalidAnchor { .. })), "{text}");
            assert!(matches!(links(), Err(V1Error::InvalidAnchor { .. })), "{text}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The options of the status field, in the order of the statuses, each bound to its status.
    #[test]
    fn the_status_options_follow_the_order_of_the_statuses() {
        let names = status_option_names();
        let binding = TrackerBinding::new(&config()).unwrap();
        let statuses: Vec<Status> = names.iter().map(|name| binding.status_of_name(name).unwrap()).collect();
        assert_eq!(statuses, Status::ALL);
    }
}
