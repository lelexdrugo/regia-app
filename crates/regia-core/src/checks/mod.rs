//! The deterministic checks of a project's dossier (#448; the prototype's `tools/controlli.py` at 247e041): the
//! commitments left outside the tracker. Nothing here reads a file, the tracker or a clock: the caller passes the
//! texts and the data it read, and gets the findings, in the order of the checks and, inside a check, in the
//! order read. The words the checks look for and the sentences of their findings are the prototype's, data of
//! format v1 in `formats/checks_v1.toml`; the patterns are Python's, run by `fancy-regex` with Python's classes
//! spelled out, since the `regex` crate's `\w`, `\s` and `\b` are not Python's.

mod paths;
mod remaining;
mod state;
mod tracker;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday};
use fancy_regex::Regex;
use serde::Deserialize;
use serde::ser::{Serialize, SerializeMap, Serializer};

use crate::model::{Status, Task};
use crate::pystr;

pub use paths::{absolute_paths, decisions_over_cap};
pub use remaining::{
    Destination, IssueFacts, ReconciliationRow, Reconciliations, ReportRemaining, classify_destination, read_reconciliations, read_report,
    remaining_unreconciled,
};
pub use state::{open_questions, segments, state_lines_removed, state_without_issue};
pub use tracker::{CommentedIssue, approved_never_started, comment_role, comments_after_close, decision_without_flag, inbox_stale, in_progress_stalled, project_field};

const FORMAT: &str = include_str!("../../formats/checks_v1.toml");
/// The product's copy of the prototype's thresholds' file: the defaults.
const DEFAULTS: &str = include_str!("../../formats/controlli.yaml");

#[derive(Deserialize)]
struct Format {
    files: Files,
    dates: Dates,
    thresholds: ThresholdKeys,
    text: Words,
    patterns: BTreeMap<String, String>,
    why: BTreeMap<String, String>,
    thresholds_reasons: BTreeMap<String, String>,
    warnings: BTreeMap<String, String>,
}

/// The files a check reads, relative to a project's folder; the thresholds' file, to the prototype's.
#[derive(Debug, Deserialize)]
pub struct Files {
    pub state: String,
    pub questions: String,
    pub reports: String,
    pub regia: String,
    pub decisions: String,
    pub archive_prefix: String,
    pub archive_suffix: String,
    pub report_suffix: String,
    pub thresholds: String,
}

#[derive(Deserialize)]
struct Dates {
    remaining_from: String,
    absolute_from: String,
}

#[derive(Deserialize)]
struct ThresholdKeys {
    inbox_days: String,
    approved_days: String,
    in_progress_days: String,
    comment_window_days: String,
    brief_max: String,
    decisions_cap: String,
}

#[derive(Deserialize)]
struct Words {
    hand: char,
    apostrophe: char,
    variation_selector: char,
    ellipsis: String,
    max_words: usize,
    decision_prefix: String,
    own_roles: Vec<String>,
    chat_role: String,
    closed_destination_days: i64,
    type_fields: Vec<String>,
    remaining_field: String,
    task_field: String,
    done: String,
    none: String,
    quotes: String,
    destination_split: char,
}

struct Compiled {
    format: Format,
    patterns: BTreeMap<String, Regex>,
    word: regex::Regex,
}

fn compiled() -> &'static Compiled {
    static COMPILED: OnceLock<Compiled> = OnceLock::new();
    COMPILED.get_or_init(|| {
        let format: Format = toml::from_str(FORMAT).expect("the embedded format of the checks is valid");
        let word = pystr::WORD;
        let space = r"\s\x1C-\x1F";
        let boundary = format!("(?:(?<=[{word}])(?![{word}])|(?<![{word}])(?=[{word}]))");
        let end = format.patterns["end"].replace("{s}", space);
        let patterns = format
            .patterns
            .iter()
            .filter(|(name, _)| name.as_str() != "end")
            .map(|(name, pattern)| {
                let pattern = pattern.replace("{end}", &end).replace("{w}", word).replace("{s}", space).replace("{b}", &boundary);
                let regex = Regex::new(&pattern).unwrap_or_else(|error| panic!("pattern {name}: {error}"));
                (name.clone(), regex)
            })
            .collect();
        Compiled { format, patterns, word: regex::Regex::new(&format!("[{word}]")).expect("a valid class") }
    })
}

fn format() -> &'static Format {
    &compiled().format
}

fn words() -> &'static Words {
    &format().text
}

