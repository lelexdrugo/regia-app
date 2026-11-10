//! The state folder (L11 of the dossier's conformance study): the prototype's rule, and its refusal of
//! the temporary folder, which Storage Sense empties.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use regia_identity::{PROTOTYPE_NAME, prototype_env_var, state_dir};

use crate::StoreError;

/// The database file. It follows the prototype's name, not the product's: both implementations must
/// open the same file while they coexist (ADR 0001, section 3).
#[must_use]
pub fn database_name() -> String {
    format!("{PROTOTYPE_NAME}.db")
}

/// Where the prototype keeps its state: the folder in its environment variable, else the per-OS
/// default. Neither checked nor created.
///
/// # Errors
///
/// [`StoreError::NoFolder`] when the environment names no folder, as on Windows without `LOCALAPPDATA`.
pub fn default_folder() -> Result<PathBuf, StoreError> {
    state_dir().ok_or_else(|| StoreError::NoFolder { variable: prototype_env_var("RUNTIME") })
}

/// Refuses a folder inside the temporary folder, as the prototype does.
pub(crate) fn check(folder: &Path) -> Result<(), StoreError> {
    let temporary = temporary_folder();
    if is_within(folder, &temporary) {
        return Err(StoreError::TemporaryFolder {
            folder: folder.to_path_buf(),
            temporary,
            variable: prototype_env_var("RUNTIME"),
        });
    }
    Ok(())
}

/// Creates the folder if it is missing; the prototype does it before every operation.
pub(crate) fn create(folder: &Path) -> Result<(), StoreError> {
    fs::create_dir_all(folder).map_err(|source| StoreError::CreateFolder { folder: folder.to_path_buf(), source })
}

/// Python's `tempfile.gettempdir()`: the first of its candidates that is a folder, else the current one.
/// Python also checks that it can create a file there; this does not, so it can only refuse more.
fn temporary_folder() -> PathBuf {
    let mut candidates: Vec<PathBuf> = ["TMPDIR", "TEMP", "TMP"]
        .into_iter()
        .filter_map(env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect();
    if cfg!(windows) {
        if let Some(home) = env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
            candidates.push(Path::new(&home).join("AppData").join("Local").join("Temp"));
        }
        if let Some(root) = env::var_os("SYSTEMROOT").filter(|value| !value.is_empty()) {
            candidates.push(Path::new(&root).join("Temp"));
        }
        candidates.extend([r"c:\temp", r"c:\tmp", r"\temp", r"\tmp"].map(PathBuf::from));
    } else {
        candidates.extend(["/tmp", "/var/tmp", "/usr/tmp"].map(PathBuf::from));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_dir())
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Whether `path` is `base` or inside it, as the prototype checks it: `commonpath([p, base]) == base` on
/// `normcase(realpath(abspath(...)))` of both.
#[must_use]
pub fn is_within(path: &Path, base: &Path) -> bool {
    resolved(path).starts_with(resolved(base))
}

/// `normcase(realpath(abspath(path)))`: absolute, `.` and `..` removed by name, links resolved where the
/// path exists, and on Windows in one case.
fn resolved(path: &Path) -> PathBuf {
    let absolute = lexically_absolute(path);
    let real = real_where_it_exists(&absolute);
    if cfg!(windows) { PathBuf::from(real.as_os_str().to_string_lossy().to_lowercase()) } else { real }
}

fn lexically_absolute(path: &Path) -> PathBuf {
    let joined = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Resolves the longest part of `path` that exists, and keeps the rest as written.
fn real_where_it_exists(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest: Vec<OsString> = Vec::new();
    loop {
        if let Ok(real) = fs::canonicalize(&existing) {
            return rest.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match existing.file_name() {
            Some(name) => {
                rest.push(name.to_os_string());
                existing.pop();
            }
            None => return path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_keeps_the_prototype_name() {
        assert_eq!(database_name(), "regia.db");
    }

    #[test]
    fn a_folder_under_the_temporary_one_is_refused_even_through_dots() {
        let temporary = temporary_folder();
        assert!(is_within(&temporary, &temporary));
        assert!(is_within(&temporary.join("a").join("b"), &temporary));
        assert!(is_within(&temporary.join("a").join("..").join("b"), &temporary));
        assert!(!is_within(&temporary.join(".."), &temporary));
    }

    #[test]
    fn the_check_refuses_before_creating_anything() {
        let inside = temporary_folder().join("regia-store-refused-folder");
        assert!(matches!(check(&inside), Err(StoreError::TemporaryFolder { .. })));
        assert!(!inside.exists());
    }

    #[test]
    fn a_folder_outside_is_accepted() {
        let target = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(check(target).is_ok());
    }
}
