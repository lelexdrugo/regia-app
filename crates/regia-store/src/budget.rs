//! The budget cache, the prototype's `sonda_cache` (F044 of the dossier's inventory): the providers
//! are asked at most once per interval, counted from the last attempt whether it worked or not, so a
//! 429 is not pressed; and each provider keeps its last valid reading, which an unknown one does not
//! erase. One probe at a time across processes and languages: whoever takes the claim probes outside
//! any transaction, and writes only if the claim is still its own (fencing), so a probe that stalled
//! past the claim's expiry cannot overwrite the one that replaced it.

use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::db::{self, Doc, TxError};
use crate::lock::parse_instant;
use crate::time::{Precision, WallTime};
use crate::usage::{CacheInfo, ProviderUsage, Reading, Served, UsageStatus, Window};
use crate::v1::format;
use crate::{Store, StoreError};

/// The interval between two real readings unless the caller names one: `BUDGET_MIN_INTERVALLO_S`.
pub const MIN_INTERVAL_S: u64 = 300;
/// How long a probe running in another process is waited for: `ATTESA_SONDA_S`.
pub const PROBE_WAIT: Duration = Duration::from_secs(45);
/// Past this age the claim of a probe counts as abandoned: `SONDA_TTL_S`.
pub const PROBE_TTL_S: f64 = 90.0;
/// The pause between two looks at another process's claim, from half to one and a half times this.
const PAUSE: Duration = Duration::from_millis(20);

/// The cache's note when another process's probe was still running at the end of the wait.
pub const NOTE_RUNNING: &str = "a probe is running in another process";
/// The cache's note when this probe's claim was taken over while it ran: its reading is dropped.
pub const NOTE_OVERTAKEN: &str = "the probe was overtaken by another process";
/// The reason of a provider the cache has a record for, and no reading.
pub const NO_VALUE: &str = "no value available";
/// Why the last attempt failed, when it did not say.
pub const UPDATE_FAILED: &str = "unknown";

/// Why a reading of the budget did not happen.
#[derive(Debug, thiserror::Error)]
pub enum BudgetError<E: std::error::Error + 'static> {
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The probe failed, and its claim was released, so the next caller probes at once.
    #[error(transparent)]
    Probe(E),
}

enum Look {
    Fresh(Reading),
    Claimed,
    Running(Doc),
}

impl Store {
    /// Waits at most `wait` for a probe another process is running, instead of [`PROBE_WAIT`].
    #[must_use]
    pub fn with_probe_wait(mut self, wait: Duration) -> Self {
        self.probe_wait = wait;
        self
    }