/// A pattern of the format, by its name.
fn pattern(name: &str) -> &'static Regex {
    compiled().patterns.get(name).unwrap_or_else(|| panic!("no pattern {name} in the format of the checks"))
}

/// Whether `pattern` matches somewhere in `text`. A pattern that runs out of its backtracking budget matches
/// nowhere, as no check needs it to.
fn found(name: &str, text: &str) -> bool {
    pattern(name).is_match(text).unwrap_or(false)
}

/// A sentence of the format, with its `{name}` values filled in.
fn sentence(table: &'static BTreeMap<String, String>, key: &str, values: &[(&str, &str)]) -> String {
    let template = table.get(key).unwrap_or_else(|| panic!("no sentence {key} in the format of the checks"));
    values.iter().fold(template.clone(), |text, (name, value)| text.replace(&format!("{{{name}}}"), value))
}

fn why(key: &str, values: &[(&str, &str)]) -> String {
    sentence(&format().why, key, values)
}

/// The files a check reads.
pub fn files() -> &'static Files {
    &format().files
}

/// The date of a report's name from which `remaining-unreconciled` reads it.
pub fn remaining_from() -> &'static str {
    &format().dates.remaining_from
}

/// The date of a report's name, and of an entry of the decisions, from which `absolute-path` reads it.
pub fn absolute_from() -> &'static str {
    &format().dates.absolute_from
}

/// A warning of the wiring, in the prototype's words, with its values.
pub fn warning(key: &str, values: &[(&str, &str)]) -> String {
    sentence(&format().warnings, key, values)
}

/// The checks, in the prototype's order: the order of the findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CheckId {
    StateWithoutIssue,
    StateLinesRemoved,
    OpenQuestionsWithoutIssue,
    RemainingUnreconciled,
    CommentAfterClose,
    DecisionWithoutFlag,
    InboxStale,
    ApprovedNeverStarted,
    InProgressStalled,
    ProjectField,
    AbsolutePath,
    DecisionsOverCap,
}

impl CheckId {
    pub const ALL: [CheckId; 12] = [
        CheckId::StateWithoutIssue,
        CheckId::StateLinesRemoved,
        CheckId::OpenQuestionsWithoutIssue,
        CheckId::RemainingUnreconciled,
        CheckId::CommentAfterClose,
        CheckId::DecisionWithoutFlag,
        CheckId::InboxStale,
        CheckId::ApprovedNeverStarted,
        CheckId::InProgressStalled,
        CheckId::ProjectField,
        CheckId::AbsolutePath,
        CheckId::DecisionsOverCap,
    ];

    /// The contract's id (`docs/contract/commands.md`, `check`).
    pub fn id(self) -> &'static str {
        match self {
            CheckId::StateWithoutIssue => "state-without-issue",
            CheckId::StateLinesRemoved => "state-lines-removed",
            CheckId::OpenQuestionsWithoutIssue => "open-questions-without-issue",
            CheckId::RemainingUnreconciled => "remaining-unreconciled",
            CheckId::CommentAfterClose => "comment-after-close",
            CheckId::DecisionWithoutFlag => "decision-without-flag",
            CheckId::InboxStale => "inbox-stale",
            CheckId::ApprovedNeverStarted => "approved-never-started",
            CheckId::InProgressStalled => "in-progress-stalled",
            CheckId::ProjectField => "project-field",
            CheckId::AbsolutePath => "absolute-path",
            CheckId::DecisionsOverCap => "decisions-over-cap",
        }
    }

    pub fn parse(id: &str) -> Option<CheckId> {
        CheckId::ALL.into_iter().find(|check| check.id() == id)
    }
}

/// A finding: the check, the project, where (a file and its line, or an issue and a comment), the text, cut to
/// the format's words, and why. A finding on the tracker carries the URL it read, `null` when it read none; one
/// on the dossier has no URL at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub check: CheckId,
    pub project: String,
    pub location: String,
    pub text: String,
    pub why: String,
    pub url: Option<Option<String>>,
}

impl Finding {
    fn new(check: CheckId, project: &str, location: String, text: &str, why: String) -> Finding {
        Finding { check, project: project.to_owned(), location, text: truncate(text, words().max_words), why, url: None }
    }

    fn with_url(mut self, url: Option<&str>) -> Finding {
        self.url = Some(url.map(str::to_owned));
        self
    }
}

