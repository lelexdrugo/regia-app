//! The moviola, read: the files a project's dossier keeps in `.regia/moviola/` (the dossier's
//! `tools/moviola/dati.py`) and the three answers the app asks for in place of the prototype's local viewer
//! (`serve.py`, which the product does not port: ADR 0001 §9). They are the portfolio of the projects (L0), the
//! timeline of one project (L1 to L3, without the sidecars) and one sidecar on demand. It reads the disk and
//! writes nothing; the links it builds come from what the caller passes, never from a process or the network.
//!
//! The records' keys and the words of their closed lists are the prototype's, in `formats/moviola_v1.toml`; the
//! answers carry the product's names, provisional until the gate of the names (#223 of the dossier). A record
//! reads as the prototype's viewer shows it, with these differences, which make it fit for a typed view:
//! - keys the contract does not name are not carried;
//! - a value outside a closed list, or of another type than its field's, makes the file unreadable, as invalid
//!   JSON already does in the prototype;
//! - titles and texts are normalized (threat model T19);
//! - a link in the way to a file is refused, not followed (`files.rs`).

mod consolidate;
mod files;
mod format;
mod links;
pub mod pyjson;
mod records;
mod report;
mod store;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

pub use consolidate::{Consolidated, aggregates_file, consolidate};
pub use links::{CommitLink, IssueLink, LinkSettings, Links, ReportKind, ReportLink, links, normalize_remote};
pub use records::{
    Aggregate, AggregateDefects, Author, Check, Deviation, Direction, DirectionStatus, Health, Milestone, MilestoneKind, ModelUse,
    Process, ProcessDefects, Refs, Snapshot, Tokens,
};
pub use report::{Report, front_matter, read_reports};
pub use store::{ActivityStore, Pruned, Upserted, fingerprint, folder_rule, folder_variable, key, line, needs_consolidation, task_key};

use crate::comment::parse_instant;
use crate::pystr;
use crate::text::normalize_title;
use files::{Fields, moviola_dir, read_lines, read_sidecar};
use format::format;

/// Why the moviola of a project could not be read. Files are named relative to the project's folder.
#[derive(Debug, thiserror::Error)]
pub enum MoviolaError {
    #[error("{file}: {source}")]
    Read { file: String, source: std::io::Error },
    #[error("{file}: not written: {source}")]
    Write { file: String, source: std::io::Error },
    #[error("{place}: {problem}")]
    Invalid { place: String, problem: String },
    #[error("{0}: a link, which the moviola does not follow")]
    Link(String),
    #[error("{0}: not a file")]
    NotAFile(String),
    #[error("milestone {id}: `{ts}` is not an instant")]
    NotAnInstant { id: String, ts: String },
}

/// A project as the caller knows it: its id, the folder of its dossier, and its title when the registry gives one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub folder: PathBuf,
    pub title: Option<String>,
}

/// The milestones of the project's `folder`, by instant and then id, as written: the order does not depend on
/// the file's (`dati.milestones`).
pub fn read_milestones(folder: &Path) -> Result<Vec<Milestone>, MoviolaError> {
    let dir = moviola_dir(folder)?;
    let mut out = read_lines(dir.as_deref(), &format().milestones)?
        .iter()
        .map(|(place, record)| records::milestone(&Fields::new(place, record)))
        .collect::<Result<Vec<_>, _>>()?;
    out.sort_by(|a, b| (&a.ts, &a.id).cmp(&(&b.ts, &b.id)));
    Ok(out)
}

/// The directions, in the file's order (`dati.direzioni`).
pub fn read_directions(folder: &Path) -> Result<Vec<Direction>, MoviolaError> {
    let dir = moviola_dir(folder)?;
    read_lines(dir.as_deref(), &format().directions)?.iter().map(|(place, record)| records::direction(&Fields::new(place, record))).collect()
}

/// The tasks' aggregates, in the file's order (`dati.aggregati`).
pub fn read_aggregates(folder: &Path) -> Result<Vec<Aggregate>, MoviolaError> {
    let dir = moviola_dir(folder)?;
    read_lines(dir.as_deref(), &format().aggregates)?.iter().map(|(place, record)| records::aggregate(&Fields::new(place, record))).collect()
}

