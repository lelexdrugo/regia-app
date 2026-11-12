//! The text of `budget` (#463/V6): a line per provider as the prototype's `usage.formatta` writes it, in
//! the catalog's words, and from the cache the notes its `cmd_budget` adds. The JSON is the contract; this
//! is the user's.

use std::fmt::Display;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use regia_agents::TOKEN_WARNING_S;
use regia_core::policy;
use regia_i18n::Catalog;
use regia_store::{ProviderUsage, Reading, TokenState, UsageStatus, Window};

use crate::python_g;

/// The windows a line shows, in this order: the prototype's `FINESTRE`.
const WINDOWS: [&str; 2] = ["5h", "7d"];
/// From this many seconds away a reset shows its day as well as its hour.
const SAME_DAY_S: i64 = 20 * 3600;

/// The text of `reading`, a line per provider in its order: local times in `zone`, and the token's
/// expiry against `now`.
pub(crate) fn lines<Tz: TimeZone>(reading: &Reading, now: DateTime<Utc>, zone: &Tz, catalog: &Catalog) -> Vec<String>
where
    Tz::Offset: Display,
{
    reading.providers.iter().map(|(_, usage)| line(usage, now, zone, catalog)).collect()
}

fn line<Tz: TimeZone>(usage: &ProviderUsage, now: DateTime<Utc>, zone: &Tz, catalog: &Catalog) -> String
where
    Tz::Offset: Display,
{
    let name = match usage.provider.as_str() {
        "claude" => "Claude",
        "codex" => "Codex",
        other => other,
    };
    let plan = usage.plan.as_deref().filter(|plan| !plan.is_empty()).map(|plan| format!(" [{plan}]")).unwrap_or_default();
    let body = match usage.status {
        UsageStatus::Unknown => catalog.message_with("budget-unknown", "reason", usage.reason.as_deref().unwrap_or_default()),
        UsageStatus::ProbeOff => catalog.message("budget-probe-off"),
        UsageStatus::Ok => {
            let mut parts: Vec<String> =
                WINDOWS.iter().filter_map(|key| usage.window(key).map(|window| window_part(key, window, catalog))).collect();
            parts.extend(usage.token.as_ref().and_then(|token| token_warning(token, now, zone, catalog)));
            parts.join("  |  ")
        }
    };
    let mut line = format!("{name}{plan}: {body}");
    if let Some(served) = &usage.served {
        if let Some(age) = served.age_s.filter(|age| *age != 0) {
            line.push_str("  ");
            line.push_str(&catalog.message_with("budget-age", "span", &span(age, catalog)));
        }
        if let Some(failed) = served.update_failed.as_deref().filter(|failed| !failed.is_empty()) {
            line.push_str("  ");
            line.push_str(&catalog.message_with("budget-update-failed", "reason", failed));
        }
    }
    line
}

/// A window: its use, and its reset or that it is not open yet; or why it is unknown.
fn window_part(key: &str, window: &Window, catalog: &Catalog) -> String {
    let label = catalog.message(&format!("budget-window-{key}"));
    match window {
        Window::Unknown { reason } => format!("{label} {}", catalog.message_with("budget-unknown", "reason", reason)),
        Window::NotOpen { used_pct, .. } => {
            format!("{label} {} · {}", catalog.message_with("budget-used", "pct", &python_g(*used_pct)), catalog.message("budget-not-open"))
        }
        Window::Open { used_pct, reset, reset_in_s, .. } => format!(
            "{label} {} · {} {}",
            catalog.message_with("budget-used", "pct", &python_g(*used_pct)),
            catalog.message_with("budget-reset", "when", &reset_time(reset, *reset_in_s)),
            catalog.message_with("budget-reset-in", "span", &span(*reset_in_s, catalog)),
        ),
    }
}

