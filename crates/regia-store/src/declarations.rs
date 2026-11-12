//! The log of what sessions declare they work on (R8 of the dossier's conformance study, ADR 0013 §5 of the
//! dossier, `tools/dichiarazioni.py`): a line per `declare`, and per command that touches a task or a
//! project. The moviola splits a session into stretches on it, since the folder a chat starts from does not
//! say what it works on. Never a comment's body, a title or a spec: the command, the project, the task, the
//! instant, the session's ids and the role.
//!
//! A file per session and per process, only appended to, one write per line: on Windows an append is not
//! atomic, so two processes of one session writing at once into one file would overwrite each other, and
//! with a file each none writes where another does. The threads of a process share its file, under one
//! lock. The names are the prototype's, data of `formats/store_v1.toml`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::StoreError;
use crate::folder::{check, default_folder};
use crate::time::WallTime;
use crate::v1::format;

/// The ids of the session a command runs in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionIds {
    /// Each id that is set, with the key the log writes it with, in the format's order.
    ids: Vec<(String, String)>,
}

impl SessionIds {
    /// The ids of this process's environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// The ids `lookup` gives by the name of their variable, each stripped as Python strips it; an empty
    /// one is none.
    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let ids = format()
            .declarations
            .ids
            .iter()
            .filter_map(|(variable, key)| {
                let id = lookup(variable)?;
                let id = id.trim_matches(is_python_space);
                (!id.is_empty()).then(|| (key.clone(), id.to_owned()))
            })
            .collect();
        Self { ids }
    }

    /// The session a line is attributed to: the first id, in the format's order.
    #[must_use]
    pub fn session(&self) -> Option<&str> {
        self.ids.first().map(|(_, id)| id.as_str())
    }
}

/// One line of the log, before its instant and the session's ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declaration<'a> {
    /// The command as the prototype names it, its words in its language: data, which the moviola
    /// attributes on its first word.
    pub command: &'a str,
    pub project: Option<&'a str>,
    pub task: Option<i64>,
    pub role: Option<&'a str>,
}

/// What [`write_declaration`] appended, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub session: String,
    /// The instant of the line, to the second.
    pub ts: String,
    pub file: PathBuf,
    pub line: String,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DeclarationError {
    #[error("no session id in the environment ({})", id_variables().join(", "))]
    NoSession,
    #[error(transparent)]
    Folder(#[from] StoreError),
    #[error("cannot write {}", file.display())]
    Write {
        file: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The variables a session's ids come from, in order.
fn id_variables() -> Vec<&'static str> {
    format().declarations.ids.iter().map(|(variable, _)| variable.as_str()).collect()
}

/// The role a step's commands record.
#[must_use]
pub fn step_role() -> &'static str {
    &format().declarations.step_role
}

/// The log's folder, under the state folder, which is refused inside the temporary folder. Neither
/// created nor opened: the state database is not touched.
///
/// # Errors
///
/// As [`default_folder`], and [`StoreError::TemporaryFolder`].
pub fn declarations_folder() -> Result<PathBuf, StoreError> {
    let root = default_folder()?;
    check(&root)?;
    Ok(root.join(&format().declarations.folder))
}

/// One writer at a time among the threads of this process, which share its file.
static WRITING: Mutex<()> = Mutex::new(());

/// Appends `declaration` at `now` to the file of this session and this process in `folder`, created if
/// missing.
///
/// # Errors
///
/// [`DeclarationError::NoSession`] without an id to attribute it to, or when it cannot be written.
pub fn write_declaration(folder: &Path, ids: &SessionIds, declaration: &Declaration<'_>, now: WallTime) -> Result<Declared, DeclarationError> {
    let session = ids.session().ok_or(DeclarationError::NoSession)?.to_owned();
    let ts = now.to_iso_seconds();
    let line = line(ids, declaration, &ts);
    fs::create_dir_all(folder).map_err(|source| DeclarationError::Write { file: folder.to_path_buf(), source })?;
    let file = folder.join(file_name(&session, std::process::id()));
    let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
    let write = |source| DeclarationError::Write { file: file.clone(), source };
    OpenOptions::new().append(true).create(true).open(&file).and_then(|mut opened| opened.write_all(line.as_bytes())).map_err(write)?;
    Ok(Declared { session, ts, file, line })
}

/// The line as the prototype writes it: `json.dumps` with no blanks and the text as it is, the keys in
/// its order, the session and its agent first among the ids, a field without a value left out.
fn line(ids: &SessionIds, declaration: &Declaration<'_>, ts: &str) -> String {
    let keys = &format().declarations;
    let text = |value: &str| serde_json::to_string(value).expect("a string serializes");
    let mut fields = vec![(keys.version.as_str(), "1".to_owned()), (keys.ts.as_str(), text(ts))];
    if let Some((agent, session)) = ids.ids.first() {
        fields.extend([(keys.session.as_str(), text(session)), (keys.agent.as_str(), text(agent))]);
    }
    fields.extend(ids.ids.iter().map(|(key, id)| (key.as_str(), text(id))));
    if let Some(project) = declaration.project {
        fields.push((keys.project.as_str(), text(project)));
    }
    if let Some(task) = declaration.task {
        fields.push((keys.task.as_str(), task.to_string()));
    }
    fields.push((keys.command.as_str(), text(declaration.command)));
    if let Some(role) = declaration.role {
        fields.push((keys.role.as_str(), text(role)));
    }
    let fields: Vec<String> = fields.iter().map(|(key, value)| format!("{}:{value}", text(key))).collect();
    format!("{{{}}}\n", fields.join(","))
}

/// `<session>.<pid>.jsonl`: the session with each character outside `[A-Za-z0-9._-]` as `_`, cut to 120.
fn file_name(session: &str, pid: u32) -> String {
    let safe: String = session.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).take(120).collect();
    format!("{safe}.{pid}.jsonl")
}

