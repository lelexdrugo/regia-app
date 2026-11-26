//! The budget's gate (§6.4 point 6 of the dossier's design note): before a project with new activity
//! starts, Claude's 5-hour window from the cache of the usage probes, as the prototype's `controlla_budget`
//! reads it. The product adds what the prototype has no notion of (S1 of `docs/app/piano/da-decidere.md`):
//!
//! - **the probes the user refused** (`probe_off`, #463): nothing is read, the gate is open, backfill is
//!   held, and there is no alarm;
//! - **Claude's window that the probes never read on this system** (#463/V11: on macOS, with the consent,
//!   Claude reads unknown by design): it is no failure, and goes as the refusal does;
//! - **a reading that keeps failing**, with the consent given: from the fourth tick in a row that asked
//!   for it, the alarm, whose text names the setting that turns the probes off. Until the user does, no
//!   step starts: an unknown budget keeps the gate closed, as in the prototype.
//!
//! The three are told apart by their type, [`BudgetSource`], never by the words of a reason.

use regia_agents::{Platform, ProbeConsent};
use regia_store::{ProviderUsage, UsageStatus, WallTime, Window};
use serde_json::Value;

use super::line::Json;
use super::v1::format;

/// What the gate reads.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetSource {
    /// The user refused the usage probes: nothing was read.
    ProbesOff,
    /// The probes have the consent, but on this system they never read Claude's window: it is unknown by
    /// design, not because a reading failed.
    NeverRead,
    /// Claude's reading from the cache of the probes, `None` when the cache holds none.
    Read(Option<Box<ProviderUsage>>),
}

impl BudgetSource {
    /// What the consent and the system say before any reading: `None` when the cache decides. Where the
    /// probes never read Claude ([`Platform::probes_read_claude`], macOS), whatever the cache holds of it
    /// was not read by them.
    #[must_use]
    pub fn before_reading(consent: ProbeConsent, platform: Platform) -> Option<BudgetSource> {
        match consent {
            ProbeConsent::Refused => Some(BudgetSource::ProbesOff),
            ProbeConsent::Granted if !platform.probes_read_claude() => Some(BudgetSource::NeverRead),
            ProbeConsent::Granted => None,
        }
    }
}

/// How a reading went, for the alarm's count.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// The window is known: fresh, or stale and of the same window. It resets the count.
    Known,
    /// The reading failed or is too old: it counts, with its reason.
    Unknown(String),
    /// Nothing to count: the probes are off, or never read Claude here. It clears the count.
    NotProbed,
}

/// What the gate decided: whether automatic work may start, the share of the window when it is known
/// (backfill compares it with its own threshold), the kind of the reading, and `payload.budget`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Check {
    pub(crate) open: bool,
    pub(crate) pct: Option<f64>,
    pub(crate) kind: Kind,
    pub(crate) detail: Vec<(String, Json)>,
}

impl Check {
    fn known(open: bool, pct: f64, extra: Vec<(String, Json)>) -> Check {
        let mut detail = vec![(format().budget.pct.clone(), Json::Float(pct))];
        detail.extend(extra);
        Check { open, pct: Some(pct), kind: Kind::Known, detail }
    }

    fn unknown(reason: &str) -> Check {
        let words = &format().budget;
        let reason: String = reason.chars().take(words.reason_length).collect();
        let detail = vec![(words.pct.clone(), Json::Null), (words.reason.clone(), Json::Str(reason.clone()))];
        Check { open: false, pct: None, kind: Kind::Unknown(reason), detail }
    }

    fn not_probed(state: &str) -> Check {
        let detail = vec![(format().budget.probe.clone(), Json::Str(state.to_owned()))];
        Check { open: true, pct: None, kind: Kind::NotProbed, detail }
    }
}

/// `template` with each `{name}` replaced.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(template.to_owned(), |text, (name, value)| text.replace(&format!("{{{name}}}"), value))
}

/// Python's `f"{x:.1f}"`: the nearest tenth of the binary value, a tie to even, as Rust's formatting does.
fn one_decimal(value: f64) -> String {
    format!("{value:.1}")
}

