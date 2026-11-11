//! The token wake-up (#190, ADR 0001 §6: "wake an expired token"). After its reset the five-hour
//! window stays closed until someone sends a message, and Claude Code renews its sign-in token only
//! while it works: with nothing running, at night, neither happens and the budget stays unknown.
//! So a round that found the budget unknown, with work waiting, may start one minimal session, at
//! most once an hour, also across restarts.
//!
//! Only with the user's consent to the usage probes, from the settings
//! ([`crate::Settings::probe_consent`]): with the probes refused there is no budget to unlock, and
//! the wake-up never starts, whatever the round says. The token's expiry around it is read by the
//! probes too ([`crate::UsageProbes::wake`]), so a refusal reads nothing there either.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::ProbeConsent;

/// How a wake-up went. A failure is a value: the wake-up never stops a round.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WakeOutcome {
    pub ok: bool,
    pub exit: Option<i32>,
    pub duration_s: Option<f64>,
    pub cost_usd: Option<f64>,
    pub error: Option<String>,
    /// Nothing was started: a surface that starts nothing wakes nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fake: bool,
    pub token_expiry_before: Option<String>,
    pub token_expiry_after: Option<String>,
    pub token_renewed: bool,
}

impl WakeOutcome {
    pub fn failed(error: impl Into<String>) -> WakeOutcome {
        WakeOutcome { error: Some(error.into()), ..WakeOutcome::default() }
    }

    pub fn fake() -> WakeOutcome {
        WakeOutcome { fake: true, ..WakeOutcome::failed("fake wake-up: no session started") }
    }

    /// `wake`, with the token's expiry read before and after it: renewed when the expiry changed. The
    /// reader gives the expiry only, never the token: the product's is
    /// [`crate::UsageProbes::claude_token_expiry`], which [`crate::UsageProbes::wake`] passes here.
    pub fn with_expiry(expiry: impl Fn() -> Option<String>, wake: impl FnOnce() -> WakeOutcome) -> WakeOutcome {
        let before = expiry();
        let mut outcome = wake();
        let after = expiry();
        outcome.token_renewed = after.is_some() && after != before;
        outcome.token_expiry_before = before;
        outcome.token_expiry_after = after;
        outcome
    }
}

/// A wake-up a round started, and when: the controller keeps the last one in its state, so the cap
/// holds across restarts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WakeAttempt {
    pub at: DateTime<Utc>,
    #[serde(flatten)]
    pub outcome: WakeOutcome,
}

/// When a round wakes the token.
#[derive(Debug, Clone)]
pub struct WakeUp {
    consent: ProbeConsent,
    off: bool,
    last: Option<WakeAttempt>,
}

impl WakeUp {
    /// At most one wake-up an hour, counted from the last attempt, whether it worked or not: a
    /// wake-up that does not help is not repeated at every round.
    pub const INTERVAL: TimeDelta = TimeDelta::hours(1);

    pub fn new(consent: ProbeConsent) -> WakeUp {
        WakeUp { consent, off: false, last: None }
    }

    /// The user turned the wake-up off.
    #[must_use]
    pub fn switched_off(mut self, off: bool) -> WakeUp {
        self.off = off;
        self
    }

    /// The last attempt of an earlier controller, from its state.
    #[must_use]
    pub fn after(mut self, last: Option<WakeAttempt>) -> WakeUp {
        self.last = last;
        self
    }

    pub fn last(&self) -> Option<&WakeAttempt> {
        self.last.as_ref()
    }

    /// Whether a round at `now` wakes the token. `budget_unknown`: the round asked for the budget,
    /// because work is waiting, and the probe could not tell it. A clock set back before the last
    /// attempt does not hold the next one back.
    pub fn due(&self, budget_unknown: bool, now: DateTime<Utc>) -> bool {
        if self.consent == ProbeConsent::Refused || self.off || !budget_unknown {
            return false;
        }
        self.last.as_ref().is_none_or(|last| {
            let since = now - last.at;
            since < TimeDelta::zero() || since >= WakeUp::INTERVAL
        })
    }

    /// The round's wake-up, when it is due: the attempt is recorded before `wake` runs, so a wake-up
    /// that never returns counts too. The attempt of this round, if any.
    pub fn round(&mut self, budget_unknown: bool, now: DateTime<Utc>, wake: impl FnOnce() -> WakeOutcome) -> Option<&WakeAttempt> {
        if !self.due(budget_unknown, now) {
            return None;
        }
        self.last = Some(WakeAttempt { at: now, outcome: WakeOutcome::failed("the wake-up did not return") });
        let outcome = wake();
        self.last = Some(WakeAttempt { at: now, outcome });
        self.last.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-23T00:30:00Z").unwrap().with_timezone(&Utc) + TimeDelta::minutes(minutes)
    }

    fn renewed() -> WakeOutcome {
        WakeOutcome { ok: true, exit: Some(0), token_renewed: true, ..WakeOutcome::default() }
    }

