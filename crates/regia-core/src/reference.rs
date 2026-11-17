//! The one resolver of the dossier's references (#266 of the dossier, `tools/riferimenti.py`). A reference
//! (a report, a proof, an artifact, a milestone's snapshot) is a path relative to the dossier's folder, the
//! project's folder with `.regia/`, so a second machine resolves it from its clone. The data placeholder of
//! format v1 stands for the papers' experimental data, whose root no machine configures yet. It reads the
//! disk and writes nothing. What a document says of a reference that does not resolve is the caller's, from
//! its catalog.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;

const FORMAT: &str = include_str!("../formats/references_v1.toml");

#[derive(Deserialize)]
struct Format {
    data_placeholder: String,
    not_a_path: Patterns,
}

/// The forms that are no path, as `references_v1.toml` writes them.
#[derive(Deserialize)]
struct Patterns {
    link: String,
    location: String,
    name_placeholder: String,
    placeholder: String,
}

struct Compiled {
    data_placeholder: String,
    /// In the prototype's order: a link, a place of the machine, a `<name>`, any other placeholder.
    forms: [(Regex, NotAPathForm); 4],
}

fn format() -> &'static Compiled {
    static COMPILED: OnceLock<Compiled> = OnceLock::new();
    COMPILED.get_or_init(|| {
        let format: Format = toml::from_str(FORMAT).expect("the embedded format is valid");
        let patterns = format.not_a_path;
        let form = |pattern: &str, form| (Regex::new(pattern).expect("a valid pattern"), form);
        Compiled {
            data_placeholder: format.data_placeholder,
            forms: [
                form(&patterns.link, NotAPathForm::Link),
                form(&patterns.location, NotAPathForm::MachineLocation),
                form(&patterns.name_placeholder, NotAPathForm::NamePlaceholder),
                form(&patterns.placeholder, NotAPathForm::UnknownPlaceholder),
            ],
        }
    })
}

fn data_placeholder() -> &'static str {
    &format().data_placeholder
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
    /// Not a path to resolve, and the form it has instead: no path, no existence, whatever the disk holds.
    NotAPath(NotAPathForm),
}

/// The form of a reference that is no path (the prototype's reason for a `non-percorso`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAPathForm {
    /// A link: `https://…`, `file://…`.
    Link,
    /// A command: a string with a space inside, as `python tools/x.py --a`, unless a file or folder of that
    /// name is in the dossier.
    Command,
    /// A place of the machine: `~/…`, `%VAR%/…`, `${VAR}/…`, `$VAR/…`.
    MachineLocation,
    /// A placeholder of a name: `<worktree>/…`.
    NamePlaceholder,
    /// A placeholder that is not the data's: `{archivio}/…`.
    UnknownPlaceholder,
}

/// Why a reference does not name a file here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    /// No root of the data is configured on this machine.
    DataRootNotConfigured,
    AbsolutePath,
    LeavesTheDossier,
    /// Nothing at that path on this machine: missing, empty, or a name the system does not take.
    Absent,
    /// The reference is not a path (see [`NotAPathForm`]).
    NotAPath,
}

/// A reference, resolved against a dossier's folder. Its fields are read, never set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    original: String,
    normalized: String,
    kind: ReferenceKind,
    path: Option<PathBuf>,
    exists: bool,
    folder: bool,
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

    /// A file or a folder is at that path.
    pub fn exists(&self) -> bool {
        self.exists
    }

    /// `exists`, and it is a folder.
    pub fn is_folder(&self) -> bool {
        self.folder
    }

    /// `None` for a file that is there.
    pub fn reason(&self) -> Option<Unresolved> {
        self.reason
    }

    /// The reference as a document shows it: its path, with `/`, in backticks, and a folder that is there with
    /// a `/` at the end; followed by `absent`, the catalog's sentence, when it is not there or does not
    /// resolve. A reference that is no path comes out as written, with no sentence: "absent" would say
    /// nothing of it, and the catalog's own is [`Reference::render_with`]'s.
    pub fn render(&self, absent: &str) -> String {
        match self.kind {
            ReferenceKind::NotAPath(_) => format!("`{}`", self.original),
            _ => self.render_path(absent),
        }
    }

    /// [`Reference::render`], with a sentence of the catalog for the form of a reference that is no path:
    /// the reference as written, in backticks, then `: ` and the sentence.
    pub fn render_with(&self, absent: &str, not_a_path: impl FnOnce(NotAPathForm) -> String) -> String {
        match self.kind {
            ReferenceKind::NotAPath(form) => format!("`{}`: {}", self.original, not_a_path(form)),
            _ => self.render_path(absent),
        }
    }

    fn render_path(&self, absent: &str) -> String {
        if !self.exists {
            format!("`{}`: {absent}", self.normalized)
        } else if self.folder && !self.normalized.ends_with('/') {
            format!("`{}/`", self.normalized)
        } else {
            format!("`{}`", self.normalized)
        }
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

/// What Python's `str.strip()` and `\s` read as a space: Unicode's White_Space, and the four separators
/// U+001C to U+001F.
fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Why `normalized`, a relative reference that is not the data's, is no path; `None` if it can be one. A
/// space inside makes it a command, unless a file or folder of that name is in `folder`.
fn not_a_path(normalized: &str, folder: &Path) -> Option<NotAPathForm> {
    if let Some((_, form)) = format().forms.iter().find(|(pattern, _)| pattern.is_match(normalized)) {
        return Some(*form);
    }
    let trimmed = normalized.trim_matches(is_space);
    if trimmed.contains(is_space) {
        // An error reading the name is a name that is not there, as the prototype's `except`.
        return (!folder.join(normalized).try_exists().unwrap_or(false)).then_some(NotAPathForm::Command);
    }
    None
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
        folder: false,
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
    if let Some(form) = not_a_path(&normalized, folder) {
        return unresolved(ReferenceKind::NotAPath(form), Unresolved::NotAPath);
    }
    // The empty reference is not the dossier itself (`.` is).
    if normalized.trim_matches(is_space).is_empty() {
        return unresolved(ReferenceKind::Relative, Unresolved::Absent);
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
    let folder = raw.is_dir();
    let exists = folder || raw.is_file();
    Reference { original, normalized, kind: ReferenceKind::Relative, path: Some(path), exists, folder, reason: (!exists).then_some(Unresolved::Absent) }
}