/// Python's `f"{x:g}"`: six significant digits, the trailing zeros and point dropped, in exponent form
/// below 1e-4 and from 1e6.
pub(crate) fn python_g(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() { "nan".to_owned() } else if value > 0.0 { "inf".to_owned() } else { "-inf".to_owned() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0".to_owned() } else { "0".to_owned() };
    }
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let trim = |text: &str| -> String {
        if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text.to_owned() }
    };
    if !(-4..6).contains(&exponent) {
        return format!("{}e{}{:02}", trim(mantissa), if exponent < 0 { '-' } else { '+' }, exponent.abs());
    }
    let decimals = usize::try_from(5 - exponent).unwrap_or(0);
    trim(&format!("{value:.decimals$}"))
}

/// A reason of the cache that says something: Python's truth of a string.
fn said(reason: Option<&String>) -> Option<&str> {
    reason.map(String::as_str).filter(|reason| !reason.is_empty())
}

/// The gate at `now`, with `max` the threshold of the 5-hour window: the prototype's `controlla_budget`
/// on a reading of the cache, and the product's two states without a reading.
pub(crate) fn check(source: &BudgetSource, max: f64, now: WallTime) -> Check {
    let words = &format().budget;
    let claude = match source {
        BudgetSource::ProbesOff => return Check::not_probed(&words.probes_off),
        BudgetSource::NeverRead => return Check::not_probed(&words.never_read),
        BudgetSource::Read(None) => return Check::unknown(&words.no_data),
        BudgetSource::Read(Some(claude)) => claude.as_ref(),
    };
    // Why the last attempt failed, else why the reading is unknown.
    let served = claude.served.as_ref();
    let failure = said(served.and_then(|served| served.update_failed.as_ref())).or_else(|| said(claude.reason.as_ref()));
    let mut reason = match failure {
        Some(failure) => fill(&words.probe_failed, &[("reason", failure)]),
        None => words.no_data.clone(),
    };
    let Some(window) = claude.window(&words.window) else { return Check::unknown(&reason) };
    let age = served.and_then(|served| served.age_s);
    if let (UsageStatus::Ok, true, Some(pct), Some(age)) = (claude.status, window.is_ok(), window.used_pct(), age) {
        let not_open = matches!(window, Window::NotOpen { .. });
        if age <= words.fresh_s {
            let extra = if not_open { vec![(words.not_open.clone(), Json::Bool(false))] } else { Vec::new() };
            return Check::known(pct < max, pct, extra);
        }
        let hours = one_decimal(age as f64 / 3600.0);
        // Two forms the probes never write, where the product does not take the prototype's reading (#466/V12, #600),
        // and the cases of `fixtures/precheck/events/budget-cache-codec` do not go:
        // - A reset without its offset is an instant in UTC here. The prototype's `_parse_reset` reads it in the
        //   machine's time zone, so its answer for a reset nearer than the zones' spread depends on the machine, which no
        //   recording can fix; a day ahead or back, the two agree, and the case fixes that.
        // - A window with neither a reset nor the flag of a window not open is not open: the store's codec
        //   (`Window::from_v1` of `regia-store`) has one form for a window without a reset. The prototype reads it as
        //   open with a reset it cannot read: fresh, the line has no flag of a window not open; old, its reason is the
        //   expired window's. The gate decides the same on both sides.
        let reset = match window {
            Window::Open { reset, .. } => WallTime::parse_iso(reset),
            Window::NotOpen { .. } | Window::Unknown { .. } => None,
        };
        let old = if reset.is_some_and(|reset| now < reset) {
            if pct < max {
                return Check::known(true, pct, vec![(words.stale.clone(), Json::Bool(true)), (words.age.clone(), Json::Int(age))]);
            }
            fill(&words.same_window_above, &[("hours", &hours), ("pct", &python_g(pct))])
        } else if not_open {
            fill(&words.window_not_open, &[("hours", &hours)])
        } else {
            fill(&words.window_expired, &[("hours", &hours)])
        };
        reason = if failure.is_some() { fill(&words.joined, &[("reason", &reason), ("old", &old)]) } else { old };
    }
    Check::unknown(&reason)
}

