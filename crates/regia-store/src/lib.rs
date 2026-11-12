//! Machine-local state in SQLite (ADR 0001, section 8), in the prototype's format while the two
//! implementations coexist: one `regia.db`, its documents table, and the rules of the dossier's
//! ADR 0008. The project lock is the one lock both languages share, so both must exclude each other
//! through it (ADR 0001, section 7); a task's lease, the cursors and the orchestrator's step live in
//! the same table, by the same rules.
//!
//! The names inside the database stay the prototype's: they are data, read from
//! `formats/store_v1.toml`. The API is typed and in English, and returns codes, not sentences.

mod budget;
mod clock;
mod cursor;
mod db;
mod folder;
mod lease;
mod lock;
mod step;
mod time;
mod usage;
mod v1;

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Number, Value};

pub use budget::{BudgetError, MIN_INTERVAL_S, NO_VALUE, NOTE_OVERTAKEN, NOTE_RUNNING, PROBE_TTL_S, PROBE_WAIT, UPDATE_FAILED};
pub use clock::{Clock, ManualClock, SystemClock};
pub use cursor::CursorName;
pub use db::{BUSY_WAIT, Row, SqliteError};
pub use folder::{database_name, default_folder, is_within};
pub use lease::{LeaseEntry, TaskNumber};
pub use lock::{LockOutcome, LockState, LockStatus, LockView};
pub use step::{EventsCursor, EventsParts, OpenedStep, OwnParts, StepClosing, StepOpening, generated_holder, reports_folder, step_commit_message};
pub use time::{Precision, WallTime};
pub use usage::{CacheInfo, NO_DATA, ProviderUsage, Reading, Served, TokenState, TokenStatus, UsageStatus, Window, round_to_tenth};

/// The state database of one folder, and the clock its expiries are read on.
#[derive(Debug, Clone)]
pub struct Store {
    db: db::Database,
    clock: Arc<dyn Clock>,
    probe_wait: Duration,
}

impl Store {
    /// The prototype's folder, from its environment variable or the per-OS default.
    ///
    /// # Errors
    ///
    /// As [`default_folder`] and [`Store::at`].
    pub fn open() -> Result<Self, StoreError> {
        Self::at(default_folder()?)
    }

    /// The database in `folder`, refused inside the temporary folder and created if missing. Nothing is
    /// opened until the first operation.
    ///
    /// # Errors
    ///
    /// [`StoreError::TemporaryFolder`], or [`StoreError::CreateFolder`].
    pub fn at(folder: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let folder = folder.into();
        folder::check(&folder)?;
        folder::create(&folder)?;
        Ok(Self { db: db::Database::new(folder, BUSY_WAIT), clock: Arc::new(SystemClock), probe_wait: PROBE_WAIT })
    }

    /// Reads time from `clock` instead of the system.
    #[must_use]
    pub fn with_clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Waits at most `wait` for a write lock held by another process, instead of [`BUSY_WAIT`].
    #[must_use]
    pub fn with_busy_wait(mut self, wait: Duration) -> Self {
        self.db.set_busy_wait(wait);
        self
    }

    #[must_use]
    pub fn folder(&self) -> &Path {
        self.db.folder()
    }

    #[must_use]
    pub fn database(&self) -> &Path {
        self.db.file()
    }

    /// Every row as stored, in kind and key order, read without the write lock: for diagnostics and tests.
    ///
    /// # Errors
    ///
    /// When the database cannot be opened or read.
    pub fn rows(&self) -> Result<Vec<Row>, StoreError> {
        self.db.rows()
    }
}

