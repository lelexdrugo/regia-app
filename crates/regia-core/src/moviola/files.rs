//! The moviola's files of a dossier, read as the prototype's `dati.py` reads them, and never through a link
//! (threat model T05). Every folder on the way from the project's folder to a file, and the file itself, is looked
//! at without following it: a symbolic link or a junction there makes the read fail, where the prototype would
//! follow it. A folder or a file that is not there reads as no data, as in the prototype. A sidecar's name never
//! comes from its caller: the folder is listed and its entries compared with the name the milestone's id gives.
//! The check and the read are two steps on the path, not one on a handle: the race between them is the threat
//! model's V15, left to the module of the dossier's files it names.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::MoviolaError;
use super::format::format;
use crate::pystr;

/// A record and where it was read.
pub(crate) type Record = (Place, Map<String, Value>);

/// Where a value was read: the file, relative to the project's folder and written with `/`, and its line.
#[derive(Debug, Clone)]
pub(crate) struct Place {
    file: String,
    line: Option<usize>,
}

impl Place {
    fn file(file: String) -> Place {
        Place { file, line: None }
    }

    fn at(&self, line: usize) -> Place {
        Place { file: self.file.clone(), line: Some(line) }
    }

    pub(crate) fn invalid(&self, problem: impl Into<String>) -> MoviolaError {
        let place = match self.line {
            Some(line) => format!("{} line {line}", self.file),
            None => self.file.clone(),
        };
        MoviolaError::Invalid { place, problem: problem.into() }
    }
}

/// `segments` joined with `/`, for a message.
fn shown(segments: &[&str]) -> String {
    segments.join("/")
}

/// The moviola's folder of the project's `folder`, or `None` when a folder on the way is missing or is a
/// file. A link on the way is refused.
pub(crate) fn moviola_dir(folder: &Path) -> Result<Option<PathBuf>, MoviolaError> {
    let mut dir = folder.to_path_buf();
    let mut walked: Vec<&str> = Vec::new();
    for segment in &format().folder {
        dir.push(segment);
        walked.push(segment);
        match fs::symlink_metadata(&dir) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(MoviolaError::Read { file: shown(&walked), source }),
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(MoviolaError::Link(shown(&walked))),
            Ok(metadata) if !metadata.is_dir() => return Ok(None),
            Ok(_) => {}
        }
    }
    Ok(Some(dir))
}

/// The moviola's segments and then `rest`, for a message.
fn relative(rest: &[&str]) -> String {
    let mut segments: Vec<&str> = format().folder.iter().map(String::as_str).collect();
    segments.extend_from_slice(rest);
    shown(&segments)
}

/// The text of a regular file at `path`: `None` when nothing is there, an error for a link or anything that
/// is not a file.
fn read_regular(path: &Path, shown: &str) -> Result<Option<String>, MoviolaError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(MoviolaError::Read { file: shown.to_owned(), source }),
        Ok(metadata) if metadata.file_type().is_symlink() => return Err(MoviolaError::Link(shown.to_owned())),
        Ok(metadata) if !metadata.is_file() => return Err(MoviolaError::NotAFile(shown.to_owned())),
        Ok(_) => {}
    }
    match fs::read(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(MoviolaError::Read { file: shown.to_owned(), source }),
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| Place::file(shown.to_owned()).invalid("not UTF-8")),
    }
}

/// The records of a JSONL file of the moviola, each with its place: lines split and skipped as Python's
/// `splitlines` and `strip` do, each one JSON object. A file that is not there has none.
pub(crate) fn read_lines(dir: Option<&Path>, name: &str) -> Result<Vec<Record>, MoviolaError> {
    let Some(dir) = dir else { return Ok(Vec::new()) };
    let shown = relative(&[name]);
    let Some(text) = read_regular(&dir.join(name), &shown)? else { return Ok(Vec::new()) };
    let file = Place::file(shown);
    let mut records = Vec::new();
    for (index, line) in pystr::split_lines(&text).into_iter().enumerate() {
        if pystr::strip(line).is_empty() {
            continue;
        }
        let place = file.at(index + 1);
        let record = match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(record)) => record,
            Ok(_) => return Err(place.invalid("not a JSON object")),
            Err(error) => return Err(place.invalid(format!("invalid JSON ({error})"))),
        };
        records.push((place, record));
    }
    Ok(records)
}

