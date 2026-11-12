//! The budget policy, as the prototype's `tools/politica_budget.py` keeps it (#141 and #156 of the
//! dossier): the thresholds and caps of `policies/budget.yaml`, whose values are also the product's
//! defaults, compiled in, and the machine's local override, which changes some of them for a while. Both
//! files are shared with the prototype while the two coexist, so their names are its names, from
//! `formats/policy_v1.toml`, and the rules are its rules: a policy file that does not read gives the
//! defaults and says why; an override that does not read, has expired or does not fit the file is not
//! applied, and says so.
//!
//! Nothing here reads a file or a clock: the caller passes what it read, the instant and the time zone.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value, json};

use crate::pystr;

const FORMAT: &str = include_str!("../formats/policy_v1.toml");
/// The product's copy of the prototype's file: the defaults.
const DEFAULTS: &str = include_str!("../formats/budget.yaml");
/// How much of a reason is kept, in characters, as the prototype cuts it.
const REASON_CHARS: usize = 120;

#[derive(Debug, Deserialize)]
struct Format {
    file: FileNames,
    switch: SwitchWords,
    local: LocalNames,
    levers: LeverNames,
}

#[derive(Debug, Deserialize)]
struct FileNames {
    path: String,
    automatic: (String, String),
    backfill: (String, String),
    open_per_project: (String, String),
    new_per_week: (String, String),
    drain: (String, String),
}

#[derive(Debug, Deserialize)]
struct SwitchWords {
    on: Vec<String>,
    off: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LocalNames {
    file: String,
    version: String,
    values: String,
    until: String,
    note: String,
    set_at: String,
}

#[derive(Debug, Deserialize)]
struct LeverNames {
    automatic_max_pct: String,
    backfill_max_pct: String,
    open_per_project: String,
    new_per_week: String,
    proposal_caps: String,
    backfill: String,
}

fn format() -> &'static Format {
    static PARSED: OnceLock<Format> = OnceLock::new();
    PARSED.get_or_init(|| toml::from_str(FORMAT).expect("formats/policy_v1.toml"))
}

/// The policy file, relative to the dossier.
pub fn file_path() -> &'static str {
    &format().file.path
}

/// The local override's file name, in the machine's state folder.
pub fn local_file_name() -> &'static str {
    &format().local.file
}

/// The window of the cap on new Proposals.
pub fn week() -> Duration {
    Duration::days(7)
}

/// Why a file, a value or an argument of the policy is refused. The names in it are the files' own.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("line {0}: a tab")]
    Tab(usize),
    #[error("line {0}: no `key: value`")]
    NotKeyValue(usize),
    #[error("line {line}: section `{name}` with a value, or repeated")]
    Section { line: usize, name: String },
    #[error("line {line}: key `{name}` outside a section, or repeated")]
    Key { line: usize, name: String },
    #[error("`{0}` is missing")]
    Missing(String),
    #[error("`{name}` is not a number: '{value}'")]
    NotANumber { name: String, value: String },
    #[error("`{name}` is not a whole number: '{value}'")]
    NotWhole { name: String, value: String },
    #[error("`{name}` is out of range: {value}")]
    OutOfRange { name: String, value: String },
    #[error("`{name}` is on or off: '{value}'")]
    NotASwitch { name: String, value: String },
    #[error("the backfill threshold is above the automatic work's")]
    BackfillAboveAutomatic,
    #[error("the file is not UTF-8")]
    NotUtf8,
    #[error("the file does not read: {0}")]
    Unreadable(String),
    #[error("not JSON: {0}")]
    NotJson(String),
    #[error("the file is not a JSON object")]
    NotAnObject,
    #[error("`{0}` is not an object")]
    ValuesNotAnObject(String),
    #[error("unknown key: `{key}` (allowed: {allowed})")]
    UnknownKey { key: String, allowed: String },
    #[error("expected key=value: '{0}'")]
    NotAPair(String),
    #[error("`{name}` is not an instant: '{value}'")]
    NotAnInstant { name: String, value: String },
    #[error("--until is already past: {0}")]
    UntilPassed(String),
}

/// A value of the policy that the local override can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lever {
    AutomaticMaxPct,
    BackfillMaxPct,
    OpenPerProject,
    NewPerWeek,
    ProposalCaps,
    Backfill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pct,
    Count,
    Switch,
}

impl Lever {
    /// The prototype's order: the numbers, then the switches.
    pub const ALL: [Lever; 6] =
        [Lever::AutomaticMaxPct, Lever::BackfillMaxPct, Lever::OpenPerProject, Lever::NewPerWeek, Lever::ProposalCaps, Lever::Backfill];

    /// The contract's name, in `policy set` and in the JSON.
    pub fn id(self) -> &'static str {
        match self {
            Lever::AutomaticMaxPct => "automatic_max_pct",
            Lever::BackfillMaxPct => "backfill_max_pct",
            Lever::OpenPerProject => "open_per_project",
            Lever::NewPerWeek => "new_per_week",
            Lever::ProposalCaps => "proposal_caps",
            Lever::Backfill => "backfill",
        }
    }

    pub fn parse(id: &str) -> Option<Lever> {
        Lever::ALL.into_iter().find(|lever| lever.id() == id)
    }

    /// The name in the override's file.
    fn file_key(self) -> &'static str {
        let names = &format().levers;
        match self {
            Lever::AutomaticMaxPct => &names.automatic_max_pct,
            Lever::BackfillMaxPct => &names.backfill_max_pct,
            Lever::OpenPerProject => &names.open_per_project,
            Lever::NewPerWeek => &names.new_per_week,
            Lever::ProposalCaps => &names.proposal_caps,
            Lever::Backfill => &names.backfill,
        }
    }

    fn of_file_key(key: &str) -> Option<Lever> {
        Lever::ALL.into_iter().find(|lever| lever.file_key() == key)
    }

    fn kind(self) -> Kind {
        match self {
            Lever::AutomaticMaxPct | Lever::BackfillMaxPct => Kind::Pct,
            Lever::OpenPerProject | Lever::NewPerWeek => Kind::Count,
            Lever::ProposalCaps | Lever::Backfill => Kind::Switch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LeverValue {
    /// A share of Claude's 5-hour window, above 0 and at most 100.
    Pct(f64),
    /// A cap, at least 1.
    Count(u64),
    Switch(bool),
}

impl LeverValue {
    fn to_json(self) -> Value {
        match self {
            LeverValue::Pct(pct) => json!(pct),
            LeverValue::Count(count) => json!(count),
            LeverValue::Switch(on) => json!(on),
        }
    }
}

/// The values of an override, in the order each was first set: the prototype's `{**old, **new}`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values(Vec<(Lever, LeverValue)>);

impl Values {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Lever, LeverValue)> + '_ {
        self.0.iter().copied()
    }

    pub fn get(&self, lever: Lever) -> Option<LeverValue> {
        self.iter().find(|(key, _)| *key == lever).map(|(_, value)| value)
    }

    /// Sets `lever`: in its place if it is there, else at the end.
    pub fn set(&mut self, lever: Lever, value: LeverValue) {
        match self.0.iter_mut().find(|(key, _)| *key == lever) {
            Some(slot) => slot.1 = value,
            None => self.0.push((lever, value)),
        }
    }

    /// These values, then `newer`'s over them.
    #[must_use]
    pub fn merged(&self, newer: &Values) -> Values {
        let mut out = self.clone();
        for (lever, value) in newer.iter() {
            out.set(lever, value);
        }
        out
    }

    /// With the contract's names.
    pub fn to_json(&self) -> Value {
        Value::Object(self.iter().map(|(lever, value)| (lever.id().to_owned(), value.to_json())).collect())
    }
}

