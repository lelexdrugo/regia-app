//! The database as the prototype opens it (L1–L3): a new connection per operation, rollback journal,
//! `synchronous=FULL`, and every mutation inside `BEGIN IMMEDIATE`, so reading, deciding and writing
//! happen under one write lock and the result exists only after `COMMIT`.

use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension};
use serde_json::{Map, Value};

use crate::v1::sql;
use crate::{StoreError, folder};

/// The longest wait for a write lock held by another process: the prototype's `ATTESA_S`.
pub const BUSY_WAIT: Duration = Duration::from_secs(3);
/// Attempts on a transient error at open, such as a file an antivirus holds on Windows: `RETRY_IO`.
const OPEN_ATTEMPTS: u32 = 40;
/// The pause between two attempts, from half to one and a half times this: `PAUSA_IO_S`.
const OPEN_PAUSE: Duration = Duration::from_millis(20);

/// A document as stored: a JSON object. A row that is not one reads as an empty object, the
/// prototype's "unreadable record".
pub(crate) type Doc = Map<String, Value>;

/// Why a write transaction did not happen.
#[derive(Debug)]
pub(crate) enum TxError {
    /// Another transaction held the database beyond the wait: the prototype's `occupato`.
    Busy,
    Failed(StoreError),
}

impl From<StoreError> for TxError {
    fn from(error: StoreError) -> Self {
        Self::Failed(error)
    }
}

/// One row of the table, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: String,
    pub key: String,
    pub generation: i64,
    pub document: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Database {
    folder: PathBuf,
    file: PathBuf,
    busy_wait: Duration,
}

impl Database {
    pub fn new(folder: PathBuf, busy_wait: Duration) -> Self {
        let file = folder.join(folder::database_name());
        Self { folder, file, busy_wait }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn set_busy_wait(&mut self, wait: Duration) {
        self.busy_wait = wait;
    }

    /// A new connection, with the schema created if missing.
    fn open(&self) -> Result<Connection, TxError> {
        folder::create(&self.folder)?;
        let mut attempt = 1;
        loop {
            match self.try_open() {
                Ok(conn) => return Ok(conn),
                Err(error) if is_busy(&error) => return Err(TxError::Busy),
                Err(error) if is_transient(&error) && attempt < OPEN_ATTEMPTS => {
                    attempt += 1;
                    thread::sleep(OPEN_PAUSE.mul_f64(0.5 + random_unit()));
                }
                Err(error) => return Err(StoreError::Unavailable { file: self.file.clone(), source: error.into() }.into()),
            }
        }
    }

    fn try_open(&self) -> rusqlite::Result<Connection> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(&self.file, flags)?;
        // rusqlite waits 5 s by default; the prototype waits 3.
        conn.busy_timeout(self.busy_wait)?;
        // Per connection: a COMMIT is durable before it is reported.
        conn.execute_batch("PRAGMA synchronous=FULL")?;
        conn.execute(&sql().schema, [])?;
        Ok(conn)
    }

    /// Runs `body` inside `BEGIN IMMEDIATE`. Its value exists only after `COMMIT`; any error rolls back.
    /// A busy database at `BEGIN` is [`TxError::Busy`]; at `COMMIT` it is an error, as in the prototype.
    pub fn write<T>(&self, body: impl FnOnce(&Connection) -> Result<T, StoreError>) -> Result<T, TxError> {
        let conn = self.open()?;
        match conn.execute_batch("BEGIN IMMEDIATE") {
            Ok(()) => {}
            Err(error) if is_busy(&error) => return Err(TxError::Busy),
            Err(error) => return Err(StoreError::Unavailable { file: self.file.clone(), source: error.into() }.into()),
        }
        let outcome = match body(&conn) {
            Ok(value) => match conn.execute_batch("COMMIT") {
                Ok(()) => Ok(value),
                Err(error) => Err(StoreError::WriteFailed(error.into())),
            },
            Err(error) => Err(error),
        };
        if outcome.is_err() && !conn.is_autocommit() {
            let _ = conn.execute_batch("ROLLBACK");
        }
        // An error while closing cannot undo what is committed: it is not an error of the operation.
        let _ = conn.close();
        outcome.map_err(TxError::Failed)
    }

