//! The one resolver of the dossier's references (#266 of the dossier, `tools/riferimenti.py`). A reference
//! (a report, a proof, an artifact, a milestone's snapshot) is a path relative to the dossier's folder, the
//! project's folder with `.regia/`, so a second machine resolves it from its clone. The data placeholder of
//! format v1 stands for the papers' experimental data, whose root no machine configures yet. It reads the
//! disk and writes nothing. What a document says of a reference that does not resolve is the caller's, from
//! its catalog.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

const FORMAT: &str = include_str!("../formats/references_v1.toml");

#[derive(Deserialize)]
struct Format {
    data_placeholder: String,
}

fn data_placeholder() -> &'static str {
    static PLACEHOLDER: OnceLock<String> = OnceLock::new();
    PLACEHOLDER.get_or_init(|| toml::from_str::<Format>(FORMAT).expect("the embedded format is valid").data_placeholder)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// It resolves under the dossier.
    Relative,
    /// It starts with the data placeholder as a whole segment.
    Data,
    /// A Windows or POSIX absolute path, to migrate to a relative one (#268 of the dossier). It does not
    /// resolve, even where this machine has the file.
    Absolute,
    /// `..`, or a link that leads out of the dossier. It does not resolve.
    Outside,
}

/// Why a reference does not name a file here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    /// No root of the data is configured on this machine.
    DataRootNotConfigured,
    AbsolutePath,
    LeavesTheDossier,
    /// No file at that path on this machine: missing, a folder, or a name the system does not take.
    Absent,
}

/// A reference, resolved against a dossier's folder. Its fields are read, never set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    original: String,
    normalized: String,
    kind: ReferenceKind,
    path: Option<PathBuf>,
    exists: bool,
    reason: Option<Unresolved>,
}

impl Reference {
    pub fn original(&self) -> &str {
        &self.original
    }

    /// With `/` in place of `\`.
    pub fn normalized(&self) -> &str {
        &self.normalized
    }

    pub fn kind(&self) -> ReferenceKind {
        self.kind
    }

    /// Absolute on this machine; `None` when the reference does not resolve.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// A file, not a folder, is at that path.
    pub fn exists(&self) -> bool {
        self.exists
    }

    /// `None` for a file that is there.
    pub fn reason(&self) -> Option<Unresolved> {
        self.reason
    }

    /// The reference as a document shows it: its path, with `/`, in backticks, followed by `absent`, the
    /// catalog's sentence, when the file is not there or the reference does not resolve.
    pub fn render(&self, absent: &str) -> String {
        if self.exists { format!("`{}`", self.normalized) } else { format!("`{}`: {absent}", self.normalized) }
    }
}

/// The root of the data on this machine: the one place a configured root will enter (#144, #145 of the
/// dossier). None is configured, so every data reference stays unresolved.
pub fn data_root() -> Option<PathBuf> {
    None
}

/// A drive (`C:`, `c:f.md`), a root or a UNC path, as Python's `PureWindowsPath` and `PurePosixPath` read
/// one: with `/` for `\`, a path that starts with `/` or whose second character is `:`.
fn is_absolute(normalized: &str) -> bool {
    normalized.starts_with('/') || normalized.chars().nth(1) == Some(':')
}

/// `path` with its links resolved as far as it exists, and the rest as written, as Python's `resolve()`
/// does: a part that cannot be read, missing or with a name the system refuses, is taken as missing.
fn resolve_leniently(path: &Path) -> Option<PathBuf> {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            let mut out = without_verbatim_prefix(canonical);
            out.extend(rest.iter().rev());
            return Some(out);
        }
        rest.push(existing.file_name()?);
        existing = existing.parent()?;
    }
}

/// On Windows the canonical path is a verbatim one, `\\?\C:\…`: the drive's form, as Python gives it.
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            std::path::Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:", char::from(letter))).join(components.as_path()),
            _ => path,
        },
        _ => path,
    }
}

/// The kind, the path on this machine and the existence of `reference` against the dossier's `folder`.
pub fn resolve(reference: &str, folder: &Path) -> Reference {
    let original = reference.to_owned();
    let normalized = original.replace('\\', "/");
    let unresolved = |kind, reason| Reference {
        original: original.clone(),
        normalized: normalized.clone(),
        kind,
        path: None,
        exists: false,
        reason: Some(reason),
    };
    let placeholder = data_placeholder();
    // A whole segment: `{placeholder}-copy/…` is a relative reference.
    if normalized == placeholder || normalized.strip_prefix(placeholder).is_some_and(|rest| rest.starts_with('/')) {
        // With a root configured, the rest of the reference would resolve under it here.
        return unresolved(ReferenceKind::Data, Unresolved::DataRootNotConfigured);
    }
    if is_absolute(&normalized) {
        return unresolved(ReferenceKind::Absolute, Unresolved::AbsolutePath);
    }
    // A NUL byte names no path: the prototype's resolver refuses it as it refuses `..`.
    if normalized.split('/').any(|part| part == "..") || normalized.contains('\0') {
        return unresolved(ReferenceKind::Outside, Unresolved::LeavesTheDossier);
    }
    // As Python's paths join: no `.` parts, no separator at the end.
    let relative: PathBuf = Path::new(&normalized).components().filter(|part| !matches!(part, Component::CurDir)).collect();
    let raw = folder.join(&relative);
    let (Some(path), Some(base)) = (resolve_leniently(&raw), resolve_leniently(folder)) else {
        return unresolved(ReferenceKind::Relative, Unresolved::Absent);
    };
    // A link must not lead out of the dossier either.
    if !path.starts_with(&base) {
        return unresolved(ReferenceKind::Outside, Unresolved::LeavesTheDossier);
    }
    // On the path as written, not on the resolved one.
    let exists = raw.is_file();
    Reference { original, normalized, kind: ReferenceKind::Relative, path: Some(path), exists, reason: (!exists).then_some(Unresolved::Absent) }
}
