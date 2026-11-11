//! A reading of the usage budget: what is left of each provider's windows, as the probes of
//! `regia-agents` read it (F065 and F122 of the dossier's inventory). The API and its JSON are in
//! English, with the contract's names. Inside the budget cache a reading is stored in format v1,
//! which the prototype reads too (`formats/store_v1.toml`): [`ProviderUsage::to_v1`] and
//! [`ProviderUsage::from_v1`] are the one codec.

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::{Map, Number, Value};

use crate::v1::format;

/// A provider's state, or a window's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageStatus {
    /// At least one window was read.
    Ok,
    /// Nothing could be read, and the reason says why. A reading that went wrong is a value, never an
    /// error.
    Unknown,
    /// The user refused the usage probes: no credentials were read and no endpoint was asked. It is
    /// not [`UsageStatus::Unknown`], and it is never stored.
    ProbeOff,
}

/// A provider's window: the five-hour one or the seven-day one.
#[derive(Debug, Clone, PartialEq)]
pub enum Window {
    /// Used and remaining percentages to one decimal, and the reset: local time to the minute, and
    /// whole seconds from the reading.
    Open { used_pct: f64, remaining_pct: f64, reset: String, reset_in_s: i64 },
    /// After its reset a window stays closed until the first message: the use the provider gives, and
    /// no reset yet (#190 of the dossier).
    NotOpen { used_pct: f64, remaining_pct: f64 },
    Unknown { reason: String },
}

impl Window {
    /// A window read from a provider's answer: `used` as the provider gives it, and its reset with the
    /// seconds to it, or none for a window not open yet.
    #[must_use]
    pub fn read(used: f64, reset: Option<(String, i64)>) -> Window {
        let used_pct = round_to_tenth(used);
        let remaining_pct = round_to_tenth((100.0 - used).max(0.0));
        match reset {
            Some((reset, reset_in_s)) => Window::Open { used_pct, remaining_pct, reset, reset_in_s },
            None => Window::NotOpen { used_pct, remaining_pct },
        }
    }

    #[must_use]
    pub fn unknown(reason: impl Into<String>) -> Window {
        Window::Unknown { reason: reason.into() }
    }

    #[must_use]
    pub fn is_ok(&self) -> bool {
        !matches!(self, Window::Unknown { .. })
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Window::Unknown { reason } => Some(reason),
            Window::Open { .. } | Window::NotOpen { .. } => None,
        }
    }

    #[must_use]
    pub fn used_pct(&self) -> Option<f64> {
        match self {
            Window::Open { used_pct, .. } | Window::NotOpen { used_pct, .. } => Some(*used_pct),
            Window::Unknown { .. } => None,
        }
    }

    /// The fields in order, under the names `names` gives, as both encodings write them.
    fn fields(&self, names: &WindowNames<'_>) -> Vec<(String, Value)> {
        let mut out = vec![(names.status.to_owned(), Value::from(names.status_value(self.is_ok())))];
        match self {
            Window::Open { used_pct, remaining_pct, reset, reset_in_s } => {
                out.push((names.used_pct.to_owned(), float(*used_pct)));
                out.push((names.remaining_pct.to_owned(), float(*remaining_pct)));
                out.push((names.reset.to_owned(), Value::from(reset.as_str())));
                out.push((names.reset_in_s.to_owned(), Value::from(*reset_in_s)));
            }
            Window::NotOpen { used_pct, remaining_pct } => {
                out.push((names.used_pct.to_owned(), float(*used_pct)));
                out.push((names.remaining_pct.to_owned(), float(*remaining_pct)));
                out.push((names.open.to_owned(), Value::Bool(false)));
                out.push((names.reset.to_owned(), Value::Null));
                out.push((names.reset_in_s.to_owned(), Value::Null));
            }
            Window::Unknown { reason } => out.push((names.reason.to_owned(), Value::from(reason.as_str()))),
        }
        out
    }

    fn from_v1(value: &Value) -> Window {
        let keys = &format().usage_window;
        let field = |name: &str| value.get(name);
        let ok = field(&keys.status).and_then(Value::as_str) == Some(format().usage_statuses.ok.as_str());
        let used = field(&keys.used_pct).and_then(Value::as_f64);
        let remaining = field(&keys.remaining_pct).and_then(Value::as_f64);
        match (ok, used, remaining) {
            (true, Some(used_pct), Some(remaining_pct)) => match field(&keys.reset).and_then(Value::as_str) {
                Some(reset) if !reset.is_empty() => Window::Open {
                    used_pct,
                    remaining_pct,
                    reset: reset.to_owned(),
                    reset_in_s: field(&keys.reset_in_s).and_then(Value::as_i64).unwrap_or(0),
                },
                _ => Window::NotOpen { used_pct, remaining_pct },
            },
            (true, _, _) => Window::unknown("the cache holds a window without its percentages"),
            (false, _, _) => {
                Window::unknown(field(&keys.reason).and_then(Value::as_str).unwrap_or_default().to_owned())
            }
        }
    }
}

