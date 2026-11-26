//! The composition root shared by the CLI and the app. Only this crate knows which adapter serves which
//! port, so the two front ends cannot drift apart.

use std::path::{Path, PathBuf};

use regia_core::{
    Brief, BriefOptions, Registry, Tracker, TrackerBinding, TrackerConfigV1, TrackerError, V1Error, build_brief, parse_date,
};
use regia_github::GhTracker;
use regia_i18n::{Catalog, Language};
use regia_identity::{config_dir, env_var, prototype_env_var, state_dir};

mod moviola;

pub use moviola::{MoviolaError, moviola_projects, project_links, project_remote, registry_projects, tracker_links};

/// An error of the tracker's configuration as the user reads it, in `catalog`'s language (threat model T08, V17): a
/// `tracker.json` that names another tracker than the app's anchor, or binds the statuses otherwise, with both sides
/// and what to do; an anchor that does not read, with what to do; any other as it is. The errors of this crate say it
/// through here, so the CLI, the app's moviola, `checkpoint` and `step finish` say it alike (#685).
pub fn config_message(error: &V1Error, catalog: &Catalog) -> String {
    let both = |head: String, anchor: &Path, found: String, anchored: String| {
        format!(
            "{head}{}\n  {}\n  {}\n  {}",
            catalog.message_with("init-anchor-file", "anchor", &anchor.display().to_string()),
            catalog.message_with("init-anchor-found", "tracker", &found),
            catalog.message_with("init-anchor-anchored", "tracker", &anchored),
            catalog.message("init-anchor-again"),
        )
    };
    let head = |id: &str, file: &Path| catalog.message_with(id, "file", &file.display().to_string());
    match error {
        V1Error::AnchorMismatch { file, anchor, found, anchored } => both(head("init-anchor-mismatch", file), anchor, found.to_string(), anchored.to_string()),
        V1Error::StatusMismatch { file, anchor, found, anchored } => {
            both(head("init-anchor-status-mismatch", file), anchor, found.to_string(), anchored.to_string())
        }
        V1Error::InvalidAnchor { path, problem } => format!(
            "{}: {problem}\n  {}",
            catalog.message_with("init-anchor-invalid", "anchor", &path.display().to_string()),
            catalog.message("init-anchor-rewrite")
        ),
        other => other.to_string(),
    }
}

/// [`config_message`] in the language the user asked for, as the CLI detects it.
fn detected(error: &V1Error) -> String {
    config_message(error, &Catalog::new(Language::detect()))
}

#[derive(Debug, thiserror::Error)]
pub enum BriefError {
    #[error("{}", detected(.0))]
    Format(#[from] V1Error),
    #[error(transparent)]
    Tracker(#[from] TrackerError),
    #[error("{name} is `{value}`, not a date YYYY-MM-DD")]
    Today { name: String, value: String },
}

/// Why the folder of a project was not read. The reasons of `checkpoint` and `step finish` repeat it.
#[derive(Debug, thiserror::Error)]
pub enum FolderError {
    #[error("{}", detected(.0))]
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
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref(), config_dir().as_deref())?;
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
    let config = TrackerConfigV1::load(state_dir().as_deref(), dossier_dir().as_deref(), config_dir().as_deref())?;
    let binding = TrackerBinding::new(&config)?;
    let raw = GhTracker::new(&config).open_issues()?;
    let tasks = raw
        .into_iter()
        .map(|issue| binding.resolve(issue))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(build_brief(tasks, &binding, &options))
}

#[cfg(test)]
mod tests {
    use regia_core::v1::StatusNames;
    use regia_core::{TrackerNames, anchor_file, local_config_file};

    use super::*;

    fn tracker(owner: &str) -> Box<TrackerNames> {
        Box::new(TrackerNames { owner: owner.into(), repo: "tracker".into(), project_number: "1".into(), project_id: "PROJECT_FIXTURE".into() })
    }