impl Serialize for Finding {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("check", self.check.id())?;
        map.serialize_entry("project", &self.project)?;
        map.serialize_entry("where", &self.location)?;
        map.serialize_entry("text", &self.text)?;
        map.serialize_entry("why", &self.why)?;
        if let Some(url) = &self.url {
            map.serialize_entry("url", url)?;
        }
        map.end()
    }
}

/// How many findings each check gave, keyed by the contract's id.
pub fn per_check(findings: &[Finding]) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for finding in findings {
        *out.entry(finding.check.id()).or_insert(0) += 1;
    }
    out
}

/// The thresholds of the checks (`policies/controlli.yaml`): days, the brief's findings per project, and the
/// cap of `decisioni.md` in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub inbox_days: u64,
    pub approved_days: u64,
    pub in_progress_days: u64,
    pub comment_window_days: u64,
    pub brief_max: u64,
    pub decisions_cap: u64,
}

impl Thresholds {
    /// The product's defaults: the values of its copy of the prototype's file.
    pub fn defaults() -> Thresholds {
        static DEFAULT: OnceLock<Thresholds> = OnceLock::new();
        *DEFAULT.get_or_init(|| {
            let mut seen = BTreeSet::new();
            let zero = Thresholds { inbox_days: 0, approved_days: 0, in_progress_days: 0, comment_window_days: 0, brief_max: 0, decisions_cap: 0 };
            let (values, reason) = parse_thresholds(DEFAULTS, "controlli.yaml", zero, &mut seen);
            assert!(reason.is_none() && seen.len() == 6, "formats/controlli.yaml gives every threshold: {reason:?}");
            values
        })
    }

    /// The thresholds of the file `name` whose text is `text`, as `carica_soglie` reads it: `key: integer` a line,
    /// each key once, an integer of 1 or more, `#` and what follows it a comment. A file with a line that does not
    /// read gives the defaults and says which line: it never applies in half.
    pub fn parse(text: &str, name: &str) -> (Thresholds, Option<String>) {
        parse_thresholds(text, name, Thresholds::defaults(), &mut BTreeSet::new())
    }

    /// The defaults, and why: the file `name` did not read, for an error of the system of type `kind` (Python's
    /// name of it) with its message, or none.
    pub fn unreadable(name: &str, kind: &str, message: Option<&str>) -> (Thresholds, Option<String>) {
        let reasons = &format().thresholds_reasons;
        let error = format!("{kind}: {}", message.unwrap_or(&reasons["not_readable"]));
        (Thresholds::defaults(), Some(sentence(reasons, "unreadable", &[("file", name), ("error", &error)])))
    }

    /// The defaults, and why: the file `name` is not UTF-8.
    pub fn undecodable(name: &str) -> (Thresholds, Option<String>) {
        (Thresholds::defaults(), Some(sentence(&format().thresholds_reasons, "unreadable", &[("file", name), ("error", "UnicodeDecodeError")])))
    }

    /// The values with the prototype's keys, in its order, for a comparison with its own.
    pub fn by_key(&self) -> Vec<(&'static str, u64)> {
        let keys = &format().thresholds;
        vec![
            (keys.inbox_days.as_str(), self.inbox_days),
            (keys.approved_days.as_str(), self.approved_days),
            (keys.in_progress_days.as_str(), self.in_progress_days),
            (keys.comment_window_days.as_str(), self.comment_window_days),
            (keys.brief_max.as_str(), self.brief_max),
            (keys.decisions_cap.as_str(), self.decisions_cap),
        ]
    }

    fn slot(&mut self, key: &str) -> Option<&mut u64> {
        let keys = &format().thresholds;
        Some(match key {
            _ if key == keys.inbox_days => &mut self.inbox_days,
            _ if key == keys.approved_days => &mut self.approved_days,
            _ if key == keys.in_progress_days => &mut self.in_progress_days,
            _ if key == keys.comment_window_days => &mut self.comment_window_days,
            _ if key == keys.brief_max => &mut self.brief_max,
            _ if key == keys.decisions_cap => &mut self.decisions_cap,
            _ => return None,
        })
    }
}

fn parse_thresholds(text: &str, name: &str, start: Thresholds, seen: &mut BTreeSet<String>) -> (Thresholds, Option<String>) {
    let mut values = start;
    for (n, line) in pystr::split_lines(text).into_iter().enumerate() {
        let line = pystr::strip(line.split('#').next().unwrap_or_default());
        if line.is_empty() {
            continue;
        }
        let invalid = || {
            let shown: String = line.chars().take(40).collect();
            let reason = sentence(&format().thresholds_reasons, "invalid_line", &[("file", name), ("n", &(n + 1).to_string()), ("line", &shown)]);
            (Thresholds::defaults(), Some(reason))
        };
        let Some((key, value)) = line.split_once(':') else { return invalid() };
        let (key, value) = (pystr::strip(key), pystr::strip(value));
        let number = python_int(value).filter(|number| *number >= 1);
        match (values.slot(key), number) {
            (Some(slot), Some(number)) if !seen.contains(key) => {
                *slot = number;
                seen.insert(key.to_owned());
            }
            _ => return invalid(),
        }
    }
    (values, None)
}

