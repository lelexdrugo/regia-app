//! The reader of the sessions' log of declarations (#471, from #445/V16): the prototype's `leggi` and `_parse` of
//! `tools/dichiarazioni.py`, which the moviola's `sync` reads to split a session into stretches. #445 ported the
//! writer; the compaction of the log reads it with this, and keeps what this returns the same.
//!
//! The prototype's rules, each kept here:
//! - every file whose name ends in `.jsonl`, in any case on Windows (pathlib's glob follows the platform), sorted as
//!   Python sorts paths: lowercased on Windows, as they are elsewhere;
//! - a file that cannot be read is skipped and keeps its place in the order; one that is not UTF-8 makes the read
//!   fail, as Python's `read_text` raises an error that is not an `OSError`;
//! - the lines as `str.splitlines` cuts them, after the universal newlines of a text read;
//! - a line that is not a JSON object, without an instant that reads, or without a session id is skipped;
//! - the instant is `datetime.fromisoformat` with `Z` as `+00:00`, in UTC when it has no offset, read from the text
//!   Python's `str()` gives the value: a string, or the digits of a whole number;
//! - the events in time order, then by the file's place, then by the line's.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::time::WallTime;
use crate::v1::format;

/// One line of the log, as the prototype's reader returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct LoggedDeclaration {
    /// The line's instant.
    pub at: WallTime,
    /// The session's ids the line carries: each of the session's and the agents' keys whose value is a text that
    /// is not empty.
    pub ids: BTreeSet<String>,
    /// The line's fields, as written.
    pub fields: Map<String, Value>,
    /// The line's text, without its end.
    pub line: String,
    /// The file's name, and the line's place in it.
    pub file: String,
    pub index: usize,
}

/// Why the log does not read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LogReadError {
    /// A file of the log is not UTF-8 text: the prototype's reader stops on it.
    #[error("{} is not UTF-8 text", file.display())]
    NotText { file: PathBuf },
}

/// A file of the log as listed: its name and, when it reads, its bytes.
#[derive(Debug, Clone)]
pub(crate) struct LogFile {
    pub name: String,
    pub bytes: Option<Vec<u8>>,
}

/// Every event of the log in `folder`, in the prototype's order; no folder, no event. Read only.
///
/// # Errors
///
/// [`LogReadError::NotText`] for a file that is not UTF-8.
pub fn read_declarations(folder: &Path) -> Result<Vec<LoggedDeclaration>, LogReadError> {
    let files = list(folder);
    let texts = texts(folder, &files)?;
    Ok(events(&texts))
}

/// The instant of a line, as `_parse(ev.get("ts"))` reads it: `None` where Python gives none.
#[must_use]
pub fn declared_instant(value: Option<&Value>) -> Option<WallTime> {
    match value? {
        Value::String(text) => WallTime::parse_iso(text),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            WallTime::parse_iso(&number.to_string())
        }
        _ => None,
    }
}

/// Whether `name` is a file of the log by the prototype's glob, `*.jsonl`.
pub(crate) fn is_log_name(name: &str) -> bool {
    let suffix = ".jsonl";
    if cfg!(windows) {
        name.len() >= suffix.len()
            && name
                .get(name.len() - suffix.len()..)
                .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
    } else {
        name.ends_with(suffix)
    }
}

/// The key Python sorts a name of one folder by.
pub(crate) fn sort_key(name: &str) -> String {
    if cfg!(windows) {
        name.to_lowercase()
    } else {
        name.to_owned()
    }
}

/// The files of the log in `folder`, in the prototype's order, with their bytes when they read.
pub(crate) fn list(folder: &Path) -> Vec<LogFile> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<LogFile> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| is_log_name(name))
        .map(|name| LogFile {
            bytes: fs::read(folder.join(&name)).ok(),
            name,
        })
        .collect();
    files.sort_by(|a, b| sort_key(&a.name).cmp(&sort_key(&b.name)));
    files
}

/// The texts of the files, `None` for one that does not read.
pub(crate) fn texts(
    folder: &Path,
    files: &[LogFile],
) -> Result<Vec<(String, Option<String>)>, LogReadError> {
    files
        .iter()
        .map(|file| match &file.bytes {
            None => Ok((file.name.clone(), None)),
            Some(bytes) => String::from_utf8(bytes.clone())
                .map(|text| (file.name.clone(), Some(text)))
                .map_err(|_| LogReadError::NotText {
                    file: folder.join(&file.name),
                }),
        })
        .collect()
}