/// Where the values come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The policy file.
    File,
    /// The product's defaults: no dossier, or a file that does not read.
    Default,
}

/// The local override, as it was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalStatus {
    Active,
    Expired,
    Unreadable,
    /// Read and in time, but its values do not fit the file: the file's apply.
    Ignored,
}

impl LocalStatus {
    pub fn id(self) -> &'static str {
        match self {
            LocalStatus::Active => "active",
            LocalStatus::Expired => "expired",
            LocalStatus::Unreadable => "unreadable",
            LocalStatus::Ignored => "ignored",
        }
    }
}

/// What is known of the local override. `until`, `note` and `set_at` are the file's, whatever their type:
/// data.
#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    pub status: LocalStatus,
    pub until: Option<Value>,
    pub values: Option<Values>,
    pub note: Option<Value>,
    pub set_at: Option<Value>,
    pub reason: Option<String>,
}

impl Local {
    fn unreadable(error: &PolicyError) -> Local {
        Local { status: LocalStatus::Unreadable, until: None, values: None, note: None, set_at: None, reason: Some(cut(&error.to_string())) }
    }

    /// With the contract's names; only the keys the prototype gives for this status.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("status".to_owned(), json!(self.status.id()));
        for (key, value) in [("until", &self.until), ("note", &self.note), ("set_at", &self.set_at)] {
            if let Some(value) = value {
                out.insert(key.to_owned(), value.clone());
            }
        }
        if let Some(values) = &self.values {
            out.insert("values".to_owned(), values.to_json());
        }
        if let Some(reason) = &self.reason {
            out.insert("reason".to_owned(), json!(reason));
        }
        Value::Object(out)
    }
}

/// The policy in use.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Above this share of Claude's 5-hour window no automatic work starts.
    pub automatic_max_pct: f64,
    /// The lower threshold of backfill: steps started only by new ideas in Inbox or by the backlog.
    pub backfill_max_pct: f64,
    pub open_per_project: u64,
    pub new_per_week: u64,
    /// Off, no cap of Proposals holds.
    pub proposal_caps: bool,
    /// Off, backfill has no threshold of its own: the automatic work's applies.
    pub backfill: bool,
    /// The backlog to drain; only the file sets it.
    pub drain: bool,
    pub origin: Origin,
    /// Whether an override applies.
    pub with_local: bool,
    /// Why the file did not read, when the defaults apply in its place.
    pub reason: Option<String>,
    pub local: Option<Local>,
}

impl Policy {
    /// The product's defaults: the values of its copy of the prototype's file.
    pub fn defaults() -> Policy {
        Policy { origin: Origin::Default, ..parse_file(DEFAULTS).expect("formats/budget.yaml is a valid policy file") }
    }

    /// The policy file as it was read: its bytes, or why it could not be read. One that does not read
    /// gives the defaults, with the reason.
    pub fn from_file(read: Result<Vec<u8>, std::io::Error>) -> Policy {
        let parsed = match read {
            Ok(bytes) => String::from_utf8(bytes).map_err(|_| PolicyError::NotUtf8).and_then(|text| parse_file(&text)),
            Err(error) => Err(PolicyError::Unreadable(error.kind().to_string())),
        };
        parsed.unwrap_or_else(|error| Policy { reason: Some(cut(&error.to_string())), ..Policy::defaults() })
    }

    /// `file` or `default`, with `+local` when an override applies.
    pub fn source(&self) -> &'static str {
        match (self.origin, self.with_local) {
            (Origin::File, false) => "file",
            (Origin::File, true) => "file+local",
            (Origin::Default, false) => "default",
            (Origin::Default, true) => "default+local",
        }
    }

    /// The policy with `values` over it, checked as a whole: the prototype's `_applica`.
    ///
    /// # Errors
    ///
    /// [`PolicyError::BackfillAboveAutomatic`].
    pub fn with(&self, values: &Values) -> Result<Policy, PolicyError> {
        let mut policy = self.clone();
        for (lever, value) in values.iter() {
            match (lever, value) {
                (Lever::AutomaticMaxPct, LeverValue::Pct(pct)) => policy.automatic_max_pct = pct,
                (Lever::BackfillMaxPct, LeverValue::Pct(pct)) => policy.backfill_max_pct = pct,
                (Lever::OpenPerProject, LeverValue::Count(count)) => policy.open_per_project = count,
                (Lever::NewPerWeek, LeverValue::Count(count)) => policy.new_per_week = count,
                (Lever::ProposalCaps, LeverValue::Switch(on)) => policy.proposal_caps = on,
                (Lever::Backfill, LeverValue::Switch(on)) => policy.backfill = on,
                _ => {}
            }
        }
        if !policy.backfill {
            policy.backfill_max_pct = policy.automatic_max_pct;
        }
        if policy.backfill_max_pct > policy.automatic_max_pct {
            return Err(PolicyError::BackfillAboveAutomatic);
        }
        Ok(policy)
    }

    /// The policy with the local override as it was read: the prototype's `carica`. Values that do not
    /// fit the file leave the file's, and the override says it was ignored.
    #[must_use]
    pub fn with_local(mut self, (values, local): (Values, Option<Local>)) -> Policy {
        self.local = local;
        if values.is_empty() {
            return self;
        }
        match self.with(&values) {
            Ok(applied) => Policy { with_local: true, ..applied },
            Err(error) => {
                if let Some(local) = &mut self.local {
                    local.status = LocalStatus::Ignored;
                    local.reason = Some(cut(&error.to_string()));
                }
                self
            }
        }
    }

    /// With the contract's names.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("automatic_max_pct".to_owned(), json!(self.automatic_max_pct));
        out.insert("backfill_max_pct".to_owned(), json!(self.backfill_max_pct));
        out.insert("open_per_project".to_owned(), json!(self.open_per_project));
        out.insert("new_per_week".to_owned(), json!(self.new_per_week));
        out.insert("proposal_caps".to_owned(), json!(self.proposal_caps));
        out.insert("backfill".to_owned(), json!(self.backfill));
        out.insert("drain".to_owned(), json!(self.drain));
        out.insert("source".to_owned(), json!(self.source()));
        out.insert("reason".to_owned(), json!(self.reason));
        if let Some(local) = &self.local {
            out.insert("local".to_owned(), local.to_json());
        }
        Value::Object(out)
    }
}

