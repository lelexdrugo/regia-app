//! A file of the state folder that the prototype reads and writes whole, beside the database: the budget
//! policy's local override. It is written as the prototype writes it, into a temporary file beside it
//! and then moved into its place, so a reader finds the old file or the new one, never half of one.

use std::fs;
use std::io;
use std::path::Path;

/// The file `name` of `folder`, or `None` when there is none.
///
/// # Errors
///
/// Any other error of the file system.
pub fn read_state_file(folder: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
    match fs::read(folder.join(name)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes `bytes` as the file `name` of `folder`, creating the folder if it is missing.
///
/// # Errors
///
/// An error of the file system; the file in place, if any, is then the old one.
pub fn write_state_file(folder: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let temporary = folder.join(format!("{name}.tmp"));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, folder.join(name))
}

/// Removes the file `name` of `folder`: `true` if there was one.
///
/// # Errors
///
/// Any error of the file system but a missing file.
pub fn remove_state_file(folder: &Path, name: &str) -> io::Result<bool> {
    match fs::remove_file(folder.join(name)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
