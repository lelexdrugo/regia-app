//! The configuration of the scheduled operations (F148, F256): the prototype's `normalizza_operazioni`,
//! `parse_giorni` and `carica_operazioni`, read and checked at every tick. An entry that does not hold is left
//! out and its reason kept; the others stay.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use super::words::format as words;

/// What an operation does when it is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A chat waiting for its owner, on a surface the owner sees.
    Chat,
    /// The chronicler's sessions (#59 of the dossier), checked here; they run with #493.
    Chronicler,
    /// An idea search (#273 of the dossier), checked here with its levers and never run: whether the product runs
    /// it is to be decided (F315-F317 of the dossier's inventory).
    IdeaSearch,
}

/// A scheduled operation of the configuration, once checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    /// Lowercase letters, digits and dashes: the key of its entry in the cursor.
    pub name: String,
    /// Its hour and minute, local time.
    pub hour: (u32, u32),
    /// The days it runs on, 0 for Monday.
    pub days: BTreeSet<u32>,
    pub action: Action,
    /// What the chat gets: required for a chat.
    pub prompt: Option<String>,
    /// The chronicler's specification, relative to the regia's home.
    pub spec: Option<String>,
    /// Where the chat opens; without it, the regia's home.
    pub folder: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// False only when the configuration says false.
    pub active: bool,
    /// An idea search's projects, when the configuration names them.
    pub projects: Option<Vec<String>>,
    /// An idea search's ideas a session, when the configuration says how many.
    pub ideas: Option<i64>,
}

/// The days of `text`: `lun-ven`, `lun,mer,ven`, a span that passes Sunday (`ven-lun`), or every day, in the
/// format's words, any case. 0 is Monday.
///
/// # Errors
///
/// A piece that is neither a day nor a span of two days.
pub fn parse_days(text: &str) -> Result<BTreeSet<u32>, String> {
    let days = &words().days;
    let text = text.trim().to_lowercase();
    if days.every.contains(&text) {
        return Ok((0..7).collect());
    }
    let index = |name: &str| days.names.iter().position(|day| day == name).and_then(|at| u32::try_from(at).ok());
    let mut found = BTreeSet::new();
    for piece in text.split(',') {
        let piece = piece.trim();
        let (first, last) = match piece.split_once('-') {
            Some((first, last)) => (index(first), index(last)),
            None => (index(piece), index(piece)),
        };
        let (Some(first), Some(last)) = (first, last) else {
            return Err(format!("days not valid: {text:?}"));
        };
        let span = (last + 7 - first) % 7;
        found.extend((0..=span).map(|step| (first + step) % 7));
    }
    Ok(found)
}

/// The prototype's check of a name: a lowercase letter or a digit, then lowercase letters, digits and dashes.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `HH:MM`, one or two digits for the hour: up to 23:59.
fn parse_hour(text: &str) -> Option<(u32, u32)> {
    let (hour, minute) = text.split_once(':')?;
    let digits = |part: &str, sizes: &[usize]| sizes.contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(hour, &[1, 2]) || !digits(minute, &[2]) {
        return None;
    }
    let (hour, minute) = (hour.parse().ok()?, minute.parse().ok()?);
    (hour <= 23 && minute <= 59).then_some((hour, minute))
}