    /// Acceptance 2 of #462: with the probes refused, no wake-up in 100 rounds that would each start
    /// one, and the token's expiry is never read. With consent the same rounds wake every time.
    #[test]
    fn with_the_probes_refused_a_hundred_rounds_wake_nothing() {
        for (consent, expected) in [(ProbeConsent::Refused, 0), (ProbeConsent::Granted, 100)] {
            let (wakes, reads) = (Cell::new(0), Cell::new(0));
            let mut wake_up = WakeUp::new(consent);
            for round in 0..100 {
                let now = at(round * 61);
                let attempt = wake_up.round(true, now, || {
                    WakeOutcome::with_expiry(
                        || {
                            reads.set(reads.get() + 1);
                            None
                        },
                        || {
                            wakes.set(wakes.get() + 1);
                            renewed()
                        },
                    )
                });
                assert_eq!(attempt.is_some(), consent == ProbeConsent::Granted, "round {round}");
            }
            assert_eq!(wakes.get(), expected, "{consent:?}");
            assert_eq!(reads.get(), 2 * expected, "{consent:?}: the expiry is read only around a wake-up");
        }
        let refused = WakeUp::new(ProbeConsent::Refused).after(None);
        assert!(!refused.due(true, at(0)), "nothing else turns it on");
    }

    /// One wake-up, then none within the hour, then the next one.
    #[test]
    fn with_the_budget_unknown_one_wake_up_and_no_other_within_the_hour() {
        let wakes = Cell::new(0);
        let mut wake_up = WakeUp::new(ProbeConsent::Granted);
        let wake = || {
            wakes.set(wakes.get() + 1);
            renewed()
        };
        let attempt = wake_up.round(true, at(0), wake).cloned().expect("a wake-up");
        assert!(attempt.outcome.ok && attempt.outcome.token_renewed);
        assert_eq!(attempt.at, at(0));
        for minutes in [15, 30, 45] {
            assert!(wake_up.round(true, at(minutes), wake).is_none(), "{minutes} minutes later");
        }
        assert_eq!(wakes.get(), 1);
        assert!(wake_up.round(true, at(60), wake).is_some(), "an hour after the first: once more");
        assert_eq!(wakes.get(), 2);
    }

    /// The controller keeps the last attempt in its state: a new one, ten minutes later, waits.
    #[test]
    fn the_cap_holds_after_a_restart() {
        let mut first = WakeUp::new(ProbeConsent::Granted);
        first.round(true, at(0), renewed);
        let state = serde_json::to_value(first.last()).unwrap();
        assert_eq!(state["at"], "2026-10-23T00:30:00Z");
        let kept: WakeAttempt = serde_json::from_value(state).unwrap();
        let wakes = Cell::new(0);
        let mut second = WakeUp::new(ProbeConsent::Granted).after(Some(kept));
        let attempt = second.round(true, at(10), || {
            wakes.set(1);
            renewed()
        });
        assert!(attempt.is_none());
        assert_eq!(wakes.get(), 0);
    }

    /// The budget known, even full, or not asked because no work waits, or the wake-up turned off.
    #[test]
    fn no_wake_up_with_the_budget_known_without_work_or_switched_off() {
        let wakes = Cell::new(0);
        for (budget_unknown, off) in [(false, false), (true, true)] {
            let mut wake_up = WakeUp::new(ProbeConsent::Granted).switched_off(off);
            wake_up.round(budget_unknown, at(0), || {
                wakes.set(wakes.get() + 1);
                renewed()
            });
        }
        assert_eq!(wakes.get(), 0);
    }

    /// A wake-up that fails does not stop the round, and counts as an attempt: none again within the
    /// hour.
    #[test]
    fn a_failed_wake_up_does_not_stop_the_round_and_counts_as_an_attempt() {
        let wakes = Cell::new(0);
        let mut wake_up = WakeUp::new(ProbeConsent::Granted);
        let wake = || {
            wakes.set(wakes.get() + 1);
            WakeOutcome::failed("boom")
        };
        let attempt = wake_up.round(true, at(0), wake).cloned().expect("an attempt");
        assert!(!attempt.outcome.ok);
        assert_eq!(attempt.outcome.error.as_deref(), Some("boom"));
        assert!(wake_up.round(true, at(15), wake).is_none());
        assert_eq!(wakes.get(), 1);
    }

    /// A clock set back before the last attempt does not hold the next wake-up back, as the
    /// prototype's `0 <= now - last` has it.
    #[test]
    fn a_clock_set_back_does_not_hold_the_wake_up() {
        let wake_up = WakeUp::new(ProbeConsent::Granted).after(Some(WakeAttempt { at: at(60), outcome: renewed() }));
        assert!(wake_up.due(true, at(0)));
        assert!(!wake_up.due(true, at(90)));
    }

    /// Renewed when the expiry changed; the same expiry, or none, is not a renewal.
    #[test]
    fn the_token_is_renewed_when_its_expiry_changes() {
        let expiries = std::cell::RefCell::new(vec![Some("2026-10-23T18:31:02+02:00"), Some("2026-10-23T03:05:23+02:00")]);
        let outcome = WakeOutcome::with_expiry(|| expiries.borrow_mut().pop().flatten().map(str::to_owned), renewed);
        assert!(outcome.token_renewed);
        assert_eq!(outcome.token_expiry_before.as_deref(), Some("2026-10-23T03:05:23+02:00"));
        assert_eq!(outcome.token_expiry_after.as_deref(), Some("2026-10-23T18:31:02+02:00"));
        let same = WakeOutcome::with_expiry(|| Some("2026-10-24T03:05:23+02:00".to_owned()), renewed);
        assert!(!same.token_renewed);
        assert!(!WakeOutcome::with_expiry(|| None, renewed).token_renewed);
        assert!(WakeOutcome::fake().fake && !WakeOutcome::fake().ok);
    }
}
