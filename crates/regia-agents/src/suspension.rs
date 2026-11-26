//! A session the usage limit stopped (the dossier's #132, ADR 0010), recognized from its transcript with the hour it
//! may go on, as the prototype's `tools/sospese.py` reads it. Read only: nothing here resumes a session.
//!
//! - **Claude** stops with a limit message as its last assistant entry. The structured field comes first
//!   (`quotaLimits`, then `apiErrorParams.rate_limit_info`), then the text, which must be the whole message
//!   (`You've hit your session limit · resets 5:30pm (Europe/Rome)`), read in the time zone it names. The entries after
//!   it (a subagent's notification, a queue operation, a cost) do not resume it; an assistant entry that is not an
//!   error does. A message of the user after the limit, without an answer yet, is a resume under way for a while.
//! - **Codex** stops with `task_complete.error.codex_error_info == usage_limit_exceeded` and no `task_started` after.
//!   The hour comes from the last `token_count` whose primary window is full, otherwise from the text `try again at
//!   5:33 AM`, in the machine's local time.
//! - An hour that cannot be read is estimated: the budget's window lasts five hours at most.
//!
//! The words of the transcripts are the agents' own; what the controller writes of a state is its format's.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Zoned};
use regex::Regex;
use serde_json::{Map, Value};

use crate::{AgentService, Env, ServiceId, children, claude_slug};

/// What a stopped session is told to go on with: the words of Claude Code's own button.
pub const CONTINUE: &str = "Continue where you left off";

/// An hour that cannot be read: the budget's window lasts at most this long.
pub const UNKNOWN_WAIT_S: f64 = 5.0 * 3600.0;

/// A message of the user after the limit, without an answer: a resume counts as under way for this long.
pub const RESUME_GRACE_S: f64 = 600.0;

/// How much of a transcript's end is read first; four times more each time it holds no assistant entry.
const TAIL: u64 = 512 * 1024;

/// The prototype's cuts of what it keeps of a text.
const TEXT_CHARS: usize = 200;
const USER_CHARS: usize = 80;
/// A limit's text is a short message: one past this many characters only quotes it.
const LIMIT_TEXT_MAX: usize = 300;

/// The limit message as Claude Code writes it, with the hour, the day of a weekly limit and the time zone.
static LIMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?is)^\s*You['’]ve hit your [A-Za-z0-9 \-]{0,40}?limit.{0,8}?resets\s+",
        r"(?:(?P<month>[A-Za-z]{3})[a-z]*\.?\s+(?P<day>\d{1,2}),?\s+)?",
        r"(?P<hour>\d{1,2})(?::(?P<minute>\d{2}))?\s*(?P<half>am|pm)\s*\((?P<zone>[^)]+)\)"
    ))
    .expect("the pattern of a limit's text")
});

/// Any limit message, with or without an hour that reads.
static ANY_LIMIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*You['’]ve hit your [A-Za-z0-9 \-]{0,40}?limit").expect("the pattern of a limit"));

/// Codex's hour, in the machine's local time.
static CODEX_HOUR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)try again at (\d{1,2}):(\d{2})\s*(AM|PM)").expect("the pattern of codex's hour"));

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// What a transcript says of its session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Its last assistant entry is a limit message.
    Suspended,
    /// Stopped, and someone wrote after the limit a moment ago, with no answer yet.
    Resuming,
    /// Its last assistant entry is an answer.
    Active,
    /// No assistant entry at all.
    Empty,
    /// The file could not be read.
    Unreadable,
}

/// Where the hour of a limit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Claude's structured field `quotaLimits`.
    QuotaLimits,
    /// Claude's `apiErrorParams.rate_limit_info`.
    ApiErrorParams,
    /// Claude's text alone.
    Text,
    /// Codex's `task_complete.error`.
    TaskError,
}

