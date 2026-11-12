//! The moviola's records (§5.2 to §5.5 of the dossier's `docs/moviola/CONTRATTO.md`) as a view reads them: every
//! key always there, `null` for none, the closed lists as the product's ids. Titles and texts are normalized
//! (threat model T19); ids, instants, hashes and references stay as written, since the caller hands them back.

use serde::{Deserialize, Serialize};

use super::MoviolaError;
use super::files::Fields;
use super::format::{closed, format};
use crate::text::{normalize, normalize_title};

/// The kind of a milestone (§5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum MilestoneKind {
    Milestone,
    Decision,
    Gate,
    Exception,
    Exploration,
    /// A direction comes back into the main one.
    Rejoin,
    Abandonment,
}

/// The health of a milestone (§5.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Healthy,
    AtRisk,
    WaitingOnOwner,
}

/// Where a direction is (§5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DirectionStatus {
    Active,
    Abandoned,
    Rejoined,
}

/// A milestone of `milestones.jsonl` (§5.3), without its links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Milestone {
    pub id: String,
    pub ts: String,
    pub kind: MilestoneKind,
    pub title: String,
    /// The id of its direction: the main one's too.
    pub direction: String,
    pub parents: Vec<String>,
    pub health: Health,
    pub opens_direction: Option<String>,
    pub refs: Refs,
    pub process: Option<Process>,
    pub author: Option<Author>,
}

/// What a milestone points at, as written: issue numbers, commit hashes, references of reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Refs {
    pub issue: Vec<u64>,
    pub commit: Vec<String>,
    pub report: Vec<String>,
}

/// The process summary of a milestone or a sidecar (§5.7): `null` is not zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Process {
    pub round: Option<u64>,
    pub checks: Vec<String>,
    pub defects: Option<ProcessDefects>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ProcessDefects {
    pub found: Option<u64>,
    pub closed: Option<u64>,
}

/// Who registered a milestone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Author {
    pub role: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub session: Option<String>,
}

/// A direction of `direzioni.jsonl` (§5.4): a lane of the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Direction {
    pub id: String,
    pub title: String,
    pub opened_by: Option<String>,
    pub status: DirectionStatus,
    pub reason: Option<String>,
}

/// A task's aggregate of `aggregati.jsonl` (§5.2, §5.6, §5.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Aggregate {
    pub project: Option<String>,
    pub task: u64,
    /// The first and the last instant or day, as written.
    pub window: Vec<String>,
    pub round: Option<u64>,
    pub declared_subagents: Option<u64>,
    pub models: Vec<ModelUse>,
    pub checks: Vec<Check>,
    pub defects: Option<AggregateDefects>,
    pub tokens: Option<Tokens>,
}

/// A model that worked on the task; `via` says when a report's session field brought it (§5.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ModelUse {
    pub agent: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub sessions: Option<u64>,
    pub via: Option<String>,
}

/// A check a report declared: its kind and outcome are the report's words, which the moviola does not check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Check {
    pub kind: Option<String>,
    pub outcome: Option<String>,
    pub defects: Vec<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AggregateDefects {
    pub found: Option<u64>,
    pub closed: Option<u64>,
    /// In a task of corrections: the defects of other tasks it closes, `#<task>/D<n>`.
    pub closes: Vec<String>,
}

/// `input` leaves the cache out (§5.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Tokens {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
}

/// A milestone's sidecar (§5.5), with the optional `in_brief` of #430, absent from the older ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Snapshot {
    pub id: Option<String>,
    pub ts: Option<String>,
    pub in_brief: Option<String>,
    pub changed: Vec<String>,
    pub direction: Option<String>,
    pub open_decisions: Vec<String>,
    pub risks: Vec<String>,
    pub next: Option<String>,
    pub changed_claims: Vec<String>,
    pub deviations: Vec<Deviation>,
    pub process: Option<Process>,
}

/// A row of a paper's table of deviations: plan, deviation, reason, impact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Deviation {
    pub plan: Option<String>,
    pub deviation: Option<String>,
    pub reason: Option<String>,
    pub impact: Option<String>,
}

fn title(text: String) -> String {
    normalize_title(&text).text
}

fn line(text: Option<String>) -> Option<String> {
    text.map(title)
}

fn texts(items: Vec<String>) -> Vec<String> {
    items.iter().map(|item| normalize(item)).collect()
}

fn one_of<T: serde::de::DeserializeOwned>(
    fields: &Fields<'_>,
    key: &str,
    list: &std::collections::BTreeMap<String, String>,
) -> Result<T, MoviolaError> {
    let word = fields.text(key)?;
    closed(list, &word).ok_or_else(|| fields.place.invalid(format!("`{key}` is `{word}`, not one of its closed list")))
}