/// The prototype's `[:120]`, in characters.
fn cut(text: &str) -> String {
    text.chars().take(REASON_CHARS).collect()
}

// ---------- the policy file ----------

type Sections = BTreeMap<String, BTreeMap<String, String>>;

/// `{section: {key: text}}` from the two-level format: a section, then its indented keys.
fn sections(text: &str) -> Result<Sections, PolicyError> {
    let mut sections = Sections::new();
    let mut current: Option<String> = None;
    for (index, line) in pystr::split_lines(text).into_iter().enumerate() {
        let number = index + 1;
        let line = pystr::rstrip(line.split('#').next().unwrap_or_default());
        if pystr::strip(line).is_empty() {
            continue;
        }
        if line.contains('\t') {
            return Err(PolicyError::Tab(number));
        }
        let Some((key, value)) = pystr::strip(line).split_once(':') else {
            return Err(PolicyError::NotKeyValue(number));
        };
        let (key, value) = (pystr::strip(key), pystr::strip(value));
        if key.is_empty() {
            return Err(PolicyError::NotKeyValue(number));
        }
        if !line.starts_with(pystr::is_space) {
            if !value.is_empty() || sections.contains_key(key) {
                return Err(PolicyError::Section { line: number, name: key.to_owned() });
            }
            sections.insert(key.to_owned(), BTreeMap::new());
            current = Some(key.to_owned());
            continue;
        }
        match current.as_ref().and_then(|name| sections.get_mut(name)) {
            Some(keys) if !keys.contains_key(key) => {
                keys.insert(key.to_owned(), value.to_owned());
            }
            _ => return Err(PolicyError::Key { line: number, name: key.to_owned() }),
        }
    }
    Ok(sections)
}

/// The prototype's `^\d+(\.\d+)?$`.
fn is_number(text: &str) -> bool {
    let (whole, fraction) = text.split_once('.').map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    digits(whole) && fraction.is_none_or(digits)
}

fn entry<'a>(sections: &'a Sections, (section, key): &(String, String)) -> Result<(&'a str, String), PolicyError> {
    let name = format!("{section}.{key}");
    match sections.get(section).and_then(|keys| keys.get(key)) {
        Some(text) => Ok((text, name)),
        None => Err(PolicyError::Missing(name)),
    }
}

fn file_pct(sections: &Sections, key: &(String, String)) -> Result<f64, PolicyError> {
    let (text, name) = entry(sections, key)?;
    if !is_number(text) {
        return Err(PolicyError::NotANumber { name, value: text.to_owned() });
    }
    let pct: f64 = text.parse().map_err(|_| PolicyError::NotANumber { name: name.clone(), value: text.to_owned() })?;
    if !(0.0 < pct && pct <= 100.0) {
        return Err(PolicyError::OutOfRange { name, value: text.to_owned() });
    }
    Ok(pct)
}

fn file_count(sections: &Sections, key: &(String, String)) -> Result<u64, PolicyError> {
    let (text, name) = entry(sections, key)?;
    if !is_number(text) || text.contains('.') {
        return Err(PolicyError::NotWhole { name, value: text.to_owned() });
    }
    match text.parse::<u64>() {
        Ok(count) if count >= 1 => Ok(count),
        _ => Err(PolicyError::OutOfRange { name, value: text.to_owned() }),
    }
}

/// A switch word, in lowercase: `None` if it is neither on nor off.
fn switch_word(text: &str) -> Option<bool> {
    let word = text.to_lowercase();
    let words = &format().switch;
    if words.on.contains(&word) {
        Some(true)
    } else if words.off.contains(&word) {
        Some(false)
    } else {
        None
    }
}

/// A switch of the file; a file written before the key existed keeps `default`.
fn file_switch(sections: &Sections, (section, key): &(String, String), default: bool) -> Result<bool, PolicyError> {
    match sections.get(section).and_then(|keys| keys.get(key)) {
        None => Ok(default),
        Some(text) => switch_word(text).ok_or_else(|| PolicyError::NotASwitch { name: format!("{section}.{key}"), value: text.clone() }),
    }
}

fn parse_file(text: &str) -> Result<Policy, PolicyError> {
    let sections = sections(text)?;
    let names = &format().file;
    let automatic_max_pct = file_pct(&sections, &names.automatic)?;
    let backfill_max_pct = file_pct(&sections, &names.backfill)?;
    let open_per_project = file_count(&sections, &names.open_per_project)?;
    let new_per_week = file_count(&sections, &names.new_per_week)?;
    if backfill_max_pct > automatic_max_pct {
        return Err(PolicyError::BackfillAboveAutomatic);
    }
    let drain = file_switch(&sections, &names.drain, true)?;
    Ok(Policy {
        automatic_max_pct,
        backfill_max_pct,
        open_per_project,
        new_per_week,
        proposal_caps: true,
        backfill: true,
        drain,
        origin: Origin::File,
        with_local: false,
        reason: None,
        local: None,
    })
}

// ---------- the local override ----------

/// A number of the override checked as the prototype's `_numero` checks it; `shown` is the value as given.
fn checked(lever: Lever, number: f64, name: &str, shown: &str) -> Result<LeverValue, PolicyError> {
    let out_of_range = || PolicyError::OutOfRange { name: name.to_owned(), value: shown.to_owned() };
    match lever.kind() {
        Kind::Pct if 0.0 < number && number <= 100.0 => Ok(LeverValue::Pct(number)),
        Kind::Pct => Err(out_of_range()),
        Kind::Count if !number.is_finite() || number.fract() != 0.0 => {
            Err(PolicyError::NotWhole { name: name.to_owned(), value: shown.to_owned() })
        }
        // Whole and in range: the cast is exact up to 2^53, past any cap that means something.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Kind::Count if number >= 1.0 => Ok(LeverValue::Count(number as u64)),
        Kind::Count => Err(out_of_range()),
        Kind::Switch => Err(PolicyError::NotASwitch { name: name.to_owned(), value: shown.to_owned() }),
    }
}