    /// The budget, from the cache when its last attempt is younger than `min_interval_s`, else from
    /// `probe`, which this runs outside any transaction and whose reading it stores. `force` probes
    /// whatever the age. A provider the user refused to probe is never stored.
    ///
    /// # Errors
    ///
    /// [`BudgetError::Store`] when the state cannot be read or written, [`BudgetError::Probe`] when
    /// `probe` fails.
    pub fn budget<E: std::error::Error + 'static>(
        &self,
        min_interval_s: u64,
        force: bool,
        probe: impl FnOnce() -> Result<Reading, E>,
    ) -> Result<Reading, BudgetError<E>> {
        let (kinds, key) = (&format().kinds, &format().budget.key);
        let token = fencing_token();
        let deadline = Instant::now() + self.probe_wait;
        loop {
            let look = self.db.write(|conn| {
                let cache = db::select(conn, &kinds.cache, key)?.unwrap_or_default();
                let now = self.clock.now();
                let age = self.attempt_age_s(&cache, now);
                if !force && age.is_some_and(|age| 0.0 <= age && age < min_interval_s as f64) {
                    return Ok(Look::Fresh(self.compose(&cache, now, true, min_interval_s, None)));
                }
                let claim = db::select(conn, &kinds.probe, key)?;
                if claim.is_none_or(|claim| self.remaining(&claim, now, &format().budget.claim_expires_at).is_none_or(|left| left <= 0.0)) {
                    db::upsert(conn, &kinds.probe, key, &self.claim(&token, now)?)?;
                    return Ok(Look::Claimed);
                }
                Ok(Look::Running(cache))
            });
            let cache = match look {
                Ok(Look::Fresh(reading)) => return Ok(reading),
                Ok(Look::Claimed) => break,
                Ok(Look::Running(cache)) => Some(cache),
                Err(TxError::Busy) => None,
                Err(TxError::Failed(error)) => return Err(error.into()),
            };
            if Instant::now() > deadline {
                let cache = match cache {
                    Some(cache) => cache,
                    None => self.db.read(&kinds.cache, key)?.unwrap_or_default(),
                };
                return Ok(self.compose(&cache, self.clock.now(), true, min_interval_s, Some(NOTE_RUNNING)));
            }
            thread::sleep(PAUSE.mul_f64(0.5 + db::random_unit()));
        }
        let reading = match probe() {
            Ok(reading) => reading,
            Err(error) => {
                // Best effort, as the prototype's: the claim expires by itself anyway.
                let _ = self.release_claim(&token);
                return Err(BudgetError::Probe(error));
            }
        };
        let written = self.db.write(|conn| {
            let mut cache = db::select(conn, &kinds.cache, key)?.unwrap_or_default();
            let now = self.clock.now();
            let claim = db::select(conn, &kinds.probe, key)?;
            let ours = claim.as_ref().and_then(|claim| claim.get(&format().budget.claim_token)).and_then(Value::as_str)
                == Some(token.as_str());
            if !ours {
                return Ok(self.compose(&cache, now, true, min_interval_s, Some(NOTE_OVERTAKEN)));
            }
            db::delete(conn, &kinds.probe, key)?;
            self.record(&mut cache, &reading, now);
            db::upsert(conn, &kinds.cache, key, &cache)?;
            Ok(self.compose(&cache, now, false, min_interval_s, None))
        });
        match written {
            Ok(reading) => Ok(reading),
            Err(TxError::Busy) => Err(StoreError::InUse.into()),
            Err(TxError::Failed(error)) => Err(error.into()),
        }
    }

    /// Seconds since the last attempt: on this clock if it stamped the cache and the machine has not
    /// restarted since, else on the wall clock. `None` before the first attempt.
    fn attempt_age_s(&self, cache: &Doc, now: WallTime) -> Option<f64> {
        let keys = &format().monotonic;
        if let Some(Value::Object(stamp)) = cache.get(&keys.key) {
            let same_clock = stamp.get(&keys.clock).and_then(Value::as_str) == Some(&*self.clock.tag());
            if let (true, Some(written)) = (same_clock, stamp.get(&keys.written).and_then(Value::as_f64)) {
                let monotonic = self.clock.monotonic();
                if monotonic >= written {
                    return Some(monotonic - written);
                }
            }
        }
        let attempted = cache.get(&format().budget.attempted_at).and_then(parse_instant)?;
        Some(attempted.seconds_until(now))
    }

    /// The claim of a probe: its token, and an expiry [`PROBE_TTL_S`] from now on both clocks.
    fn claim(&self, token: &str, now: WallTime) -> Result<Doc, StoreError> {
        let keys = &format().budget;
        let expires = now.checked_add_secs(PROBE_TTL_S).ok_or(StoreError::TtlOutOfRange { ttl: PROBE_TTL_S })?;
        let mut doc = Doc::new();
        doc.insert(keys.claim_token.clone(), token.into());
        doc.insert(keys.claim_expires_at.clone(), expires.to_iso().into());
        Ok(self.stamped(doc, now, &keys.claim_expires_at))
    }

    /// Deletes the claim if it is still the one with `token`.
    fn release_claim(&self, token: &str) -> Result<(), TxError> {
        let (kinds, keys) = (&format().kinds, &format().budget);
        self.db.write(|conn| {
            let claim = db::select(conn, &kinds.probe, &keys.key)?;
            if claim.and_then(|claim| claim.get(&keys.claim_token).cloned()).as_ref().and_then(Value::as_str) == Some(token) {
                db::delete(conn, &kinds.probe, &keys.key)?;
            }
            Ok(())
        })
    }

    /// Stores `reading` in the cache: the last reading of each provider, and the valid one when it is.
    fn record(&self, cache: &mut Doc, reading: &Reading, now: WallTime) {
        let keys = &format().budget;
        if !cache.get(&keys.providers).is_some_and(Value::is_object) {
            cache.insert(keys.providers.clone(), Value::Object(Map::new()));
        }
        let Some(Value::Object(records)) = cache.get_mut(&keys.providers) else { unreachable!("an object was just put there") };
        for (name, usage) in &reading.providers {
            let Some(stored) = usage.to_v1() else { continue };
            if !records.get(name).is_some_and(Value::is_object) {
                let mut fresh = Map::new();
                fresh.insert(keys.valid.clone(), Value::Null);
                fresh.insert(keys.valid_at.clone(), Value::Null);
                records.insert(name.clone(), Value::Object(fresh));
            }
            let Some(Value::Object(record)) = records.get_mut(name) else { unreachable!("an object was just put there") };
            record.insert(keys.last.clone(), stored.clone());
            if usage.status == UsageStatus::Ok {
                record.insert(keys.valid.clone(), stored);
                record.insert(keys.valid_at.clone(), now.to_iso().into());
            }
        }
        cache.insert(keys.attempted_at.clone(), now.to_iso().into());
        let monotonic = &format().monotonic;
        let mut stamp = Map::new();
        stamp.insert(monotonic.written.clone(), Value::from(self.clock.monotonic()));
        stamp.insert(monotonic.clock.clone(), self.clock.tag().into_owned().into());
        cache.insert(monotonic.key.clone(), Value::Object(stamp));
    }

    /// The reading the cache gives at `now`: each provider's last valid reading with its age and the
    /// seconds to its resets counted again, or its last reading if none was valid.
    fn compose(&self, cache: &Doc, now: WallTime, from_cache: bool, min_interval_s: u64, note: Option<&str>) -> Reading {
        let keys = &format().budget;
        let attempted = cache.get(&keys.attempted_at).and_then(parse_instant);
        let providers = cache
            .get(&keys.providers)
            .and_then(Value::as_object)
            .map(|records| {
                records.iter().map(|(name, record)| (name.clone(), compose_one(name, record, now, from_cache))).collect()
            })
            .unwrap_or_default();
        let cache = CacheInfo {
            from_cache,
            min_interval_s,
            attempt_age_s: attempted.map(|at| whole_seconds(at, now)),
            note: note.map(str::to_owned),
        };
        let generated = attempted.map(|at| at.to_local_iso(self.clock.local_offset_s(at), Precision::Seconds));
        Reading { generated, providers, cache: Some(cache) }
    }
}