/// The value of a string of Unicode's decimal digits, as Python's `int()` reads one; `None` for an empty string or
/// one with another character. A number past `u64` reads as `u64::MAX`, a number no tracker has.
pub fn python_int(digits: &str) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    let mut overflow = false;
    for c in digits.chars() {
        let digit = decimal_digit(c)?;
        match value.checked_mul(10).and_then(|value| value.checked_add(u64::from(digit))) {
            Some(next) => value = next,
            None => overflow = true,
        }
    }
    Some(if overflow { u64::MAX } else { value })
}

/// The value of one of Unicode's decimal digits (`\p{Nd}`). They come in runs of ten from a zero, and a run of
/// consecutive code points is made of whole runs of ten, so the value is the distance from the run's start, modulo
/// ten.
pub(crate) fn decimal_digit(c: char) -> Option<u32> {
    if let Some(digit) = c.to_digit(10) {
        return Some(digit);
    }
    static DECIMAL: OnceLock<regex::Regex> = OnceLock::new();
    let decimal = DECIMAL.get_or_init(|| regex::Regex::new(r"^\p{Nd}$").expect("a valid class"));
    let is_decimal = |code: u32| char::from_u32(code).is_some_and(|c| decimal.is_match(c.encode_utf8(&mut [0; 4])));
    let code = u32::from(c);
    if !is_decimal(code) {
        return None;
    }
    let mut start = code;
    while start > 0 && is_decimal(start - 1) {
        start -= 1;
    }
    Some((code - start) % 10)
}

/// Whether `text` has a character of Python's `\w`.
fn has_word(text: &str) -> bool {
    compiled().word.is_match(text)
}

/// `_pulito`: no `\r`, the typographic apostrophe as `'`, no variation selector.
fn clean(text: &str) -> String {
    let w = words();
    text.chars().filter(|c| *c != '\r' && *c != w.variation_selector).map(|c| if c == w.apostrophe { '\'' } else { c }).collect()
}

/// `_minuscolo`: cleaned, in lowercase as Python's `str.lower()`, without `*` and backticks.
fn lowercase(text: &str) -> String {
    clean(text).to_lowercase().replace(['*', '`'], "")
}

/// `text` without the bullet at its start (`_PUNTO_ELENCO_RE.sub`).
fn without_bullet(text: &str) -> String {
    match pattern("bullet").find(text) {
        Ok(Some(found)) if found.start() == 0 => text[found.end()..].to_owned(),
        _ => text.to_owned(),
    }
}

/// `re.sub(r"\s+", " ", text)`.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = false;
    for c in text.chars() {
        if pystr::is_space(c) {
            if !blank {
                out.push(' ');
            }
            blank = true;
        } else {
            out.push(c);
            blank = false;
        }
    }
    out
}

/// `_norm_riga`: a line as two lines compare, so that spaces, case, markup and the bullet do not count.
fn normalized_line(text: &str) -> String {
    pystr::strip(&collapse_spaces(&without_bullet(&lowercase(text)))).to_owned()
}

/// `tronca`: the first `words` words of `text`, without `*` and backticks, and the ellipsis if there were more.
pub fn truncate(text: &str, words: usize) -> String {
    let text = text.replace(['*', '`'], "");
    let all: Vec<&str> = text.split(pystr::is_space).filter(|word| !word.is_empty()).collect();
    let mut out = all.iter().take(words).copied().collect::<Vec<_>>().join(" ");
    if all.len() > words {
        out.push_str(&self::words().ellipsis);
    }
    out
}

/// `riferimenti`: the issues a text cites, `#N` and the URLs `…/issues/N`.
pub fn references(text: &str) -> BTreeSet<u64> {
    let text = clean(text);
    let mut out = BTreeSet::new();
    for captures in pattern("references").captures_iter(&text).flatten() {
        if let Some(digits) = captures.get(1).or_else(|| captures.get(2)) {
            out.extend(python_int(digits.as_str()));
        }
    }
    out
}

