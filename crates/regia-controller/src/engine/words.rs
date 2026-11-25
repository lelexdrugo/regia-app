//! The words of format v1 of the controller's tick, from `formats/controller_v1.toml`, read once.

use std::sync::OnceLock;

use serde::Deserialize;

pub(crate) const FORMAT: &str = include_str!("../../formats/controller_v1.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Format {
    pub(crate) version: i64,
    pub(crate) step_limit_s: f64,
    pub(crate) grace_without_pid_s: f64,
    pub(crate) reserved_project: String,
    pub(crate) record: RecordWords,
    pub(crate) decisions: Decisions,
    pub(crate) row: RowWords,
    pub(crate) actions: Actions,
    pub(crate) launched: LaunchedWords,
    pub(crate) adapters: Adapters,
    pub(crate) finished: FinishedWords,
    pub(crate) budget: BudgetWords,
    pub(crate) wake: WakeWords,
    pub(crate) postpone: PostponeWords,
    pub(crate) status: StatusWords,
    pub(crate) registry: RegistryWords,
    pub(crate) dossier: DossierWords,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordWords {
    pub(crate) v: String,
    pub(crate) t: String,
    pub(crate) round: String,
    pub(crate) wakeup: String,
    pub(crate) precheck: String,
    pub(crate) decision: String,
    pub(crate) projects: String,
    pub(crate) launched: String,
    pub(crate) budget: String,
    pub(crate) wake: String,
    pub(crate) finished: String,
    pub(crate) alive: String,
    pub(crate) postpone_errors: String,
    pub(crate) precheck_keys: PrecheckKeys,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrecheckKeys {
    pub(crate) exit: String,
    pub(crate) outcome: String,
    pub(crate) reason: String,
    pub(crate) t: String,
    pub(crate) error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Decisions {
    pub(crate) launch: String,
    pub(crate) no_launch: String,
    pub(crate) precheck_broken: String,
    pub(crate) idle: String,
    pub(crate) error: String,
    pub(crate) unexpected_exit: String,
    pub(crate) incoherent: String,
    pub(crate) truncated: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RowWords {
    pub(crate) project: String,
    pub(crate) outcome: String,
    pub(crate) reason: String,
    pub(crate) action: String,
    pub(crate) error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Actions {
    pub(crate) launched: String,
    pub(crate) skipped_alive: String,
    pub(crate) skipped_suspended: String,
    pub(crate) postponed: String,
    pub(crate) launch_error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LaunchedWords {
    pub(crate) adapter: String,
    pub(crate) agent: String,
    pub(crate) model: String,
    pub(crate) effort: String,
    pub(crate) session: String,
    pub(crate) transcript: String,
    pub(crate) pid: String,
    pub(crate) log: String,
    pub(crate) started: String,
    pub(crate) project: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Adapters {
    pub(crate) fake: String,
    pub(crate) cli: String,
    pub(crate) orca: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FinishedWords {
    pub(crate) project: String,
    pub(crate) why: String,
    pub(crate) notes: Notes,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Notes {
    pub(crate) not_launched: String,
    pub(crate) limit: String,
    pub(crate) grace: String,
    pub(crate) grace_over: String,
    pub(crate) alive: String,
    pub(crate) ended: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BudgetWords {
    pub(crate) unknown: String,
    pub(crate) rounds: String,
    pub(crate) since: String,
    pub(crate) alarm: String,
    pub(crate) reason: String,
    pub(crate) pct: String,
    pub(crate) stale: String,
    pub(crate) open: String,
    pub(crate) alarm_rounds: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WakeWords {
    pub(crate) t: String,
    pub(crate) ok: String,
    pub(crate) exit: String,
    pub(crate) duration: String,
    pub(crate) cost: String,
    pub(crate) error: String,
    pub(crate) fake: String,
    pub(crate) expiry_before: String,
    pub(crate) expiry_after: String,
    pub(crate) renewed: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PostponeWords {
    pub(crate) cursor: String,
    pub(crate) v: String,
    pub(crate) t: String,
    pub(crate) id: String,
    pub(crate) reason: String,
    pub(crate) round: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusWords {
    pub(crate) v: String,
    pub(crate) pid: String,
    pub(crate) holder: String,
    pub(crate) started: String,
    pub(crate) interval: String,
    pub(crate) last_round: String,
    pub(crate) round: String,
    pub(crate) precheck: String,
    pub(crate) decision: String,
    pub(crate) budget: String,
    pub(crate) unknown_budget: String,
    pub(crate) alive: String,
    pub(crate) suspended: String,
    pub(crate) to_resume: String,
    pub(crate) to_open: String,
    pub(crate) operations: String,
    pub(crate) wake: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryWords {
    pub(crate) cursor: String,
    pub(crate) v: String,
    pub(crate) steps: String,
    pub(crate) keep_finished_s: f64,
    pub(crate) record: RegistryRecord,
    pub(crate) states: RegistryStates,
    pub(crate) origins: RegistryOrigins,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryRecord {
    pub(crate) project: String,
    pub(crate) folder: String,
    pub(crate) origin: String,
    pub(crate) adapter: String,
    pub(crate) agent: String,
    pub(crate) model: String,
    pub(crate) effort: String,
    pub(crate) session: String,
    pub(crate) resume: String,
    pub(crate) orca_session: String,
    pub(crate) transcript: String,
    pub(crate) pid: String,
    pub(crate) log: String,
    pub(crate) marker: String,
    pub(crate) started_t: String,
    pub(crate) started: String,
    pub(crate) state: String,
    pub(crate) updated_t: String,
    pub(crate) resumes: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryStates {
    pub(crate) launched: String,
    pub(crate) finished: String,
    pub(crate) suspended: String,
    pub(crate) resuming: String,
    pub(crate) to_resume: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryOrigins {
    pub(crate) controller: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DossierWords {
    pub(crate) state_file: String,
    pub(crate) memo_model: String,
}

/// The format, parsed once: a format that does not parse is a defect of the build, caught by the tests.
pub(crate) fn format() -> &'static Format {
    static FORMAT_V1: OnceLock<Format> = OnceLock::new();
    FORMAT_V1.get_or_init(|| toml::from_str(FORMAT).expect("formats/controller_v1.toml is valid"))
}