/// Python's `str()` of a value the configuration gives where it wants text.
fn shown(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// A text the configuration may give; any other value counts as none.
fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

/// One entry of the configuration, checked.
fn operation(entry: &Value, seen: &BTreeSet<String>) -> Result<Operation, String> {
    let k = &words().config_keys;
    let field = |key: &str| entry.as_object().and_then(|entry| entry.get(key));
    let name = field(&k.name).and_then(Value::as_str).filter(|name| valid_name(name)).ok_or("the name is missing or not valid (lowercase letters, digits, dashes)")?;
    if seen.contains(name) {
        return Err("the name repeats".to_owned());
    }
    let hour = match field(&k.hour) {
        None => parse_hour(""),
        Some(Value::String(text)) => parse_hour(text),
        Some(_) => None,
    };
    let hour = hour.ok_or_else(|| format!("the hour is not valid: {} (HH:MM, local time)", shown(field(&k.hour))))?;
    let actions = &words().actions;
    let action = match field(&k.action).and_then(Value::as_str) {
        Some(action) if action == actions.chat => Action::Chat,
        Some(action) if action == actions.chronicler => Action::Chronicler,
        Some(action) if action == actions.idea_search => Action::IdeaSearch,
        _ => {
            let known = [&actions.chat, &actions.chronicler, &actions.idea_search].map(String::as_str).join(", ");
            return Err(format!("the action is unknown: {} (one of {known})", shown(field(&k.action))));
        }
    };
    let prompt = text(field(&k.prompt));
    if action == Action::Chat && prompt.as_deref().is_none_or(|prompt| prompt.trim().is_empty()) {
        return Err("the prompt is missing".to_owned());
    }
    let spec = match field(&k.spec) {
        None | Some(Value::Null) => None,
        Some(Value::String(spec)) => Some(spec.clone()),
        Some(_) => return Err("the spec must be a path, relative to the regia's home".to_owned()),
    };
    let (projects, ideas) = if action == Action::IdeaSearch { levers(field(&k.projects), field(&k.ideas))? } else { (None, None) };
    let days = match field(&k.days) {
        None => parse_days(&words().days.default)?,
        Some(Value::String(days)) => parse_days(days)?,
        Some(other) => return Err(format!("days not valid: {other}")),
    };
    Ok(Operation {
        name: name.to_owned(),
        hour,
        days,
        action,
        prompt,
        spec,
        folder: text(field(&k.folder)),
        model: text(field(&k.model)),
        effort: text(field(&k.effort)),
        active: field(&k.active) != Some(&Value::Bool(false)),
        projects,
        ideas,
    })
}

/// The prototype's check of an idea search's levers: a list of ids without repeats, and a whole number of ideas in
/// the format's range. A lever the configuration does not give takes the prototype's default, which holds.
fn levers(projects: Option<&Value>, ideas: Option<&Value>) -> Result<(Option<Vec<String>>, Option<i64>), String> {
    let k = &words().config_keys;
    let projects = match projects {
        None => None,
        Some(value) => {
            let ids: Option<Vec<String>> = value.as_array().filter(|ids| !ids.is_empty()).and_then(|ids| {
                ids.iter().map(|id| id.as_str().filter(|id| valid_name(id)).map(str::to_owned)).collect()
            });
            let ids = ids.ok_or_else(|| format!("the projects must be a list of ids, not empty (lowercase letters, digits, dashes): {value}"))?;
            if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
                return Err(format!("the projects repeat an id: {value}"));
            }
            Some(ids)
        }
    };
    let ideas = match ideas {
        None => None,
        Some(value) => {
            let count = value.as_i64().filter(|count| (k.ideas_min..=k.ideas_max).contains(count) && !value.is_f64());
            Some(count.ok_or_else(|| format!("the ideas must be a whole number from {} to {}: {value}", k.ideas_min, k.ideas_max))?)
        }
    };
    Ok((projects, ideas))
}

/// The operations of a configuration's document, and why each entry left out was: the others stay.
#[must_use]
pub fn normalize(document: &Value) -> (Vec<Operation>, Vec<String>) {
    let key = &words().config_keys.operations;
    let Some(entries) = document.as_object().and_then(|document| document.get(key)).and_then(Value::as_array) else {
        return (Vec::new(), vec![format!("config: the list `{key}` is missing")]);
    };
    let mut operations = Vec::new();
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    for (at, entry) in entries.iter().enumerate() {
        match operation(entry, &seen) {
            Ok(operation) => {
                seen.insert(operation.name.clone());
                operations.push(operation);
            }
            Err(error) => {
                let name = entry.as_object().and_then(|entry| entry.get(&words().config_keys.name));
                let label = match name {
                    Some(Value::String(name)) if !name.is_empty() => name.clone(),
                    None | Some(Value::Null | Value::Bool(false)) | Some(Value::String(_)) => at.to_string(),
                    Some(Value::Number(number)) if number.as_f64() == Some(0.0) => at.to_string(),
                    Some(Value::Array(items)) if items.is_empty() => at.to_string(),
                    Some(Value::Object(fields)) if fields.is_empty() => at.to_string(),
                    Some(other) => other.to_string(),
                };
                errors.push(format!("operation {label}: {error}"));
            }
        }
    }
    (operations, errors)
}

/// The operations of the configuration file at `path`: none and no error for a file that is not there; none and
/// the error, cut as the prototype cuts it, for one that does not read.
#[must_use]
pub fn load(path: &Path) -> (Vec<Operation>, Vec<String>) {
    let cut = |error: String| error.chars().take(words().file_error_chars).collect::<String>();
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(error) => return (Vec::new(), vec![cut(format!("{}: {error}", path.display()))]),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(document) => normalize(&document),
        Err(error) => (Vec::new(), vec![cut(format!("{}: {error}", path.display()))]),
    }
}
