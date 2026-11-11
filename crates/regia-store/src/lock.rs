//! The project lock (L4–L8 of the dossier's conformance study): one holder at a time, taken over once
//! expired, with the outcomes of the prototype's `acquisisci`, `rinnova`, `rilascia` and lock status.

use serde_json::{Number, Value};

use crate::db::{self, Doc, TxError};
use crate::time::WallTime;
use crate::v1::format;
use crate::{Holder, ProjectId, Store, StoreError, Ttl};

/// What an operation on a lock did. `ok` in the prototype's output is [`LockState::is_ok`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockState {
    /// Nobody held it: now the caller does.
    Taken,
    /// The caller held it, or held it until it expired: its expiry moved.
    Renewed,
    /// Another holder's lock had expired: the caller holds it now.
    Stolen,
    /// The caller held it and let it go.
    Released,
    /// Nothing to release.
    Free,
    /// Another holder has it, or the database stayed busy beyond the wait.
    Busy,
    /// The lock is someone else's: neither renewed nor released.
    Foreign,
    /// Nothing to renew.
    Absent,
}

impl LockState {
    #[must_use]
    pub fn is_ok(self) -> bool {
        matches!(self, Self::Taken | Self::Renewed | Self::Stolen | Self::Released | Self::Free)
    }
}

/// The outcome of [`Store::acquire_lock`], [`Store::renew_lock`] or [`Store::release_lock`].
#[derive(Debug, Clone, PartialEq)]
pub enum LockOutcome {
    Taken(LockView),
    Renewed(LockView),
    Stolen(LockView),
    Released,
    Free,
    /// The lock of another holder, still valid.
    Busy(LockView),
    /// The database stayed held by another transaction beyond the wait. `current` is the lock as a
    /// read without the write lock saw it, if it could.
    Contended { current: Option<LockView> },
    /// Someone else's lock. `None` when the row is not a lock document.
    Foreign(Option<LockView>),
    Absent,
}

impl LockOutcome {
    #[must_use]
    pub fn state(&self) -> LockState {
        match self {
            Self::Taken(_) => LockState::Taken,
            Self::Renewed(_) => LockState::Renewed,
            Self::Stolen(_) => LockState::Stolen,
            Self::Released => LockState::Released,
            Self::Free => LockState::Free,
            Self::Busy(_) | Self::Contended { .. } => LockState::Busy,
            Self::Foreign(_) => LockState::Foreign,
            Self::Absent => LockState::Absent,
        }
    }

    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.state().is_ok()
    }

    /// The prototype's exit code: 0 when done, 3 when refused, which callers tell apart from an error (1).
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        if self.is_ok() { 0 } else { 3 }
    }

    /// The lock as written, or as found when refused.
    #[must_use]
    pub fn lock(&self) -> Option<&LockView> {
        match self {
            Self::Taken(view) | Self::Renewed(view) | Self::Stolen(view) | Self::Busy(view) => Some(view),
            Self::Contended { current } | Self::Foreign(current) => current.as_ref(),
            Self::Released | Self::Free | Self::Absent => None,
        }
    }
}

/// What [`Store::lock_status`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum LockStatus {
    Free,
    Held(LockView),
    Expired(LockView),
    /// A row that is not a lock document, nobody's: it can be taken.
    Unreadable,
    /// Another transaction held the database beyond the wait. The status fails closed: busy.
    Contended,
}

impl LockStatus {
    /// The prototype's `occupato`: someone may hold the lock.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Held(_) | Self::Contended)
    }

    #[must_use]
    pub fn lock(&self) -> Option<&LockView> {
        match self {
            Self::Held(view) | Self::Expired(view) => Some(view),
            Self::Free | Self::Unreadable | Self::Contended => None,
        }
    }
}

/// A lock document as stored, without the fields no view shows, and its expiry at the time of reading.
#[derive(Debug, Clone, PartialEq)]
pub struct LockView {
    fields: Doc,
    expired: bool,
    expires_in_s: i64,
}