/// An instant as `datetime.fromisoformat` reads one after `Z` becomes `+00:00`: a date, a separator, a time with
/// its offset. One with no offset the prototype cannot compare with its clock: none here.
fn instant(text: Option<&str>) -> Option<DateTime<Utc>> {
    let text = text.filter(|text| !text.is_empty())?.replace('Z', "+00:00");
    let date = NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()?;
    let rest: String = text.get(10..)?.chars().skip(1).collect();
    let at = rest.rfind(['+', '-'])?;
    let (time, offset) = rest.split_at(at);
    let time = NaiveTime::parse_from_str(time, "%H:%M:%S%.f").or_else(|_| NaiveTime::parse_from_str(time, "%H:%M")).ok()?;
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let parts: Vec<&str> = offset[1..].split(':').collect();
    let number = |part: Option<&&str>| part.filter(|part| part.len() == 2).and_then(|part| part.parse::<i32>().ok());
    let seconds = match parts.len() {
        2 | 3 => number(parts.first())? * 3600 + number(parts.get(1))? * 60 + if parts.len() == 3 { number(parts.get(2))? } else { 0 },
        _ => return None,
    };
    let offset = chrono::FixedOffset::east_opt(sign * seconds)?;
    offset.from_local_datetime(&NaiveDateTime::new(date, time)).single().map(|instant| instant.with_timezone(&Utc))
}

/// `_giorni`: the days from `then` to `now`, as Python's float of the microseconds.
fn days_between(now: DateTime<Utc>, then: DateTime<Utc>) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)]
    (now - then).num_microseconds().map(|micros| micros as f64 / 1e6 / 86400.0)
}

/// `_data_roma`: the calendar date in Rome of an instant, by the rule of the European Union since 1996 (summer
/// time from the last Sunday of March to the last Sunday of October, at 01:00 UTC), which is what the prototype's
/// time zone database says for every date a tracker closes an issue on.
pub fn rome_date(text: Option<&str>) -> Option<NaiveDate> {
    let instant = instant(text)?;
    let last_sunday = |year: i32, month: u32| {
        let first_of_next = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) };
        let mut day = first_of_next? - Duration::days(1);
        while day.weekday() != Weekday::Sun {
            day -= Duration::days(1);
        }
        Some(day)
    };
    let year = instant.year();
    let one = NaiveTime::from_hms_opt(1, 0, 0)?;
    let start = Utc.from_utc_datetime(&last_sunday(year, 3)?.and_time(one));
    let end = Utc.from_utc_datetime(&last_sunday(year, 10)?.and_time(one));
    let hours = if instant >= start && instant < end { 2 } else { 1 };
    Some((instant + Duration::hours(hours)).date_naive())
}

/// What a project's checks read, as the prototype's check of a project takes it. Each input that is `None` skips the checks that
/// use it, as the wiring declares in its warnings.
#[derive(Debug, Clone, Default)]
pub struct ProjectInputs<'a> {
    pub project: &'a str,
    /// Every open issue, of any project, by number, with its status.
    pub open: BTreeMap<u64, Option<Status>>,
    pub issues: Option<&'a [Task]>,
    pub comments: Option<&'a [CommentedIssue]>,
    /// The state file now and, when there is one to compare with, before.
    pub state: Option<&'a str>,
    pub previous_state: Option<&'a str>,
    pub questions: Option<&'a str>,
    /// The numbers with a report in `.regia/report/`.
    pub with_report: Option<&'a BTreeSet<u64>>,
    pub live_leases: Option<&'a BTreeSet<u64>>,
    /// A removed line that cites only closed issues is lost too: the strict variant, off in the wiring.
    pub only_open_removed: bool,
    /// The Project's field of the project is configured, and is compared with the label.
    pub project_field: bool,
    pub remaining: Option<RemainingInputs<'a>>,
    pub absolute: Option<AbsoluteInputs<'a>>,
    /// The decisions' files for the cap: `None` takes those of `absolute`.
    pub decisions: Option<&'a [(String, Option<String>)]>,
}

/// What `remaining-unreconciled` reads: the reports `(name, text)`, the decisions' files `(path, text)`, archives
/// first, and every issue the tracker has, open and closed.
#[derive(Debug, Clone, Copy)]
pub struct RemainingInputs<'a> {
    pub reports: &'a [(String, String)],
    pub decisions: &'a [(String, String)],
    pub issues: &'a BTreeMap<u64, IssueFacts>,
}