    /// One document, read without the write lock: `None` if the row is missing.
    pub fn read(&self, kind: &str, key: &str) -> Result<Option<Doc>, StoreError> {
        let conn = match self.open() {
            Ok(conn) => conn,
            Err(TxError::Busy) => return Err(StoreError::InUse),
            Err(TxError::Failed(error)) => return Err(error),
        };
        let read = select(&conn, kind, key);
        let _ = conn.close();
        read.map_err(|error| match error {
            StoreError::ReadFailed(source) if source.is_busy() => StoreError::InUse,
            other => other,
        })
    }

    /// Every row, in key order, without the write lock: for diagnostics and tests.
    pub fn rows(&self) -> Result<Vec<Row>, StoreError> {
        let conn = match self.open() {
            Ok(conn) => conn,
            Err(TxError::Busy) => return Err(StoreError::InUse),
            Err(TxError::Failed(error)) => return Err(error),
        };
        let rows = all_rows(&conn).map_err(|error| StoreError::ReadFailed(error.into()));
        let _ = conn.close();
        rows
    }
}

fn all_rows(conn: &Connection) -> rusqlite::Result<Vec<Row>> {
    let mut statement = conn.prepare(&sql().rows)?;
    let rows = statement.query_map([], |row| {
        let document: SqlValue = row.get(3)?;
        let document = match document {
            SqlValue::Text(text) => text,
            other => format!("{other:?}"),
        };
        Ok(Row { kind: row.get(0)?, key: row.get(1)?, generation: row.get(2)?, document })
    })?;
    rows.collect()
}

/// The document of a row inside a transaction.
pub(crate) fn select(conn: &Connection, kind: &str, key: &str) -> Result<Option<Doc>, StoreError> {
    let value: Option<SqlValue> = conn
        .query_row(&sql().select, [kind, key], |row| row.get(0))
        .optional()
        .map_err(|error| StoreError::ReadFailed(error.into()))?;
    Ok(value.map(decode))
}

pub(crate) fn upsert(conn: &Connection, kind: &str, key: &str, doc: &Doc) -> Result<(), StoreError> {
    let text = serde_json::to_string(doc).expect("a JSON object always serializes");
    conn.execute(&sql().upsert, [kind, key, text.as_str()]).map_err(|error| StoreError::WriteFailed(error.into()))?;
    Ok(())
}

pub(crate) fn delete(conn: &Connection, kind: &str, key: &str) -> Result<(), StoreError> {
    conn.execute(&sql().delete, [kind, key]).map_err(|error| StoreError::WriteFailed(error.into()))?;
    Ok(())
}

/// `json.loads` of the stored value, or `{}` if it is not a JSON object.
fn decode(value: SqlValue) -> Doc {
    let parsed = match value {
        SqlValue::Text(text) => serde_json::from_str(&text).ok(),
        SqlValue::Blob(bytes) => serde_json::from_slice(&bytes).ok(),
        _ => None,
    };
    match parsed {
        Some(Value::Object(doc)) => doc,
        _ => Doc::new(),
    }
}

fn code(error: &rusqlite::Error) -> Option<ErrorCode> {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => Some(failure.code),
        _ => None,
    }
}

/// The prototype's "locked" or "busy" in the message.
fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(code(error), Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked))
}

/// The prototype's "unable to open" or "disk i/o" in the message.
fn is_transient(error: &rusqlite::Error) -> bool {
    matches!(code(error), Some(ErrorCode::CannotOpen | ErrorCode::SystemIoFailure))
}

/// A number in [0, 1) without a random-number crate.
pub(crate) fn random_unit() -> f64 {
    (random_u64() >> 11) as f64 / (1_u64 << 53) as f64
}

/// 64 random bits without a random-number crate: each `RandomState` has fresh keys.
pub(crate) fn random_u64() -> u64 {
    RandomState::new().hash_one(thread::current().id())
}

/// A SQLite error, without exposing the driver in the public API.
#[derive(Debug)]
pub struct SqliteError(rusqlite::Error);

impl SqliteError {
    /// The database was held by another connection beyond the wait.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        is_busy(&self.0)
    }
}

impl From<rusqlite::Error> for SqliteError {
    fn from(error: rusqlite::Error) -> Self {
        Self(error)
    }
}

impl std::fmt::Display for SqliteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for SqliteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}