/// The values of the override's file, in its order: the prototype's `_valida`.
fn file_values(pairs: &[(String, Value)]) -> Result<Values, PolicyError> {
    let mut values = Values::default();
    for (key, value) in pairs {
        let lever = Lever::of_file_key(key).ok_or_else(|| PolicyError::UnknownKey {
            key: key.clone(),
            allowed: Lever::ALL.map(Lever::file_key).join(", "),
        })?;
        let parsed = match (lever.kind(), value) {
            (Kind::Switch, Value::Bool(on)) => LeverValue::Switch(*on),
            (Kind::Switch, other) => return Err(PolicyError::NotASwitch { name: key.clone(), value: other.to_string() }),
            (_, Value::Number(number)) => {
                let shown = number.to_string();
                checked(lever, number.as_f64().unwrap_or(f64::NAN), key, &shown)?
            }
            (_, other) => return Err(PolicyError::NotANumber { name: key.clone(), value: other.to_string() }),
        };
        values.set(lever, parsed);
    }
    Ok(values)
}

/// The `key=value` pairs of `policy set`, with the contract's names: the prototype's `_coppie`.
///
/// # Errors
///
/// A pair that is not one, an unknown key, or a value that does not fit its key.
pub fn parse_pairs(pairs: &[String]) -> Result<Values, PolicyError> {
    let mut values = Values::default();
    for pair in pairs {
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) if !pystr::strip(key).is_empty() => (pystr::strip(key), pystr::strip(value)),
            _ => return Err(PolicyError::NotAPair(pair.clone())),
        };
        let lever =
            Lever::parse(key).ok_or_else(|| PolicyError::UnknownKey { key: key.to_owned(), allowed: Lever::ALL.map(Lever::id).join(", ") })?;
        let parsed = match lever.kind() {
            Kind::Switch => LeverValue::Switch(
                switch_word(value).ok_or_else(|| PolicyError::NotASwitch { name: key.to_owned(), value: value.to_owned() })?,
            ),
            _ if !is_number(value) => return Err(PolicyError::NotANumber { name: key.to_owned(), value: value.to_owned() }),
            _ => {
                let number: f64 = value.parse().map_err(|_| PolicyError::NotANumber { name: key.to_owned(), value: value.to_owned() })?;
                checked(lever, number, key, value)?
            }
        };
        values.set(lever, parsed);
    }
    Ok(values)
}

/// An instant as Python's `datetime.fromisoformat` reads the common forms: a date, alone or with a time
/// after any one separator, with seconds, fractions and an offset (`Z`, `+01`, `+0100`, `+01:00`) or none.
fn iso_instant(text: &str) -> Option<(NaiveDateTime, Option<FixedOffset>)> {
    let (date, rest) = if text.len() >= 10 && text.as_bytes().get(4) == Some(&b'-') {
        (NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()?, text.get(10..)?)
    } else {
        (NaiveDate::parse_from_str(text.get(..8)?, "%Y%m%d").ok()?, text.get(8..)?)
    };
    if !text.get(..date_len(text))?.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-') {
        return None;
    }
    let mut chars = rest.chars();
    let Some(_separator) = chars.next() else {
        return Some((date.and_time(NaiveTime::MIN), None));
    };
    let rest = chars.as_str();
    let (time, offset) = match rest.find(['+', '-', 'Z']) {
        Some(at) => (&rest[..at], Some(&rest[at..])),
        None => (rest, None),
    };
    let time = iso_time(time)?;
    let offset = match offset {
        Some(text) => Some(iso_offset(text)?),
        None => None,
    };
    Some((date.and_time(time), offset))
}

fn date_len(text: &str) -> usize {
    if text.as_bytes().get(4) == Some(&b'-') { 10 } else { 8 }
}

fn digits_value(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())).then(|| text.parse().ok()).flatten()
}

fn iso_time(text: &str) -> Option<NaiveTime> {
    let (clock, fraction) = text.split_once(['.', ',']).map_or((text, None), |(clock, fraction)| (clock, Some(fraction)));
    let parts: Vec<&str> = if clock.contains(':') { clock.split(':').collect() } else { clock.as_bytes().chunks(2).filter_map(|c| std::str::from_utf8(c).ok()).collect() };
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|part| part.len() != 2) {
        return None;
    }
    let hour = digits_value(parts[0])?;
    let minute = parts.get(1).map_or(Some(0), |part| digits_value(part))?;
    let second = parts.get(2).map_or(Some(0), |part| digits_value(part))?;
    let micro = match fraction {
        None => 0,
        Some(fraction) if parts.len() == 3 && digits_value(fraction).is_some() => {
            let six: String = fraction.chars().chain(std::iter::repeat('0')).take(6).collect();
            six.parse().ok()?
        }
        Some(_) => return None,
    };
    NaiveTime::from_hms_micro_opt(hour, minute, second, micro)
}

fn iso_offset(text: &str) -> Option<FixedOffset> {
    if text == "Z" {
        return FixedOffset::east_opt(0);
    }
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let body = text.get(1..)?.replace(':', "");
    let (hours, minutes) = match body.len() {
        2 => (digits_value(&body)?, 0),
        4 => (digits_value(&body[..2])?, digits_value(&body[2..])?),
        _ => return None,
    };
    if hours > 23 || minutes > 59 {
        return None;
    }
    FixedOffset::east_opt(sign * i32::try_from(hours * 3600 + minutes * 60).ok()?)
}

/// The prototype's `_istante`: an instant of the file, `Z` read as `+00:00`; one with no offset is local.
fn instant<Tz: TimeZone>(text: &str, zone: &Tz) -> Option<DateTime<Utc>> {
    let (naive, offset) = iso_instant(&text.replace('Z', "+00:00"))?;
    match offset {
        Some(offset) => offset.from_local_datetime(&naive).single().map(|at| at.with_timezone(&Utc)),
        None => zone.from_local_datetime(&naive).earliest().map(|at| at.with_timezone(&Utc)),
    }
}

