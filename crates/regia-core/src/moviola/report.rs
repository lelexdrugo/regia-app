//! The worker's reports of a dossier (`.regia/report/YYYY-MM-DD-<N>-<slug>.md`, ARCHITECTURE §5.2 of the dossier),
//! read as the prototype's `report.py` reads them: the fields of their front matter that the moviola uses, and
//! nothing of the summary or the body. The front matter is a subset of YAML (scalars, inline lists `[a, b]`, block
//! lists, lists of maps), read here with the prototype's own rules, mistakes included: a field that does not read
//! is left out, as old reports have no process fields. One reader for `consolidate`, and for `due` and the
//! collection of facts that come next (#492 of the dossier).
//!
//! Where it differs from the prototype: a report that is a link, or not a file, is skipped as one that does not
//! read; a link on the way to the folder of the reports is refused, as the moviola refuses one on the way to its
//! own files; an integer past `i128` stays text.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use super::MoviolaError;
use super::format::format;
use super::pyjson::{self, Json, Object};
use crate::pystr;

/// A report as the moviola reads it (`leggi_report`): what is absent is `None` or empty, never zero.
#[derive(Debug, Clone)]
pub struct Report {
    /// The file's name.
    pub file: String,
    /// The date at the head of the file's name, when it has one.
    pub date: Option<String>,
    /// The task: the front matter's, else the number in the file's name.
    pub task: i128,
    pub kind: Json,
    pub stop_reason: Json,
    pub round: Option<i128>,
    /// The session the report names, as written.
    pub session: Json,
    pub subagents: Option<i128>,
    /// The checks that are maps, in their order.
    pub checks: Vec<Object>,
    /// Whether the front matter has the checks' field at all.
    pub has_checks: bool,
    /// Whether it has one of the process fields: round, subagents, checks, closed defects.
    pub has_process: bool,
    /// The closed defects, each as Python's `str()` writes it.
    pub closed_defects: Vec<String>,
}

/// `_CHIAVE`: a key at the start of a line, then its value after a blank.
fn key_line() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(&format!(r"^([A-Za-z_][{word}-]*):(?:{space}+(.*))?$", word = pystr::WORD, space = pystr::SPACE)).expect("a valid pattern")
    })
}

/// `_NOME_FILE`: the date and the task at the head of a report's name.
fn file_name() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^(\d{4}-\d{2}-\d{2})-(\d+)-").expect("a valid pattern"))
}

fn integer() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^-?\d+$").expect("a valid pattern"))
}

/// A line's key and the rest after it, `None` for the rest when the key ends the line.
fn key_of(line: &str) -> Option<(&str, Option<&str>)> {
    let captures = key_line().captures(line)?;
    Some((captures.get(1)?.as_str(), captures.get(2).map(|rest| rest.as_str())))
}

