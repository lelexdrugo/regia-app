//! The orchestrator's step (L10, L12 and L13 of the dossier's conformance study), on the project's lock
//! and two cursors: `step`, the step that is open, and `events`, where the precheck starts counting.
//!
//! Each operation is one transaction that checks who it is for before it writes. Opening checks that the
//! lock is still the caller's: the lock can expire and pass to someone else between taking it and writing
//! the step, and that window is D3 of the prototype's #417. Closing checks that the open step is the
//! caller's, writes the events cursor and empties the step together, so a comment the step records either
//! comes before and enters the cursor, or after and is no longer the step's. The lock is taken and
//! released around them by the caller, with the lock's own operations.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::cursor::{CursorName, cursor_doc};
use crate::db::{self, TxError, random_u64};
use crate::time::WallTime;
use crate::v1::format;
use crate::{Holder, ProjectId, Store, StoreError};

/// What [`Store::open_step`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum StepOpening {
    Opened(OpenedStep),
    /// The lock was no longer the caller's when the step was to be written, and nothing was written.
    /// `holder` names who has it now, `None` when nobody does.
    LockLost { holder: Option<Value> },
}

/// The step as the step cursor now holds it: a new one, or the caller's own, still open, resumed with
/// its instant and its comments.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedStep {
    step: Map<String, Value>,
    resumed: bool,
}

impl OpenedStep {
    /// When the step began to observe: the new step's instant, or the resumed one's as it was stored.
    #[must_use]
    pub fn t(&self) -> &Value {
        self.step.get(&format().step.t).unwrap_or(&Value::Null)
    }

    #[must_use]
    pub fn resumed(&self) -> bool {
        self.resumed
    }
}

/// What [`Store::close_step`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum StepClosing {
    /// The events cursor was written as this, and the step emptied.
    Closed(EventsCursor),
    /// The caller has no step open on the project: nothing was written.
    NoStep,
}

/// The events cursor of a project, as the precheck reads it.
#[derive(Debug, Clone, PartialEq)]
pub enum EventsCursor {
    /// No row, or no value: nothing was processed, and every report and comment is new.
    Absent,
    /// The old form, the instant alone, which the precheck still reads.
    Instant(String),
    /// The form a step's end writes, each part as it is stored.
    Parts(EventsParts),
    /// A value of any other kind, which the precheck refuses as unreadable.
    Other(Value),
}

/// `{v, t, own: {comments, reports}}`, each part `None` when the document lacks it. `reports` maps the
/// file name of a report to the date it had when the step processed it.
#[derive(Debug, Clone, PartialEq)]
pub struct EventsParts {
    pub version: Option<Value>,
    pub t: Option<Value>,
    pub own: Option<OwnParts>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OwnParts {
    pub comments: Option<Value>,
    pub reports: Option<Value>,
}

impl EventsCursor {
    fn read(value: Value) -> Self {
        match value {
            Value::Null => Self::Absent,
            Value::String(instant) => Self::Instant(instant),
            Value::Object(fields) => {
                let keys = &format().events;
                let own = fields.get(&keys.own).map(|own| OwnParts {
                    comments: own.get(&keys.comments).cloned(),
                    reports: own.get(&keys.reports).cloned(),
                });
                Self::Parts(EventsParts { version: fields.get(&keys.version).cloned(), t: fields.get(&keys.t).cloned(), own })
            }
            other => Self::Other(other),
        }
    }
}

impl Store {
    /// The wall clock this store reads its expiries on: a step takes its instant from it before the lock.
    #[must_use]
    pub fn now(&self) -> WallTime {
        self.clock.now()
    }

    /// The project's events cursor, read without the write lock.
    ///
    /// # Errors
    ///
    /// As [`Store::cursor`].
    pub fn events_cursor(&self, project: &ProjectId) -> Result<EventsCursor, StoreError> {
        Ok(EventsCursor::read(self.cursor(project, &CursorName::events())?))
    }

    /// Opens the caller's step on the project, whose lock the caller has just taken, in one transaction
    /// that first checks the lock is still the caller's. The caller's own step, still open, is resumed with
    /// its instant and its comments; any other is replaced by a new one that began to observe at `t`. The
    /// step records `session`, the session that opened it last.
    ///
    /// # Errors
    ///
    /// [`StoreError::InUse`] when the database is held beyond the wait, or when it cannot be read or
    /// written.
    pub fn open_step(&self, project: &ProjectId, holder: &Holder, session: Option<&str>, t: WallTime) -> Result<StepOpening, StoreError> {
        let kinds = &format().kinds;
        let keys = &format().step;
        let step_key = CursorName::step().key(project);
        let written = self.db.write(|conn| {
            let lock = db::select(conn, &kinds.lock, project.as_str())?;
            let lock_holder = lock.as_ref().and_then(|doc| doc.get(&format().lock.holder)).cloned();
            if lock_holder.as_ref().and_then(Value::as_str) != Some(holder.as_str()) {
                return Ok(StepOpening::LockLost { holder: lock_holder.filter(truthy) });
            }
            let previous = db::select(conn, &kinds.cursor, &step_key)?.and_then(|doc| doc.get(&format().cursor.value).cloned());
            let session = session.map_or(Value::Null, Value::from);
            let (mut step, resumed) = match previous {
                Some(Value::Object(previous))
                    if previous.get(&keys.holder).and_then(Value::as_str) == Some(holder.as_str())
                        && previous.get(&keys.t).is_some_and(truthy) =>
                {
                    (previous, true)
                }
                _ => {
                    let mut fresh = Map::new();
                    fresh.insert(keys.version.clone(), 1.into());
                    fresh.insert(keys.holder.clone(), holder.as_str().into());
                    fresh.insert(keys.session.clone(), session.clone());
                    fresh.insert(keys.t.clone(), t.to_iso_seconds().into());
                    fresh.insert(keys.comments.clone(), Value::Array(Vec::new()));
                    (fresh, false)
                }
            };
            // A missing session reads as none, as the prototype reads it: no key is added for it.
            if step.get(&keys.session).unwrap_or(&Value::Null) != &session {
                step.insert(keys.session.clone(), session);
            }
            db::upsert(conn, &kinds.cursor, &step_key, &cursor_doc(Value::Object(step.clone()), self.clock.now()))?;
            Ok(StepOpening::Opened(OpenedStep { step, resumed }))
        });
        match written {
            Ok(opening) => Ok(opening),
            Err(TxError::Busy) => Err(StoreError::InUse),
            Err(TxError::Failed(error)) => Err(error),
        }
    }