/// `str.isspace()`: Unicode's whitespace and the four separators U+001C to U+001F.
fn is_python_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> &'static crate::v1::DeclarationKeys {
        &format().declarations
    }

    /// The ids of the format's variables, in the order given.
    fn ids(values: &[(usize, &str)]) -> SessionIds {
        let variables = id_variables();
        SessionIds::from_lookup(|name| values.iter().find(|(i, _)| variables[*i] == name).map(|(_, value)| (*value).to_owned()))
    }

    /// The first id set is the session, whatever the order of the environment; blanks around an id go, and
    /// an empty one is none.
    #[test]
    fn the_session_is_the_first_id_in_the_format_s_order() {
        assert_eq!(ids(&[(2, "x"), (1, "o")]).session(), Some("o"));
        assert_eq!(ids(&[(2, "x")]).session(), Some("x"));
        assert_eq!(ids(&[(0, " \t "), (1, " o\u{1f}")]).session(), Some("o"));
        assert_eq!(ids(&[]).session(), None);
    }

    /// The prototype's line for a declaration of a project and a task in a Claude session that has an Orca
    /// id too, key by key, and with fields left out.
    #[test]
    fn the_line_has_the_prototype_s_keys_in_its_order() {
        let k = keys();
        let both = ids(&[(1, "orca-1"), (0, "claude-1")]);
        let declaration = Declaration { command: "c d", project: Some("alpha"), task: Some(146), role: Some("r\u{e9}\"") };
        let expected = format!(
            "{{\"{}\":1,\"{}\":\"T\",\"{}\":\"claude-1\",\"{}\":\"{}\",\"{}\":\"claude-1\",\"{}\":\"orca-1\",\"{}\":\"alpha\",\"{}\":146,\"{}\":\"c d\",\"{}\":\"r\u{e9}\\\"\"}}\n",
            k.version, k.ts, k.session, k.agent, k.ids[0].1, k.ids[0].1, k.ids[1].1, k.project, k.task, k.command, k.role
        );
        assert_eq!(line(&both, &declaration, "T"), expected, "compact, the text unescaped but for JSON's own");
        let bare = Declaration { command: "c", project: None, task: None, role: None };
        let line = line(&ids(&[(2, "x")]), &bare, "T");
        assert!(!line.contains(&format!("\"{}\"", k.project)) && !line.contains(&format!("\"{}\"", k.role)), "{line}");
    }

    /// The file's name is safe for any session's id, and per process.
    #[test]
    fn the_file_is_the_session_s_safe_name_and_the_process_s() {
        assert_eq!(file_name("a", 7), "a.7.jsonl");
        assert_eq!(file_name("../b:x", 7), ".._b_x.7.jsonl");
        assert_eq!(file_name("ü日", 7), "__.7.jsonl");
        assert_eq!(file_name(&"x".repeat(200), 1), format!("{}.1.jsonl", "x".repeat(120)));
    }

    #[test]
    fn without_a_session_the_error_names_the_variables() {
        let message = DeclarationError::NoSession.to_string();
        assert!(id_variables().iter().all(|variable| message.contains(variable)), "{message}");
    }
}