/// `str.splitlines()` of a text read with universal newlines: `\r\n` is one end, and so are `\n`, `\r`, `\v`,
/// `\f`, `\x1c`, `\x1d`, `\x1e`, `\x85`, ` ` and ` `; a last end opens no line.
#[must_use]
pub fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            lines.push(&text[start..at]);
            let mut end = at + c.len_utf8();
            if c == '\r'
                && let Some((next, '\n')) = chars.peek().copied()
            {
                chars.next();
                end = next + 1;
            }
            start = end;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// One line as an event, or `None` where the prototype's reader skips it.
pub(crate) fn event(line: &str, file: &str, index: usize) -> Option<LoggedDeclaration> {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(line) else {
        return None;
    };
    let keys = &format().declarations;
    let at = declared_instant(fields.get(&keys.ts))?;
    let id_keys = std::iter::once(&keys.session).chain(keys.ids.iter().map(|(_, key)| key));
    let ids: BTreeSet<String> = id_keys
        .filter_map(|key| fields.get(key).and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    if ids.is_empty() {
        return None;
    }
    Some(LoggedDeclaration {
        at,
        ids,
        fields,
        line: line.to_owned(),
        file: file.to_owned(),
        index,
    })
}

/// The events of texts already listed in the prototype's order, sorted as it sorts them.
pub(crate) fn events(texts: &[(String, Option<String>)]) -> Vec<LoggedDeclaration> {
    let mut out: Vec<(usize, LoggedDeclaration)> = Vec::new();
    for (place, (name, text)) in texts.iter().enumerate() {
        let Some(text) = text else { continue };
        for (index, line) in split_lines(text).into_iter().enumerate() {
            if let Some(event) = event(line, name, index) {
                out.push((place, event));
            }
        }
    }
    out.sort_by(|(a_place, a), (b_place, b)| {
        (a.at.unix_micros(), *a_place, a.index).cmp(&(b.at.unix_micros(), *b_place, b.index))
    });
    out.into_iter().map(|(_, event)| event).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_cut_as_python_cuts_them() {
        assert_eq!(split_lines("a\nb\n"), ["a", "b"]);
        assert_eq!(split_lines("a\r\nb"), ["a", "b"]);
        assert_eq!(split_lines("a\r\r\nb"), ["a", "", "b"]);
        assert_eq!(split_lines("a\u{2028}b\u{b}c\u{85}"), ["a", "b", "c"]);
        assert_eq!(split_lines(""), Vec::<&str>::new());
        assert_eq!(split_lines("\n"), [""]);
    }

    #[test]
    fn the_instant_is_read_from_python_s_text_of_the_value() {
        let at = |value: Value| declared_instant(Some(&value)).map(WallTime::to_iso_seconds);
        assert_eq!(
            at(Value::from("2026-01-10T09:00:00Z")).as_deref(),
            Some("2026-01-10T09:00:00Z")
        );
        assert_eq!(
            at(Value::from("2026-01-10T10:00:00+01:00")).as_deref(),
            Some("2026-01-10T09:00:00Z")
        );
        assert_eq!(
            at(Value::from("2026-01-10T09:00:00")).as_deref(),
            Some("2026-01-10T09:00:00Z"),
            "naive is UTC"
        );
        assert_eq!(
            at(Value::from(20_260_110)).as_deref(),
            Some("2026-01-10T00:00:00Z"),
            "str() of a whole number"
        );
        assert_eq!(at(Value::from(2026.5)), None);
        assert_eq!(at(Value::Bool(true)), None);
        assert_eq!(at(Value::Null), None);
        assert_eq!(declared_instant(None), None);
    }

    #[test]
    fn the_glob_follows_the_platform_s_case() {
        assert!(is_log_name("a.1.jsonl"));
        assert!(is_log_name(".jsonl"));
        assert!(!is_log_name("a.jsonl.tmp"));
        assert_eq!(is_log_name("a.JSONL"), cfg!(windows));
    }
}