/// What went wrong with the state. Refusals of a lock are outcomes, not errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("no state folder: set {variable}")]
    NoFolder { variable: String },
    #[error(
        "the state folder {} is inside the temporary folder {}, which the system empties: choose another with {variable}",
        folder.display(),
        temporary.display()
    )]
    TemporaryFolder { folder: PathBuf, temporary: PathBuf, variable: String },
    #[error("cannot create the state folder {}", folder.display())]
    CreateFolder {
        folder: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the state database {} is not accessible", file.display())]
    Unavailable {
        file: PathBuf,
        #[source]
        source: SqliteError,
    },
    #[error("the state is in use by another process")]
    InUse,
    #[error("cannot read the state database")]
    ReadFailed(#[source] SqliteError),
    #[error("cannot write the state database")]
    WriteFailed(#[source] SqliteError),
    #[error("a ttl of {ttl} s runs past the last date the state can hold")]
    TtlOutOfRange { ttl: f64 },
    #[error("the open step holds comments that are not whole numbers")]
    StepComments,
}

/// An argument the prototype would refuse before touching the state.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidArgument {
    #[error("invalid project id {0:?}: lowercase letters, digits and hyphens")]
    Project(String),
    #[error("empty holder")]
    Holder,
    #[error("invalid ttl {0}: seconds, more than 0")]
    Ttl(f64),
    #[error("invalid cursor name {0:?}: lowercase letters, digits and hyphens")]
    Cursor(String),
}

/// A project id as the prototype accepts it: `[a-z0-9][a-z0-9-]*`. It is also the key of the lock row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(String);

impl ProjectId {
    /// # Errors
    ///
    /// [`InvalidArgument::Project`] when `id` does not match the pattern.
    pub fn new(id: impl Into<String>) -> Result<Self, InvalidArgument> {
        let id = id.into();
        let mut bytes = id.bytes();
        let valid = bytes.next().is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
            && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if valid { Ok(Self(id)) } else { Err(InvalidArgument::Project(id)) }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProjectId {
    type Err = InvalidArgument;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Who holds a lock: any text that is not blank by Python's `str.strip()`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Holder(String);

impl Holder {
    /// # Errors
    ///
    /// [`InvalidArgument::Holder`] when `holder` is empty or only whitespace.
    pub fn new(holder: impl Into<String>) -> Result<Self, InvalidArgument> {
        let holder = holder.into();
        // Python's whitespace is Unicode's, plus the four ASCII separators U+001C to U+001F.
        let blank = holder.chars().all(|c| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c));
        if blank { Err(InvalidArgument::Holder) } else { Ok(Self(holder)) }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Holder {
    type Err = InvalidArgument;

    fn from_str(holder: &str) -> Result<Self, Self::Err> {
        Self::new(holder)
    }
}

impl fmt::Display for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How long a lock lives, in seconds. It is written as given: whole seconds as an integer, as the
/// prototype's controller does, a fraction as a float, as its command line does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ttl {
    secs: f64,
    whole: bool,
}

impl Ttl {
    /// # Errors
    ///
    /// [`InvalidArgument::Ttl`] for 0.
    pub fn from_secs(secs: u64) -> Result<Self, InvalidArgument> {
        if secs == 0 {
            return Err(InvalidArgument::Ttl(0.0));
        }
        Ok(Self { secs: secs as f64, whole: true })
    }

    /// # Errors
    ///
    /// [`InvalidArgument::Ttl`] unless `secs` is finite and more than 0.
    pub fn from_secs_f64(secs: f64) -> Result<Self, InvalidArgument> {
        if !secs.is_finite() || secs <= 0.0 {
            return Err(InvalidArgument::Ttl(secs));
        }
        Ok(Self { secs, whole: false })
    }

    #[must_use]
    pub fn as_secs_f64(self) -> f64 {
        self.secs
    }

    fn to_json(self) -> Value {
        if self.whole {
            Value::Number(Number::from(self.secs as u64))
        } else {
            Number::from_f64(self.secs).map_or(Value::Null, Value::Number)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_checked_as_the_prototype_checks_them() {
        for good in ["a", "0", "p-1", "controller", "a--"] {
            assert!(ProjectId::new(good).is_ok(), "{good}");
        }
        for bad in ["", "-a", "A", "../x", "a/b", "a\\b", ".", "a b", "à"] {
            assert!(ProjectId::new(bad).is_err(), "{bad:?}");
        }
        assert!(Holder::new("w1").is_ok());
        for blank in ["", " ", "\t\n", "\u{1c}", "\u{3000}"] {
            assert_eq!(Holder::new(blank), Err(InvalidArgument::Holder), "{blank:?}");
        }
        assert!(Ttl::from_secs(0).is_err());
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(Ttl::from_secs_f64(bad).is_err(), "{bad}");
        }
        assert_eq!(Ttl::from_secs(600).map(Ttl::to_json), Ok(Value::from(600)));
        assert_eq!(Ttl::from_secs_f64(0.05).map(Ttl::to_json), Ok(Value::from(0.05)));
    }
}