/// The count of the ticks in a row whose reading failed, as the detector keeps it: how many, since when,
/// and why the last one failed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Streak {
    pub(crate) rounds: i64,
    pub(crate) since: String,
}

impl Streak {
    /// The count in the detector's row, `None` when there is none or it does not read: it starts again.
    pub(crate) fn from_row(row: &Value) -> Option<Streak> {
        let words = (&format().detector, &format().budget);
        let streak = row.get(&words.0.budget)?;
        let rounds = streak.get(&words.1.rounds)?.as_i64().filter(|rounds| *rounds > 0)?;
        let since = streak.get(&words.1.since)?.as_str()?.to_owned();
        Some(Streak { rounds, since })
    }

    /// The detector's row with this count, or with none.
    pub(crate) fn row(streak: Option<&Streak>, reason: Option<&str>) -> Value {
        let words = (&format().detector, &format().budget);
        let value = streak.map_or(Value::Null, |streak| {
            serde_json::json!({words.1.rounds.as_str(): streak.rounds, words.1.since.as_str(): streak.since, words.1.reason.as_str(): reason})
        });
        serde_json::json!({words.0.version.as_str(): 1, words.0.budget.as_str(): value})
    }

    /// The count after a tick of this kind at `now`: one more for a failed reading, none otherwise.
    pub(crate) fn after(previous: Option<&Streak>, kind: &Kind, now: &str) -> Option<Streak> {
        match kind {
            Kind::Unknown(_) => Some(match previous {
                Some(previous) => Streak { rounds: previous.rounds + 1, since: previous.since.clone() },
                None => Streak { rounds: 1, since: now.to_owned() },
            }),
            Kind::Known | Kind::NotProbed => None,
        }
    }

    /// Whether the count is the alarm's.
    pub(crate) fn alarms(&self) -> bool {
        self.rounds >= format().budget.alarm_rounds
    }

    /// The alarm's part of `payload.budget`: the count, since when, and the text that names the setting.
    pub(crate) fn alarm_detail(&self) -> Vec<(String, Json)> {
        let words = &format().budget;
        vec![
            (words.rounds.clone(), Json::Int(self.rounds)),
            (words.since.clone(), Json::Str(self.since.clone())),
            (words.alarm.clone(), Json::Str(fill(&words.alarm_text, &[("rounds", &self.rounds.to_string())]))),
        ]
    }
}

#[cfg(test)]
mod tests {
    use regia_store::{Served, TokenState};

    use super::*;

    const NOW: &str = "2026-01-10T09:00:00Z";

    fn now() -> WallTime {
        WallTime::parse_iso(NOW).unwrap()
    }

    /// Claude's reading as the cache composes it: `window` as the 5-hour one, the valid reading `age_s`
    /// old, and why the last attempt failed.
    fn claude(window: Window, age_s: Option<i64>, update_failed: Option<&str>) -> BudgetSource {
        let mut usage = ProviderUsage::new("claude", None, vec![(format().budget.window.clone(), window)], None, None);
        usage.served = Some(Served { age_s, from_cache: true, update_failed: update_failed.map(str::to_owned) });
        BudgetSource::Read(Some(Box::new(usage)))
    }

    fn open(pct: f64, reset: &str) -> Window {
        Window::read(pct, Some((reset.to_owned(), 0)))
    }

    fn line(check: &Check) -> String {
        Json::Object(check.detail.clone()).to_ascii()
    }

    fn key(name: &str) -> String {
        format!("\"{name}\"")
    }

    #[test]
    fn a_fresh_reading_is_known_and_the_window_not_open_says_so() {
        let words = &format().budget;
        let fresh = check(&claude(open(10.0, "2026-01-10T11:00+01:00"), Some(0), None), 75.0, now());
        assert_eq!((fresh.open, fresh.pct, &fresh.kind), (true, Some(10.0), &Kind::Known));
        assert_eq!(line(&fresh), format!("{{{}:10.0}}", key(&words.pct)));
        let full = check(&claude(open(75.0, "2026-01-10T11:00+01:00"), Some(3600), None), 75.0, now());
        assert_eq!((full.open, full.kind), (false, Kind::Known), "the threshold itself is full, an hour old is still fresh");
        let closed = check(&claude(Window::read(0.0, None), Some(60), None), 75.0, now());
        assert_eq!(line(&closed), format!("{{{}:0.0,{}:false}}", key(&words.pct), key(&words.not_open)));
    }