impl Serialize for Window {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_pairs(serializer, self.fields(&WindowNames::ENGLISH))
    }
}

/// The names of a window's fields in one of the two encodings.
struct WindowNames<'a> {
    status: &'a str,
    used_pct: &'a str,
    remaining_pct: &'a str,
    reset: &'a str,
    reset_in_s: &'a str,
    open: &'a str,
    reason: &'a str,
    ok: &'a str,
    unknown: &'a str,
}

impl WindowNames<'static> {
    const ENGLISH: WindowNames<'static> = WindowNames {
        status: "status",
        used_pct: "used_pct",
        remaining_pct: "remaining_pct",
        reset: "reset",
        reset_in_s: "reset_in_s",
        open: "open",
        reason: "reason",
        ok: "ok",
        unknown: "unknown",
    };

    fn v1() -> WindowNames<'static> {
        let keys = &format().usage_window;
        let statuses = &format().usage_statuses;
        WindowNames {
            status: &keys.status,
            used_pct: &keys.used_pct,
            remaining_pct: &keys.remaining_pct,
            reset: &keys.reset,
            reset_in_s: &keys.reset_in_s,
            open: &keys.open,
            reason: &keys.reason,
            ok: &statuses.ok,
            unknown: &statuses.unknown,
        }
    }
}

impl WindowNames<'_> {
    fn status_value(&self, ok: bool) -> &str {
        if ok { self.ok } else { self.unknown }
    }
}

/// The state of Codex's sign-in token, from its expiry alone: never the token itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenStatus {
    Valid,
    /// It expires within the warning threshold: run Codex, which renews it.
    Expiring,
    Expired,
    /// Its expiry could not be read.
    Unknown,
    /// The endpoint answered 401 or 403, whatever the expiry says.
    Refused,
}

impl TokenStatus {
    const ALL: [TokenStatus; 5] =
        [TokenStatus::Valid, TokenStatus::Expiring, TokenStatus::Expired, TokenStatus::Unknown, TokenStatus::Refused];

    fn v1(self) -> &'static str {
        let names = &format().token_statuses;
        match self {
            TokenStatus::Valid => &names.valid,
            TokenStatus::Expiring => &names.expiring,
            TokenStatus::Expired => &names.expired,
            TokenStatus::Unknown => &names.unknown,
            TokenStatus::Refused => &names.refused,
        }
    }
}

/// Codex's token as a reading shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenState {
    pub status: TokenStatus,
    /// When it expires, local time to the second; `None` when its expiry could not be read.
    pub expires_at: Option<String>,
    /// Whole seconds to the expiry, 0 once passed, when it was read. Written once: a reading from the
    /// cache keeps it as it was, and only [`TokenState::expires_at`] stays true.
    pub expires_in_s: Option<i64>,
    pub reason: Option<String>,
    /// The status of the refusal.
    pub http_status: Option<u16>,
}

impl TokenState {
    fn fields(&self, english: bool) -> Vec<(String, Value)> {
        let keys = &format().usage_token;
        let name = |english_name: &str, v1_name: &str| if english { english_name.to_owned() } else { v1_name.to_owned() };
        let status = if english {
            serde_json::to_value(self.status).unwrap_or(Value::Null)
        } else {
            Value::from(self.status.v1())
        };
        let mut out = vec![
            (name("status", &keys.status), status),
            (name("expires_at", &keys.expires_at), self.expires_at.clone().map_or(Value::Null, Value::from)),
        ];
        if let Some(seconds) = self.expires_in_s {
            out.push((name("expires_in_s", &keys.expires_in_s), Value::from(seconds)));
        }
        if let Some(reason) = &self.reason {
            out.push((name("reason", &keys.reason), Value::from(reason.as_str())));
        }
        if let Some(code) = self.http_status {
            out.push((name("http_status", &keys.http_status), Value::from(code)));
        }
        out
    }