impl LockView {
    /// The stored fields, under their v1 names: everything the document holds, unknown fields too.
    #[must_use]
    pub fn fields(&self) -> &serde_json::Map<String, Value> {
        &self.fields
    }

    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expired
    }

    /// Whole seconds to the expiry, 0 once expired.
    #[must_use]
    pub fn expires_in_s(&self) -> i64 {
        self.expires_in_s
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.fields.get(key).and_then(Value::as_str)
    }

    #[must_use]
    pub fn holder(&self) -> Option<&str> {
        self.text(&format().lock.holder)
    }

    #[must_use]
    pub fn project(&self) -> Option<&str> {
        self.text(&format().lock.project)
    }

    #[must_use]
    pub fn taken_at(&self) -> Option<&str> {
        self.text(&format().lock.taken_at)
    }

    #[must_use]
    pub fn expires_at(&self) -> Option<&str> {
        self.text(&format().lock.expires_at)
    }

    #[must_use]
    pub fn renewed_at(&self) -> Option<&str> {
        self.text(&format().lock.renewed_at)
    }

    #[must_use]
    pub fn stolen_at(&self) -> Option<&str> {
        self.text(&format().lock.stolen_at)
    }

    /// The holder whose expired lock this one replaced, as it was stored (any JSON value, or null).
    #[must_use]
    pub fn replaced(&self) -> Option<&Value> {
        self.fields.get(&format().lock.replaced)
    }

    #[must_use]
    pub fn ttl_s(&self) -> Option<f64> {
        self.fields.get(&format().lock.ttl_s).and_then(Value::as_f64)
    }
}

enum Decision {
    Exit(Exit),
    Write(Doc, Written),
    Delete,
}

enum Exit {
    Busy(Doc),
    Foreign(Doc),
    Absent,
    Free,
}

#[derive(Clone, Copy)]
enum Written {
    Taken,
    Renewed,
    Stolen,
}

enum Transition {
    Exit(Exit),
    Wrote(Doc, Written),
    Deleted,
    Contended,
}

impl Store {
    /// Takes the project's lock for `ttl`, or renews it if the caller already holds it. An expired lock
    /// is taken from its holder, and the new document names the holder it replaced.
    ///
    /// # Errors
    ///
    /// [`StoreError`] when the database cannot be opened, read or written, or `ttl` runs past the
    /// years a date can hold. Refusals are outcomes, not errors.
    pub fn acquire_lock(&self, project: &ProjectId, holder: &Holder, ttl: Ttl) -> Result<LockOutcome, StoreError> {
        let transition = self.transition(&format().kinds.lock, project.as_str(), |doc, now| {
            let expires = expiry(now, ttl)?;
            let keys = &format().lock;
            let fresh = || {
                let mut doc = Doc::new();
                doc.insert(keys.project.clone(), project.as_str().into());
                doc.insert(keys.holder.clone(), holder.as_str().into());
                doc.insert(keys.taken_at.clone(), now.to_iso().into());
                doc.insert(keys.ttl_s.clone(), ttl.to_json());
                doc.insert(keys.expires_at.clone(), expires.clone().into());
                doc.insert(keys.replaced.clone(), Value::Null);
                doc.insert(keys.stolen_at.clone(), Value::Null);
                doc
            };
            let Some(mut doc) = doc else { return Ok(Decision::Write(fresh(), Written::Taken)) };
            let mine = is_holder(&doc, holder);
            if !self.is_expired(&doc, now) {
                if !mine {
                    return Ok(Decision::Exit(Exit::Busy(doc)));
                }
                doc.insert(keys.ttl_s.clone(), ttl.to_json());
                doc.insert(keys.expires_at.clone(), expires.clone().into());
                doc.insert(keys.renewed_at.clone(), now.to_iso().into());
                return Ok(Decision::Write(doc, Written::Renewed));
            }
            if mine {
                return Ok(Decision::Write(fresh(), Written::Renewed));
            }
            let previous = doc.get(&keys.holder).cloned().unwrap_or(Value::Null);
            let mut stolen = fresh();
            stolen.insert(keys.replaced.clone(), previous);
            stolen.insert(keys.stolen_at.clone(), now.to_iso().into());
            Ok(Decision::Write(stolen, Written::Stolen))
        })?;
        Ok(self.outcome(project, transition))
    }

