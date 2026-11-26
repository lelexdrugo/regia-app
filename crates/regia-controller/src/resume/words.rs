//! The words of format v1 of the resume of suspended sessions, from `formats/resume_v1.toml`, read once.

use std::sync::OnceLock;

use serde::Deserialize;

pub(crate) const FORMAT: &str = include_str!("../../formats/resume_v1.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Format {
    pub(crate) version: i64,
    pub(crate) margin_s: f64,
    pub(crate) max_failed: i64,
    pub(crate) stale_s: f64,
    pub(crate) opt_in_days: f64,
    pub(crate) release_kept_s: f64,
    pub(crate) error_chars: usize,
    pub(crate) files: Files,
    pub(crate) record: Record,
    pub(crate) analysis: AnalysisWords,
    pub(crate) attempt: Attempt,
    pub(crate) origins: Origins,
    pub(crate) line: Line,
    pub(crate) suspended: Suspended,
    pub(crate) resume: ResumeWords,
    pub(crate) wait: Wait,
    pub(crate) waiting: Waiting,
    pub(crate) decisions: Decisions,
    pub(crate) to_resume: ToResume,
    pub(crate) opt_in: OptIn,
    pub(crate) release: Release,
    pub(crate) reasons: Reasons,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Files {
    pub(crate) to_resume: String,
    pub(crate) opt_in: String,
    pub(crate) releases: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub(crate) analysis: String,
    pub(crate) resets: String,
    pub(crate) resets_iso: String,
    pub(crate) failed: String,
    pub(crate) released: String,
    pub(crate) resumed_t: String,
    pub(crate) parent: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisWords {
    pub(crate) state: String,
    pub(crate) resets_iso: String,
    pub(crate) source: String,
    pub(crate) zone: String,
    pub(crate) estimated: String,
    pub(crate) limit_iso: String,
    pub(crate) entrypoint: String,
    pub(crate) states: AnalysisStates,
    pub(crate) sources: Sources,
    pub(crate) zones: Zones,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisStates {
    pub(crate) suspended: String,
    pub(crate) resuming: String,
    pub(crate) active: String,
    pub(crate) empty: String,
    pub(crate) unreadable: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sources {
    pub(crate) quota_limits: String,
    pub(crate) api_error_params: String,
    pub(crate) text: String,
    pub(crate) task_error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Zones {
    pub(crate) local: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Attempt {
    pub(crate) limit: String,
    pub(crate) effective: String,
    pub(crate) pid: String,
    pub(crate) log: String,
    pub(crate) new_session: String,
    pub(crate) error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Origins {
    pub(crate) worker: String,
    pub(crate) opt_in: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Line {
    pub(crate) suspended: String,
    pub(crate) resumes: String,
    pub(crate) resume_error: String,
    pub(crate) releases: String,
    pub(crate) wait: String,
    pub(crate) waiting_to_resume: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Suspended {
    pub(crate) project: String,
    pub(crate) session: String,
    pub(crate) adapter: String,
    pub(crate) origin: String,
    pub(crate) resets: String,
    pub(crate) due: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResumeWords {
    pub(crate) session: String,
    pub(crate) project: String,
    pub(crate) adapter: String,
    pub(crate) agent: String,
    pub(crate) expected: String,
    pub(crate) effective: String,
    pub(crate) pid: String,
    pub(crate) log: String,
    pub(crate) new_session: String,
    pub(crate) error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Wait {
    pub(crate) until: String,
    pub(crate) sessions: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Waiting {
    pub(crate) project: String,
    pub(crate) session: String,
    pub(crate) reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Decisions {
    pub(crate) resumed: String,
    pub(crate) resume_error: String,
    pub(crate) suspended: String,
    pub(crate) to_resume: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToResume {
    pub(crate) v: String,
    pub(crate) updated: String,
    pub(crate) sessions: String,
    pub(crate) project: String,
    pub(crate) folder: String,
    pub(crate) origin: String,
    pub(crate) adapter: String,
    pub(crate) agent: String,
    pub(crate) session: String,
    pub(crate) surface_session: String,
    pub(crate) transcript: String,
    pub(crate) resets: String,
    pub(crate) reason: String,
    pub(crate) how: String,
    pub(crate) command: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OptIn {
    pub(crate) sessions: String,
    pub(crate) folders: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Release {
    pub(crate) session: String,
    pub(crate) t: String,
    pub(crate) all: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reasons {
    pub(crate) agent: String,
    pub(crate) app_session: String,
    pub(crate) app_chat: String,
    pub(crate) no_session: String,
    pub(crate) launched: String,
    pub(crate) no_answer: String,
    pub(crate) shown: String,
    pub(crate) opted_in: String,
    pub(crate) failed: String,
    pub(crate) how_app: String,
}

/// The format, parsed once: a format that does not parse is a defect of the build, caught by the tests.
pub(crate) fn format() -> &'static Format {
    static FORMAT_V1: OnceLock<Format> = OnceLock::new();
    FORMAT_V1.get_or_init(|| toml::from_str(FORMAT).expect("formats/resume_v1.toml is valid"))
}