/// `--until`: `12h`, `3d`, `YYYY-MM-DD` (to the end of that day, local time) or an ISO instant; read in
/// lowercase, as the prototype reads it. In UTC.
///
/// # Errors
///
/// [`PolicyError::NotAnInstant`], or [`PolicyError::UntilPassed`] for an instant not after `now`.
pub fn until<Tz: TimeZone>(text: &str, now: DateTime<Utc>, zone: &Tz) -> Result<DateTime<Utc>, PolicyError> {
    let text = pystr::strip(text).to_lowercase();
    let wrong = || PolicyError::NotAnInstant { name: "--until".to_owned(), value: text.clone() };
    let digits_end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    let unit = pystr::strip(&text[digits_end..]);
    let at = if digits_end > 0 && (unit == "h" || unit == "d") {
        let count: i64 = text[..digits_end].parse().map_err(|_| wrong())?;
        let span = if unit == "h" { Duration::try_hours(count) } else { Duration::try_days(count) }.ok_or_else(wrong)?;
        now.checked_add_signed(span).ok_or_else(wrong)?
    } else if text.len() == 10 && date_len(&text) == 10 && !text.contains(['t', ' ']) {
        let day = NaiveDate::parse_from_str(&text, "%Y-%m-%d").map_err(|_| wrong())?;
        let end = day.and_hms_opt(23, 59, 59).ok_or_else(wrong)?;
        zone.from_local_datetime(&end).earliest().ok_or_else(wrong)?.with_timezone(&Utc)
    } else {
        instant(&text, zone).ok_or_else(wrong)?
    };
    if at <= now {
        return Err(PolicyError::UntilPassed(text));
    }
    Ok(at)
}

/// The prototype's `isoformat(timespec="seconds")` of a UTC instant.
pub fn utc_seconds(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

/// A JSON object, with its keys in the file's order and the last of a repeated key in the first one's
/// place, as Python's `json.loads` gives it; the values of `values` likewise.
enum Ordered {
    Object(Vec<(String, Value)>, Option<OrderedValues>),
    Other,
}

/// The override's values, in their order, or what stood in their place.
enum OrderedValues {
    Object(Vec<(String, Value)>),
    Other,
}

fn put(pairs: &mut Vec<(String, Value)>, key: String, value: Value) {
    match pairs.iter_mut().find(|(name, _)| *name == key) {
        Some(slot) => slot.1 = value,
        None => pairs.push((key, value)),
    }
}

impl<'de> Deserialize<'de> for OrderedValues {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Pairs;
        impl<'de> Visitor<'de> for Pairs {
            type Value = OrderedValues;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("any JSON value")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<OrderedValues, A::Error> {
                let mut pairs = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    put(&mut pairs, key, value);
                }
                Ok(OrderedValues::Object(pairs))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<OrderedValues, A::Error> {
                while seq.next_element::<Value>()?.is_some() {}
                Ok(OrderedValues::Other)
            }
            fn visit_bool<E>(self, _: bool) -> Result<OrderedValues, E> {
                Ok(OrderedValues::Other)
            }
            fn visit_i64<E>(self, _: i64) -> Result<OrderedValues, E> {
                Ok(OrderedValues::Other)
            }
            fn visit_u64<E>(self, _: u64) -> Result<OrderedValues, E> {
                Ok(OrderedValues::Other)
            }
            fn visit_f64<E>(self, _: f64) -> Result<OrderedValues, E> {
                Ok(OrderedValues::Other)
            }
            fn visit_str<E>(self, _: &str) -> Result<OrderedValues, E> {
                Ok(OrderedValues::Other)
            }
            fn visit_unit<E>(self) -> Result<OrderedValues, E> {
                Ok(OrderedValues::Other)
            }
        }
        deserializer.deserialize_any(Pairs)
    }
}

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Top;
        impl<'de> Visitor<'de> for Top {
            type Value = Ordered;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("any JSON value")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
                let (mut pairs, mut values) = (Vec::new(), None);
                while let Some(key) = map.next_key::<String>()? {
                    if key == format().local.values {
                        values = Some(map.next_value::<OrderedValues>()?);
                        put(&mut pairs, key, Value::Null);
                    } else {
                        let value = map.next_value::<Value>()?;
                        put(&mut pairs, key, value);
                    }
                }
                Ok(Ordered::Object(pairs, values))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Ordered, A::Error> {
                while seq.next_element::<Value>()?.is_some() {}
                Ok(Ordered::Other)
            }
            fn visit_bool<E>(self, _: bool) -> Result<Ordered, E> {
                Ok(Ordered::Other)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Ordered, E> {
                Ok(Ordered::Other)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Ordered, E> {
                Ok(Ordered::Other)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Ordered, E> {
                Ok(Ordered::Other)
            }
            fn visit_str<E>(self, _: &str) -> Result<Ordered, E> {
                Ok(Ordered::Other)
            }
            fn visit_unit<E>(self) -> Result<Ordered, E> {
                Ok(Ordered::Other)
            }
        }
        deserializer.deserialize_any(Top)
    }
}

/// The override as it was read: `Ok(None)` when there is no file. Its valid values in time, and what is
/// known of it: the prototype's `leggi_locale`. Never an error.
pub fn read_local<Tz: TimeZone>(read: Result<Option<Vec<u8>>, std::io::Error>, now: DateTime<Utc>, zone: &Tz) -> (Values, Option<Local>) {
    match read {
        Ok(None) => (Values::default(), None),
        Ok(Some(bytes)) => match local_of(&bytes, now, zone) {
            Ok(found) => found,
            Err(error) => (Values::default(), Some(Local::unreadable(&error))),
        },
        Err(error) => (Values::default(), Some(Local::unreadable(&PolicyError::Unreadable(error.kind().to_string())))),
    }
}

fn local_of<Tz: TimeZone>(bytes: &[u8], now: DateTime<Utc>, zone: &Tz) -> Result<(Values, Option<Local>), PolicyError> {
    let text = std::str::from_utf8(bytes).map_err(|_| PolicyError::NotUtf8)?;
    let Ordered::Object(pairs, values) = serde_json::from_str::<Ordered>(text).map_err(|error| PolicyError::NotJson(error.to_string()))? else {
        return Err(PolicyError::NotAnObject);
    };
    let names = &format().local;
    let values = match values {
        Some(OrderedValues::Object(pairs)) => file_values(&pairs)?,
        _ => return Err(PolicyError::ValuesNotAnObject(names.values.clone())),
    };
    let field = |name: &str| pairs.iter().find(|(key, _)| key == name).map_or(Value::Null, |(_, value)| value.clone());
    let until = field(&names.until);
    let ends = match &until {
        Value::Null => None,
        Value::String(text) => Some(instant(text, zone).ok_or_else(|| PolicyError::NotAnInstant { name: names.until.clone(), value: text.clone() })?),
        other => return Err(PolicyError::NotAnInstant { name: names.until.clone(), value: other.to_string() }),
    };
    if ends.is_some_and(|ends| ends <= now) {
        let local = Local { status: LocalStatus::Expired, until: Some(until), values: Some(values), note: None, set_at: None, reason: None };
        return Ok((Values::default(), Some(local)));
    }
    let local = Local {
        status: LocalStatus::Active,
        until: Some(until),
        values: Some(values.clone()),
        note: Some(field(&names.note)),
        set_at: Some(field(&names.set_at)),
        reason: None,
    };
    Ok((values, Some(local)))
}

/// `policy set`: the override's file with `pairs` over the values in use, as the prototype's `imposta`
/// writes it. Without `until` the earlier expiry stays, if the earlier values were in use, and so does
/// the earlier note; `file` is the policy file, without any override, which the values must fit.
///
/// # Errors
///
/// A pair, a value or `until` that does not fit, or values that do not fit the file together.
#[allow(clippy::too_many_arguments)]
pub fn set_local<Tz: TimeZone>(
    file: &Policy,
    current: &(Values, Option<Local>),
    pairs: &[String],
    until_text: Option<&str>,
    note: Option<&str>,
    now: DateTime<Utc>,
    zone: &Tz,
) -> Result<String, PolicyError> {
    let (earlier, local) = current;
    let values = earlier.merged(&parse_pairs(pairs)?);
    file.with(&values)?;
    let kept = |pick: fn(&Local) -> Option<&Value>| {
        if earlier.is_empty() { Value::Null } else { local.as_ref().and_then(pick).cloned().unwrap_or(Value::Null) }
    };
    let until = match until_text {
        Some(text) => json!(utc_seconds(until(text, now, zone)?)),
        None => kept(|local| local.until.as_ref()),
    };
    let note = match note.filter(|note| !note.is_empty()) {
        Some(note) => json!(note),
        None => kept(|local| local.note.as_ref()),
    };
    Ok(file_text(&values, &until, &note, &utc_seconds(now)))
}

/// The override's file as Python's `json.dumps(…, ensure_ascii=False, indent=1)` writes it, in `\n` lines.
fn file_text(values: &Values, until: &Value, note: &Value, set_at: &str) -> String {
    let names = &format().local;
    let key = |name: &str| serde_json::to_string(name).unwrap_or_default();
    let mut text = format!("{{\n {}: 1,\n {}: ", key(&names.version), key(&names.values));
    if values.is_empty() {
        text.push_str("{}");
    } else {
        let lines: Vec<String> =
            values.iter().map(|(lever, value)| format!("  {}: {}", key(lever.file_key()), python_json(&value.to_json(), 2))).collect();
        text.push_str(&format!("{{\n{}\n }}", lines.join(",\n")));
    }
    for (name, value) in [(&names.until, until), (&names.note, note), (&names.set_at, &json!(set_at))] {
        text.push_str(&format!(",\n {}: {}", key(name), python_json(value, 1)));
    }
    text.push_str("\n}");
    text
}

/// A value as `json.dumps(indent=1)` writes it at nesting `depth`: one space per level.
fn python_json(value: &Value, depth: usize) -> String {
    let pad = |depth: usize| " ".repeat(depth);
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            let lines: Vec<String> = fields
                .iter()
                .map(|(key, value)| format!("{}{}: {}", pad(depth + 1), serde_json::to_string(key).unwrap_or_default(), python_json(value, depth + 1)))
                .collect();
            format!("{{\n{}\n{}}}", lines.join(",\n"), pad(depth))
        }
        Value::Array(items) if !items.is_empty() => {
            let lines: Vec<String> = items.iter().map(|item| format!("{}{}", pad(depth + 1), python_json(item, depth + 1))).collect();
            format!("[\n{}\n{}]", lines.join(",\n"), pad(depth))
        }
        Value::Number(number) if number.is_f64() => {
            let float = number.as_f64().unwrap_or_default();
            if float.fract() == 0.0 && float.abs() < 1e16 { format!("{float:.1}") } else { number.to_string() }
        }
        other => other.to_string(),
    }
}

