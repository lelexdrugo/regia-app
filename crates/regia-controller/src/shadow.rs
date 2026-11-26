//! The shadow run's state (ADR 0001 §7, "Shadow before the gate"): a fresh copy of the state folder, so a run that
//! launches nothing never writes the live files.
//!
//! - **The database** is copied with `VACUUM INTO` from a connection opened read-only: a consistent snapshot, and the
//!   live file is never opened for writing, so not even its schema is created there. The state's own API opens every
//!   database read-write, which is why it never gets the live folder in a shadow run.
//! - **The files at the top of the folder** are copied as they are: the tracker's configuration, the state file of the
//!   controller before, the budget policy's local override, the log, to which the shadow adds its own lines.
//! - **The controller's lock** in the copy is the live controller's: it is released there as its holder, so the shadow
//!   takes its own and is not refused by a controller that runs. The suspended sessions stay: the shadow decides as
//!   the live controller would, and it resumes none.
//! - **Where**: beside the live folder, `<folder>-shadow`, with its marker written first. A later run replaces only a
//!   folder that carries the marker; any other is left alone, and the run stops.
//!
//! A live folder that is not there is not created: the copy is then empty. The names of the copy's folder and of its
//! marker are provisional, for the gate of names (#223).

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use regia_store::{BUSY_WAIT, Holder, LockStatus, ProjectId, Store, database_name};
use rusqlite::{Connection, OpenFlags};

use crate::engine::words::format as words;

/// What the copy's folder adds to the live folder's name.
pub const FOLDER_SUFFIX: &str = "-shadow";

/// The file that makes a folder a shadow copy, which a later run may replace.
pub const MARKER: &str = "shadow-copy.txt";

const MARKER_TEXT: &str = "A copy of a state folder for a shadow run of the controller: the next shadow run replaces this folder.\n";

/// Why there is no copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowError {
    /// The live folder has no name to put the suffix after.
    NoName(PathBuf),
    /// One folder is inside the other.
    Nested { live: PathBuf, copy: PathBuf },
    /// The folder of the copy is there without the marker: it is not a copy, and it is left alone.
    NotACopy(PathBuf),
    /// A file could not be read, written or removed.
    Io(String),
    /// The live database could not be copied, or the copy's lock released.
    State(String),
}

impl fmt::Display for ShadowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShadowError::NoName(live) => write!(f, "the state folder {} has no name", live.display()),
            ShadowError::Nested { live, copy } => write!(f, "the shadow copy {} and the state folder {} are inside one another", copy.display(), live.display()),
            ShadowError::NotACopy(copy) => write!(f, "{} is there and is not a shadow copy (no {MARKER}): it is left alone", copy.display()),
            ShadowError::Io(error) | ShadowError::State(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for ShadowError {}

/// A fresh copy, and the live controller whose lock it held, now released in the copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub folder: PathBuf,
    pub released: Option<String>,
}

/// The folder of the copy of `live`: beside it, with [`FOLDER_SUFFIX`] after its name.
///
/// # Errors
///
/// A folder without a name, as a root.
pub fn folder_of(live: &Path) -> Result<PathBuf, ShadowError> {
    let name = live.file_name().ok_or_else(|| ShadowError::NoName(live.to_path_buf()))?;
    let mut copy = name.to_os_string();
    copy.push(FOLDER_SUFFIX);
    Ok(live.with_file_name(copy))
}

fn io(what: &Path, error: &std::io::Error) -> ShadowError {
    ShadowError::Io(format!("{}: {error}", what.display()))
}

/// The database and the files beside it that its journal leaves: never copied as files.
fn is_database(name: &str) -> bool {
    let database: &str = &database_name();
    name == database || name.strip_prefix(database).is_some_and(|rest| rest.starts_with('-'))
}