fn compose_one(name: &str, record: &Value, now: WallTime, from_cache: bool) -> ProviderUsage {
    let (keys, usage_keys) = (&format().budget, &format().usage);
    let field = |key: &str| record.get(key).filter(|value| is_truthy(value));
    if let Some(mut valid) = field(&keys.valid).and_then(|valid| ProviderUsage::from_v1(name, valid)) {
        let age_s = record.get(&keys.valid_at).and_then(parse_instant).map(|at| whole_seconds(at, now));
        for (_, window) in &mut valid.windows {
            if let Window::Open { reset, reset_in_s, .. } = window
                && let Some(at) = WallTime::parse_iso(reset)
            {
                *reset_in_s = whole_seconds(now, at);
            }
        }
        let last = field(&keys.last).and_then(Value::as_object);
        let last_ok = last.and_then(|last| last.get(&usage_keys.status)).and_then(Value::as_str)
            == Some(format().usage_statuses.ok.as_str());
        let update_failed = (!last_ok).then(|| {
            last.and_then(|last| last.get(&usage_keys.reason)).and_then(Value::as_str).unwrap_or(UPDATE_FAILED).to_owned()
        });
        valid.served = Some(Served { age_s, from_cache, update_failed });
        return valid;
    }
    let mut usage = field(&keys.last)
        .and_then(|last| ProviderUsage::from_v1(name, last))
        .unwrap_or_else(|| ProviderUsage::unknown(name, None, NO_VALUE, None));
    usage.served = Some(Served { age_s: None, from_cache, update_failed: None });
    usage
}

/// `max(0, int((later - earlier).total_seconds()))`: truncated toward zero, never negative.
fn whole_seconds(earlier: WallTime, later: WallTime) -> i64 {
    (earlier.seconds_until(later).trunc() as i64).max(0)
}

/// Python's truth of a JSON value, as `rec.get(...) or …` reads it.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// 32 hexadecimal digits, as the prototype's `uuid4().hex`: only its uniqueness counts.
fn fencing_token() -> String {
    format!("{:016x}{:016x}", db::random_u64(), db::random_u64() ^ u64::from(std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truth_and_seconds_follow_python() {
        for (value, truthy) in [
            (Value::Null, false),
            (Value::from(0), false),
            (Value::from(""), false),
            (Value::Object(Map::new()), false),
            (serde_json::json!({"a": 1}), true),
            (Value::from("x"), true),
        ] {
            assert_eq!(is_truthy(&value), truthy, "{value}");
        }
        let start = WallTime::from_unix_micros(10_000_000).unwrap();
        assert_eq!(whole_seconds(start, WallTime::from_unix_micros(17_900_000).unwrap()), 7);
        assert_eq!(whole_seconds(start, WallTime::from_unix_micros(9_000_000).unwrap()), 0);
        let token = fencing_token();
        assert_eq!(token.len(), 32);
        assert_ne!(token, fencing_token());
    }
}