    #[test]
    fn an_old_reading_of_the_same_window_below_the_threshold_is_stale() {
        let words = &format().budget;
        let failed = Some("too many requests (HTTP 429)");
        let stale = check(&claude(open(30.0, "2026-01-10T11:00:00+00:00"), Some(10_800), failed), 75.0, now());
        assert_eq!((stale.open, stale.kind.clone()), (true, Kind::Known));
        assert_eq!(line(&stale), format!("{{{}:30.0,{}:true,{}:10800}}", key(&words.pct), key(&words.stale), key(&words.age)));
        // Above the threshold it stays unknown, and the reason says why: the probe's failure first.
        let above = check(&claude(open(80.0, "2026-01-10T10:00:00+00:00"), Some(10_800), failed), 75.0, now());
        let expected = fill(
            &words.joined,
            &[("reason", &fill(&words.probe_failed, &[("reason", "too many requests (HTTP 429)")])), ("old", &fill(&words.same_window_above, &[("hours", "3.0"), ("pct", "80")]))],
        );
        assert_eq!(above.kind, Kind::Unknown(expected));
        assert!(!above.open && above.pct.is_none());
    }

    #[test]
    fn an_expired_window_or_one_never_opened_is_unknown() {
        let words = &format().budget;
        let expired = check(&claude(open(10.0, NOW), Some(7200), None), 75.0, now());
        assert_eq!(expired.kind, Kind::Unknown(fill(&words.window_expired, &[("hours", "2.0")])), "a reset at this very instant has passed");
        let never = check(&claude(Window::read(0.0, None), Some(7200), None), 75.0, now());
        assert_eq!(never.kind, Kind::Unknown(fill(&words.window_not_open, &[("hours", "2.0")])));
    }

    #[test]
    fn without_a_valid_reading_the_reason_is_the_probe_s_or_none() {
        let words = &format().budget;
        assert_eq!(check(&BudgetSource::Read(None), 75.0, now()).kind, Kind::Unknown(words.no_data.clone()));
        let failed = ProviderUsage::unknown("claude", None, "timeout", Some(TokenState { status: regia_store::TokenStatus::Unknown, expires_at: None, expires_in_s: None, reason: None, http_status: None }));
        let failed = check(&BudgetSource::Read(Some(Box::new(failed))), 75.0, now());
        assert_eq!(failed.kind, Kind::Unknown(fill(&words.probe_failed, &[("reason", "timeout")])));
        assert_eq!(line(&failed), format!("{{{}:null,{}:\"{}\"}}", key(&words.pct), key(&words.reason), fill(&words.probe_failed, &[("reason", "timeout")])));
        let long = ProviderUsage::unknown("claude", None, "x".repeat(400), None);
        let Kind::Unknown(cut) = check(&BudgetSource::Read(Some(Box::new(long))), 75.0, now()).kind else { panic!("unknown") };
        assert_eq!(cut.chars().count(), words.reason_length);
    }

    #[test]
    fn the_refused_probes_and_the_window_never_read_open_the_gate_and_are_not_counted() {
        let words = &format().budget;
        for (source, state) in [(BudgetSource::ProbesOff, &words.probes_off), (BudgetSource::NeverRead, &words.never_read)] {
            let gate = check(&source, 75.0, now());
            assert_eq!((gate.open, gate.pct, &gate.kind), (true, None, &Kind::NotProbed), "{source:?}");
            assert_eq!(line(&gate), format!("{{{}:\"{state}\"}}", key(&words.probe)));
        }
        assert_eq!(BudgetSource::before_reading(ProbeConsent::Refused, Platform::Other), Some(BudgetSource::ProbesOff));
        assert_eq!(BudgetSource::before_reading(ProbeConsent::Refused, Platform::MacOs), Some(BudgetSource::ProbesOff));
        assert_eq!(BudgetSource::before_reading(ProbeConsent::Granted, Platform::MacOs), Some(BudgetSource::NeverRead));
        assert_eq!(BudgetSource::before_reading(ProbeConsent::Granted, Platform::Other), None, "the cache decides");
    }