pub(crate) fn milestone(fields: &Fields<'_>) -> Result<Milestone, MoviolaError> {
    let format = format();
    let keys = &format.milestone;
    let refs = match fields.object(&keys.refs)? {
        Some(refs) => Refs {
            issue: refs.numbers(&format.refs.issue)?,
            commit: refs.texts(&format.refs.commit)?,
            report: refs.texts(&format.refs.report)?,
        },
        None => Refs::default(),
    };
    let author = match fields.object(&keys.author)? {
        Some(author) => Some(Author {
            role: line(author.opt_text(&format.author.role)?),
            agent: line(author.opt_text(&format.author.agent)?),
            model: line(author.opt_text(&format.author.model)?),
            session: author.opt_text(&format.author.session)?,
        }),
        None => None,
    };
    Ok(Milestone {
        id: fields.text(&keys.id)?,
        ts: fields.text(&keys.ts)?,
        kind: one_of(fields, &keys.kind, &format.kinds)?,
        title: title(fields.text(&keys.title)?),
        direction: fields.text(&keys.direction)?,
        parents: fields.texts(&keys.parents)?,
        health: one_of(fields, &keys.health, &format.health)?,
        opens_direction: fields.opt_text(&keys.opens_direction)?,
        refs,
        process: fields.object(&keys.process)?.map(|process| self::process(&process)).transpose()?,
        author,
    })
}

fn process(fields: &Fields<'_>) -> Result<Process, MoviolaError> {
    let format = format();
    let keys = &format.process;
    let defects = match fields.object(&keys.defects)? {
        Some(defects) => Some(ProcessDefects {
            found: defects.opt_number(&format.defects.found)?,
            closed: defects.opt_number(&format.defects.closed)?,
        }),
        None => None,
    };
    Ok(Process { round: fields.opt_number(&keys.round)?, checks: fields.texts(&keys.checks)?.into_iter().map(title).collect(), defects })
}

pub(crate) fn direction(fields: &Fields<'_>) -> Result<Direction, MoviolaError> {
    let format = format();
    let keys = &format.direction;
    Ok(Direction {
        id: fields.text(&keys.id)?,
        title: title(fields.text(&keys.title)?),
        opened_by: fields.opt_text(&keys.opened_by)?,
        status: one_of(fields, &keys.status, &format.direction_status)?,
        reason: fields.opt_text(&keys.reason)?.map(|reason| normalize(&reason)),
    })
}

pub(crate) fn aggregate(fields: &Fields<'_>) -> Result<Aggregate, MoviolaError> {
    let format = format();
    let keys = &format.aggregate;
    let models = fields
        .objects(&keys.models)?
        .iter()
        .map(|model| {
            Ok(ModelUse {
                agent: line(model.opt_text(&format.model.agent)?),
                model: line(model.opt_text(&format.model.model)?),
                effort: line(model.opt_text(&format.model.effort)?),
                sessions: model.opt_number(&format.model.sessions)?,
                via: line(model.opt_text(&format.model.via)?),
            })
        })
        .collect::<Result<_, MoviolaError>>()?;
    let checks = fields
        .objects(&keys.checks)?
        .iter()
        .map(|check| {
            Ok(Check {
                kind: line(check.opt_text(&format.check.kind)?),
                outcome: line(check.opt_text(&format.check.outcome)?),
                defects: check.texts(&format.check.defects)?.into_iter().map(title).collect(),
                model: line(check.opt_text(&format.check.model)?),
            })
        })
        .collect::<Result<_, MoviolaError>>()?;
    let defects = match fields.object(&keys.defects)? {
        Some(defects) => Some(AggregateDefects {
            found: defects.opt_number(&format.defects.found)?,
            closed: defects.opt_number(&format.defects.closed)?,
            closes: defects.texts(&format.defects.closes)?,
        }),
        None => None,
    };
    let tokens = match fields.object(&keys.tokens)? {
        Some(tokens) => Some(Tokens {
            input: tokens.opt_number(&format.tokens.input)?,
            output: tokens.opt_number(&format.tokens.output)?,
            cache_read: tokens.opt_number(&format.tokens.cache_read)?,
            cache_write: tokens.opt_number(&format.tokens.cache_write)?,
        }),
        None => None,
    };
    Ok(Aggregate {
        project: fields.opt_text(&keys.project)?,
        task: fields.number(&keys.task)?,
        window: fields.texts(&keys.window)?,
        round: fields.opt_number(&keys.round)?,
        declared_subagents: fields.opt_number(&keys.declared_subagents)?,
        models,
        checks,
        defects,
        tokens,
    })
}

pub(crate) fn snapshot(fields: &Fields<'_>) -> Result<Snapshot, MoviolaError> {
    let format = format();
    let keys = &format.snapshot;
    let deviations = fields
        .objects(&keys.deviations)?
        .iter()
        .map(|row| {
            let text = |key: &str| row.opt_text(key).map(|text| text.map(|text| normalize(&text)));
            Ok(Deviation {
                plan: text(&format.deviation.plan)?,
                deviation: text(&format.deviation.deviation)?,
                reason: text(&format.deviation.reason)?,
                impact: text(&format.deviation.impact)?,
            })
        })
        .collect::<Result<_, MoviolaError>>()?;
    let text = |key: &str| fields.opt_text(key).map(|text| text.map(|text| normalize(&text)));
    Ok(Snapshot {
        id: fields.opt_text(&keys.id)?,
        ts: fields.opt_text(&keys.ts)?,
        in_brief: text(&keys.in_brief)?,
        changed: texts(fields.texts(&keys.changed)?),
        direction: text(&keys.direction)?,
        open_decisions: texts(fields.texts(&keys.open_decisions)?),
        risks: texts(fields.texts(&keys.risks)?),
        next: text(&keys.next)?,
        changed_claims: texts(fields.texts(&keys.changed_claims)?),
        deviations,
        process: fields.object(&keys.process)?.map(|process| self::process(&process)).transpose()?,
    })
}
