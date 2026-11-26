//! Writes as the holder of a lock (D2 of the dossier's `0001-review-codex.md`, ADR 0001 §7, and the dossier's
//! #507): each one is a single `BEGIN IMMEDIATE` transaction that first checks the lock is still the holder's,
//! then reads and writes. A holder that stood still past its lock's expiry, and that another holder took over,
//! writes nothing, whatever it remembers. The owner is checked, not the expiry, as a renewal checks it: a lock
//! that expired and that nobody took is still the holder's, so a write after a long launch goes through.

use serde_json::Value;

use crate::cursor::{CursorName, cursor_doc};
use crate::db::{self, TxError};
use crate::v1::format;
use crate::{Holder, ProjectId, Store, StoreError};

/// What an operation as a lock's holder did.
#[derive(Debug, Clone, PartialEq)]
pub enum Fenced<T> {
    /// The lock was the holder's, and the operation happened.
    Done(T),
    /// The lock was no longer the holder's, and nothing was written. `holder` names who has it now, `None` when
    /// nobody does.
    LockLost { holder: Option<Value> },
}

/// The operations of the holder of one lock: [`Store::as_holder`].
#[derive(Debug, Clone, Copy)]
pub struct AsHolder<'a> {
    store: &'a Store,
    lock: &'a ProjectId,
    holder: &'a Holder,
}

impl Store {
    /// The operations of `holder`, who holds the lock of `lock`: each checks, in its own transaction, that the
    /// lock is still the holder's before it reads or writes.
    #[must_use]
    pub fn as_holder<'a>(&'a self, lock: &'a ProjectId, holder: &'a Holder) -> AsHolder<'a> {
        AsHolder { store: self, lock, holder }
    }
}

impl AsHolder<'_> {
    /// The value of a cursor, `null` without one, read in a transaction that first checks the lock: what the
    /// holder knows of the state is what the state held while the lock was the holder's.
    ///
    /// # Errors
    ///
    /// [`StoreError::InUse`] when the database is held beyond the wait, or when it cannot be read.
    pub fn cursor(&self, project: &ProjectId, name: &CursorName) -> Result<Fenced<Value>, StoreError> {
        self.write_cursor(project, name, |stored| (None, stored))
    }

    /// Reads the value of a cursor and writes what `update` makes of it, in one transaction that first checks
    /// the lock. `update` gets the stored value, `null` without one, and gives back the value to write, `None`
    /// to write nothing, and what to return.
    ///
    /// # Errors
    ///
    /// [`StoreError::InUse`] when the database is held beyond the wait, or when it cannot be read or written.
    pub fn write_cursor<T>(
        &self,
        project: &ProjectId,
        name: &CursorName,
        update: impl FnOnce(Value) -> (Option<Value>, T),
    ) -> Result<Fenced<T>, StoreError> {
        let kinds = &format().kinds;
        let key = name.key(project);
        let written = self.store.db.write(|conn| {
            let lock = db::select(conn, &kinds.lock, self.lock.as_str())?;
            let lock_holder = lock.as_ref().and_then(|doc| doc.get(&format().lock.holder)).cloned();
            if lock_holder.as_ref().and_then(Value::as_str) != Some(self.holder.as_str()) {
                return Ok(Fenced::LockLost { holder: lock_holder.filter(named) });
            }
            let stored = db::select(conn, &kinds.cursor, &key)?.and_then(|doc| doc.get(&format().cursor.value).cloned());
            let (value, out) = update(stored.unwrap_or(Value::Null));
            if let Some(value) = value {
                db::upsert(conn, &kinds.cursor, &key, &cursor_doc(value, self.store.clock.now()))?;
            }
            Ok(Fenced::Done(out))
        });
        match written {
            Ok(done) => Ok(done),
            Err(TxError::Busy) => Err(StoreError::InUse),
            Err(TxError::Failed(error)) => Err(error),
        }
    }
}

/// Whether a stored holder names someone: not `null`, not an empty text.
fn named(holder: &Value) -> bool {
    match holder {
        Value::Null => false,
        Value::String(text) => !text.is_empty(),
        _ => true,
    }
}