/// The time zone an hour was read in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Zone {
    /// The IANA name the text gives.
    Named(String),
    /// The machine's own, for Codex.
    Local,
}

/// A transcript, read: its state and, for a stopped session, the limit and when it may go on.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    pub state: State,
    /// The last working folder, session and entrypoint its entries name.
    pub cwd: Option<String>,
    pub session: Option<String>,
    pub entrypoint: Option<String>,
    /// The instant of the last answer, for an active session.
    pub last_at: Option<f64>,
    /// When the session may go on, in seconds since the epoch.
    pub resets: Option<f64>,
    pub source: Option<Source>,
    pub zone: Option<Zone>,
    /// The hour was estimated, not read.
    pub estimated: Option<bool>,
    /// When the limit was written.
    pub limit_at: Option<f64>,
    /// The start of the limit's text.
    pub text: Option<String>,
    /// The start of the last message of the user after the limit.
    pub user_after: Option<String>,
    /// Why the file could not be read.
    pub error: Option<String>,
}

impl Analysis {
    fn of(state: State) -> Analysis {
        Analysis {
            state,
            cwd: None,
            session: None,
            entrypoint: None,
            last_at: None,
            resets: None,
            source: None,
            zone: None,
            estimated: None,
            limit_at: None,
            text: None,
            user_after: None,
            error: None,
        }
    }

    /// Stopped, also with a resume under way.
    #[must_use]
    pub fn stopped(&self) -> bool {
        matches!(self.state, State::Suspended | State::Resuming)
    }
}

/// The machine's offset from UTC at an instant, in seconds east: what a local hour is read with.
pub type LocalOffset = fn(f64) -> Option<i32>;

/// This machine's offset at `at`.
#[must_use]
pub fn system_offset(at: f64) -> Option<i32> {
    use chrono::{Offset as _, TimeZone as _};
    let micros = seconds_to_micros(at)?;
    let utc = chrono::DateTime::from_timestamp_micros(micros)?;
    Some(chrono::Local.offset_from_utc_datetime(&utc.naive_utc()).fix().local_minus_utc())
}

/// How a transcript is read: at `now`, with the grace of a resume under way and the machine's offset.
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    pub now: f64,
    pub grace_s: f64,
    pub offset: LocalOffset,
}

