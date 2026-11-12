//! The words of format v1 of the precheck, from `formats/precheck_v1.toml`, read once.

use std::sync::OnceLock;

use serde::Deserialize;

pub(crate) const FORMAT: &str = include_str!("../../formats/precheck_v1.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Format {
    pub(crate) line: Line,
    pub(crate) outcomes: Outcomes,
    pub(crate) payload: PayloadWords,
    pub(crate) project: ProjectWords,
    pub(crate) cursor: CursorWords,
    pub(crate) events: EventsWords,
    pub(crate) options: OptionWords,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Line {
    pub(crate) version: u32,
    pub(crate) max_length: usize,
    pub(crate) detail_length: usize,
    pub(crate) diagnostic: String,
    pub(crate) keys: LineKeys,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LineKeys {
    pub(crate) version: String,
    pub(crate) outcome: String,
    pub(crate) exit: String,
    pub(crate) reason: String,
    pub(crate) cursor: String,
    pub(crate) payload: String,
    pub(crate) t: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Outcomes {
    pub(crate) new_activity: OutcomeWords<NewActivityReasons>,
    pub(crate) idle: OutcomeWords<IdleReasons>,
    pub(crate) terminal_state: OutcomeWords<TerminalReasons>,
    pub(crate) error: OutcomeWords<ErrorReasons>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutcomeWords<R> {
    pub(crate) word: String,
    pub(crate) exit: u8,
    pub(crate) reasons: R,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewActivityReasons {
    pub(crate) ready: String,
    pub(crate) reports: String,
    pub(crate) answers: String,
    pub(crate) inbox: String,
    pub(crate) leases: String,
    pub(crate) backlog: String,
    pub(crate) silent_workers: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IdleReasons {
    pub(crate) no_event: String,
    pub(crate) busy: String,
    pub(crate) wip_limit: String,
    pub(crate) budget: String,
    pub(crate) proposals_exhausted: String,
    pub(crate) waiting_on_owner: String,
    pub(crate) work_in_progress: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerminalReasons {
    pub(crate) objective_done: String,
    pub(crate) stall: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ErrorReasons {
    pub(crate) tracker: String,
    pub(crate) runtime: String,
    pub(crate) unreadable: String,
    pub(crate) internal: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PayloadWords {
    pub(crate) projects: String,
    pub(crate) truncated: String,
    pub(crate) detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectWords {
    pub(crate) id: String,
    pub(crate) outcome: String,
    pub(crate) reason: String,
    pub(crate) events: String,
    pub(crate) ready: String,
    pub(crate) reports: String,
    pub(crate) answers: String,
    pub(crate) inbox: String,
    pub(crate) leases: String,
    pub(crate) silent: String,
    pub(crate) blocked_by_wip: String,
    pub(crate) in_progress: String,
    pub(crate) waiting_on_owner: String,
    pub(crate) tasks: String,
    pub(crate) max_tasks: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CursorWords {
    pub(crate) project: String,
    pub(crate) value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventsWords {
    pub(crate) t: String,
    pub(crate) own: String,
    pub(crate) comments: String,
    pub(crate) reports: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OptionWords {
    pub(crate) project: String,
    pub(crate) stall_n: String,
    pub(crate) stall_s: String,
    pub(crate) realert_s: String,
    pub(crate) budget_max: String,
    pub(crate) deadline_s: String,
    pub(crate) defaults: OptionDefaults,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OptionDefaults {
    pub(crate) stall_n: i64,
    pub(crate) stall_s: f64,
    pub(crate) realert_s: f64,
    pub(crate) deadline_s: f64,
}

pub(crate) fn format() -> &'static Format {
    static FORMAT_V1: OnceLock<Format> = OnceLock::new();
    FORMAT_V1.get_or_init(|| toml::from_str(FORMAT).expect("the embedded v1 format of the precheck is valid"))
}
