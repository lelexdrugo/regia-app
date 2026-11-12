//! The moviola's words of format v1, in `formats/moviola_v1.toml`: where its files are, the keys of their
//! records and the words of their closed lists. The sources hold none of them.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use serde::de::DeserializeOwned;

const FORMAT: &str = include_str!("../../formats/moviola_v1.toml");

/// The keys of one kind of record: the product's name of each field, the prototype's key as its value.
macro_rules! keys {
    ($name:ident { $($field:ident),* $(,)? }) => {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct $name {
            $(pub $field: String,)*
        }
    };
}

keys!(MilestoneKeys { id, ts, kind, title, direction, parents, health, opens_direction, refs, process, author });
keys!(RefsKeys { issue, commit, report });
keys!(ProcessKeys { round, checks, defects });
keys!(DefectsKeys { found, closed, closes });
keys!(AuthorKeys { role, agent, model, session });
keys!(DirectionKeys { id, title, opened_by, status, reason });
keys!(AggregateKeys { project, task, window, round, declared_subagents, models, checks, defects, tokens });
keys!(ModelKeys { agent, model, effort, sessions, via });
keys!(CheckKeys { kind, outcome, defects, model });
keys!(TokensKeys { input, output, cache_read, cache_write });
keys!(SnapshotKeys { id, ts, changed, direction, open_decisions, risks, next, in_brief, changed_claims, deviations, process });
keys!(DeviationKeys { plan, deviation, reason, impact });

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Format {
    pub folder: Vec<String>,
    pub milestones: String,
    pub directions: String,
    pub aggregates: String,
    pub snapshots: String,
    pub snapshot_extension: String,
    pub main_direction: String,
    pub milestone: MilestoneKeys,
    pub refs: RefsKeys,
    pub process: ProcessKeys,
    pub defects: DefectsKeys,
    pub author: AuthorKeys,
    pub direction: DirectionKeys,
    pub aggregate: AggregateKeys,
    pub model: ModelKeys,
    pub check: CheckKeys,
    pub tokens: TokensKeys,
    pub snapshot: SnapshotKeys,
    pub deviation: DeviationKeys,
    /// A closed list each: the product's id, then the prototype's word.
    pub kinds: BTreeMap<String, String>,
    pub health: BTreeMap<String, String>,
    pub direction_status: BTreeMap<String, String>,
}

pub(crate) fn format() -> &'static Format {
    static PARSED: OnceLock<Format> = OnceLock::new();
    PARSED.get_or_init(|| toml::from_str(FORMAT).expect("the embedded moviola format is valid"))
}

/// The value of a closed list that `word` names, through the product's id the list gives it; `None` for a word
/// the list does not have.
pub(crate) fn closed<T: DeserializeOwned>(list: &BTreeMap<String, String>, word: &str) -> Option<T> {
    let id = list.iter().find_map(|(id, known)| (known == word).then_some(id))?;
    serde_json::from_value(serde_json::Value::String(id.clone())).ok()
}

/// The prototype's word for a value of a closed list.
#[cfg(test)]
pub(crate) fn word_of<T: serde::Serialize>(list: &BTreeMap<String, String>, value: &T) -> Option<String> {
    let id = serde_json::to_value(value).ok()?;
    list.get(id.as_str()?).cloned()
}
