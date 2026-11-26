//! The database as the prototype opens it (L1–L3): a new connection per operation, rollback journal,
//! `synchronous=FULL`, and every mutation inside `BEGIN IMMEDIATE`, so reading, deciding and writing
//! happen under one write lock and the result exists only after `COMMIT`.
//!
//! The prototype never sets the journal mode, so a database it creates stays in the default `delete`, and
//! it would go on reading and writing a database someone put in WAL. This crate checks it at every open and
//! puts it back (#439/D4): the one lock the two languages share must not depend on a journal mode that one
//! of them never looks at.

use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension};
use serde_json::{Map, Value};

use crate::v1::sql;
use crate::{StoreError, folder};

/// The journal mode of the prototype's database and of this one.
const ROLLBACK_JOURNAL: &str = "delete";
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
    /// Faults the unit tests inject where the prototype's cases patch `sqlite3.connect`. Not part of the build.
    #[cfg(test)]
    faults: Arc<faults::Faults>,
}

impl Database {
    pub fn new(folder: PathBuf, busy_wait: Duration) -> Self {
        let file = folder.join(folder::database_name());
        Self {
            folder,
            file,
            busy_wait,
            #[cfg(test)]
            faults: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn faults(&self) -> &faults::Faults {
        &self.faults
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
                Ok(conn) => return self.rollback_journal(conn),
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
        #[cfg(test)]
        self.faults.on_open()?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(&self.file, flags)?;
        // rusqlite waits 5 s by default; the prototype waits 3.
        conn.busy_timeout(self.busy_wait)?;
        // Per connection: a COMMIT is durable before it is reported.
        conn.execute_batch("PRAGMA synchronous=FULL")?;
        conn.execute(&sql().schema, [])?;
        Ok(conn)
    }

    /// The connection, once the database is in the rollback journal (`delete`): any other mode is set back
    /// to it, and if SQLite cannot, that is an error, never an open database that is not.
    fn rollback_journal(&self, conn: Connection) -> Result<Connection, TxError> {
        let unavailable = |error: rusqlite::Error| TxError::Failed(StoreError::Unavailable { file: self.file.clone(), source: error.into() });
        let mode = journal_mode(&conn, "PRAGMA journal_mode").map_err(|error| if is_busy(&error) { TxError::Busy } else { unavailable(error) })?;
        if mode.eq_ignore_ascii_case(ROLLBACK_JOURNAL) {
            return Ok(conn);
        }
        // SQLite answers with the mode it ended in, which is the old one if it could not switch.
        let now = journal_mode(&conn, "PRAGMA journal_mode=DELETE").unwrap_or(mode);
        if now.eq_ignore_ascii_case(ROLLBACK_JOURNAL) {
            Ok(conn)
        } else {
            Err(StoreError::JournalMode { file: self.file.clone(), mode: now }.into())
        }
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
            Ok(value) => match self.commit(&conn) {
                Ok(()) => Ok(value),
                Err(error) => Err(StoreError::WriteFailed(error.into())),
            },
            Err(error) => Err(error),
        };
        if outcome.is_err() && !conn.is_autocommit() {
            let _ = conn.execute_batch("ROLLBACK");
        }
        // An error while closing cannot undo what is committed: it is not an error of the operation.
        let _ = self.close(conn);
        outcome.map_err(TxError::Failed)
    }

    fn commit(&self, conn: &Connection) -> rusqlite::Result<()> {
        #[cfg(test)]
        self.faults.on_commit()?;
        conn.execute_batch("COMMIT")
    }

    fn close(&self, conn: Connection) -> rusqlite::Result<()> {
        let closed = conn.close().map_err(|(_, error)| error);
        #[cfg(test)]
        self.faults.on_close()?;
        closed
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

    /// The key and document of every row of `kind`, without the write lock, in no order.
    pub fn documents(&self, kind: &str) -> Result<Vec<(String, Doc)>, StoreError> {
        let conn = match self.open() {
            Ok(conn) => conn,
            Err(TxError::Busy) => return Err(StoreError::InUse),
            Err(TxError::Failed(error)) => return Err(error),
        };
        let rows = documents_of(&conn, kind).map_err(|error| StoreError::ReadFailed(error.into()));
        let _ = conn.close();
        rows
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

/// The journal mode a `PRAGMA journal_mode` statement answers with.
fn journal_mode(conn: &Connection, statement: &str) -> rusqlite::Result<String> {
    conn.query_row(statement, [], |row| row.get(0))
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

fn documents_of(conn: &Connection, kind: &str) -> rusqlite::Result<Vec<(String, Doc)>> {
    let mut statement = conn.prepare(&sql().documents)?;
    let rows = statement.query_map([kind], |row| Ok((row.get::<_, String>(0)?, decode(row.get(1)?))))?;
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

/// What the prototype's cases do with `mock.patch("sqlite3.connect", ...)`: an open that fails a number of times,
/// a `COMMIT` that does not happen, a close that raises after the commit. A test sets the fault; the next
/// operation meets it where the real error would be, and the counters tell how often.
#[cfg(test)]
pub(crate) mod faults {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use rusqlite::ffi;

    #[derive(Debug, Default)]
    pub(crate) struct Faults {
        opens: AtomicU32,
        fail_opens: AtomicU32,
        fail_commit: AtomicBool,
        fail_close: AtomicBool,
    }

    fn failure(code: std::ffi::c_int, message: &str) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(ffi::Error::new(code), Some(message.to_owned()))
    }

    impl Faults {
        /// The next `count` opens fail with "unable to open database file", as a file an antivirus holds.
        pub(crate) fn fail_next_opens(&self, count: u32) {
            self.fail_opens.store(count, Ordering::SeqCst);
        }

        /// Every `COMMIT` fails with "disk I/O error" without happening, until told otherwise.
        pub(crate) fn fail_commits(&self, on: bool) {
            self.fail_commit.store(on, Ordering::SeqCst);
        }

        /// Every close raises after the transaction is over, until told otherwise.
        pub(crate) fn fail_closes(&self, on: bool) {
            self.fail_close.store(on, Ordering::SeqCst);
        }

        /// How many times a connection was tried, failed or not.
        pub(crate) fn opens(&self) -> u32 {
            self.opens.load(Ordering::SeqCst)
        }

        pub(super) fn on_open(&self) -> rusqlite::Result<()> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            let left = self.fail_opens.load(Ordering::SeqCst);
            if left == 0 {
                return Ok(());
            }
            self.fail_opens.store(left - 1, Ordering::SeqCst);
            Err(failure(ffi::SQLITE_CANTOPEN, "unable to open database file"))
        }

        pub(super) fn on_commit(&self) -> rusqlite::Result<()> {
            if self.fail_commit.load(Ordering::SeqCst) { Err(failure(ffi::SQLITE_IOERR, "disk I/O error")) } else { Ok(()) }
        }

        pub(super) fn on_close(&self) -> rusqlite::Result<()> {
            if self.fail_close.load(Ordering::SeqCst) { Err(failure(ffi::SQLITE_MISUSE, "close failed")) } else { Ok(()) }
        }
    }
}