    fn from_v1(value: &Value) -> Option<TokenState> {
        let keys = &format().usage_token;
        let doc = value.as_object().filter(|doc| !doc.is_empty())?;
        let text = |name: &str| doc.get(name).and_then(Value::as_str).map(str::to_owned);
        let status = doc.get(&keys.status).and_then(Value::as_str);
        Some(TokenState {
            status: TokenStatus::ALL.into_iter().find(|candidate| Some(candidate.v1()) == status).unwrap_or(TokenStatus::Unknown),
            expires_at: text(&keys.expires_at),
            expires_in_s: doc.get(&keys.expires_in_s).and_then(Value::as_i64),
            reason: text(&keys.reason),
            http_status: doc.get(&keys.http_status).and_then(Value::as_u64).and_then(|code| u16::try_from(code).ok()),
        })
    }
}

impl Serialize for TokenState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_pairs(serializer, self.fields(true))
    }
}

/// What a provider's reading says, and when it came from the cache, how the cache served it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderUsage {
    pub provider: String,
    pub status: UsageStatus,
    /// The subscription, as the credentials or the endpoint name it; always written, null if unknown.
    pub plan: Option<String>,
    /// In the order they were read.
    pub windows: Vec<(String, Window)>,
    /// Only Codex's: its expiry, read from the token without sending it anywhere.
    pub token: Option<TokenState>,
    /// Why nothing could be read. Only when the status is not ok.
    pub reason: Option<String>,
    pub served: Option<Served>,
}

/// How the cache served a reading.
#[derive(Debug, Clone, PartialEq)]
pub struct Served {
    /// Seconds since the valid reading was read; `None` when there is no valid reading.
    pub age_s: Option<i64>,
    pub from_cache: bool,
    /// Why the last attempt failed, when the cache answers with an older valid reading.
    pub update_failed: Option<String>,
}

/// The reason of a provider whose windows and caller give none.
pub const NO_DATA: &str = "no data available";

impl ProviderUsage {
    /// A provider's reading: ok when any of its windows is. Otherwise the reason is `reason`, else the
    /// first window's, else [`NO_DATA`].
    #[must_use]
    pub fn new(
        provider: impl Into<String>,
        plan: Option<String>,
        windows: Vec<(String, Window)>,
        token: Option<TokenState>,
        reason: Option<String>,
    ) -> ProviderUsage {
        let ok = windows.iter().any(|(_, window)| window.is_ok());
        let reason = (!ok).then(|| {
            reason
                .filter(|reason| !reason.is_empty())
                .or_else(|| windows.iter().find_map(|(_, window)| window.reason().filter(|reason| !reason.is_empty()).map(str::to_owned)))
                .unwrap_or_else(|| NO_DATA.to_owned())
        });
        let status = if ok { UsageStatus::Ok } else { UsageStatus::Unknown };
        ProviderUsage { provider: provider.into(), status, plan, windows, token, reason, served: None }
    }

    /// A provider that could not be read at all.
    #[must_use]
    pub fn unknown(provider: impl Into<String>, plan: Option<String>, reason: impl Into<String>, token: Option<TokenState>) -> ProviderUsage {
        ProviderUsage::new(provider, plan, Vec::new(), token, Some(reason.into()))
    }

    /// A provider the user does not want probed.
    #[must_use]
    pub fn probe_off(provider: impl Into<String>, reason: impl Into<String>) -> ProviderUsage {
        ProviderUsage {
            provider: provider.into(),
            status: UsageStatus::ProbeOff,
            plan: None,
            windows: Vec::new(),
            token: None,
            reason: Some(reason.into()),
            served: None,
        }
    }

    #[must_use]
    pub fn window(&self, name: &str) -> Option<&Window> {
        self.windows.iter().find(|(key, _)| key == name).map(|(_, window)| window)
    }