    /// Closes the caller's open step on the project in one transaction: the events cursor gets the step's
    /// instant, its comments sorted and once each, and `reports`, the reports it processed, by file name
    /// with their dates; the step cursor is emptied. Releasing the lock is the caller's, after.
    ///
    /// # Errors
    ///
    /// [`StoreError::InUse`] when the database is held beyond the wait, [`StoreError::StepComments`]
    /// when the step holds comments that are not numbers, or when it cannot be read or written.
    pub fn close_step(&self, project: &ProjectId, holder: &Holder, reports: &BTreeMap<String, String>) -> Result<StepClosing, StoreError> {
        let kinds = &format().kinds;
        let (keys, events) = (&format().step, &format().events);
        let step_key = CursorName::step().key(project);
        let events_key = CursorName::events().key(project);
        let written = self.db.write(|conn| {
            let step = db::select(conn, &kinds.cursor, &step_key)?.and_then(|doc| doc.get(&format().cursor.value).cloned());
            let Some(Value::Object(step)) = step else { return Ok(StepClosing::NoStep) };
            let t = step.get(&keys.t).filter(|t| truthy(t));
            let (true, Some(t)) = (step.get(&keys.holder).and_then(Value::as_str) == Some(holder.as_str()), t) else {
                return Ok(StepClosing::NoStep);
            };
            let mut own = Map::new();
            own.insert(events.comments.clone(), Value::Array(sorted_comments(step.get(&keys.comments))?));
            if !reports.is_empty() {
                let reports = reports.iter().map(|(name, date)| (name.clone(), Value::from(date.as_str()))).collect();
                own.insert(events.reports.clone(), Value::Object(reports));
            }
            let mut cursor = Map::new();
            cursor.insert(events.version.clone(), 1.into());
            cursor.insert(events.t.clone(), t.clone());
            cursor.insert(events.own.clone(), Value::Object(own));
            let cursor = Value::Object(cursor);
            let now = self.clock.now();
            db::upsert(conn, &kinds.cursor, &events_key, &cursor_doc(cursor.clone(), now))?;
            db::upsert(conn, &kinds.cursor, &step_key, &cursor_doc(Value::Null, now))?;
            Ok(StepClosing::Closed(EventsCursor::read(cursor)))
        });
        match written {
            Ok(closing) => Ok(closing),
            Err(TxError::Busy) => Err(StoreError::InUse),
            Err(TxError::Failed(error)) => Err(error),
        }
    }
}

/// `sorted(set(comments or []))`, for the comments a step records: whole numbers.
fn sorted_comments(comments: Option<&Value>) -> Result<Vec<Value>, StoreError> {
    let Some(comments) = comments.filter(|comments| truthy(comments)) else { return Ok(Vec::new()) };
    let list = comments.as_array().ok_or(StoreError::StepComments)?;
    let mut numbers = list.iter().map(|comment| comment.as_i64().ok_or(StoreError::StepComments)).collect::<Result<Vec<i64>, _>>()?;
    numbers.sort_unstable();
    numbers.dedup();
    Ok(numbers.into_iter().map(Value::from).collect())
}

/// Python's truth of a JSON value: `null`, `false`, zero and empty are false.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// A holder for a step that was given none and has no session to take its id from: the prototype's prefix
/// and 12 hex digits, which the step prints for the caller to pass back when it closes.
#[must_use]
pub fn generated_holder() -> Holder {
    let id = format!("{}{:012x}", format().step.generated_holder, random_u64() & 0xffff_ffff_ffff);
    Holder::new(id).expect("a generated holder is not blank")
}

/// The message of the dossier's commit that closes a step: `<project>: <word> <t>`, with the step's
/// instant as the events cursor holds it.
#[must_use]
pub fn step_commit_message(project: &ProjectId, t: &Value) -> String {
    let t = t.as_str().map_or_else(|| t.to_string(), str::to_owned);
    format!("{project}: {} {t}", format().step.commit_word)
}

/// The folder of a project's reports, whose dates a step's end records for the ones it processed.
#[must_use]
pub fn reports_folder(project_folder: &Path) -> PathBuf {
    format().events.report_folder.split('/').fold(project_folder.to_path_buf(), |path, part| path.join(part))
}