    /// Extends the caller's lock, even an expired one nobody took. Refuses another holder's lock and a
    /// missing one.
    ///
    /// # Errors
    ///
    /// As [`Store::acquire_lock`].
    pub fn renew_lock(&self, project: &ProjectId, holder: &Holder, ttl: Ttl) -> Result<LockOutcome, StoreError> {
        let transition = self.transition(&format().kinds.lock, project.as_str(), |doc, now| {
            let Some(mut doc) = doc else { return Ok(Decision::Exit(Exit::Absent)) };
            if !is_holder(&doc, holder) {
                return Ok(Decision::Exit(Exit::Foreign(doc)));
            }
            let keys = &format().lock;
            doc.insert(keys.ttl_s.clone(), ttl.to_json());
            doc.insert(keys.expires_at.clone(), expiry(now, ttl)?.into());
            doc.insert(keys.renewed_at.clone(), now.to_iso().into());
            Ok(Decision::Write(doc, Written::Renewed))
        })?;
        Ok(self.outcome(project, transition))
    }

    /// Deletes the caller's lock, expired or not. Another holder's lock is refused; a missing one is
    /// already free, which is a success.
    ///
    /// # Errors
    ///
    /// When the database cannot be opened, read or written.
    pub fn release_lock(&self, project: &ProjectId, holder: &Holder) -> Result<LockOutcome, StoreError> {
        let transition = self.transition(&format().kinds.lock, project.as_str(), |doc, _now| {
            let Some(doc) = doc else { return Ok(Decision::Exit(Exit::Free)) };
            if !is_holder(&doc, holder) {
                return Ok(Decision::Exit(Exit::Foreign(doc)));
            }
            Ok(Decision::Delete)
        })?;
        Ok(self.outcome(project, transition))
    }

    /// Who holds the project's lock. It takes the write lock to read, as the prototype does, so a lock
    /// being written by another process reads as busy and the check fails closed (L8).
    ///
    /// # Errors
    ///
    /// When the database cannot be opened or read.
    pub fn lock_status(&self, project: &ProjectId) -> Result<LockStatus, StoreError> {
        let read = self.db.write(|conn| db::select(conn, &format().kinds.lock, project.as_str()));
        let doc = match read {
            Ok(doc) => doc,
            Err(TxError::Busy) => return Ok(LockStatus::Contended),
            Err(TxError::Failed(error)) => return Err(error),
        };
        let now = self.clock.now();
        Ok(match doc {
            None => LockStatus::Free,
            Some(doc) if doc.is_empty() => LockStatus::Unreadable,
            Some(doc) => {
                let view = self.view(doc, now);
                if view.expired { LockStatus::Expired(view) } else { LockStatus::Held(view) }
            }
        })
    }

    /// Reads, decides and writes one document in a single `BEGIN IMMEDIATE` transaction.
    fn transition(
        &self,
        kind: &str,
        key: &str,
        decide: impl FnOnce(Option<Doc>, WallTime) -> Result<Decision, StoreError>,
    ) -> Result<Transition, StoreError> {
        let result = self.db.write(|conn| {
            let doc = db::select(conn, kind, key)?;
            let now = self.clock.now();
            match decide(doc, now)? {
                Decision::Exit(exit) => Ok(Transition::Exit(exit)),
                Decision::Delete => {
                    db::delete(conn, kind, key)?;
                    Ok(Transition::Deleted)
                }
                Decision::Write(doc, written) => {
                    let doc = self.stamped(doc, now, &format().lock.expires_at);
                    db::upsert(conn, kind, key, &doc)?;
                    Ok(Transition::Wrote(doc, written))
                }
            }
        });
        match result {
            Ok(transition) => Ok(transition),
            Err(TxError::Busy) => Ok(Transition::Contended),
            Err(TxError::Failed(error)) => Err(error),
        }
    }

