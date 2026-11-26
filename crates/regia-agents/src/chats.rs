//! Orca, the app with its own sessions, as the resume of the sessions a usage limit stopped asks it (the dossier's
//! #132 and #191): which sessions it shows as chats, which workers a step of it started, and how its owner wakes one by
//! hand. A session it shows is never resumed from outside: two writers on one transcript break its view.
//!
//! The rest of the product names none of this: it asks a [`ChatHost`] and reads the commands as data.

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use regia_exec::{Limits, Pin, Program, Search, Text, neutral_folder, orca};
use serde_json::Value;

use crate::orca::{first_message_starts_with, holds, session_files, session_of};
use crate::suspension::{CONTINUE, modified};
use crate::{AgentService, Env, ServiceId, children, claude_slug};

/// The entrypoint the app's chats write in their transcript: such a session is one of its chats, whatever its record says.
pub const CHAT_ENTRYPOINT: &str = "sdk-ts";

/// The text of a limit the app's index is searched for.
const QUERY: &str = "hit your session limit";

/// The pages of a search, at most: a hundred hits each.
const PAGES: usize = 10;

/// How long a page of the search may take.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);

/// How the preamble of a worker names its coordinator, the step's session in the app.
const COORDINATOR: &str = "orca_session_id:";

/// A session id of the app's chats; a terminal's handle or a structured session's id is none.
static CHAT_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:claude|codex)_[A-Za-z0-9_]+$").expect("the pattern of a chat's id"));

/// The task and the dispatch a worker's preamble reports to.
static DISPATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"--dispatch-id (ctx_[0-9a-f]+)").expect("the pattern of a dispatch"));
static TASK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"--task-id (task_[0-9a-f]+)").expect("the pattern of a task"));

/// The sessions of a folder the app indexed: the agent's session, and the app's id of its chat, none when it is no chat.
pub type Known = BTreeMap<String, Option<String>>;

/// The app with its own sessions, asked which sessions it shows.
pub trait ChatHost: Send + Sync {
    /// The sessions under `folder` the app indexed with a usage limit's text, and which of them it shows as chats. None
    /// when it does not answer: the caller reads it as "maybe a chat", and resumes nothing it does not own (fail-closed).
    fn known(&self, folder: &str) -> Option<Known>;
}

/// No app to ask: it never answers, so nothing the regia did not start itself is resumed. A shadow run's, and a fake one's.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoChatHost;

impl ChatHost for NoChatHost {
    fn known(&self, _folder: &str) -> Option<Known> {
        None
    }
}

/// Orca's index of its sessions, `orca search`, on `orca` as setup finds it, or pinned, or looked for in a search of
/// its own.
#[derive(Debug, Clone, Default)]
pub struct OrcaChats {
    pin: Option<Pin>,
    search: Option<Search>,
}

impl OrcaChats {
    #[must_use]
    pub fn new() -> OrcaChats {
        OrcaChats::default()
    }

    #[must_use]
    pub fn with_pin(mut self, pin: Pin) -> OrcaChats {
        self.pin = Some(pin);
        self
    }

    #[must_use]
    pub fn with_search(mut self, search: Search) -> OrcaChats {
        self.search = Some(search);
        self
    }

    fn pin(&self) -> Option<Pin> {
        match (&self.pin, &self.search) {
            (Some(pin), _) => Some(pin.clone()),
            (None, Some(search)) => Pin::search(Program::Orca, search).ok(),
            (None, None) => Pin::resolve(Program::Orca).ok(),
        }
    }
}

/// The JSON truth of a field, as Python reads it.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
    }
}

impl ChatHost for OrcaChats {
    /// The prototype's `_cerca_orca`: page after page, Claude's hits, each with its chat when Orca has one.
    fn known(&self, folder: &str) -> Option<Known> {
        let pin = self.pin()?;
        let (query, path) = (Text::parse(QUERY).ok()?, Text::parse(folder).ok()?);
        let mut known = Known::new();
        let mut cursor: Option<Text> = None;
        for _ in 0..PAGES {
            let args = orca::search(&query, &path, cursor.as_ref());
            let output = pin.launch(args, &neutral_folder()).limits(Limits { timeout: SEARCH_TIMEOUT, ..Limits::default() }).run().ok()?;
            let answer: Value = serde_json::from_slice(&output.stdout).ok()?;
            if !answer.is_object() || !truthy(answer.get("ok")) {
                return None;
            }
            let result = answer.get("result").filter(|result| result.is_object());
            for hit in result.and_then(|result| result.get("hits")).and_then(Value::as_array).into_iter().flatten() {
                let Some(hit) = hit.as_object() else { continue };
                let session = hit.get("sessionId").and_then(Value::as_str).filter(|session| !session.is_empty());
                if hit.get("agent").and_then(Value::as_str) == Some("claude")
                    && let Some(session) = session
                {
                    let chat = hit.get("structuredSession").and_then(Value::as_object).and_then(|chat| chat.get("sessionId")).and_then(Value::as_str);
                    known.insert(session.to_owned(), chat.map(str::to_owned));
                }
            }
            let page = result.and_then(|result| result.get("page"));
            cursor = if truthy(page.and_then(|page| page.get("hasMore"))) {
                page.and_then(|page| page.get("cursor")).and_then(Value::as_str).and_then(|cursor| Text::parse(cursor).ok())
            } else {
                None
            };
            if cursor.is_none() {
                return Some(known);
            }
        }
        Some(known)
    }
}