impl Reading {
    /// At `now`, with the prototype's grace and this machine's offset.
    #[must_use]
    pub fn at(now: f64) -> Reading {
        Reading { now, grace_s: RESUME_GRACE_S, offset: system_offset }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn seconds_to_micros(at: f64) -> Option<i64> {
    let micros = (at * 1e6).round();
    (micros.is_finite() && micros.abs() < 9.0e18).then_some(micros as i64)
}

/// An instant as the prototype writes it: UTC, to the second, the second truncated.
#[must_use]
pub fn iso(at: f64) -> Option<String> {
    let micros = seconds_to_micros(at)?;
    let utc = chrono::DateTime::from_timestamp_micros(micros)?;
    Some(utc.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// The seconds of an ISO instant (`...Z` included); a naive one is local time, as Python reads it.
#[must_use]
pub fn epoch(value: Option<&Value>) -> Option<f64> {
    let text = value?.as_str()?;
    let at = match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(at) => at.with_timezone(&chrono::Utc),
        Err(_) => {
            use chrono::TimeZone as _;
            let naive = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
            chrono::Local.from_local_datetime(&naive).earliest()?.with_timezone(&chrono::Utc)
        }
    };
    #[allow(clippy::cast_precision_loss)]
    Some(at.timestamp_micros() as f64 / 1e6)
}

/// A number of seconds, or of milliseconds past 10¹¹, as some fields carry them; nothing else, not a boolean.
fn epoch_seconds(value: Option<&Value>) -> Option<f64> {
    let number = value?.as_number()?.as_f64()?;
    Some(if number > 1e11 { number / 1000.0 } else { number })
}

fn cut(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

fn string(entry: &Map<String, Value>, key: &str) -> Option<String> {
    entry.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// The text of a Claude entry: its content when it is a string, else its `text` blocks joined.
fn text_of(entry: &Map<String, Value>) -> String {
    let content = entry.get("message").and_then(Value::as_object).and_then(|message| message.get("content"));
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(Value::as_object)
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

/// The JSON entries at the end of a file, and whether the read reached its start. A line that is not JSON (the one the
/// read cut) is skipped.
fn tail_entries(path: &Path, part: u64) -> std::io::Result<(Vec<Map<String, Value>>, bool)> {
    let mut file = File::open(path)?;
    let end = file.seek(SeekFrom::End(0))?;
    let start = end.saturating_sub(part);
    file.seek(SeekFrom::Start(start))?;
    let mut raw = Vec::new();
    file.read_to_end(&mut raw)?;
    if start > 0
        && let Some(at) = raw.iter().position(|byte| *byte == b'\n')
    {
        raw.drain(..=at);
    }
    let entries = raw
        .split(|byte| *byte == b'\n' || *byte == b'\r')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter_map(|value| match value {
            Value::Object(entry) => Some(entry),
            _ => None,
        })
        .collect();
    Ok((entries, start == 0))
}

/// The hour a limit's text gives, read in its time zone: the first such hour after `reference`, so `2am` written at
/// 23:00 is the next day, and a weekly limit's day without a year is the next one.
#[derive(Debug, Clone, PartialEq)]
pub struct TextReset {
    pub at: f64,
    pub zone: String,
}

/// The prototype's reading of a limit's hour in the text: none when the text has not that shape, or the zone is
/// unknown.
#[must_use]
pub fn reset_from_text(text: &str, reference: Option<f64>) -> Option<TextReset> {
    let captures = LIMIT.captures(text)?;
    let reference = reference?;
    let name = captures.name("zone")?.as_str().trim();
    let zone = TimeZone::get(name).ok()?;
    let pm = captures.name("half")?.as_str().eq_ignore_ascii_case("pm");
    let hour = captures.name("hour")?.as_str().parse::<u32>().ok()? % 12 + if pm { 12 } else { 0 };
    let minute = captures.name("minute").map_or(Some(0), |minute| minute.as_str().parse::<u32>().ok())?;
    let (hour, minute) = (i8::try_from(hour).ok()?, i8::try_from(minute).ok()?);
    let now = Timestamp::from_microsecond(seconds_to_micros(reference)?).ok()?.to_zoned(zone.clone());
    let at = |year: i16, month: i8, day: i8| -> Option<Zoned> { DateTime::new(year, month, day, hour, minute, 0, 0).ok()?.to_zoned(zone.clone()).ok() };
    let candidate = if let Some(month) = captures.name("month") {
        let month = MONTHS.iter().position(|name| *name == month.as_str().to_lowercase())?;
        let (month, day) = (i8::try_from(month + 1).ok()?, captures.name("day")?.as_str().parse::<i8>().ok()?);
        let candidate = at(now.year(), month, day)?;
        if candidate.timestamp() <= now.timestamp() { at(now.year().checked_add(1)?, month, day)? } else { candidate }
    } else {
        let candidate = at(now.year(), now.month(), now.day())?;
        if candidate.timestamp() <= now.timestamp() { candidate.checked_add(Span::new().days(1)).ok()? } else { candidate }
    };
    #[allow(clippy::cast_precision_loss)]
    let seconds = candidate.timestamp().as_second() as f64;
    Some(TextReset { at: seconds, zone: name.to_owned() })
}

/// The limit of a Claude assistant entry, and its hour when a field gives it: the structured fields first, then a
/// short message that is all a limit.
fn claude_limit(entry: &Map<String, Value>) -> Option<(Source, Option<f64>)> {
    let rejected = |field: Option<&Value>| field.and_then(Value::as_object).filter(|field| field.get("status").and_then(Value::as_str) == Some("rejected")).cloned();
    if let Some(quota) = rejected(entry.get("quotaLimits")) {
        return Some((Source::QuotaLimits, epoch_seconds(quota.get("resetsAt"))));
    }
    let params = entry.get("apiErrorParams").and_then(Value::as_object);
    if let Some(info) = rejected(params.and_then(|params| params.get("rate_limit_info"))) {
        return Some((Source::ApiErrorParams, epoch_seconds(info.get("resetsAt"))));
    }
    let text = text_of(entry);
    let model = entry.get("message").and_then(Value::as_object).and_then(|message| message.get("model"));
    let synthetic = match model {
        None | Some(Value::Null) => true,
        Some(Value::String(model)) => model == "<synthetic>",
        Some(_) => false,
    };
    (text.chars().count() < LIMIT_TEXT_MAX && synthetic && ANY_LIMIT.is_match(&text)).then_some((Source::Text, None))
}

fn claude(entries: &[Map<String, Value>], reading: &Reading) -> Analysis {
    let last = |key: &str| entries.iter().rev().find_map(|entry| string(entry, key));
    let mut base = Analysis::of(State::Empty);
    base.cwd = last("cwd");
    base.session = last("sessionId");
    base.entrypoint = last("entrypoint");
    let assistant = entries.iter().rposition(|entry| {
        entry.get("type").and_then(Value::as_str) == Some("assistant") && entry.get("message").is_some_and(Value::is_object)
    });
    let Some(at) = assistant else { return base };
    let entry = &entries[at];
    let Some((source, structured)) = claude_limit(entry) else {
        return Analysis { state: State::Active, last_at: epoch(entry.get("timestamp")), ..base };
    };
    let limit_at = epoch(entry.get("timestamp"));
    let text = text_of(entry);
    let (mut resets, mut zone) = (structured, None);
    if resets.is_none() {
        if let Some(found) = reset_from_text(&text, limit_at) {
            resets = Some(found.at);
            zone = Some(Zone::Named(found.zone));
        }
    } else if let Some(name) = LIMIT.captures(&text).and_then(|captures| captures.name("zone").map(|zone| zone.as_str().trim().to_owned())) {
        zone = Some(Zone::Named(name));
    }
    let estimated = resets.is_none();
    if estimated && let Some(limit_at) = limit_at {
        resets = Some(limit_at + UNKNOWN_WAIT_S);
    }
    let mut analysis = Analysis {
        state: State::Suspended,
        resets,
        source: Some(source),
        zone,
        estimated: Some(estimated),
        limit_at,
        text: Some(cut(text.trim(), TEXT_CHARS)),
        ..base
    };
    for later in &entries[at + 1..] {
        if later.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let said = text_of(later);
        // The automatic notices start with `<`: they are no message of the user.
        if said.trim_start().starts_with('<') || said.trim().is_empty() {
            continue;
        }
        if let Some(written) = epoch(later.get("timestamp"))
            && reading.now - written < reading.grace_s
        {
            analysis.state = State::Resuming;
        }
        analysis.user_after = Some(cut(said.trim(), USER_CHARS));
    }
    analysis
}

/// Python's `str` of a field of Codex's error: its text, `None` for a null.
fn codex_message(error: &Map<String, Value>) -> String {
    match error.get("message") {
        None => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) => "None".to_owned(),
        Some(other) => other.to_string(),
    }
}

/// The hour of `try again at H:MM AM` after the limit, in the machine's offset at the limit.
fn codex_local_hour(message: &str, limit_at: f64, offset: LocalOffset) -> Option<f64> {
    let captures = CODEX_HOUR.captures(message)?;
    let pm = captures[3].eq_ignore_ascii_case("pm");
    let hour = captures[1].parse::<u32>().ok()? % 12 + if pm { 12 } else { 0 };
    let minute = captures[2].parse::<u32>().ok().filter(|minute| *minute < 60)?;
    let offset = f64::from(offset(limit_at)?);
    let day = ((limit_at + offset) / 86_400.0).floor() * 86_400.0;
    let candidate = day + f64::from(hour * 3600 + minute * 60) - offset;
    Some(if candidate > limit_at { candidate } else { candidate + 86_400.0 })
}

fn codex(entries: &[Map<String, Value>], reading: &Reading) -> Analysis {
    let mut base = Analysis::of(State::Empty);
    base.cwd = entries
        .iter()
        .rev()
        .find(|entry| entry.get("type").and_then(Value::as_str) == Some("turn_context"))
        .and_then(|entry| entry.get("payload").and_then(Value::as_object).and_then(|payload| string(payload, "cwd")));
    let (mut end, mut resets) = (None, None);
    for (at, entry) in entries.iter().enumerate() {
        let payload = entry.get("payload").and_then(Value::as_object);
        let kind = entry.get("type").and_then(Value::as_str);
        if kind == Some("session_meta") && base.session.is_none() {
            base.session = payload.and_then(|payload| string(payload, "id"));
        }
        if kind != Some("event_msg") {
            continue;
        }
        match payload.and_then(|payload| payload.get("type")).and_then(Value::as_str) {
            Some("token_count") => {
                let limits = payload.and_then(|payload| payload.get("rate_limits")).and_then(Value::as_object);
                let primary = limits.and_then(|limits| limits.get("primary")).and_then(Value::as_object);
                if let Some(primary) = primary
                    && primary.get("used_percent").and_then(Value::as_f64).unwrap_or(0.0) >= 100.0
                {
                    resets = epoch_seconds(primary.get("resets_at"));
                }
            }
            Some("task_started") => (end, resets) = (None, None),
            Some("task_complete") => {
                let error = payload.and_then(|payload| payload.get("error")).and_then(Value::as_object);
                match error.filter(|error| error.get("codex_error_info").and_then(Value::as_str) == Some("usage_limit_exceeded")) {
                    Some(error) => end = Some((at, error.clone())),
                    None => (end, resets) = (None, None),
                }
            }
            _ => {}
        }
    }
    let Some((at, error)) = end else {
        return Analysis { state: if entries.is_empty() { State::Empty } else { State::Active }, ..base };
    };
    let limit_at = epoch(entries[at].get("timestamp"));
    let message = codex_message(&error);
    let mut zone = None;
    if resets.is_none()
        && let Some(limit_at) = limit_at
        && let Some(hour) = codex_local_hour(&message, limit_at, reading.offset)
    {
        resets = Some(hour);
        zone = Some(Zone::Local);
    }
    let estimated = resets.is_none();
    if estimated && let Some(limit_at) = limit_at {
        resets = Some(limit_at + UNKNOWN_WAIT_S);
    }
    Analysis {
        state: State::Suspended,
        resets,
        source: Some(Source::TaskError),
        zone,
        estimated: Some(estimated),
        limit_at,
        text: Some(cut(&message, TEXT_CHARS)),
        ..base
    }
}

/// The state of a transcript of `service`. A session is stopped when its **last assistant entry** is a limit message;
/// the entries after it do not resume it. Without any assistant entry in the end read, more of the file is read.
#[must_use]
pub fn analyze(path: &Path, service: ServiceId, reading: &Reading) -> Analysis {
    let mut part = TAIL;
    loop {
        let (entries, whole) = match tail_entries(path, part) {
            Ok(read) => read,
            Err(error) => return Analysis { error: Some(error.to_string()), ..Analysis::of(State::Unreadable) },
        };
        let analysis = match service {
            ServiceId::Codex => codex(&entries, reading),
            ServiceId::Claude => claude(&entries, reading),
        };
        if analysis.state != State::Empty || whole {
            return analysis;
        }
        part = part.saturating_mul(4);
    }
}

/// A subagent of a Claude session that the limit stopped. Informative: a subagent's limit does not stop its parent,
/// which sees the notice and decides.
#[derive(Debug, Clone, PartialEq)]
pub struct Subagent {
    pub id: String,
    pub resets: Option<f64>,
    pub limit_at: Option<f64>,
    pub transcript: PathBuf,
}

/// The subagents of a Claude session (`<session>/subagents/agent-*.jsonl`) stopped by the limit.
#[must_use]
pub fn suspended_subagents(transcript: &Path, reading: &Reading) -> Vec<Subagent> {
    let folder = transcript.with_extension("").join("subagents");
    let mut found = Vec::new();
    for file in jsonl_files(&folder) {
        let analysis = analyze(&file, ServiceId::Claude, reading);
        if analysis.state == State::Suspended {
            let stem = file.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let id = stem.strip_prefix("agent-").unwrap_or(&stem).to_owned();
            found.push(Subagent { id, resets: analysis.resets, limit_at: analysis.limit_at, transcript: file });
        }
    }
    found
}

/// The `.jsonl` files right in `folder`, sorted.
fn jsonl_files(folder: &Path) -> Vec<PathBuf> {
    children(folder).into_iter().filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "jsonl")).collect()
}

/// The modification time of a file, in seconds since the epoch.
#[must_use]
pub fn modified(path: &Path) -> Option<f64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let since = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(since.as_secs_f64())
}

/// Claude's folder of the transcripts of every working folder: `<its home>/projects`.
fn claude_projects(env: &Env) -> Option<PathBuf> {
    AgentService::known(ServiceId::Claude).home(env).map(|home| home.join("projects"))
}

/// A session of a scan: a stopped one, or one with a stopped subagent.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub session: String,
    pub transcript: PathBuf,
    /// The folder of the scan it belongs to.
    pub folder: String,
    pub analysis: Analysis,
    /// The id of its chat, when an app shows it as one.
    pub chat: Option<String>,
    pub subagents: Vec<Subagent>,
    /// Its hour has passed.
    pub due: bool,
}

/// Claude's folders of transcripts for `folders`: each one's own and those of the worktrees nested in it (its name
/// followed by `-`). With `exact`, its own only.
fn transcript_folders(env: &Env, folders: &[String], exact: bool) -> Vec<(PathBuf, String)> {
    let Some(projects) = claude_projects(env) else { return Vec::new() };
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let all = children(&projects);
    for folder in folders {
        let slug = claude_slug(Path::new(folder));
        for dir in all.iter().filter(|dir| dir.is_dir()) {
            let name = dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let nested = !exact && name.starts_with(&format!("{slug}-"));
            if name == slug || nested {
                // A later folder of the scan takes a transcript folder an earlier one took, keeping its place.
                match out.iter_mut().find(|(known, _)| known == dir) {
                    Some(entry) => entry.1.clone_from(folder),
                    None => out.push((dir.clone(), folder.clone())),
                }
            }
        }
    }
    out
}

/// The prototype's `scansiona`, read only: the stopped sessions and those with a stopped subagent in the transcripts of
/// `folders` changed within `days`, by the hour they may go on. `chats` names the ones an app shows as chats.
#[must_use]
pub fn scan(env: &Env, folders: &[String], days: f64, reading: &Reading, chats: &BTreeMap<String, Option<String>>, exact: bool) -> Vec<Found> {
    let limit = reading.now - days * 86_400.0;
    let mut found = Vec::new();
    for (dir, folder) in transcript_folders(env, folders, exact) {
        for file in jsonl_files(&dir) {
            if modified(&file).is_none_or(|at| at < limit) {
                continue;
            }
            let analysis = analyze(&file, ServiceId::Claude, reading);
            let subagents = suspended_subagents(&file, reading);
            if !analysis.stopped() && subagents.is_empty() {
                continue;
            }
            let session = file.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let due = analysis.resets.is_some_and(|resets| resets != 0.0 && resets <= reading.now);
            found.push(Found { chat: chats.get(&session).cloned().flatten(), session, transcript: file, folder: folder.clone(), analysis, subagents, due });
        }
    }
    let order = |found: &Found| (found.analysis.resets.and_then(iso).unwrap_or_else(|| "~".to_owned()), found.session.clone());
    found.sort_by_key(order);
    found
}

/// A Claude session the owner allowed to resume besides the regia's steps: by its id, in any folder, or by its folder,
/// changed within the days of the search.
#[derive(Debug, Clone, PartialEq)]
pub struct OptedIn {
    pub session: String,
    pub transcript: PathBuf,
    /// Its working folder: the transcript's, or the one of the opt-in it was found by.
    pub cwd: String,
    pub modified: f64,
}

/// The prototype's `scopri_optin`: the sessions `sessions` names, and those of the folders `folders` changed within
/// `days` of `now`. Nothing with both empty.
#[must_use]
pub fn opted_in(env: &Env, sessions: &[String], folders: &[String], days: f64, now: f64) -> Vec<OptedIn> {
    if sessions.is_empty() && folders.is_empty() {
        return Vec::new();
    }
    let Some(projects) = claude_projects(env) else { return Vec::new() };
    let slugs: Vec<String> = folders.iter().map(|folder| claude_slug(Path::new(folder))).collect();
    let limit = now - days * 86_400.0;
    let mut found = Vec::new();
    for dir in children(&projects).into_iter().filter(|dir| dir.is_dir()) {
        let name = dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let by_folder = slugs.iter().position(|slug| *slug == name);
        for file in jsonl_files(&dir) {
            let session = file.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let changed = modified(&file);
            if !(sessions.contains(&session) || (by_folder.is_some() && changed.is_some_and(|at| at >= limit))) {
                continue;
            }
            let analysis = analyze(&file, ServiceId::Claude, &Reading::at(now));
            let Some(cwd) = analysis.cwd.filter(|cwd| !cwd.is_empty()).or_else(|| by_folder.map(|at| folders[at].clone())) else { continue };
            found.push(OptedIn { session, transcript: file, cwd, modified: changed.unwrap_or(0.0) });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_hour_in_milliseconds_reads_as_seconds_and_a_boolean_is_no_hour() {
        assert_eq!(epoch_seconds(Some(&serde_json::json!(1_792_616_400_000_u64))), Some(1_792_616_400.0));
        assert_eq!(epoch_seconds(Some(&serde_json::json!(1_792_616_400))), Some(1_792_616_400.0));
        assert_eq!(epoch_seconds(Some(&serde_json::json!(true))), None);
        assert_eq!(epoch_seconds(Some(&serde_json::json!("1792616400"))), None);
        assert_eq!(epoch(Some(&serde_json::json!("2026-10-21T21:00:00.000Z"))), Some(1_792_616_400.0));
        assert_eq!(iso(1_792_616_400.9).as_deref(), Some("2026-10-21T21:00:00Z"));
    }

    #[test]
    fn codex_s_local_hour_keeps_the_offset_of_the_limit() {
        let plus_one: LocalOffset = |_| Some(3600);
        // 04:00Z is 05:00 at +01:00: 5:30 AM is half an hour later; 4:30 AM is the next day's.
        let limit = 1_792_555_200.0;
        assert_eq!(codex_local_hour("try again at 5:30 AM.", limit, plus_one), Some(limit + 1800.0));
        assert_eq!(codex_local_hour("Try again at 4:30 am", limit, plus_one), Some(limit + 86_400.0 - 1800.0));
        assert_eq!(codex_local_hour("try again at 5:75 AM", limit, plus_one), None);
        assert_eq!(codex_local_hour("try again later", limit, plus_one), None);
    }
}