    fn outcome(&self, project: &ProjectId, transition: Transition) -> LockOutcome {
        let now = self.clock.now();
        match transition {
            Transition::Contended => {
                let current = self.db.read(&format().kinds.lock, project.as_str()).ok().flatten();
                LockOutcome::Contended { current: current.filter(|doc| !doc.is_empty()).map(|doc| self.view(doc, now)) }
            }
            Transition::Deleted => LockOutcome::Released,
            Transition::Wrote(doc, written) => {
                let view = self.view(doc, now);
                match written {
                    Written::Taken => LockOutcome::Taken(view),
                    Written::Renewed => LockOutcome::Renewed(view),
                    Written::Stolen => LockOutcome::Stolen(view),
                }
            }
            Transition::Exit(Exit::Busy(doc)) => LockOutcome::Busy(self.view(doc, now)),
            Transition::Exit(Exit::Foreign(doc)) => {
                LockOutcome::Foreign((!doc.is_empty()).then(|| self.view(doc, now)))
            }
            Transition::Exit(Exit::Absent) => LockOutcome::Absent,
            Transition::Exit(Exit::Free) => LockOutcome::Free,
        }
    }

    /// The document with its monotonic stamp: when it was written and when it expires, on this clock.
    /// `expires_key` names its expiry on the wall clock: a lock's, or the claim of a probe.
    pub(crate) fn stamped(&self, mut doc: Doc, now: WallTime, expires_key: &str) -> Doc {
        let keys = &format().monotonic;
        let monotonic = self.clock.monotonic();
        let expires = doc.get(expires_key).and_then(parse_instant).expect("a stamped document has an expiry");
        let mut stamp = Doc::new();
        stamp.insert(keys.written.clone(), number(monotonic));
        stamp.insert(keys.expires.clone(), number(monotonic + now.seconds_until(expires)));
        stamp.insert(keys.clock.clone(), self.clock.tag().into_owned().into());
        doc.insert(keys.key.clone(), Value::Object(stamp));
        doc
    }

    /// Seconds to the expiry, on this clock if it wrote the document, else on the wall clock at
    /// `expires_key`; `None` if the document has no readable expiry (L4, L5).
    pub(crate) fn remaining(&self, doc: &Doc, now: WallTime, expires_key: &str) -> Option<f64> {
        if doc.is_empty() {
            return None;
        }
        let keys = &format().monotonic;
        if let Some(Value::Object(stamp)) = doc.get(&keys.key) {
            let same_clock = stamp.get(&keys.clock).and_then(Value::as_str) == Some(&*self.clock.tag());
            let written = stamp.get(&keys.written).and_then(Value::as_f64);
            let expires = stamp.get(&keys.expires).and_then(Value::as_f64);
            if let (true, Some(written), Some(expires)) = (same_clock, written, expires) {
                let monotonic = self.clock.monotonic();
                // The clock went back: the machine restarted, and whoever held the lock is gone.
                if monotonic < written {
                    return Some(0.0);
                }
                return Some(expires - monotonic);
            }
        }
        let expires = doc.get(expires_key).and_then(parse_instant)?;
        Some(now.seconds_until(expires))
    }

    fn is_expired(&self, doc: &Doc, now: WallTime) -> bool {
        self.remaining(doc, now, &format().lock.expires_at).is_none_or(|left| left <= 0.0)
    }

    fn view(&self, mut doc: Doc, now: WallTime) -> LockView {
        let remaining = self.remaining(&doc, now, &format().lock.expires_at);
        for hidden in &format().view.hidden {
            doc.remove(hidden);
        }
        LockView {
            fields: doc,
            expired: remaining.is_none_or(|left| left <= 0.0),
            // Python's `max(0, int(left))`: truncated toward zero.
            expires_in_s: remaining.map_or(0, |left| (left.trunc() as i64).max(0)),
        }
    }
}

fn is_holder(doc: &Doc, holder: &Holder) -> bool {
    doc.get(&format().lock.holder).and_then(Value::as_str) == Some(holder.as_str())
}

fn expiry(now: WallTime, ttl: Ttl) -> Result<String, StoreError> {
    now.checked_add_secs(ttl.as_secs_f64()).map(WallTime::to_iso).ok_or(StoreError::TtlOutOfRange { ttl: ttl.as_secs_f64() })
}

/// `_parse(str(value))`: a string, or an integer, which Python turns into its digits first.
pub(crate) fn parse_instant(value: &Value) -> Option<WallTime> {
    match value {
        Value::String(text) => WallTime::parse_iso(text),
        Value::Number(number) if number.is_i64() || number.is_u64() => WallTime::parse_iso(&number.to_string()),
        _ => None,
    }
}

fn number(value: f64) -> Value {
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}