/// The bytes of the override's file as the prototype writes them: Python's text mode ends its lines in
/// `\r\n` on Windows (Q5 of the dossier's conformance study).
pub fn file_bytes(text: &str) -> Vec<u8> {
    if cfg!(windows) { text.replace('\n', "\r\n").into_bytes() } else { text.as_bytes().to_vec() }
}

// ---------- the caps of Proposals ----------

/// What the orchestrator reads before it opens a Proposal: the prototype's `posto_per_proposte`. `new`
/// is `None` when the weekly source did not answer: then only the cap on the open ones holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub open: Vec<u64>,
    pub open_cap: u64,
    pub new: Option<Vec<u64>>,
    pub new_cap: u64,
    pub caps: bool,
    pub room_left: bool,
}

impl Room {
    pub fn new(policy: &Policy, open: Vec<u64>, new: Option<Vec<u64>>) -> Room {
        let below = |count: usize, cap: u64| u64::try_from(count).is_ok_and(|count| count < cap);
        let room_left = !policy.proposal_caps
            || (below(open.len(), policy.open_per_project) && new.as_ref().is_none_or(|new| below(new.len(), policy.new_per_week)));
        Room { open, open_cap: policy.open_per_project, new, new_cap: policy.new_per_week, caps: policy.proposal_caps, room_left }
    }