/// The sidecar of the milestone `id`, an id the caller has already found among the milestones, as one JSON
/// object: the folder of the sidecars is listed and the entry whose name is the id's file name is read, if it is
/// a regular file. `None` when there is none.
pub(crate) fn read_sidecar(dir: Option<&Path>, id: &str) -> Result<Option<Record>, MoviolaError> {
    let Some(dir) = dir else { return Ok(None) };
    let format = format();
    let snapshots = dir.join(&format.snapshots);
    let shown_dir = relative(&[&format.snapshots]);
    match fs::symlink_metadata(&snapshots) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(MoviolaError::Read { file: shown_dir, source }),
        Ok(metadata) if metadata.file_type().is_symlink() => return Err(MoviolaError::Link(shown_dir)),
        Ok(metadata) if !metadata.is_dir() => return Ok(None),
        Ok(_) => {}
    }
    let wanted = OsString::from(format!("{id}.{}", format.snapshot_extension));
    let entries = fs::read_dir(&snapshots).map_err(|source| MoviolaError::Read { file: shown_dir.clone(), source })?;
    for entry in entries {
        let entry = entry.map_err(|source| MoviolaError::Read { file: shown_dir.clone(), source })?;
        if entry.file_name() != wanted {
            continue;
        }
        let shown = relative(&[&format.snapshots, &wanted.to_string_lossy()]);
        let Some(text) = read_regular(&entry.path(), &shown)? else { return Ok(None) };
        let place = Place::file(shown);
        return match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(record)) => Ok(Some((place, record))),
            Ok(_) => Err(place.invalid("not a JSON object")),
            Err(error) => Err(place.invalid(format!("invalid JSON ({error})"))),
        };
    }
    Ok(None)
}

/// The fields of one record, read by the prototype's keys. A key that is missing or `null` is an absent
/// value; a value of another type than the field's makes the record unreadable.
pub(crate) struct Fields<'a> {
    pub place: &'a Place,
    record: &'a Map<String, Value>,
}

impl<'a> Fields<'a> {
    pub fn new(place: &'a Place, record: &'a Map<String, Value>) -> Fields<'a> {
        Fields { place, record }
    }

    fn present(&self, key: &str) -> Option<&'a Value> {
        self.record.get(key).filter(|value| !value.is_null())
    }

    fn wrong(&self, key: &str, wanted: &str) -> MoviolaError {
        self.place.invalid(format!("`{key}` is not {wanted}"))
    }

    pub fn text(&self, key: &str) -> Result<String, MoviolaError> {
        self.opt_text(key)?.ok_or_else(|| self.place.invalid(format!("`{key}` is missing")))
    }

    pub fn opt_text(&self, key: &str) -> Result<Option<String>, MoviolaError> {
        match self.present(key) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            Some(_) => Err(self.wrong(key, "a string")),
        }
    }

    pub fn opt_number(&self, key: &str) -> Result<Option<u64>, MoviolaError> {
        match self.present(key) {
            None => Ok(None),
            Some(value) => value.as_u64().map(Some).ok_or_else(|| self.wrong(key, "a whole number")),
        }
    }

    pub fn number(&self, key: &str) -> Result<u64, MoviolaError> {
        self.opt_number(key)?.ok_or_else(|| self.place.invalid(format!("`{key}` is missing")))
    }

    fn list(&self, key: &str) -> Result<&'a [Value], MoviolaError> {
        match self.present(key) {
            None => Ok(&[]),
            Some(Value::Array(items)) => Ok(items),
            Some(_) => Err(self.wrong(key, "a list")),
        }
    }

    pub fn texts(&self, key: &str) -> Result<Vec<String>, MoviolaError> {
        self.list(key)?
            .iter()
            .map(|item| item.as_str().map(str::to_owned).ok_or_else(|| self.wrong(key, "a list of strings")))
            .collect()
    }

    pub fn numbers(&self, key: &str) -> Result<Vec<u64>, MoviolaError> {
        self.list(key)?.iter().map(|item| item.as_u64().ok_or_else(|| self.wrong(key, "a list of whole numbers"))).collect()
    }

    pub fn object(&self, key: &str) -> Result<Option<Fields<'a>>, MoviolaError> {
        match self.present(key) {
            None => Ok(None),
            Some(Value::Object(record)) => Ok(Some(Fields { place: self.place, record })),
            Some(_) => Err(self.wrong(key, "an object")),
        }
    }

    pub fn objects(&self, key: &str) -> Result<Vec<Fields<'a>>, MoviolaError> {
        self.list(key)?
            .iter()
            .map(|item| match item {
                Value::Object(record) => Ok(Fields { place: self.place, record }),
                _ => Err(self.wrong(key, "a list of objects")),
            })
            .collect()
    }
}
