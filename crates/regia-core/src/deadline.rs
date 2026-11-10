//! The deadline guard (ADR 0001 §6). A job handed to a service with a deadline, such as a review by
//! another model, is not waited for past it, whatever the service and the surface. Past the deadline,
//! or when the job ended without what it was asked for, the caller does not wait and does not launch
//! the same job again, which may still be running: it takes what the job's log already holds and
//! falls back as its role says. The core has no clock: the caller passes the time.

use chrono::{DateTime, TimeDelta, Utc};

/// What a job has done so far, as the caller sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    /// It ended with what it was asked for: a list of defects, for a review.
    EndedWithResult,
    EndedWithoutResult,
}

/// Why the caller stops waiting for a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    DeadlinePassed,
    NoResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Within the deadline: look again later, at most this much later.
    Wait { remaining: TimeDelta },
    /// Its result is there, even if it came late: use it.
    Done,
    /// Do not wait any longer, and do not launch the same job again.
    FallBack(FallbackReason),
}

/// When a job started, and how long it may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    started: DateTime<Utc>,
    limit: TimeDelta,
}

impl Deadline {
    pub fn minutes(started: DateTime<Utc>, minutes: u32) -> Deadline {
        Deadline { started, limit: TimeDelta::minutes(i64::from(minutes)) }
    }

    pub fn started(&self) -> DateTime<Utc> {
        self.started
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.started + self.limit
    }

    /// What the caller does at `now`. A clock set back makes the wait longer, never the fallback
    /// earlier.
    pub fn judge(&self, now: DateTime<Utc>, state: JobState) -> Verdict {
        match state {
            JobState::EndedWithResult => Verdict::Done,
            JobState::EndedWithoutResult => Verdict::FallBack(FallbackReason::NoResult),
            JobState::Running if now >= self.expires_at() => Verdict::FallBack(FallbackReason::DeadlinePassed),
            JobState::Running => Verdict::Wait { remaining: self.expires_at() - now },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-10T12:00:00Z").unwrap().with_timezone(&Utc) + TimeDelta::minutes(minutes)
    }

    /// The protocol's rule for a review by another model, with its 20 minutes.
    #[test]
    fn past_the_deadline_or_without_a_result_the_caller_falls_back() {
        let review = Deadline::minutes(at(0), 20);
        assert_eq!(review.expires_at(), at(20));
        assert_eq!(review.judge(at(5), JobState::Running), Verdict::Wait { remaining: TimeDelta::minutes(15) });
        assert_eq!(review.judge(at(20), JobState::Running), Verdict::FallBack(FallbackReason::DeadlinePassed));
        assert_eq!(review.judge(at(90), JobState::Running), Verdict::FallBack(FallbackReason::DeadlinePassed));
        assert_eq!(review.judge(at(3), JobState::EndedWithoutResult), Verdict::FallBack(FallbackReason::NoResult));
        assert_eq!(review.judge(at(3), JobState::EndedWithResult), Verdict::Done);
        assert_eq!(review.judge(at(25), JobState::EndedWithResult), Verdict::Done, "a late result is still a result");
    }

    #[test]
    fn a_clock_set_back_only_waits_longer() {
        let job = Deadline::minutes(at(0), 20);
        assert_eq!(job.judge(at(-10), JobState::Running), Verdict::Wait { remaining: TimeDelta::minutes(30) });
        assert_eq!(Deadline::minutes(at(0), 0).judge(at(0), JobState::Running), Verdict::FallBack(FallbackReason::DeadlinePassed));
    }
}