    /// The reading as the cache stores it, in format v1; `None` for [`UsageStatus::ProbeOff`], which is
    /// never stored. How the cache served it is not part of it.
    #[must_use]
    pub fn to_v1(&self) -> Option<Value> {
        let keys = &format().usage;
        let statuses = &format().usage_statuses;
        let status = match self.status {
            UsageStatus::Ok => &statuses.ok,
            UsageStatus::Unknown => &statuses.unknown,
            UsageStatus::ProbeOff => return None,
        };
        let names = WindowNames::v1();
        let windows: Map<String, Value> =
            self.windows.iter().map(|(name, window)| (name.clone(), Value::Object(window.fields(&names).into_iter().collect()))).collect();
        let mut doc = Map::new();
        doc.insert(keys.provider.clone(), Value::from(self.provider.as_str()));
        doc.insert(keys.status.clone(), Value::from(status.as_str()));
        doc.insert(keys.plan.clone(), self.plan.clone().map_or(Value::Null, Value::from));
        doc.insert(keys.windows.clone(), Value::Object(windows));
        if let Some(token) = &self.token {
            doc.insert(keys.token.clone(), Value::Object(token.fields(false).into_iter().collect()));
        }
        if let Some(reason) = &self.reason {
            doc.insert(keys.reason.clone(), Value::from(reason.as_str()));
        }
        Some(Value::Object(doc))
    }

    /// A reading as the cache holds it under `key`, written by either implementation; `None` unless it
    /// is a JSON object with something in it.
    #[must_use]
    pub fn from_v1(key: &str, value: &Value) -> Option<ProviderUsage> {
        let keys = &format().usage;
        let doc = value.as_object().filter(|doc| !doc.is_empty())?;
        let text = |name: &str| doc.get(name).and_then(Value::as_str).map(str::to_owned);
        let ok = doc.get(&keys.status).and_then(Value::as_str) == Some(format().usage_statuses.ok.as_str());
        let windows = doc
            .get(&keys.windows)
            .and_then(Value::as_object)
            .map(|windows| windows.iter().map(|(name, window)| (name.clone(), Window::from_v1(window))).collect())
            .unwrap_or_default();
        Some(ProviderUsage {
            provider: text(&keys.provider).unwrap_or_else(|| key.to_owned()),
            status: if ok { UsageStatus::Ok } else { UsageStatus::Unknown },
            plan: text(&keys.plan),
            windows,
            token: doc.get(&keys.token).and_then(TokenState::from_v1),
            reason: text(&keys.reason),
            served: None,
        })
    }
}

impl Serialize for ProviderUsage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("provider", &self.provider)?;
        map.serialize_entry("status", &self.status)?;
        map.serialize_entry("plan", &self.plan)?;
        map.serialize_entry("windows", &Pairs(&self.windows))?;
        if let Some(token) = &self.token {
            map.serialize_entry("token", token)?;
        }
        if let Some(reason) = &self.reason {
            map.serialize_entry("reason", reason)?;
        }
        if let Some(served) = &self.served {
            map.serialize_entry("age_s", &served.age_s)?;
            map.serialize_entry("from_cache", &served.from_cache)?;
            if let Some(failed) = &served.update_failed {
                map.serialize_entry("update_failed", failed)?;
            }
        }
        map.end()
    }
}

/// What the cache did for a reading.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CacheInfo {
    pub from_cache: bool,
    pub min_interval_s: u64,
    /// Seconds since the last attempt, valid or not; `None` before the first.
    pub attempt_age_s: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One reading of every provider: what `budget` answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// When it was read, local time to the second; from the cache, when it was last attempted.
    pub generated: Option<String>,
    /// Each provider's reading, under its id.
    pub providers: Vec<(String, ProviderUsage)>,
    /// Only from the cache.
    pub cache: Option<CacheInfo>,
}

impl Reading {
    #[must_use]
    pub fn provider(&self, id: &str) -> Option<&ProviderUsage> {
        self.providers.iter().find(|(key, _)| key == id).map(|(_, usage)| usage)
    }
}

impl Serialize for Reading {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("generated", &self.generated)?;
        map.serialize_entry("provider", &Pairs(&self.providers))?;
        if let Some(cache) = &self.cache {
            map.serialize_entry("cache", cache)?;
        }
        map.end()
    }
}

