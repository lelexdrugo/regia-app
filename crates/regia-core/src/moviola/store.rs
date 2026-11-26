//! The raw activity store (`docs/moviola/CONTRATTO.md` of the dossier, §3, §4, §5.6), as the prototype's `store.py`
//! keeps it: one store for every project, outside git, a JSONL file a month named by the month of each record's
//! start. Both implementations read and write it while they coexist, so its files are written byte for byte as
//! Python writes them, its keys in the order they were read, and its record of what was consolidated keeps the
//! prototype's fingerprints.
//!
//! - A record's key is stable: a dispatch's is `dispatch:<id>`; a session written with its attribution is cut into
//!   stretches, `<source>:<session>@<start>`; a whole session, as older versions wrote it, `<source>:<session>`.
//! - Retention is 90 days. A record with a project and a task is the matter of an aggregate: it is neither removed
//!   nor admitted again until its task has been consolidated, with the fingerprint of its records at that moment,
//!   and all of its records are past the cut. A record without one of the two has no aggregate, and just expires.
//! - A store that is not there reads as empty, and only a write creates it: on a machine without the collection of
//!   the activity there is none (#491 of the dossier).
//!
//! Where the folder is, the caller decides: [`folder_rule`] gives the prototype's rule, and refusing a folder inside
//! the temporary one, which Storage Sense empties, is `regia-store`'s (`is_within`, `temporary_folder`), where that
//! rule lives. The collection's own writes (the replacement of a session's stretches, the sessions a dispatch
//! absorbs) are not ported: only the collection uses them.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};

use super::MoviolaError;
use super::format::format;
use super::pyjson::{self, Dict, Json, Object};

/// The instants of the store, at the second, in UTC.
pub(crate) const INSTANT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// The variable that moves the store, in the prototype's name: the two implementations find one store.
pub fn folder_variable() -> &'static str {
    &format().store.variable
}

/// The store's folder by the prototype's rule (`cartella_store`): the variable when it is set and not empty, else
/// the store's folder under the state folder; `None` when neither names one.
pub fn folder_rule(variable: Option<&OsStr>, state_dir: Option<&Path>) -> Option<PathBuf> {
    match variable.filter(|value| !value.is_empty()) {
        Some(value) => Some(PathBuf::from(value)),
        None => state_dir.map(|dir| dir.join(&format().store.folder)),
    }
}

/// The record's value at `key`, `None` when it is absent.
fn field<'a>(record: &'a Object, key: &str) -> &'a Json {
    record.value(key)
}

fn missing(key: &str) -> String {
    format!("`{key}` is missing")
}

/// A record's key in the store (`chiave`).
pub fn key(record: &Object) -> Result<String, String> {
    let keys = &format().record;
    let dispatch = field(record, &keys.dispatch);
    if dispatch.truthy() {
        return Ok(format!("{}:{}", format().store.dispatch_prefix, dispatch.py_str()));
    }
    let source = record.get(&keys.source).ok_or_else(|| missing(&keys.source))?.py_str();
    let session = record.get(&keys.session).ok_or_else(|| missing(&keys.session))?.py_str();
    if record.contains_key(&keys.attribution) {
        let start = record.get(&keys.start).ok_or_else(|| missing(&keys.start))?.py_str();
        return Ok(format!("{source}:{session}@{start}"));
    }
    Ok(format!("{source}:{session}"))
}

/// Whether a record is the matter of an aggregate (`richiede_consolidamento`): a project, and a task.
pub fn needs_consolidation(record: &Object) -> bool {
    let keys = &format().record;
    field(record, &keys.project).truthy() && !field(record, &keys.task).is_null()
}

/// `<project>#<task>`, the key of a task in the record of what was consolidated (`chiave_task`).
pub fn task_key(record: &Object) -> String {
    let keys = &format().record;
    format!("{}#{}", field(record, &keys.project).py_str(), field(record, &keys.task).py_str())
}

/// A record as one line of the store (`riga`).
pub fn line(record: &Object) -> String {
    pyjson::dumps_object(record)
}