/// L0, one project: how many milestones and directions it has, the health and the main fields of its last
/// milestone, and the instants of its first and last (`_portafoglio` of `serve.py`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ProjectSummary {
    pub id: String,
    pub title: Option<String>,
    pub milestones: u64,
    pub directions: u64,
    pub health: Option<Health>,
    pub last: Option<LastMilestone>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LastMilestone {
    pub id: String,
    pub ts: String,
    pub kind: MilestoneKind,
    pub title: String,
    pub direction: String,
}

/// The portfolio of `projects`, by id.
pub fn portfolio(projects: &[Project]) -> Result<Vec<ProjectSummary>, MoviolaError> {
    let mut sorted: Vec<&Project> = projects.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    sorted
        .into_iter()
        .map(|project| {
            let milestones = read_milestones(&project.folder)?;
            let directions = read_directions(&project.folder)?;
            let last = milestones.last();
            Ok(ProjectSummary {
                id: project.id.clone(),
                title: project.title.as_deref().filter(|title| !title.is_empty()).map(|title| normalize_title(title).text),
                milestones: milestones.len() as u64,
                directions: directions.len() as u64,
                health: last.map(|last| last.health),
                last: last.map(|last| LastMilestone {
                    id: last.id.clone(),
                    ts: last.ts.clone(),
                    kind: last.kind,
                    title: last.title.clone(),
                    direction: last.direction.clone(),
                }),
                from: milestones.first().map(|first| first.ts.clone()),
                to: last.map(|last| last.ts.clone()),
            })
        })
        .collect()
}

/// A milestone of a timeline, with its links resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkedMilestone {
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub milestone: Milestone,
    pub links: Links,
}

/// L1 to L3 of a project (`_vista` of `serve.py`) without the sidecars, which [`snapshot`] reads one at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Timeline {
    pub project: String,
    pub project_url: Option<String>,
    /// The id of the implicit direction, which has no record: its lane comes first.
    pub main_direction: String,
    pub milestones: Vec<LinkedMilestone>,
    pub directions: Vec<Direction>,
    pub aggregates: Vec<Aggregate>,
}

/// An instant as the prototype's `parse_ts` reads it, in UTC: ISO 8601 with `Z` or an offset, SQLite's
/// `YYYY-MM-DD HH:MM:SS`, a date alone; without an offset it is UTC. `None` when it does not read.
pub fn parse_ts(text: &str) -> Option<DateTime<Utc>> {
    let text = pystr::strip(text);
    if text.is_empty() {
        return None;
    }
    parse_instant(text).ok().map(|at| at.with_timezone(&Utc))
}

/// A window of time: from `from`, included, to `to`, excluded; either end may be open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Range {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("`{0}` is neither a date (YYYY-MM-DD) nor an ISO 8601 instant")]
pub struct RangeError(pub String);

impl Range {
    /// The window of `moviola export --from --to` (`_intervallo` of the prototype's `esporta.py`): a date or an
    /// instant each; a date alone at the end means the whole day, so `--to 2026-10-19` takes in the 19th.
    pub fn parse(from: Option<&str>, to: Option<&str>) -> Result<Range, RangeError> {
        let read = |text: &str| parse_ts(text).ok_or_else(|| RangeError(text.to_owned()));
        let from = from.map(read).transpose()?;
        let to = match to {
            None => None,
            Some(text) => {
                let at = read(text)?;
                let whole_day = {
                    let date = pystr::strip(text).as_bytes();
                    date.len() == 10
                        && date.iter().enumerate().all(|(i, byte)| if i == 4 || i == 7 { *byte == b'-' } else { byte.is_ascii_digit() })
                };
                Some(if whole_day { at + Duration::days(1) } else { at })
            }
        };
        Ok(Range { from, to })
    }

    pub fn is_all(&self) -> bool {
        self.from.is_none() && self.to.is_none()
    }

    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        self.from.is_none_or(|from| at >= from) && self.to.is_none_or(|to| at < to)
    }
}

/// The timeline of `project`, with the milestones inside `range` only: directions and aggregates are always
/// whole. With the whole range, a milestone's instant is not read, as in the prototype's viewer.
pub fn timeline(project: &Project, settings: &LinkSettings, range: &Range) -> Result<Timeline, MoviolaError> {
    let folder = &project.folder;
    let mut milestones = Vec::new();
    for milestone in read_milestones(folder)? {
        if !range.is_all() {
            let at = parse_ts(&milestone.ts).ok_or_else(|| MoviolaError::NotAnInstant { id: milestone.id.clone(), ts: milestone.ts.clone() })?;
            if !range.contains(at) {
                continue;
            }
        }
        let links = links(&milestone.refs, folder, settings);
        milestones.push(LinkedMilestone { milestone, links });
    }
    Ok(Timeline {
        project: project.id.clone(),
        project_url: settings.project_url(),
        main_direction: format().main_direction.clone(),
        milestones,
        directions: read_directions(folder)?,
        aggregates: read_aggregates(folder)?,
    })
}