/// Python's `round(x, 1)`: the nearest tenth of the exact binary value, an exact tie to even, which
/// is what Rust's formatting does too.
#[must_use]
pub fn round_to_tenth(value: f64) -> f64 {
    format!("{value:.1}").parse().unwrap_or(value)
}

/// A float as Python's `json.dumps` writes one: `14.0`, never `14`.
fn float(value: f64) -> Value {
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}

struct Pairs<'a, T>(&'a [(String, T)]);

impl<T: Serialize> Serialize for Pairs<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

fn serialize_pairs<S: Serializer>(serializer: S, pairs: Vec<(String, Value)>) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(pairs.len()))?;
    for (key, value) in &pairs {
        map.serialize_entry(key, value)?;
    }
    map.end()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Python's `round(x, 1)` on the values where binary fractions and ties differ, recorded from 3.12.
    #[test]
    fn rounds_to_the_tenth_as_python_rounds() {
        for (value, python) in [(0.25, 0.2), (0.35, 0.3), (2.675, 2.7), (14.05, 14.1), (99.95, 100.0), (0.05, 0.1), (1.45, 1.4), (85.95, 86.0)] {
            assert_eq!(round_to_tenth(value), python, "{value}");
        }
    }

    /// The JSON of the contract, null against absent as the prototype has them.
    #[test]
    fn a_reading_writes_the_contract_keys() {
        let windows = vec![
            ("5h".to_owned(), Window::read(0.0, None)),
            ("7d".to_owned(), Window::read(30.0, Some(("2026-10-28T23:00+01:00".to_owned(), 7200)))),
        ];
        let claude = ProviderUsage::new("claude", Some("max".to_owned()), windows, None, None);
        let expired = TokenState { status: TokenStatus::Expired, expires_at: Some("2026-11-11T15:37:02+01:00".to_owned()), expires_in_s: Some(0), reason: None, http_status: None };
        let codex = ProviderUsage::unknown("codex", None, "token expired", Some(expired));
        let reading = Reading { generated: Some("2026-10-23T02:30:00+02:00".to_owned()), providers: vec![("claude".to_owned(), claude), ("codex".to_owned(), codex)], cache: None };
        assert_eq!(
            serde_json::to_value(&reading).unwrap(),
            json!({
                "generated": "2026-10-23T02:30:00+02:00",
                "provider": {
                    "claude": {"provider": "claude", "status": "ok", "plan": "max", "windows": {
                        "5h": {"status": "ok", "used_pct": 0.0, "remaining_pct": 100.0, "open": false, "reset": null, "reset_in_s": null},
                        "7d": {"status": "ok", "used_pct": 30.0, "remaining_pct": 70.0, "reset": "2026-10-28T23:00+01:00", "reset_in_s": 7200}}},
                    "codex": {"provider": "codex", "status": "unknown", "plan": null, "windows": {}, "reason": "token expired",
                        "token": {"status": "expired", "expires_at": "2026-11-11T15:37:02+01:00", "expires_in_s": 0}}
                }
            })
        );
        let off = ProviderUsage::probe_off("claude", "off");
        assert_eq!(serde_json::to_value(&off).unwrap()["status"], "probe_off");
        assert_eq!(off.to_v1(), None, "a reading the user refused is never stored");
    }

    /// What one implementation stores, the other reads back the same.
    #[test]
    fn the_v1_codec_round_trips() {
        let refused = TokenState { status: TokenStatus::Refused, expires_at: None, expires_in_s: None, reason: Some("exp unreadable".to_owned()), http_status: Some(401) };
        let windows = vec![("5h".to_owned(), Window::unknown("field missing")), ("7d".to_owned(), Window::read(12.34, Some(("x".to_owned(), 5))))];
        for usage in [
            ProviderUsage::new("codex", Some("plus".to_owned()), windows, Some(refused), None),
            ProviderUsage::unknown("claude", None, "timeout", None),
        ] {
            let stored = usage.to_v1().expect("stored");
            assert_eq!(ProviderUsage::from_v1("ignored", &stored), Some(usage));
        }
        assert_eq!(ProviderUsage::from_v1("claude", &json!({})), None, "an empty record is no reading");
        assert_eq!(ProviderUsage::new("claude", None, Vec::new(), None, None).reason.as_deref(), Some(NO_DATA));
    }
}