/// The reset as the reading wrote it, local time to the minute: its hour, and its day too from 20 hours
/// away.
fn reset_time(reset: &str, reset_in_s: i64) -> String {
    let Ok(at) = NaiveDateTime::parse_from_str(reset.get(..16).unwrap_or(reset), "%Y-%m-%dT%H:%M") else {
        return reset.to_owned();
    };
    at.format(if reset_in_s < SAME_DAY_S { "%H:%M" } else { "%d/%m %H:%M" }).to_string()
}

/// The prototype's `_tra`: days and hours from two days on, hours and minutes from an hour, else minutes.
fn span(seconds: i64, catalog: &Catalog) -> String {
    let minutes = seconds.div_euclid(60);
    let count = |id: &str, n: String| catalog.message_with(id, "n", &n);
    if minutes >= 48 * 60 {
        format!("{} {}", count("budget-days", (minutes / 1440).to_string()), count("budget-hours", (minutes % 1440 / 60).to_string()))
    } else if minutes >= 60 {
        format!("{} {}", count("budget-hours", (minutes / 60).to_string()), count("budget-minutes", format!("{:02}", minutes % 60)))
    } else {
        count("budget-minutes", minutes.to_string())
    }
}

/// The warning on Codex's token, from its expiry against `now`: none a day or more before it, as the
/// prototype's `_avviso_token`.
fn token_warning<Tz: TimeZone>(token: &TokenState, now: DateTime<Utc>, zone: &Tz, catalog: &Catalog) -> Option<String>
where
    Tz::Offset: Display,
{
    let expires = policy::instant(token.expires_at.as_deref()?, zone)?;
    let left = (expires - now).num_seconds();
    if left >= TOKEN_WARNING_S {
        return None;
    }
    let when = expires.with_timezone(zone).format("%d/%m %H:%M").to_string();
    Some(if left <= 0 {
        catalog.message_with("budget-token-expired", "when", &when)
    } else {
        format!("{} {}", catalog.message_with("budget-token-expiring", "span", &span(left, catalog)), catalog.message_with("budget-token-renew", "when", &when))
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::FixedOffset;
    use regia_i18n::Language;
    use regia_store::{Served, TokenStatus};
    use serde_json::Value;

    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-12T10:00:00Z").unwrap().with_timezone(&Utc)
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn cases() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("fixtures").join("budget").join("cases")
    }

    /// What the prototype printed for `budget-text`, both providers unknown, with LF line breaks.
    fn python_text() -> String {
        std::fs::read_to_string(cases().join("budget-text").join("python.it.txt")).unwrap().replace("\r\n", "\n")
    }

    fn text(reading: &Reading, catalog: &Catalog) -> String {
        lines(reading, now(), &utc(), catalog).iter().map(|line| format!("{line}\n")).collect()
    }

    /// #463/V6: the reading the prototype recorded for `budget-unknown`, read as the cache reads it, prints
    /// in Italian what the prototype printed for `budget-text`, the same case in text, reasons included.
    #[test]
    fn the_text_is_the_prototype_s() {
        let recorded: Value = serde_json::from_str(&std::fs::read_to_string(cases().join("budget-unknown").join("python.it.json")).unwrap()).unwrap();
        let providers: Vec<(String, ProviderUsage)> = recorded["provider"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), ProviderUsage::from_v1(key, value).unwrap()))
            .collect();
        let reading = Reading { generated: None, providers, cache: None };
        assert_eq!(text(&reading, &Catalog::new(Language::It)), python_text());
    }

    /// The product's own reading of that case has its reasons in English (#463): the frame stays the
    /// prototype's.
    #[test]
    fn the_product_s_reasons_keep_the_prototype_s_frame() {
        let unknown = |provider: &str| (provider.to_owned(), ProviderUsage::unknown(provider, None, "credentials not found", None));
        let reading = Reading { generated: None, providers: vec![unknown("claude"), unknown("codex")], cache: None };
        let frame = |line: &str| line.split_once(" (").map(|(head, _)| head.to_owned());
        let ours = text(&reading, &Catalog::new(Language::It));
        assert_eq!(ours.lines().map(frame).collect::<Vec<_>>(), python_text().lines().map(frame).collect::<Vec<_>>());
        assert!(ours.lines().all(|line| line.ends_with("(credentials not found)")), "{ours}");
    }

    /// `TestFinestraNonAperta.test_il_testo_di_regia_budget_dice_non_aperta`: the text says the window is
    /// not open.
    #[test]
    fn the_text_says_a_window_is_not_open() {
        let windows = vec![("5h".to_owned(), Window::read(0.0, None)), ("7d".to_owned(), Window::read(30.0, Some(("2099-01-01T00:00+00:00".to_owned(), 60))))];
        let reading = Reading { generated: None, providers: vec![("claude".to_owned(), ProviderUsage::new("claude", Some("max".to_owned()), windows, None, None))], cache: None };
        let text = text(&reading, &Catalog::new(Language::It));
        assert!(text.contains("5h 0% usato · finestra non aperta"), "{text}");
        assert!(text.contains("7g 30% usato"), "{text}");
    }

    /// Open windows with their resets, the token's warning, the probes refused, and the cache's notes.
    #[test]
    fn resets_the_token_and_the_cache_s_notes() {
        let windows = vec![
            ("5h".to_owned(), Window::read(10.0, Some(("2026-11-12T15:50+01:00".to_owned(), 17_580)))),
            ("7d".to_owned(), Window::read(19.0, Some(("2026-11-18T23:00+01:00".to_owned(), 6 * 86_400 + 11 * 3600)))),
        ];
        let claude = ProviderUsage::new("claude", Some("max".to_owned()), windows, None, None);
        let token = |expires_at: &str| TokenState {
            status: TokenStatus::Expiring,
            expires_at: Some(expires_at.to_owned()),
            expires_in_s: None,
            reason: None,
            http_status: None,
        };
        let not_open = vec![("5h".to_owned(), Window::read(0.0, None))];
        let mut codex = ProviderUsage::new("codex", Some("plus".to_owned()), not_open.clone(), Some(token("2026-11-12T22:00:00+00:00")), None);
        codex.served = Some(Served { age_s: Some(120), from_cache: true, update_failed: Some("timeout".to_owned()) });
        let en = Catalog::new(Language::En);
        let reading = Reading { generated: None, providers: vec![("claude".to_owned(), claude), ("codex".to_owned(), codex)], cache: None };
        assert_eq!(
            lines(&reading, now(), &utc(), &en),
            [
                "Claude [max]: 5h 10% used · resets 15:50 (in 4 h 53 min)  |  7d 19% used · resets 18/11 23:00 (in 6 d 11 h)",
                "Codex [plus]: 5h 0% used · window not open (it opens at the first message)  |  token expires in 12 h 00 min (on 12/11 22:00): run Codex to renew it  [data from 2 min ago]  [update failed: timeout]",
            ]
        );
        let expired = ProviderUsage::new("codex", None, not_open.clone(), Some(token("2026-11-12T09:00:00+00:00")), None);
        let far = ProviderUsage::new("codex", None, not_open, Some(token("2026-11-14T10:00:00+00:00")), None);
        let off = ProviderUsage::probe_off("claude", "off");
        let reading = Reading { generated: None, providers: vec![("codex".to_owned(), expired), ("codex".to_owned(), far), ("claude".to_owned(), off)], cache: None };
        let lines = lines(&reading, now(), &utc(), &en);
        assert!(lines[0].ends_with("token expired on 12/11 09:00: run Codex to renew it"), "{}", lines[0]);
        assert!(!lines[1].contains("token"), "a day or more before its expiry, no warning: {}", lines[1]);
        assert_eq!(lines[2], "Claude: usage probes off: you did not consent to them");
    }
}