/// What `absolute-path` reads besides the state file: the reports and the decisions' files.
#[derive(Debug, Clone, Copy)]
pub struct AbsoluteInputs<'a> {
    pub reports: &'a [(String, String)],
    pub decisions: Option<&'a [(String, Option<String>)]>,
}

/// Every finding of a project (the prototype's check of a project), in the order of the checks.
pub fn check_project(inputs: &ProjectInputs, now: DateTime<Utc>, thresholds: &Thresholds) -> Vec<Finding> {
    let project = inputs.project;
    let state_file = &files().state;
    let mut out = Vec::new();
    let decisions = inputs.decisions.or_else(|| inputs.absolute.and_then(|absolute| absolute.decisions));
    if let Some(absolute) = inputs.absolute {
        out.extend(absolute_paths(inputs.state, absolute.reports, absolute.decisions, project, None));
    }
    if let Some(state) = inputs.state {
        out.extend(state_without_issue(state, &inputs.open, project, state_file, false));
        if let Some(previous) = inputs.previous_state {
            out.extend(state_lines_removed(previous, state, &inputs.open, project, state_file, inputs.only_open_removed, false));
        }
    }
    if let Some(questions) = inputs.questions {
        out.extend(open_questions(questions, project, &files().questions));
    }
    if let Some(remaining) = inputs.remaining {
        out.extend(remaining_unreconciled(remaining.reports, remaining.decisions, remaining.issues, project, None));
    }
    if let Some(comments) = inputs.comments {
        out.extend(comments_after_close(comments, project, now, thresholds.comment_window_days));
        out.extend(decision_without_flag(comments, project, now, thresholds.comment_window_days));
    }
    if let Some(issues) = inputs.issues {
        out.extend(inbox_stale(issues, project, now, thresholds.inbox_days));
        if let Some(with_report) = inputs.with_report {
            out.extend(approved_never_started(issues, project, now, thresholds.approved_days, with_report));
        }
        if let Some(leases) = inputs.live_leases {
            out.extend(in_progress_stalled(issues, project, now, thresholds.in_progress_days, leases));
        }
        if inputs.project_field {
            out.extend(project_field(issues, project));
        }
    }
    for (path, text) in decisions.unwrap_or_default() {
        if path == &files().decisions
            && let Some(text) = text
        {
            out.extend(decisions_over_cap(Some(text), project, thresholds.decisions_cap, path));
        }
    }
    // Stable: inside a check, the order read.
    out.sort_by_key(|finding| finding.check);
    out
}

/// The numbers of the tasks with a report, from the names of `.regia/report/` (`AAAA-MM-GG-<N>-<slug>.md`).
pub fn numbers_with_report<'a>(names: impl IntoIterator<Item = &'a str>) -> BTreeSet<u64> {
    names
        .into_iter()
        .filter_map(|name| pattern("report_name").captures(name).ok().flatten().and_then(|captures| captures.get(2)).and_then(|n| python_int(n.as_str())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_and_sentence_of_the_format_reads() {
        for name in compiled().format.patterns.keys().filter(|name| name.as_str() != "end") {
            let _ = pattern(name);
        }
        assert_eq!(Thresholds::defaults().decisions_cap, 30_000);
        assert_eq!(Thresholds::defaults().by_key().len(), 6);
    }

    #[test]
    fn digits_read_as_python_s_int() {
        assert_eq!(python_int("007"), Some(7));
        assert_eq!(python_int("\u{663}\u{664}"), Some(34));
        assert_eq!(python_int("\u{1d7d9}"), Some(1), "a mathematical digit, in the fourth run of its block");
        assert_eq!(python_int("\u{b2}"), None, "a superscript is a digit but not a decimal one");
        assert_eq!(python_int(""), None);
        assert_eq!(python_int("99999999999999999999999"), Some(u64::MAX));
    }

    #[test]
    fn instants_read_as_fromisoformat() {
        let at = |text: &str| instant(Some(text)).map(|instant| instant.to_rfc3339());
        assert_eq!(at("2026-01-02T03:04:05Z").as_deref(), Some("2026-01-02T03:04:05+00:00"));
        assert_eq!(at("2026-01-02T05:04:05.5+02:00").as_deref(), Some("2026-01-02T03:04:05.500+00:00"));
        assert_eq!(at("2026-01-02 03:04-01:30").as_deref(), Some("2026-01-02T04:34:00+00:00"));
        assert_eq!(at("2026-01-02T03:04:05"), None, "no offset");
        assert_eq!(at("x"), None);
    }
}
