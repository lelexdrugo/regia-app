//! Cursors (L9 and L10 of the dossier's conformance study): one row per project and name, its document
//! a value and when it was written. The events cursor keeps the key it always had, the project's id;
//! every other name lives at `<project>:<name>`, which no project's id can spell.

use std::fmt;

use serde_json::Value;

use crate::db::{self, Doc, TxError};
use crate::time::WallTime;
use crate::v1::format;
use crate::{InvalidArgument, ProjectId, Store, StoreError};

/// The name of a cursor, with the pattern of a project's id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CursorName(String);

impl CursorName {
    /// # Errors
    ///
    /// [`InvalidArgument::Cursor`] when `name` does not match the pattern of a project's id.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidArgument> {
        let name = name.into();
        match ProjectId::new(name.clone()) {
            Ok(_) => Ok(Self(name)),
            Err(_) => Err(InvalidArgument::Cursor(name)),
        }
    }

    /// Where the precheck starts counting reports and comments as new.
    #[must_use]
    pub fn events() -> Self {
        Self(format().cursor.events.clone())
    }

    /// The orchestrator's step that is open, if any.
    #[must_use]
    pub fn step() -> Self {
        Self(format().cursor.step.clone())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The key of the cursor's row for `project`.
    pub(crate) fn key(&self, project: &ProjectId) -> String {
        if self.0 == format().cursor.events { project.as_str().to_owned() } else { format!("{project}:{}", self.0) }
    }
}

impl fmt::Display for CursorName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Store {
    /// The value of a cursor, `null` without its row, with a row that is not a cursor's document, or
    /// with a value of `null`: the prototype's `leggi_cursore` with no default.
    ///
    /// # Errors
    ///
    /// When the database is held beyond the wait at open, or cannot be read.
    pub fn cursor(&self, project: &ProjectId, name: &CursorName) -> Result<Value, StoreError> {
        let doc = self.db.read(&format().kinds.cursor, &name.key(project))?;
        Ok(doc.and_then(|doc| doc.get(&format().cursor.value).cloned()).unwrap_or(Value::Null))
    }

    /// Writes the value of a cursor; every name is a row of its own, so writing one leaves the others.
    ///
    /// # Errors
    ///
    /// [`StoreError::InUse`] when the database is held beyond the wait, or when it cannot be written.
    pub fn write_cursor(&self, project: &ProjectId, name: &CursorName, value: Value) -> Result<(), StoreError> {
        let key = name.key(project);
        let written = self.db.write(|conn| db::upsert(conn, &format().kinds.cursor, &key, &cursor_doc(value, self.clock.now())));
        match written {
            Ok(()) => Ok(()),
            Err(TxError::Busy) => Err(StoreError::InUse),
            Err(TxError::Failed(error)) => Err(error),
        }
    }
}

/// A cursor's document: `{value, updated}`.
pub(crate) fn cursor_doc(value: Value, now: WallTime) -> Doc {
    let keys = &format().cursor;
    let mut doc = Doc::new();
    doc.insert(keys.value.clone(), value);
    doc.insert(keys.updated.clone(), now.to_iso().into());
    doc
}