/// The app's index of this machine, on `orca` as setup finds it.
#[must_use]
pub fn chat_host() -> Box<dyn ChatHost> {
    Box::new(OrcaChats::new())
}

/// The prototype's `scopri_worker`: the Claude sessions of `folder` that a worker of the app's session `coordinator`
/// wrote, by its preamble, changed since `after` (5 s of slack). The sessions `skip` names are the step's own.
#[must_use]
pub fn coordinated_workers(env: &Env, folder: &str, coordinator: &str, skip: &[&str], after: Option<f64>) -> Vec<(String, PathBuf)> {
    let Some(home) = AgentService::known(ServiceId::Claude).home(env) else { return Vec::new() };
    let key = format!("{COORDINATOR}{coordinator}");
    let dir = home.join("projects").join(claude_slug(Path::new(folder)));
    let mut found = Vec::new();
    for path in children(&dir).into_iter().filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "jsonl")) {
        let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        if skip.contains(&stem.as_str()) {
            continue;
        }
        if let Some(after) = after
            && modified(&path).is_some_and(|changed| changed < after - 5.0)
        {
            continue;
        }
        if holds(&path, &key) {
            found.push((stem, path));
        }
    }
    found
}

/// The task and the dispatch a worker reports to, from the first lines of its transcript, its preamble.
#[must_use]
pub fn worker_ids(transcript: &Path) -> Option<(String, String)> {
    let file = std::fs::File::open(transcript).ok()?;
    let mut reader = BufReader::new(file);
    let mut head = String::new();
    let mut line = Vec::new();
    for _ in 0..5 {
        line.clear();
        if reader.read_until(b'\n', &mut line).ok()? == 0 {
            break;
        }
        head.push_str(&String::from_utf8_lossy(&line));
    }
    let dispatch = DISPATCH.captures(&head)?.get(1)?.as_str().to_owned();
    let task = TASK.captures(&head)?.get(1)?.as_str().to_owned();
    Some((task, dispatch))
}

/// The command that wakes, by hand, a worker of the app with its dispatch still open: a status message to it. None
/// without the ids. A worker already done has no open dispatch: the message fails, and it stays to resume by hand.
#[must_use]
pub fn worker_wake_command(transcript: &Path) -> Option<String> {
    let (task, dispatch) = worker_ids(transcript)?;
    Some(format!(
        "orca orchestration send --to dispatch:{dispatch} --type status --subject \"{CONTINUE}\" --body \"{CONTINUE}\" --task-id {task} --dispatch-id {dispatch}"
    ))
}

/// The command that wakes, by hand, a chat of the app a step was launched in: a status message to its session. None
/// when the id is not a chat's.
#[must_use]
pub fn chat_wake_command(chat: &str) -> Option<String> {
    CHAT_ID
        .is_match(chat)
        .then(|| format!("orca orchestration send --to {COORDINATOR}{chat} --type status --subject \"{CONTINUE}\" --body \"{CONTINUE}\""))
}

/// The prototype's `ritrova_sessione`: the session file of `folder` changed since `after` (5 s of slack) whose agent got
/// `marker` first, the oldest first. A step launched on the app whose session was not found at its launch. Claude's
/// first message must start with it, as at the launch (#509: the app's naming session only quotes it); codex's file
/// holds it.
#[must_use]
pub fn session_by_marker(service: ServiceId, env: &Env, folder: &Path, after: f64, marker: &str) -> Option<(String, PathBuf)> {
    if marker.is_empty() {
        return None;
    }
    let agent = AgentService::known(service);
    let mut candidates: Vec<(f64, PathBuf)> =
        session_files(&agent, env, folder).into_iter().filter_map(|path| modified(&path).filter(|changed| *changed >= after - 5.0).map(|changed| (changed, path))).collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    candidates.into_iter().find_map(|(_, path)| {
        let found = match service {
            ServiceId::Claude => first_message_starts_with(&path, marker),
            ServiceId::Codex => holds(&path, marker),
        };
        if found { session_of(&agent, &path).map(|session| (session, path)) } else { None }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_s_id_is_an_agent_s_word_then_letters() {
        assert!(chat_wake_command("claude_o_1").is_some_and(|command| command.contains("--to orca_session_id:claude_o_1")));
        for other in ["structured-agent-session-1", "term_1", "claude_", "claude_x y", "gemini_x"] {
            assert_eq!(chat_wake_command(other), None, "{other}");
        }
        assert!(!truthy(Some(&Value::from(0))) && truthy(Some(&Value::from("x"))));
    }
}