    /// With the contract's names, and the policy's source.
    pub fn to_json(&self, project: &str, policy: &Policy) -> Value {
        json!({
            "project": project,
            "open": self.open.len(),
            "open_cap": self.open_cap,
            "open_numbers": self.open,
            "new_7_days": self.new.as_ref().map(Vec::len),
            "new_7_days_cap": self.new_cap,
            "new_numbers": self.new,
            "caps": self.caps,
            "room_left": self.room_left,
            "policy": policy.source(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-10T09:00:00Z").unwrap().with_timezone(&Utc)
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn file(text: &str) -> Policy {
        Policy::from_file(Ok(text.as_bytes().to_vec()))
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// The prototype's file at its values, with the names of the format: the guard on Italian words reads
    /// tests too.
    fn good_with(automatic: &str, backfill: &str, open: &str, new: &str) -> String {
        let names = &format().file;
        format!(
            "{}:\n  {}: {automatic}\n{}:\n  {}: {backfill}\n{}:\n  {}: {open}\n  {}: {new}\n",
            names.automatic.0, names.automatic.1, names.backfill.0, names.backfill.1, names.open_per_project.0, names.open_per_project.1,
            names.new_per_week.1
        )
    }

    fn good() -> String {
        good_with("75", "50", "3", "5")
    }

    fn open_key() -> &'static str {
        &format().file.open_per_project.1
    }

    /// The override's file after `policy set`, at `now`, over what `current` is.
    fn set(current: Option<&str>, pairs: &[&str], until_text: Option<&str>, now: DateTime<Utc>) -> Result<String, PolicyError> {
        let read = read_local(Ok(current.map(|text| text.as_bytes().to_vec())), now, &utc());
        set_local(&Policy::defaults(), &read, &strings(pairs), until_text, None, now, &utc())
    }

    fn load(local: Option<&str>, now: DateTime<Utc>) -> Policy {
        file(&good()).with_local(read_local(Ok(local.map(|text| text.as_bytes().to_vec())), now, &utc()))
    }

    /// `test_il_file_del_repo_ha_gli_stessi_valori_dei_default`: the compiled copy of the file has the
    /// prototype's values, and reads as a file.
    #[test]
    fn the_defaults_are_the_files_values() {
        let defaults = Policy::defaults();
        assert_eq!(
            (defaults.automatic_max_pct, defaults.backfill_max_pct, defaults.open_per_project, defaults.new_per_week, defaults.drain),
            (75.0, 50.0, 3, 5, true)
        );
        assert_eq!((defaults.source(), defaults.reason.as_deref()), ("default", None));
        let read = file(DEFAULTS);
        assert_eq!((read.source(), read.reason.as_deref()), ("file", None));
        assert_eq!(Policy { origin: Origin::Default, ..read }, defaults);
    }

    /// `test_valori_del_file_con_commenti`.
    #[test]
    fn values_of_the_file_with_comments() {
        let text = format!("# c\n{}", good_with("80  # a note", "40.5", "2", "7"));
        let policy = file(&text);
        assert_eq!(
            (policy.automatic_max_pct, policy.backfill_max_pct, policy.open_per_project, policy.new_per_week, policy.source()),
            (80.0, 40.5, 2, 7, "file")
        );
    }

    /// `test_file_mancante_default_dichiarati`.
    #[test]
    fn a_missing_file_gives_the_defaults_with_the_reason() {
        let policy = Policy::from_file(Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
        assert_eq!(Policy { reason: None, ..policy.clone() }, Policy::defaults());
        assert_eq!(policy.source(), "default");
        assert!(policy.reason.unwrap().contains("not found"));
    }

    /// `test_file_illeggibile_default_dichiarati`: every case gives the defaults, with a reason.
    #[test]
    fn a_file_that_does_not_read_gives_the_defaults() {
        let (good, open, section) = (good(), open_key(), &format().file.open_per_project.0);
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", b"".to_vec()),
            ("not yaml", b"{{{ no".to_vec()),
            ("binary", b"\xff\xfe\x00 nonsenso".to_vec()),
            ("a key missing", good.replace(&format!("  {}: 5\n", format().file.new_per_week.1), "").into_bytes()),
            ("not a number", good_with("tanto", "50", "3", "5").into_bytes()),
            ("pct above 100", good_with("175", "50", "3", "5").into_bytes()),
            ("cap zero", good_with("75", "50", "0", "5").into_bytes()),
            ("cap with decimals", good_with("75", "50", "2.5", "5").into_bytes()),
            ("backfill above automatic", good_with("75", "90", "3", "5").into_bytes()),
            ("section repeated", format!("{good}{section}:\n  {open}: 1\n").into_bytes()),
            ("key repeated", format!("{good}  {open}: 1\n").into_bytes()),
            ("key outside a section", b"  claude_5h_max_pct: 75\n".to_vec()),
            ("a tab", good.replace(&format!("  {open}"), &format!("\t{open}")).into_bytes()),
            ("a byte order mark", format!("\u{feff}{good}").into_bytes()),
            ("a switch that is not one", format!("{good}scodare:\n  acceso: forse\n").into_bytes()),
        ];
        for (name, bytes) in cases {
            let policy = Policy::from_file(Ok(bytes));
            assert_eq!(Policy { reason: None, ..policy.clone() }, Policy::defaults(), "{name}");
            assert!(policy.reason.as_deref().is_some_and(|reason| !reason.is_empty()), "{name}");
        }
        assert!(!file(&format!("{good}scodare:\n  acceso: OFF\n")).drain, "a switch reads in lowercase");
        assert!(file(&format!("{good}  # a tab in a comment\t\n")).reason.is_none(), "a tab in a comment counts for nothing");
    }

    /// `test_senza_override_vale_il_file`.
    #[test]
    fn without_an_override_the_file_holds() {
        let policy = load(None, t0());
        assert_eq!((policy.source(), policy.proposal_caps, policy.backfill, policy.local.is_none()), ("file", true, true, true));
    }

    /// `test_tetti_e_riempimento_spenti_resta_solo_la_soglia_automatica`.
    #[test]
    fn with_caps_and_backfill_off_only_the_automatic_threshold_is_left() {
        let written = set(None, &["proposal_caps=off", "backfill=off"], Some("3d"), t0()).unwrap();
        let policy = load(Some(&written), t0());
        assert_eq!((policy.source(), policy.proposal_caps, policy.backfill), ("file+local", false, false));
        assert_eq!(policy.backfill_max_pct, policy.automatic_max_pct);
        assert_eq!(policy.local.unwrap().status, LocalStatus::Active);
    }

    /// `test_scaduto_non_vale_piu`, with the clock injected: Acceptance 2.
    #[test]
    fn an_expired_override_no_longer_holds() {
        let written = set(None, &["proposal_caps=off"], Some("12h"), t0()).unwrap();
        assert!(load(Some(&written), t0() + Duration::hours(12) - Duration::seconds(1)).with_local);
        for later in [Duration::hours(12), Duration::hours(13)] {
            let policy = load(Some(&written), t0() + later);
            assert_eq!((policy.source(), policy.proposal_caps), ("file", true));
            assert_eq!(policy.local.unwrap().status, LocalStatus::Expired, "the expiry is `<=`");
        }
    }

    /// `test_imposta_aggiunge_e_tiene_la_scadenza`, and the file as the prototype writes it.
    #[test]
    fn set_adds_and_keeps_the_expiry() {
        let first = set(None, &["proposal_caps=off"], Some("2d"), t0()).unwrap();
        let second = set(Some(&first), &["automatic_max_pct=80"], None, t0()).unwrap();
        assert_eq!(
            second,
            "{\n \"v\": 1,\n \"valori\": {\n  \"tetti_proposte\": false,\n  \"automatico_max_pct\": 80.0\n },\n \"fino\": \"2026-01-12T09:00:00+00:00\",\n \"nota\": null,\n \"impostato\": \"2026-01-10T09:00:00+00:00\"\n}"
        );
        // An expired override leaves neither its values nor its expiry.
        let later = set(Some(&first), &["backfill=off"], None, t0() + Duration::days(3)).unwrap();
        let (values, local) = read_local(Ok(Some(later.into_bytes())), t0() + Duration::days(3), &utc());
        assert_eq!((values.iter().count(), local.unwrap().until), (1, Some(Value::Null)));
    }

    /// `test_valori_sbagliati_non_si_scrivono`.
    #[test]
    fn wrong_values_are_not_written() {
        for (pairs, until_text) in [
            (&["backfill_max_pct=90"][..], None),
            (&["caps=off"][..], None),
            (&["open_per_project=0"][..], None),
            (&["proposal_caps=maybe"][..], None),
            (&["proposal_caps=off"][..], Some("2020-01-01")),
            (&["open_per_project=2.5"][..], None),
            (&["no pair"][..], None),
            (&["proposal_caps=off"][..], Some("tomorrow")),
        ] {
            assert!(set(None, pairs, until_text, t0()).is_err(), "{pairs:?} {until_text:?}");
        }
        assert_eq!(parse_pairs(&strings(&["open_per_project=2.0"])).unwrap().get(Lever::OpenPerProject), Some(LeverValue::Count(2)));
        assert_eq!(parse_pairs(&strings(&[" backfill = OFF "])).unwrap().get(Lever::Backfill), Some(LeverValue::Switch(false)));
    }

    /// `test_file_illeggibile_si_ignora`, and the other ways an override does not read.
    #[test]
    fn an_override_that_does_not_read_is_ignored() {
        let whole = format!("{{\"valori\": {{\"{}\": 2.5}}}}", open_key());
        for text in ["{rotto", "[1, 2]", "{\"valori\": 3}", "{\"valori\": {\"tetti\": false}}", "{\"valori\": {\"tetti_proposte\": 1}}", &whole,
            "{\"valori\": {}, \"fino\": 5}", "{\"valori\": {}, \"fino\": \"domani\"}"] {
            let policy = load(Some(text), t0());
            assert_eq!(policy.source(), "file", "{text}");
            let local = policy.local.unwrap();
            assert_eq!(local.status, LocalStatus::Unreadable, "{text}");
            assert!(local.reason.is_some(), "{text}");
        }
    }

    /// `test_senza_locale_si_legge_solo_il_file`: the file alone ignores any override.
    #[test]
    fn without_the_local_override_only_the_file_counts() {
        assert!(file(&good()).proposal_caps);
    }

    /// `test_tetti_spenti_lasciano_posto_all_orchestratore`.
    #[test]
    fn caps_off_leave_room() {
        let written = set(None, &["proposal_caps=off"], None, t0()).unwrap();
        let room = Room::new(&load(Some(&written), t0()), vec![10, 11, 12, 13], Some(vec![10, 11, 12, 13, 14, 15]));
        assert_eq!((room.caps, room.room_left), (false, true));
    }

    /// An override that does not fit the file is not applied, and says so.
    #[test]
    fn an_override_that_does_not_fit_the_file_is_ignored() {
        let policy = load(Some("{\"v\": 1, \"valori\": {\"riempimento_max_pct\": 90.0}, \"fino\": null}"), t0());
        assert_eq!((policy.source(), policy.backfill_max_pct), ("file", 50.0));
        assert_eq!(policy.local.as_ref().unwrap().status, LocalStatus::Ignored);
        assert!(policy.local.unwrap().reason.unwrap().contains("backfill"));
    }

    /// `test_quadro_e_posto`.
    #[test]
    fn the_room_for_proposals() {
        let policy = file(&good());
        let room = Room::new(&policy, vec![10], Some(vec![1, 2]));
        assert_eq!((room.open.len(), room.open_cap, room.new.as_ref().map(Vec::len), room.new_cap, room.room_left), (1, 3, Some(2), 5, true));
        assert!(!Room::new(&policy, vec![10, 11, 12], Some(vec![])).room_left);
        assert!(!Room::new(&policy, vec![], Some(vec![1, 2, 3, 4, 5])).room_left);
        assert!(Room::new(&policy, vec![10], Some(vec![1, 2, 3, 4])).room_left);
        let room = Room::new(&policy, vec![10], None);
        assert_eq!((room.new.is_none(), room.room_left), (true, true));
        assert!(!Room::new(&policy, vec![10, 11, 12], None).room_left);
        let json = room.to_json("p", &policy);
        assert_eq!((json["new_7_days"].clone(), json["policy"].clone(), json["open_numbers"].clone()), (Value::Null, json!("file"), json!([10])));
    }

    /// `--until`, as the prototype reads it: lowercase first, so a `Z` is not an offset any more.
    #[test]
    fn until_reads_durations_days_and_instants() {
        let east = FixedOffset::east_opt(3600).unwrap();
        assert_eq!(utc_seconds(until("12h", t0(), &utc()).unwrap()), "2026-01-10T21:00:00+00:00");
        assert_eq!(utc_seconds(until(" 3 D ", t0(), &utc()).unwrap()), "2026-01-13T09:00:00+00:00");
        assert_eq!(utc_seconds(until("2026-01-11", t0(), &east).unwrap()), "2026-01-11T22:59:59+00:00");
        assert_eq!(utc_seconds(until("2026-01-11T10:00:00+01:00", t0(), &utc()).unwrap()), "2026-01-11T09:00:00+00:00");
        assert_eq!(utc_seconds(until("2026-01-11 10:00", t0(), &east).unwrap()), "2026-01-11T09:00:00+00:00");
        assert!(until("2026-01-11T10:00:00Z", t0(), &utc()).is_err(), "the prototype lowercases the Z");
        assert_eq!(until("2026-01-10T09:00:00+00:00", t0(), &utc()), Err(PolicyError::UntilPassed("2026-01-10t09:00:00+00:00".to_owned())));
        assert!(until("0h", t0(), &utc()).is_err() && until("3g", t0(), &utc()).is_err());
    }

    /// The reason is cut at 120 characters, not bytes.
    #[test]
    fn reasons_are_cut_in_characters() {
        let name = "è".repeat(200);
        let reason = file(&format!("{}{name}:\n{name}:\n", good())).reason.unwrap();
        assert_eq!(reason.chars().count(), 120);
    }

    /// The file's own order of values stays when `set` adds one, and a repeated key keeps its first place.
    #[test]
    fn the_order_of_the_values_is_the_files() {
        let current = "{\"valori\": {\"tetti_proposte\": false, \"automatico_max_pct\": 70, \"tetti_proposte\": true}, \"fino\": null}";
        let written = set(Some(current), &["backfill=off"], None, t0()).unwrap();
        let order: Vec<&str> = written.lines().filter(|line| line.starts_with("  ")).collect();
        assert_eq!(order, ["  \"tetti_proposte\": true,", "  \"automatico_max_pct\": 70.0,", "  \"riempimento\": false"]);
    }
}
