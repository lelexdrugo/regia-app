//! A task's lease (L7 of the dossier's conformance study): the prototype's `prendi`, `battito`,
//! `rilascia_lease` and `elenca`. At most one valid lease per task, kept alive by heartbeats and taken
//! over once expired; the machine is the lock's, on rows of their own kind.

use serde_json::Value;

use crate::db::Doc;
use crate::lock::{Decision, Exit, Written, expiry, is_holder};
use crate::v1::format;
use crate::{Holder, LockOutcome, LockView, ProjectId, Store, StoreError, Ttl};

/// A task's number: in digits, the key of its lease's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskNumber(u64);

impl TaskNumber {
    #[must_use]
    pub fn new(number: u64) -> Self {
        Self(number)
    }

    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }

    fn key(self) -> String {
        self.0.to_string()
    }
}

/// A row of [`Store::leases`].
#[derive(Debug, Clone, PartialEq)]
pub enum LeaseEntry {
    /// A lease document, with its expiry at the time of reading.
    Lease(LockView),
    /// A row that is not a lease document, which lists as unreadable and expired. `task` is its key:
    /// a number when the key is digits, the key as it is otherwise.
    Unreadable { task: Value },
}

impl LeaseEntry {
    #[must_use]
    pub fn is_expired(&self) -> bool {
        match self {
            Self::Lease(view) => view.is_expired(),
            Self::Unreadable { .. } => true,
        }
    }
}

impl Store {
    /// Takes the lease of `task` for `ttl`, or renews it if the caller already holds it, which also
    /// moves it to `project`. An expired lease is taken from its holder, and the new document names the
    /// holder it replaced.
    ///
    /// # Errors
    ///
    /// As [`Store::acquire_lock`]. Refusals are outcomes, not errors.
    pub fn acquire_lease(&self, task: TaskNumber, project: &ProjectId, holder: &Holder, ttl: Ttl) -> Result<LockOutcome, StoreError> {
        let (kind, key) = (&format().kinds.lease, task.key());
        let transition = self.transition(kind, &key, |doc, now| {
            let expires = expiry(now, ttl)?;
            let keys = &format().lease;
            let fresh = || {
                let mut doc = Doc::new();
                doc.insert(keys.task.clone(), task.get().into());
                doc.insert(keys.project.clone(), project.as_str().into());
                doc.insert(keys.holder.clone(), holder.as_str().into());
                doc.insert(keys.taken_at.clone(), now.to_iso().into());
                doc.insert(keys.heartbeat_at.clone(), now.to_iso().into());
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
                // The prototype's renewal of a lease beats: it writes `heartbeat_at`, never `renewed_at`.
                doc.insert(keys.project.clone(), project.as_str().into());
                doc.insert(keys.ttl_s.clone(), ttl.to_json());
                doc.insert(keys.expires_at.clone(), expires.clone().into());
                doc.insert(keys.heartbeat_at.clone(), now.to_iso().into());
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
        Ok(self.outcome(kind, &key, transition))
    }

    /// Extends the caller's lease by `ttl`, or without it by the ttl it was taken with, as stored. Another
    /// holder's lease is refused, and so is one with no ttl to extend it by: the prototype answers it as
    /// someone else's.
    ///
    /// # Errors
    ///
    /// As [`Store::acquire_lock`].
    pub fn heartbeat_lease(&self, task: TaskNumber, holder: &Holder, ttl: Option<Ttl>) -> Result<LockOutcome, StoreError> {
        let (kind, key) = (&format().kinds.lease, task.key());
        let transition = self.transition(kind, &key, |doc, now| {
            let Some(mut doc) = doc else { return Ok(Decision::Exit(Exit::Absent)) };
            if !is_holder(&doc, holder) {
                return Ok(Decision::Exit(Exit::Foreign(doc)));
            }
            let keys = &format().lease;
            // `ttl_s or doc.get("ttl_s") or 0`: the stored one keeps its form, an integer or a float.
            let (ttl, written) = match ttl {
                Some(ttl) => (ttl, ttl.to_json()),
                None => {
                    let stored = doc.get(&keys.ttl_s).filter(|value| value.is_number()).cloned();
                    match stored.as_ref().and_then(Value::as_f64).and_then(|secs| Ttl::from_secs_f64(secs).ok()) {
                        Some(ttl) => (ttl, stored.unwrap_or(Value::Null)),
                        None => return Ok(Decision::Exit(Exit::Foreign(doc))),
                    }
                }
            };
            doc.insert(keys.ttl_s.clone(), written);
            doc.insert(keys.heartbeat_at.clone(), now.to_iso().into());
            doc.insert(keys.expires_at.clone(), expiry(now, ttl)?.into());
            Ok(Decision::Write(doc, Written::Heartbeat))
        })?;
        Ok(self.outcome(kind, &key, transition))
    }

    /// Deletes the caller's lease, expired or not. Another holder's lease is refused; a missing one is
    /// already free, which is a success.
    ///
    /// # Errors
    ///
    /// When the database cannot be opened, read or written.
    pub fn release_lease(&self, task: TaskNumber, holder: &Holder) -> Result<LockOutcome, StoreError> {
        let (kind, key) = (&format().kinds.lease, task.key());
        let transition = self.transition(kind, &key, |doc, _now| {
            let Some(doc) = doc else { return Ok(Decision::Exit(Exit::Free)) };
            if !is_holder(&doc, holder) {
                return Ok(Decision::Exit(Exit::Foreign(doc)));
            }
            Ok(Decision::Delete)
        })?;
        Ok(self.outcome(kind, &key, transition))
    }

    /// Every lease, by task number (shorter keys first, then in order), read without the write lock; only
    /// the expired ones with `expired_only`. A row that is not a lease document always lists, as expired.
    ///
    /// # Errors
    ///
    /// When the database is held beyond the wait at open, or cannot be read.
    pub fn leases(&self, expired_only: bool) -> Result<Vec<LeaseEntry>, StoreError> {
        let mut rows = self.db.documents(&format().kinds.lease)?;
        let now = self.clock.now();
        rows.sort_by(|(a, _), (b, _)| (a.len(), a).cmp(&(b.len(), b)));
        let entries = rows.into_iter().map(|(key, doc)| {
            if doc.is_empty() {
                let task = if !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_digit()) {
                    key.parse::<u64>().map_or(Value::String(key.clone()), Value::from)
                } else {
                    Value::String(key)
                };
                LeaseEntry::Unreadable { task }
            } else {
                LeaseEntry::Lease(self.view(doc, now))
            }
        });
        Ok(entries.filter(|entry| !expired_only || entry.is_expired()).collect())
    }
}
