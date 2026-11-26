//! The words of format v1 of the scheduled operations, from `formats/operations_v1.toml`, read once.

use std::sync::OnceLock;

use serde::Deserialize;

pub(crate) const FORMAT: &str = include_str!("../../formats/operations_v1.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Format {
    pub(crate) version: i64,
    pub(crate) config: String,
    pub(crate) lookback_days: i64,
    pub(crate) skipped_kept: usize,
    pub(crate) dailies: String,
    pub(crate) chat_surface: String,
    pub(crate) reason_chars: usize,
    pub(crate) file_error_chars: usize,
    pub(crate) config_keys: ConfigKeys,
    pub(crate) days: Days,
    pub(crate) actions: Actions,
    pub(crate) cursor: CursorWords,
    pub(crate) entry: EntryWords,
    pub(crate) outcomes: Outcomes,
    pub(crate) voices: Voices,
    pub(crate) to_open: ToOpen,
    pub(crate) closing: Closing,
    pub(crate) reasons: Reasons,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigKeys {
    pub(crate) operations: String,
    pub(crate) name: String,
    pub(crate) hour: String,
    pub(crate) days: String,
    pub(crate) action: String,
    pub(crate) prompt: String,
    pub(crate) spec: String,
    pub(crate) folder: String,
    pub(crate) model: String,
    pub(crate) effort: String,
    pub(crate) active: String,
    pub(crate) projects: String,
    pub(crate) ideas: String,
    pub(crate) ideas_min: i64,
    pub(crate) ideas_max: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Days {
    pub(crate) names: Vec<String>,
    pub(crate) every: Vec<String>,
    pub(crate) default: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Actions {
    pub(crate) chat: String,
    pub(crate) chronicler: String,
    pub(crate) idea_search: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CursorWords {
    pub(crate) operations: String,
    pub(crate) started: String,
    pub(crate) v: String,
    pub(crate) ops: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryWords {
    pub(crate) counted: String,
    pub(crate) skipped: String,
    pub(crate) day: String,
    pub(crate) outcome: String,
    pub(crate) t: String,
    pub(crate) attempts: String,
    pub(crate) last_launched: String,
    pub(crate) open_since: String,
    pub(crate) reason: String,
    pub(crate) origin: String,
    pub(crate) session: String,
    pub(crate) scheduled: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Outcomes {
    pub(crate) in_progress: String,
    pub(crate) launched: String,
    pub(crate) started: String,
    pub(crate) to_open: String,
    pub(crate) error: String,
    pub(crate) previous_open: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Voices {
    pub(crate) operations: String,
    pub(crate) config_errors: String,
    pub(crate) error: String,
    pub(crate) name: String,
    pub(crate) action: String,
    pub(crate) days: String,
    pub(crate) t: String,
    pub(crate) outcome: String,
    pub(crate) open_since: String,
    pub(crate) reason: String,
    pub(crate) adapter: String,
    pub(crate) session: String,
    pub(crate) log: String,
    pub(crate) actions: VoiceActions,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VoiceActions {
    pub(crate) skipped: String,
    pub(crate) launched: String,
    pub(crate) already_started: String,
    pub(crate) already_done: String,
    pub(crate) postponed_suspended: String,
    pub(crate) previous_open: String,
    pub(crate) to_open: String,
    pub(crate) launch_error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToOpen {
    pub(crate) name: String,
    pub(crate) day: String,
    pub(crate) outcome: String,
    pub(crate) reason: String,
    pub(crate) prompt: String,
    pub(crate) folder: String,
    pub(crate) attempts: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Closing {
    pub(crate) folder: String,
    pub(crate) extension: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reasons {
    pub(crate) no_chat: String,
    pub(crate) previous_open: String,
}

/// The format, parsed once: it is compiled in, so a word it lacks is a bug its test catches.
pub(crate) fn format() -> &'static Format {
    static FORMAT_V1: OnceLock<Format> = OnceLock::new();
    FORMAT_V1.get_or_init(|| toml::from_str(FORMAT).expect("formats/operations_v1.toml parses"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_parses_and_its_numbers_are_the_prototype_s() {
        let format = format();
        assert_eq!((format.version, format.lookback_days, format.skipped_kept), (1, 14, 10));
        assert_eq!((format.reason_chars, format.file_error_chars), (300, 200));
        assert_eq!(format.days.names.len(), 7);
        assert_eq!((format.config_keys.ideas_min, format.config_keys.ideas_max), (1, 5));
    }
}