    fn statuses(field: &str) -> Box<StatusNames> {
        Box::new(StatusNames(vec![("status_field_id".into(), field.into())]))
    }

    /// The anchor's three errors, as `load` gives them: another tracker, the statuses bound otherwise, an anchor that
    /// does not read.
    fn error(index: usize) -> V1Error {
        let (file, anchor) = (local_config_file(Path::new("state")), anchor_file(Path::new("config")));
        let errors = [
            V1Error::AnchorMismatch { file: file.clone(), anchor: anchor.clone(), found: tracker("example"), anchored: tracker("example-other") },
            V1Error::StatusMismatch { file, anchor: anchor.clone(), found: statuses("FIELD_B"), anchored: statuses("FIELD_A") },
            V1Error::InvalidAnchor { path: anchor, problem: "missing field `repo`".into() },
        ];
        errors.into_iter().nth(index).unwrap()
    }

    /// #685, #456/V17: the errors of this crate, which the app's moviola, `checkpoint` and `step finish` show as they
    /// are, say the anchor's errors in the catalog's words, in English and in Italian, with both sides and what to do;
    /// the CLI says them through the same function.
    #[test]
    fn the_anchor_s_errors_say_the_catalog_s_sentence_in_each_language() {
        let file = local_config_file(Path::new("state")).display().to_string();
        let anchor = anchor_file(Path::new("config")).display().to_string();
        let english = Catalog::new(Language::En);
        let en = |index| config_message(&error(index), &english);
        let again = "if the tracker really moved, run `init derive` again to anchor the app to it";
        assert_eq!(
            en(0),
            format!("{file} names another tracker than the one this app is anchored to ({anchor})\n  in the file: example/tracker #1 PROJECT_FIXTURE\n  anchored:    example-other/tracker #1 PROJECT_FIXTURE\n  {again}")
        );
        assert_eq!(
            en(1),
            format!("{file} binds the statuses otherwise than the anchor of this app ({anchor})\n  in the file: status_field_id=FIELD_B\n  anchored:    status_field_id=FIELD_A\n  {again}")
        );
        assert_eq!(en(2), format!("the anchor of this app does not read: {anchor}: missing field `repo`\n  run `init derive` again to write it anew"));
        let italian = Catalog::new(Language::It);
        let it = |index| config_message(&error(index), &italian);
        let heads = [("init-anchor-mismatch", "file", &file), ("init-anchor-status-mismatch", "file", &file), ("init-anchor-invalid", "anchor", &anchor)];
        for (index, (id, name, value)) in heads.into_iter().enumerate() {
            let said = it(index);
            assert!(said.starts_with(&italian.message_with(id, name, value)) && !said.contains("init-anchor"), "{said}");
            assert_ne!(said, en(index));
        }
        for (index, (found, anchored)) in [("example/tracker #1 PROJECT_FIXTURE", "example-other/tracker #1 PROJECT_FIXTURE"), ("status_field_id=FIELD_B", "status_field_id=FIELD_A")].into_iter().enumerate() {
            let lines = [italian.message_with("init-anchor-found", "tracker", found), italian.message_with("init-anchor-anchored", "tracker", anchored), italian.message("init-anchor-again")];
            assert!(it(index).ends_with(&format!("\n  {}", lines.join("\n  "))), "{}", it(index));
        }
        assert!(it(2).ends_with(&italian.message("init-anchor-rewrite")));
        let detected = Catalog::new(Language::detect());
        for index in 0..3 {
            let said = config_message(&error(index), &detected);
            assert_eq!([BriefError::from(error(index)).to_string(), FolderError::from(error(index)).to_string(), MoviolaError::from(error(index)).to_string()], [said.clone(), said.clone(), said]);
        }
        assert_eq!(FolderError::from(V1Error::Missing("x".into())).to_string(), "no tracker config: looked in x", "any other error as it is");
    }
}