/// `_senza_commento`: drops a closing ` # …` outside quotes. A backslash inside double quotes escapes nothing, as
/// in the prototype.
fn without_comment(text: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous: Option<char> = None;
    for (at, c) in text.char_indices() {
        if let Some(open) = quote {
            if !(c == '\\' && open == '"') && c == open {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '#' && previous.is_none_or(|before| before == ' ' || before == '\t') {
            return pystr::rstrip(&text[..at]);
        }
        previous = Some(c);
    }
    text
}

/// `_scalare`: a quoted string, `null`, a boolean, an integer, or the text as it is.
fn scalar(text: &str) -> Json {
    let text = without_comment(pystr::strip(text));
    if text.is_empty() {
        return Json::Null;
    }
    let quoted = |quote: char| text.chars().count() >= 2 && text.starts_with(quote) && text.ends_with(quote);
    let inner = || {
        let mut chars = text.chars();
        chars.next();
        chars.next_back();
        chars.as_str()
    };
    if quoted('"') {
        return match pyjson::loads(text) {
            Ok(value) => value,
            Err(_) => Json::Str(inner().to_owned()),
        };
    }
    if quoted('\'') {
        return Json::Str(inner().replace("''", "'"));
    }
    match text {
        "null" | "~" => return Json::Null,
        "true" => return Json::Bool(true),
        "false" => return Json::Bool(false),
        _ => {}
    }
    if integer().is_match(text)
        && let Some(value) = pyjson::int_of_text(text)
    {
        return Json::Int(value);
    }
    Json::Str(text.to_owned())
}

/// `_lista_inline`: `[a, "b, c", 'd']`, split at the commas outside quotes; `None` when the text is not one.
fn inline_list(text: &str) -> Option<Json> {
    let text = without_comment(pystr::strip(text));
    if !(text.starts_with('[') && text.ends_with(']')) || text.chars().count() < 2 {
        return None;
    }
    let inner = &text[1..text.len() - 1];
    let mut items: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        if let Some(open) = quote {
            current.push(c);
            if c == open {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            current.push(c);
        } else if c == ',' {
            items.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    items.push(current);
    Some(Json::List(items.iter().filter(|item| !pystr::strip(item).is_empty()).map(|item| scalar(item)).collect()))
}

/// `_valore`: an inline list, else a scalar.
fn value(text: &str) -> Json {
    inline_list(text).unwrap_or_else(|| scalar(text))
}

/// `_blocco`: a block list, `- scalar` or `- key: value` with the keys that follow it indented.
fn block(lines: &[&str]) -> Json {
    let mut items: Vec<Json> = Vec::new();
    for line in lines {
        let trimmed = pystr::strip(line);
        let item = line.trim_start_matches(pystr::is_space);
        if trimmed.is_empty() || item.starts_with('#') {
            continue;
        }
        if item.starts_with("- ") || item == "-" {
            let rest = pystr::strip(item.get(2..).unwrap_or(""));
            match key_of(rest) {
                Some((key, after)) => {
                    let mut map = Object::new();
                    map.insert(key, value(after.unwrap_or("")));
                    items.push(Json::Object(map));
                }
                None => items.push(value(rest)),
            }
        } else if let Some(Json::Object(map)) = items.last_mut()
            && let Some((key, after)) = key_of(item)
        {
            map.insert(key, value(after.unwrap_or("")));
        }
    }
    Json::List(items)
}

/// `front_matter`: the fields of the front matter the moviola reads, in the order they first come; an empty map
/// when the text opens with no `---` or never closes it.
pub fn front_matter(text: &str) -> Object {
    let mut out = Object::new();
    let lines = pystr::split_lines(text.trim_start_matches('\u{feff}'));
    if lines.first().is_none_or(|first| pystr::strip(first) != "---") {
        return out;
    }
    let Some(end) = (1..lines.len()).find(|at| pystr::strip(lines[*at]) == "---") else { return out };
    let body = &lines[1..end];
    let fields = format().report.fields();
    let mut at = 0;
    while at < body.len() {
        let Some((name, rest)) = key_of(body[at]) else {
            at += 1;
            continue;
        };
        let rest = pystr::strip(rest.unwrap_or(""));
        let mut next = at + 1;
        let found = if rest.is_empty() {
            while next < body.len() && !(key_of(body[next]).is_some() && !body[next].starts_with([' ', '-'])) {
                next += 1;
            }
            block(&body[at + 1..next])
        } else {
            value(rest)
        };
        if fields.contains(&name) {
            out.insert(name, found);
        }
        at = if rest.is_empty() { next } else { at + 1 };
    }
    out
}

/// `_intero`: Python's `int()`, `None` where it raises.
fn whole(value: &Json) -> Option<i128> {
    match pyjson::int_of(value) {
        pyjson::IntOf::Value(value) => Some(value),
        pyjson::IntOf::Not | pyjson::IntOf::Overflow => None,
    }
}

/// The reports of the dossier in `folder`, by the order of their files' names, and so by date (`leggi_report`).
/// A folder that is not there has none; a file that does not read, or names no task, is skipped.
pub fn read_reports(folder: &Path) -> Result<Vec<Report>, MoviolaError> {
    let format = format();
    let mut dir = folder.to_path_buf();
    let mut walked: Vec<&str> = Vec::new();
    for segment in &format.report.folder {
        dir.push(segment);
        walked.push(segment);
        match fs::symlink_metadata(&dir) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(MoviolaError::Read { file: walked.join("/"), source }),
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(MoviolaError::Link(walked.join("/"))),
            Ok(metadata) if !metadata.is_dir() => return Ok(Vec::new()),
            Ok(_) => {}
        }
    }
    let shown = walked.join("/");
    let entries = fs::read_dir(&dir).map_err(|source| MoviolaError::Read { file: shown.clone(), source })?;
    let mut names: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| MoviolaError::Read { file: shown.clone(), source })?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if has_extension(&name, &format.report.extension) {
            names.push((name, entry.path()));
        }
    }
    names.sort_by_cached_key(|(name, _)| sort_key(name));
    let mut out = Vec::new();
    for (name, path) in names {
        let Some(text) = read_text(&path) else { continue };
        if let Some(report) = report_of(&name, &text)? {
            out.push(report);
        }
    }
    Ok(out)
}

/// `*.<extension>`, matched as the prototype's `glob` does: without regard to case on Windows.
fn has_extension(name: &str, extension: &str) -> bool {
    let suffix = format!(".{extension}");
    if cfg!(windows) { name.to_lowercase().ends_with(&suffix.to_lowercase()) } else { name.ends_with(&suffix) }
}

/// The order Python's `sorted` gives the paths of one folder: by name, without regard to case on Windows.
fn sort_key(name: &str) -> String {
    if cfg!(windows) { name.to_lowercase() } else { name.to_owned() }
}

/// A regular file's text, `None` for a link, for what is not a file, and for what does not read as UTF-8.
fn read_text(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    String::from_utf8(fs::read(path).ok()?).ok()
}

/// One report from its file's name and text, `None` when it names no task. An error where the prototype raises:
/// a checks' field that is a number other than zero, which Python cannot walk.
fn report_of(name: &str, text: &str) -> Result<Option<Report>, MoviolaError> {
    let format = &format().report;
    let fields = front_matter(text);
    let named = file_name().captures(name);
    let task = whole(fields.value(&format.task)).or_else(|| named.as_ref().and_then(|captures| pyjson::int_of_text(&captures[2])));
    let Some(task) = task else { return Ok(None) };
    let checks = match fields.value(&format.checks) {
        Json::List(items) => items.iter().filter_map(|item| item.as_object().cloned()).collect(),
        Json::Int(_) | Json::Bool(_) | Json::Float(_) if fields.value(&format.checks).truthy() => {
            return Err(MoviolaError::Invalid { place: name.to_owned(), problem: format!("`{}` is a number", format.checks) });
        }
        // A string walks as its characters, none of them a map; what is false walks as nothing.
        _ => Vec::new(),
    };
    let closed_defects = match fields.value(&format.closed_defects) {
        Json::List(items) => items.iter().map(Json::py_str).collect(),
        _ => Vec::new(),
    };
    Ok(Some(Report {
        file: name.to_owned(),
        date: named.as_ref().map(|captures| captures[1].to_owned()),
        task,
        kind: fields.value(&format.kind).clone(),
        stop_reason: fields.value(&format.stop_reason).clone(),
        round: whole(fields.value(&format.round)),
        session: fields.value(&format.session).clone(),
        subagents: whole(fields.value(&format.subagents)),
        checks,
        has_checks: fields.contains_key(&format.checks),
        has_process: format.process_fields().iter().any(|field| fields.contains_key(field)),
        closed_defects,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_ends_at_a_hash_after_a_blank_and_outside_quotes() {
        assert_eq!(without_comment("2   # a note"), "2");
        assert_eq!(without_comment("#a"), "");
        assert_eq!(without_comment("a#b"), "a#b");
        assert_eq!(without_comment("\"a # b\" # c"), "\"a # b\"");
        assert_eq!(without_comment("'a # b'"), "'a # b'");
        assert_eq!(without_comment("\"a\\\" # b"), "\"a\\\"", "a backslash escapes nothing");
    }

    #[test]
    fn scalars_read_as_the_prototype_s() {
        assert!(scalar("  ").is_null());
        assert!(scalar("~").is_null());
        assert!(matches!(scalar("true"), Json::Bool(true)));
        assert!(matches!(scalar("-12"), Json::Int(-12)));
        assert!(matches!(scalar("\u{663}"), Json::Int(3)), "an Arabic-Indic digit is a digit");
        assert_eq!(scalar("1.5").as_str(), Some("1.5"));
        assert_eq!(scalar("\"a\\u00e8\"").as_str(), Some("a\u{e8}"));
        assert_eq!(scalar("\"a\" \"b\"").as_str(), Some("a\" \"b"), "quoted, but not one JSON string");
        assert_eq!(scalar("'it''s'").as_str(), Some("it's"));
        assert_eq!(scalar("\"\"").as_str(), Some(""));
        assert_eq!(scalar("\"").as_str(), Some("\""));
    }

    #[test]
    fn inline_lists_split_outside_quotes() {
        let list = inline_list("[D1, \"D2, D3\", 'x', , 4]").unwrap();
        assert_eq!(pyjson::dumps(&list), r#"["D1","D2, D3","x",4]"#);
        assert!(inline_list("[").is_none());
        assert!(inline_list("D1").is_none());
        assert_eq!(pyjson::dumps(&inline_list("[]").unwrap()), "[]");
    }

    #[test]
    fn the_front_matter_keeps_only_the_moviola_s_fields() {
        let format = &format().report;
        let text = format!("---\nother: 1\n{}: 3\n{}:\n  - \"#1/D1\"\n---\nbody\n", format.round, format.closed_defects);
        let fields = front_matter(&text);
        assert_eq!(fields.len(), 2);
        assert!(matches!(fields.value(&format.round), Json::Int(3)));
        assert_eq!(pyjson::dumps(fields.value(&format.closed_defects)), r##"["#1/D1"]"##);
        assert!(front_matter("---\nround: 1\n").is_empty(), "never closed");
        assert!(front_matter("round: 1\n---\n").is_empty(), "never opened");
    }
}
