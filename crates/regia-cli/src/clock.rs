//! The product's clock: `REGIA_NOW` when it is set, which fixes it, and the machine's otherwise. A fixed
//! clock dates the dossier's commits and stands the state's clock still at its instant (#442/V3), so a run
//! at the fixtures' instant writes what the prototype wrote at it.

use chrono::{DateTime, FixedOffset, Utc};
use regia_git::GitDate;
use regia_identity::env_var;
use regia_store::{ManualClock, Store, WallTime};

#[derive(Debug)]
pub(crate) struct Clock {
    pub(crate) now: DateTime<FixedOffset>,
    fixed: bool,
}

impl Clock {
    /// A `REGIA_NOW` that is not an instant with its offset is an error, never the machine's clock in
    /// its place.
    pub(crate) fn read() -> Result<Clock, String> {
        let name = env_var("NOW");
        Clock::of(&name, std::env::var(&name).ok())
    }

    pub(crate) fn of(name: &str, value: Option<String>) -> Result<Clock, String> {
        match value.filter(|value| !value.is_empty()) {
            Some(value) => DateTime::parse_from_rfc3339(&value)
                .map(|now| Clock { now, fixed: true })
                .map_err(|_| format!("{name} is `{value}`, not an ISO 8601 instant with its offset")),
            None => Ok(Clock { now: Utc::now().fixed_offset(), fixed: false }),
        }
    }

    /// With the clock fixed, a commit is made at its instant, so it has the same id on every run;
    /// otherwise git dates it, as the user's own git would.
    pub(crate) fn commit_date(&self) -> Option<GitDate> {
        self.fixed.then(|| GitDate::new(self.now.timestamp(), self.now.offset().local_minus_utc()))
    }

    /// The machine's state database, on this clock: fixed, the state's clock stands still at its instant
    /// with a tag of its own; otherwise it is the system's.
    pub(crate) fn store(&self) -> Result<Store, String> {
        let store = Store::open().map_err(|error| error.to_string())?;
        if !self.fixed {
            return Ok(store);
        }
        Ok(store.with_clock(ManualClock::fixed(self.wall()?)))
    }

    /// The clock's instant, as the state writes its dates.
    pub(crate) fn wall(&self) -> Result<WallTime, String> {
        WallTime::from_unix_micros(self.now.timestamp_micros())
            .ok_or_else(|| format!("{} is out of the range of the state's dates", self.now.to_rfc3339()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `REGIA_NOW` fixes the clock and dates the commit with its offset; a value that is not an instant
    /// is an error; without it, git dates the commit.
    #[test]
    fn the_fixed_clock_dates_the_commit() {
        let fixed = Clock::of("REGIA_NOW", Some("2026-01-10T09:00:00Z".to_owned())).unwrap();
        assert_eq!(fixed.now.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string(), "2026-01-10T09:00:00Z");
        assert_eq!(fixed.commit_date().map(|date| date.as_env()).as_deref(), Some("@1768035600 +0000"));
        let east = Clock::of("REGIA_NOW", Some("2026-01-10T10:30:00+01:30".to_owned())).unwrap();
        assert_eq!(east.commit_date().map(|date| date.as_env()).as_deref(), Some("@1768035600 +0130"));
        assert!(Clock::of("REGIA_NOW", Some("2026-01-10".to_owned())).unwrap_err().contains("REGIA_NOW"));
        assert!(Clock::of("REGIA_NOW", None).unwrap().commit_date().is_none());
        assert!(Clock::of("REGIA_NOW", Some(String::new())).unwrap().commit_date().is_none(), "empty is unset");
    }
}