/// The fingerprint of a task's records (`impronta`): SHA-256 of each key and line, by key.
pub fn fingerprint(records: &[&Object]) -> Result<String, String> {
    let mut keyed: Vec<(String, &Object)> = records.iter().map(|record| key(record).map(|key| (key, *record))).collect::<Result<_, _>>()?;
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hash = Sha256::new();
    for (key, record) in keyed {
        hash.update(format!("{key}\t{}\n", line(record)).as_bytes());
    }
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A record's start, which the store compares as text, as the prototype does.
fn start(record: &Object) -> Result<&str, String> {
    let keys = &format().record;
    match record.get(&keys.start) {
        Some(Json::Str(text)) => Ok(text),
        Some(_) => Err(format!("`{}` is not a string", keys.start)),
        None => Err(missing(&keys.start)),
    }
}

/// `mese`: the month of an instant, its first seven characters.
fn month(start: &str) -> String {
    start.chars().take(7).collect()
}

/// `_taglio`: the instant past which a record is kept, as text.
fn cut(now: DateTime<Utc>) -> String {
    (now - Duration::days(format().store.retention_days)).format(INSTANT).to_string()
}

/// Writes `text` to `path` as the prototype does (`_scrivi_atomico`): a file beside it named for this process,
/// flushed to the disk, then moved over the old one. The folder is created if it is missing.
pub(crate) fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    let temporary = path.with_file_name(name);
    let mut file = fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

/// What [`ActivityStore::upsert`] did, by record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Upserted {
    pub new: u64,
    pub updated: u64,
    pub unchanged: u64,
    pub skipped: u64,
    pub removed: u64,
}

/// What [`ActivityStore::prune`] did: the records removed, and those past the cut that it kept, because their task
/// is not consolidated or is still alive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pruned {
    pub removed: u64,
    pub kept_unconsolidated: u64,
}

/// The raw activity store in a folder.
#[derive(Debug, Clone)]
pub struct ActivityStore {
    folder: PathBuf,
}