/// The sidecar of the milestone `id` of the project's `folder`: `None` when no milestone has that id, or it has
/// no sidecar. The id is compared with the milestones' ids, and the sidecar found by listing its folder: it never
/// becomes part of a path (threat model T05).
pub fn snapshot(folder: &Path, id: &str) -> Result<Option<Snapshot>, MoviolaError> {
    if !read_milestones(folder)?.iter().any(|milestone| milestone.id == id) {
        return Ok(None);
    }
    let dir = moviola_dir(folder)?;
    match read_sidecar(dir.as_deref(), id)? {
        Some((place, record)) => records::snapshot(&Fields::new(&place, &record)).map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moviola::format::{closed, word_of};

    #[test]
    fn every_value_of_a_closed_list_has_one_word_and_back() {
        let format = format();
        fn both_ways<T>(list: &std::collections::BTreeMap<String, String>, values: &[T])
        where
            T: serde::de::DeserializeOwned + Serialize + PartialEq + std::fmt::Debug,
        {
            assert_eq!(list.len(), values.len(), "one word for each value");
            for value in values {
                let word = word_of(list, value).unwrap_or_else(|| panic!("{value:?} has a word"));
                assert_eq!(closed::<T>(list, &word).as_ref(), Some(value));
            }
            assert_eq!(closed::<T>(list, "none of them"), None);
        }
        use MilestoneKind as K;
        both_ways(&format.kinds, &[K::Milestone, K::Decision, K::Gate, K::Exception, K::Exploration, K::Rejoin, K::Abandonment]);
        both_ways(&format.health, &[Health::Healthy, Health::AtRisk, Health::WaitingOnOwner]);
        both_ways(&format.direction_status, &[DirectionStatus::Active, DirectionStatus::Abandoned, DirectionStatus::Rejoined]);
    }

    #[test]
    fn a_remote_becomes_its_web_page_without_credentials() {
        let cases = [
            ("https://user:secret@github.com/acme/p.git", Some("https://github.com/acme/p")),
            ("git@github.com:acme/p.git", Some("https://github.com/acme/p")),
            ("ssh://git@host.example:2222/acme/p.git/", Some("https://host.example/acme/p")),
            ("http://host/a/b/", Some("http://host/a/b")),
            ("https://github.com/acme/p.git\n", Some("https://github.com/acme/p")),
            ("file:///srv/p.git", None),
            ("/srv/p.git", None),
            ("", None),
        ];
        for (url, web) in cases {
            assert_eq!(normalize_remote(url).as_deref(), web, "{url:?}");
        }
    }

    #[test]
    fn a_range_reads_dates_and_instants_and_takes_the_whole_last_day() {
        let at = |text: &str| parse_ts(text).unwrap();
        let range = Range::parse(Some("2026-10-15"), Some("2026-10-19")).unwrap();
        assert_eq!(range, Range { from: Some(at("2026-10-15T00:00:00Z")), to: Some(at("2026-10-20T00:00:00Z")) });
        assert!(range.contains(at("2026-10-19T23:59:59Z")) && !range.contains(at("2026-10-20T00:00:00Z")));
        let range = Range::parse(None, Some("2026-10-19T12:00:00+02:00")).unwrap();
        assert_eq!(range.to, Some(at("2026-10-19T10:00:00Z")), "an instant is not widened");
        assert_eq!(parse_ts("2026-10-14 17:43:17"), Some(at("2026-10-14T17:43:17Z")), "SQLite's form, in UTC");
        assert_eq!(parse_ts("2026-10-14T17:43:17"), Some(at("2026-10-14T17:43:17Z")), "no offset is UTC");
        assert!(Range::parse(Some("yesterday"), None).is_err());
        assert!(Range::parse(None, Some("")).is_err());
        assert!(Range::default().is_all() && Range::default().contains(at("1999-01-01")));
    }
}