/// A fresh copy of the state folder `live` in `copy`, which is replaced if it is a copy already.
///
/// # Errors
///
/// The folders are inside one another, `copy` is there and is not a copy, a file or the database could not be copied,
/// or the copy's controller lock could not be released.
pub fn fresh(live: &Path, copy: &Path) -> Result<Copy, ShadowError> {
    let absolute = |path: &Path| std::path::absolute(path).map_err(|error| io(path, &error));
    let (live_at, copy_at) = (absolute(live)?, absolute(copy)?);
    if live_at.starts_with(&copy_at) || copy_at.starts_with(&live_at) {
        return Err(ShadowError::Nested { live: live.to_path_buf(), copy: copy.to_path_buf() });
    }
    if fs::symlink_metadata(copy).is_ok() {
        if !copy.join(MARKER).is_file() {
            return Err(ShadowError::NotACopy(copy.to_path_buf()));
        }
        fs::remove_dir_all(copy).map_err(|error| io(copy, &error))?;
    }
    fs::create_dir_all(copy).map_err(|error| io(copy, &error))?;
    // The marker first: a copy that stops halfway is still one the next run replaces.
    fs::write(copy.join(MARKER), MARKER_TEXT).map_err(|error| io(copy, &error))?;
    if live.is_dir() {
        let database = live.join(database_name());
        if database.is_file() {
            vacuum_into(&database, &copy.join(database_name()))?;
        }
        for entry in fs::read_dir(live).map_err(|error| io(live, &error))? {
            let entry = entry.map_err(|error| io(live, &error))?;
            let name = entry.file_name();
            let skip = name.to_str().is_some_and(|name| is_database(name) || name == MARKER);
            let file = entry.file_type().map_err(|error| io(&entry.path(), &error))?.is_file();
            if file && !skip {
                fs::copy(entry.path(), copy.join(&name)).map_err(|error| io(&entry.path(), &error))?;
            }
        }
    }
    let released = release_controller_lock(copy)?;
    Ok(Copy { folder: copy.to_path_buf(), released })
}

/// The live database into `copy`, read through a connection that cannot write: a consistent snapshot.
fn vacuum_into(live: &Path, copy: &Path) -> Result<(), ShadowError> {
    let failed = |error: rusqlite::Error| ShadowError::State(format!("{}: {error}", live.display()));
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(live, flags).map_err(failed)?;
    connection.busy_timeout(BUSY_WAIT).map_err(failed)?;
    let target = copy.to_str().ok_or_else(|| ShadowError::State(format!("{}: not a UTF-8 path", copy.display())))?;
    connection.execute("VACUUM INTO ?1", [target]).map_err(failed)?;
    connection.close().map_err(|(_, error)| failed(error))
}

/// The controller's lock the copy holds, released there as its holder: the live controller's name, or none.
fn release_controller_lock(copy: &Path) -> Result<Option<String>, ShadowError> {
    if !copy.join(database_name()).is_file() {
        return Ok(None);
    }
    let failed = |error: String| ShadowError::State(format!("{}: {error}", copy.display()));
    let store = Store::at(copy).map_err(|error| failed(error.to_string()))?;
    let lock = ProjectId::new(words().reserved_project.clone()).map_err(|error| failed(error.to_string()))?;
    let holder = match store.lock_status(&lock).map_err(|error| failed(error.to_string()))? {
        LockStatus::Held(view) | LockStatus::Expired(view) => view.holder().map(str::to_owned),
        _ => None,
    };
    let Some(holder) = holder else { return Ok(None) };
    let name = Holder::new(holder.clone()).map_err(|error| failed(error.to_string()))?;
    let released = store.release_lock(&lock, &name).map_err(|error| failed(error.to_string()))?;
    let free = matches!(store.lock_status(&lock).map_err(|error| failed(error.to_string()))?, LockStatus::Free);
    if !released.is_ok() || !free {
        return Err(failed(format!("the copied lock of {holder} was not released")));
    }
    Ok(Some(holder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_is_beside_the_live_folder_and_the_database_is_never_copied_as_a_file() {
        assert_eq!(folder_of(Path::new("state")).unwrap(), PathBuf::from("state-shadow"));
        assert_eq!(folder_of(Path::new("a/b/state")).unwrap(), Path::new("a/b").join("state-shadow"));
        let database = database_name();
        for name in [database.to_owned(), format!("{database}-journal"), format!("{database}-wal")] {
            assert!(is_database(&name), "{name}");
        }
        for name in [format!("{database}x"), "controller.log".to_owned(), "tracker.json".to_owned()] {
            assert!(!is_database(&name), "{name}");
        }
    }
}