impl ActivityStore {
    pub fn new(folder: impl Into<PathBuf>) -> ActivityStore {
        ActivityStore { folder: folder.into() }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    fn error(&self, file: &str, source: std::io::Error) -> MoviolaError {
        MoviolaError::Read { file: self.folder.join(file).display().to_string(), source }
    }

    /// The monthly files, by name (`file_mensili`): `????-??.<extension>`, without regard to case on Windows.
    fn month_files(&self) -> Result<Vec<String>, MoviolaError> {
        let entries = match fs::read_dir(&self.folder) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(self.error("", source)),
            Ok(entries) => entries,
        };
        let suffix = format!(".{}", format().store.month_extension);
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| self.error("", source))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
            let (head, tail) = name.char_indices().nth(7).map_or((name.as_str(), ""), |(at, _)| name.split_at(at));
            let tail_matches = if cfg!(windows) { tail.eq_ignore_ascii_case(&suffix) } else { tail == suffix };
            if head.chars().count() == 7 && head.chars().nth(4) == Some('-') && tail_matches {
                names.push(name);
            }
        }
        names.sort_by_cached_key(|name| if cfg!(windows) { name.to_lowercase() } else { name.clone() });
        Ok(names)
    }

    /// Every record of every month, by key (`leggi`): a key read again replaces the record in its first place.
    pub fn read(&self) -> Result<Dict<Object>, MoviolaError> {
        let mut out = Dict::new();
        for name in self.month_files()? {
            let bytes = fs::read(self.folder.join(&name)).map_err(|source| self.error(&name, source))?;
            let text = String::from_utf8(bytes).map_err(|_| MoviolaError::Invalid { place: name.clone(), problem: "not UTF-8".to_owned() })?;
            for (index, text_line) in crate::pystr::split_lines(&text).into_iter().enumerate() {
                if crate::pystr::strip(text_line).is_empty() {
                    continue;
                }
                let place = || format!("{name} line {}", index + 1);
                let record = match pyjson::loads(text_line) {
                    Ok(Json::Object(record)) => record,
                    Ok(_) => return Err(MoviolaError::Invalid { place: place(), problem: "not a JSON object".to_owned() }),
                    Err(error) => return Err(MoviolaError::Invalid { place: place(), problem: format!("invalid JSON ({error})") }),
                };
                let key = key(&record).map_err(|problem| MoviolaError::Invalid { place: place(), problem })?;
                out.insert(key, record);
            }
        }
        Ok(out)
    }

    /// The record of what was consolidated (`consolidati`): task key → fingerprint and instant. Empty when the file is
    /// not there or does not read as JSON; a JSON value other than an object is an error, where the prototype fails
    /// at its first use.
    pub fn consolidated(&self) -> Result<Object, MoviolaError> {
        let name = &format().store.consolidated;
        let bytes = match fs::read(self.folder.join(name)) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Object::new()),
            Err(source) => return Err(self.error(name, source)),
            Ok(bytes) => bytes,
        };
        let Ok(text) = String::from_utf8(bytes) else { return Ok(Object::new()) };
        match pyjson::loads(&text) {
            Ok(Json::Object(object)) => Ok(object),
            Ok(_) => Err(MoviolaError::Invalid { place: name.clone(), problem: "not a JSON object".to_owned() }),
            Err(_) => Ok(Object::new()),
        }
    }

    /// Records that `task` of `project` is consolidated with `records` as they are now (`segna_consolidato`), at
    /// `now`.
    pub fn mark_consolidated(&self, project: &str, task: i128, records: &[&Object], now: DateTime<Utc>) -> Result<(), MoviolaError> {
        let store = &format().store;
        let mut consolidated = self.consolidated()?;
        let fingerprint = fingerprint(records).map_err(|problem| MoviolaError::Invalid { place: store.consolidated.clone(), problem })?;
        let mut entry = Object::new();
        entry.insert(store.fingerprint.clone(), Json::Str(fingerprint));
        entry.insert(store.stamp.clone(), Json::Str(now.format(INSTANT).to_string()));
        consolidated.insert(format!("{project}#{task}"), Json::Object(entry));
        let text = format!("{}\n", pyjson::dumps_sorted(&Json::Object(consolidated)));
        write_atomic(&self.folder.join(&store.consolidated), &text).map_err(|source| self.write_error(&store.consolidated, source))
    }

    fn write_error(&self, file: &str, source: std::io::Error) -> MoviolaError {
        MoviolaError::Write { file: self.folder.join(file).display().to_string(), source }
    }

    /// Rewrites the months in `touched` from `records` (`_scrivi_mesi`): each by start and key, a month left with
    /// none removed, a file whose text would not change left as it is.
    fn write_months(&self, records: &Dict<Object>, touched: &BTreeSet<String>) -> Result<(), MoviolaError> {
        let mut by_month: BTreeMap<String, Vec<(&str, String, &Object)>> = BTreeMap::new();
        for record in records.values() {
            let start = start(record).map_err(|problem| MoviolaError::Invalid { place: self.folder.display().to_string(), problem })?;
            let key = key(record).map_err(|problem| MoviolaError::Invalid { place: self.folder.display().to_string(), problem })?;
            by_month.entry(month(start)).or_default().push((start, key, record));
        }
        for month in touched {
            let name = format!("{month}.{}", format().store.month_extension);
            let path = self.folder.join(&name);
            let mut lines = by_month.remove(month).unwrap_or_default();
            if lines.is_empty() {
                match fs::remove_file(&path) {
                    Err(error) if error.kind() != ErrorKind::NotFound => return Err(self.write_error(&name, error)),
                    _ => {}
                }
                continue;
            }
            lines.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
            let text: String = lines.iter().map(|(_, _, record)| format!("{}\n", line(record))).collect();
            if fs::read(&path).ok().and_then(|bytes| String::from_utf8(bytes).ok()).as_deref() != Some(text.as_str()) {
                write_atomic(&path, &text).map_err(|source| self.write_error(&name, source))?;
            }
        }
        Ok(())
    }

    /// Adds or replaces records by key (`upsert`), then removes the keys of `remove`. A new record already past the
    /// cut, of a task consolidated or with no aggregate, is not admitted. A record of a key already seen in
    /// `records` is skipped.
    pub fn upsert(&self, records: &[Object], remove: &[String], now: DateTime<Utc>) -> Result<Upserted, MoviolaError> {
        let mut existing = self.read()?;
        let consolidated = self.consolidated()?;
        let cut = cut(now);
        let mut stats = Upserted::default();
        let mut touched = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let invalid = |problem: String| MoviolaError::Invalid { place: "record".to_owned(), problem };
        for record in records {
            let key = key(record).map_err(invalid)?;
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(old) = existing.get(&key) {
                if old.py_eq(record) {
                    stats.unchanged += 1;
                    continue;
                }
                touched.insert(month(start(old).map_err(invalid)?));
                touched.insert(month(start(record).map_err(invalid)?));
                existing.insert(key, record.clone());
                stats.updated += 1;
                continue;
            }
            let begins = start(record).map_err(invalid)?;
            if begins < cut.as_str() && (!needs_consolidation(record) || consolidated.contains_key(&task_key(record))) {
                stats.skipped += 1;
                continue;
            }
            touched.insert(month(begins));
            existing.insert(key, record.clone());
            stats.new += 1;
        }
        let mut removing = BTreeSet::new();
        for key in remove {
            if !removing.insert(key) {
                continue;
            }
            if let Some(old) = existing.remove(key) {
                touched.insert(month(start(&old).map_err(invalid)?));
                stats.removed += 1;
            }
        }
        if !touched.is_empty() {
            self.write_months(&existing, &touched)?;
        }
        Ok(stats)
    }

    /// Removes the records past the retention (`pota`), as consolidation allows: a record of a task goes only when
    /// the task's fingerprint is the one consolidated and every record of the task is past the cut.
    pub fn prune(&self, now: DateTime<Utc>) -> Result<Pruned, MoviolaError> {
        let mut existing = self.read()?;
        let consolidated = self.consolidated()?;
        let cut = cut(now);
        let store = &format().store;
        let invalid = |problem: String| MoviolaError::Invalid { place: self.folder.display().to_string(), problem };
        let mut by_task: BTreeMap<String, Vec<&Object>> = BTreeMap::new();
        for record in existing.values().filter(|record| needs_consolidation(record)) {
            by_task.entry(task_key(record)).or_default().push(record);
        }
        // Whether a task's records may go, asked only for a record past the cut, in the prototype's order: the entry,
        // then its fingerprint, then the starts of the whole group.
        let mut removable: BTreeMap<String, bool> = BTreeMap::new();
        let mut may_go = |task: &str| -> Result<bool, MoviolaError> {
            if let Some(known) = removable.get(task) {
                return Ok(*known);
            }
            let group = by_task.get(task).map(Vec::as_slice).unwrap_or_default();
            let answer = match consolidated.value(task) {
                held if !held.truthy() => false,
                Json::Object(entry) => {
                    entry.value(&store.fingerprint).py_eq(&Json::Str(fingerprint(group).map_err(invalid)?))
                        && group.iter().map(|record| start(record).map(|begins| begins < cut.as_str())).collect::<Result<Vec<_>, _>>().map_err(invalid)?.into_iter().all(|past| past)
                }
                _ => return Err(invalid(format!("`{task}` of {} is not an object", store.consolidated))),
            };
            removable.insert(task.to_owned(), answer);
            Ok(answer)
        };
        let mut doomed: Vec<String> = Vec::new();
        let mut kept = 0;
        for (key, record) in existing.iter() {
            if start(record).map_err(invalid)? >= cut.as_str() {
                continue;
            }
            if !needs_consolidation(record) || may_go(&task_key(record))? {
                doomed.push(key.to_owned());
            } else {
                kept += 1;
            }
        }
        if !doomed.is_empty() {
            let mut touched = BTreeSet::new();
            for key in &doomed {
                if let Some(old) = existing.remove(key) {
                    touched.insert(month(start(&old).map_err(invalid)?));
                }
            }
            self.write_months(&existing, &touched)?;
        }
        Ok(Pruned { removed: doomed.len() as u64, kept_unconsolidated: kept })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_takes_the_variable_unless_it_is_empty() {
        let state = Path::new("state");
        assert_eq!(folder_rule(Some(OsStr::new("moved")), Some(state)), Some(PathBuf::from("moved")));
        assert_eq!(folder_rule(Some(OsStr::new("")), Some(state)), Some(state.join(&format().store.folder)));
        assert_eq!(folder_rule(None, Some(state)), Some(state.join(&format().store.folder)));
        assert_eq!(folder_rule(Some(OsStr::new("moved")), None), Some(PathBuf::from("moved")));
        assert_eq!(folder_rule(None, None), None);
    }

    #[test]
    fn the_month_is_the_first_seven_characters_and_the_cut_is_ninety_days_back() {
        assert_eq!(month("2026-10-14T10:00:00Z"), "2026-10");
        assert_eq!(month("abc"), "abc");
        let now = DateTime::parse_from_rfc3339("2026-10-19T00:00:00Z").unwrap().with_timezone(&Utc);
        assert_eq!(cut(now), "2026-07-21T00:00:00Z");
    }
}