    #[test]
    fn the_streak_grows_with_failed_readings_and_ends_with_any_other() {
        let failed = Kind::Unknown("timeout".to_owned());
        let mut streak = None;
        for round in 1..=5 {
            streak = Streak::after(streak.as_ref(), &failed, &format!("t{round}"));
            let current = streak.as_ref().unwrap();
            assert_eq!((current.rounds, current.since.as_str(), current.alarms()), (round, "t1", round >= 4));
        }
        let row = Streak::row(streak.as_ref(), Some("timeout"));
        assert_eq!(Streak::from_row(&row), streak, "the row reads back");
        assert_eq!(Streak::after(streak.as_ref(), &Kind::Known, "t6"), None);
        assert_eq!(Streak::after(streak.as_ref(), &Kind::NotProbed, "t6"), None);
        assert_eq!(Streak::from_row(&Streak::row(None, None)), None);
        assert_eq!(Streak::from_row(&serde_json::json!("not a row")), None);
        let alarm = Json::Object(streak.unwrap().alarm_detail()).to_ascii();
        assert!(alarm.contains("agents.toml") && alarm.contains('5'), "the text names the setting: {alarm}");
    }

    /// The fact [`BudgetSource::before_reading`] stands on: with the consent, on macOS, the probes never ask
    /// for Claude's window, while elsewhere the same credentials make them ask. If `regia-agents` starts
    /// reading Claude on macOS, this fails, and the gate must read the cache there too.
    #[test]
    fn on_macos_the_probes_never_ask_for_claude() {
        use std::sync::{Arc, Mutex};

        use regia_agents::{Bearer, CLAUDE_URL, Env, Transport, TransportError, UsageProbes};

        struct Recorded(Arc<Mutex<Vec<String>>>);
        impl Transport for Recorded {
            fn get(&self, url: &str, _bearer: &Bearer, _headers: &[(&'static str, String)]) -> Result<Vec<u8>, TransportError> {
                self.0.lock().unwrap().push(url.to_owned());
                Err(TransportError::Unreachable)
            }
        }
        let dir = std::env::current_exe().unwrap().parent().unwrap().join("regia-controller-macos-probe");
        std::fs::create_dir_all(&dir).unwrap();
        let credentials = dir.join("claude.json");
        std::fs::write(&credentials, r#"{"claudeAiOauth": {"accessToken": "not-a-token", "expiresAt": 99999999999999}}"#).unwrap();
        let env = || Env::from_pairs([(regia_identity::prototype_env_var("CLAUDE_CREDENTIALS"), credentials.as_os_str())]);
        for (platform, asked) in [(Platform::MacOs, Vec::<String>::new()), (Platform::Other, vec![CLAUDE_URL.to_owned()])] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let probes = UsageProbes::new(ProbeConsent::Granted, env(), Recorded(Arc::clone(&calls))).on_platform(platform);
            assert_eq!(probes.claude().status, UsageStatus::Unknown, "{platform:?}");
            assert_eq!(*calls.lock().unwrap(), asked, "{platform:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn numbers_print_as_python_prints_them() {
        for (value, text) in [(80.0, "80"), (74.9, "74.9"), (100.0, "100"), (0.5, "0.5"), (1_234_567.0, "1.23457e+06"), (0.0001, "0.0001"), (0.000_01, "1e-05"), (123_456.7, "123457"), (0.0, "0")] {
            assert_eq!(python_g(value), text, "{value}");
        }
        for (value, text) in [(2.25, "2.2"), (2.35, "2.4"), (3.0, "3.0"), (0.05, "0.1")] {
            assert_eq!(one_decimal(value), text, "{value}");
        }
    }
}
