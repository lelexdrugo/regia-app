//! The Orca surface (ADR 0001 §6): optional, an adapter like any other. `orca agent launch` starts the
//! agent in a workspace of Orca, where the session shows in its app. Everything about Orca stays here
//! and in the builders of regia-exec.
//!
//! - **Before any effect**, in this order: the program, `orca status` (twice, a second apart, because
//!   its CLI sometimes misses the first), `orca worktree show` for the folder. Orca missing, not
//!   reachable, or a folder that is not one of its workspaces is `FailedBeforeEffect`: the caller may
//!   fall back to `Cli`, and nothing was written.
//! - **The prompt is an argument**, as Orca takes it: the one exception to the prompt on stdin
//!   (threat model §3.6, rule 3, with its limit). The surface keeps it short: a prompt past
//!   [`PROMPT_IN_ARGV_MAX`] characters, or one that is not a valid argument, goes in a file, and the
//!   argument is a sentence that points to it.
//! - **No stream:** the agent's own session is found by its marker, the start of what the agent got,
//!   among the session files that appeared since the launch. Once `orca agent launch` has started, a
//!   failure is after an effect: the session may exist, and it is never launched again.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use regia_exec::{AbsolutePath, Args, Effort, ExecError, Limits, Pin, Program, Search, Text, orca};
use regia_identity::state_dir;
use serde_json::{Value, json};

use crate::cli::{Files, steps_folder};
use crate::launch::{Dry, LaunchError, LaunchRequest, LaunchSurface, Receipt, SurfaceKind, launch_with_fallback};
use crate::{AgentService, Cli, Env, ServiceId, WakeOutcome, children};

/// Past this many characters the prompt goes in a file: the prototype's limit, well under what a
/// command line holds.
pub const PROMPT_IN_ARGV_MAX: usize = 6000;

/// The marker is the start of the argument, this many characters.
const MARKER_CHARS: usize = 60;

/// The lines of a session file the marker is looked for in.
const HEAD_LINES: usize = 40;

/// How long the surface waits for Orca, and how often it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrcaWaits {
    /// For `orca status` and for `orca worktree show`, each.
    pub check: Duration,
    /// Between the two tries of `orca status`.
    pub retry: Duration,
    /// For `orca agent launch`.
    pub launch: Duration,
    /// For the agent's session file: the chat starts after Orca's start hooks.
    pub session: Duration,
    pub tick: Duration,
}

impl Default for OrcaWaits {
    /// The prototype's: 20 s a check, 1 s between the two, 60 s for the launch, 120 s for the session
    /// file, which took 70 s on a busy machine.
    fn default() -> Self {
        OrcaWaits {
            check: Duration::from_secs(20),
            retry: Duration::from_secs(1),
            launch: Duration::from_secs(60),
            session: Duration::from_secs(120),
            tick: Duration::from_millis(500),
        }
    }
}

/// `orca agent launch` in a workspace of Orca.
#[derive(Debug, Clone)]
pub struct Orca {
    env: Env,
    state: Option<PathBuf>,
    pin: Option<Pin>,
    search: Option<Search>,
    waits: OrcaWaits,
}

impl Orca {
    /// The surface over `env`, where the agents' folders are read, with the state folder of this
    /// process and `orca` found as setup finds it.
    pub fn new(env: Env) -> Orca {
        Orca { env, state: state_dir(), pin: None, search: None, waits: OrcaWaits::default() }
    }

    #[must_use]
    pub fn with_state(mut self, folder: PathBuf) -> Orca {
        self.state = Some(folder);
        self
    }

    /// `orca` at a pinned path, as the setup confirmed it or as a test names a fake.
    #[must_use]
    pub fn with_pin(mut self, pin: Pin) -> Orca {
        self.pin = Some(pin);
        self
    }

    /// Where `orca` is looked for without a pin, in place of this process's search.
    #[must_use]
    pub fn with_search(mut self, search: Search) -> Orca {
        self.search = Some(search);
        self
    }

    #[must_use]
    pub fn with_waits(mut self, waits: OrcaWaits) -> Orca {
        self.waits = waits;
        self
    }

    fn pin(&self) -> Result<Pin, LaunchError> {
        let pinned = match (&self.pin, &self.search) {
            (Some(pin), _) => Ok(pin.clone()),
            (None, Some(search)) => Pin::search(Program::Orca, search),
            (None, None) => Pin::resolve(Program::Orca),
        };
        pinned.map_err(|error| LaunchError::before(format!("Orca is not available: {error}")))
    }

    /// `orca <args>`, in `folder`: the `result` of its answer, or what went wrong.
    fn call(&self, pin: &Pin, name: &'static str, args: Args, folder: &Path, timeout: Duration) -> Result<Value, Call> {
        let output = pin.launch(args, folder).limits(Limits { timeout, ..Limits::default() }).run().map_err(|error| Call::Run(name, error))?;
        let shown = || {
            let text = if output.stdout.is_empty() { &output.stderr } else { &output.stdout };
            String::from_utf8_lossy(text).chars().take(300).collect::<String>()
        };
        let code = output.status.code().map_or_else(|| output.status.to_string(), |code| code.to_string());
        let answer: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| Call::Answer(format!("orca {name}: an answer that is not JSON (exit {code}): {}", shown())))?;
        if answer.get("ok").and_then(Value::as_bool) != Some(true) {
            let error = answer.get("error");
            let field = |key: &str| error.and_then(|error| error.get(key)).and_then(Value::as_str).unwrap_or_default().to_owned();
            let code = Some(field("code")).filter(|code| !code.is_empty()).unwrap_or_else(|| "error".to_owned());
            return Err(Call::Answer(format!("orca {name}: {code}: {}", field("message"))));
        }
        Ok(answer.get("result").filter(|result| !result.is_null()).cloned().unwrap_or_else(|| json!({})))
    }

    /// Whether the runtime is reachable, or why not.
    fn reachable(&self, pin: &Pin, folder: &Path) -> Result<(), String> {
        let mut reason = String::new();
        for attempt in 0..2 {
            match self.call(pin, "status", orca::status(), folder, self.waits.check) {
                Ok(result) if result.pointer("/runtime/reachable") == Some(&Value::Bool(true)) => return Ok(()),
                Ok(_) => reason = "reachable is not true".to_owned(),
                Err(error) => reason = error.to_string(),
            }
            if attempt == 0 {
                thread::sleep(self.waits.retry);
            }
        }
        Err(reason)
    }

    /// The agent's own session among the session files that are new since `before`: for claude the
    /// one whose first message starts with the marker, because Orca first starts a short session that
    /// names the chat and only quotes the prompt; for codex one that holds the marker in its first
    /// lines. Waited for until the deadline.
    fn find_session(&self, service: &AgentService, folder: &Path, before: &BTreeSet<PathBuf>, marker: &str) -> Found {
        let deadline = Instant::now() + self.waits.session;
        let mut quoted = false;
        loop {
            let mut fresh: Vec<(SystemTime, PathBuf)> = session_files(service, &self.env, folder)
                .into_iter()
                .filter(|path| !before.contains(path))
                .map(|path| (fs::metadata(&path).and_then(|meta| meta.modified()).unwrap_or(SystemTime::UNIX_EPOCH), path))
                .collect();
            fresh.sort();
            for (_, path) in fresh {
                let starts = match service.id() {
                    ServiceId::Codex => holds(&path, marker),
                    ServiceId::Claude => {
                        let starts = first_message_starts_with(&path, marker);
                        quoted |= !starts && holds(&path, marker);
                        starts
                    }
                };
                if let Some(session) = starts.then(|| session_of(service, &path)).flatten() {
                    return Found::Session(session, path);
                }
            }
            if Instant::now() > deadline {
                return Found::Nothing { quoted };
            }
            thread::sleep(self.waits.tick);
        }
    }
}

/// What went wrong with a call of `orca`.
enum Call {
    Run(&'static str, ExecError),
    Answer(String),
}

impl Call {
    /// Whether `orca` ran: an answer, a deadline or a cap say it did.
    fn started(&self) -> bool {
        match self {
            Call::Run(_, error) => matches!(error, ExecError::TimedOut { .. } | ExecError::TooMuchOutput { .. }),
            Call::Answer(_) => true,
        }
    }
}

impl std::fmt::Display for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Call::Run(name, error) => write!(f, "orca {name}: {error}"),
            Call::Answer(text) => f.write_str(text),
        }
    }
}

enum Found {
    Session(String, PathBuf),
    /// Whether a new file quoted the marker without starting with it: Orca's naming session.
    Nothing { quoted: bool },
}

impl LaunchSurface for Orca {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Orca
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        let pin = self.pin()?;
        let folder = AbsolutePath::new(&request.folder).map_err(|error| LaunchError::before(error.to_string()))?;
        self.reachable(&pin, &request.folder)
            .map_err(|reason| LaunchError::before(format!("Orca is not reachable (`orca status`: {reason})")))?;
        self.call(&pin, "worktree show", orca::worktree_show(&folder), &request.folder, self.waits.check).map_err(|error| {
            LaunchError::before(format!(
                "{} is not a workspace of Orca, and the Orca surface launches only in its workspaces: {error}",
                request.folder.display()
            ))
        })?;

        let files = Files::reserve(&steps_folder(self.state.as_deref(), &request.label)?, &request.prompt)?;
        let text = argument(&request.prompt, &files.prompt)?;
        let marker = marker(text.as_str());
        let service = AgentService::known(request.service);
        let before = session_files(&service, &self.env, &request.folder);
        let mut warnings = Vec::new();
        let mut effort = request.effort;
        if let (Some(level), None) = (effort, &request.model) {
            // Orca refuses `--effort` without `--model` before it creates anything.
            warnings.push(format!("effort {} without a model: Orca refuses it, launched without", level.as_str()));
            effort = None;
        }
        let mut receipt = Receipt::new(SurfaceKind::Orca, request);
        let agent_launch = |effort: Option<Effort>| {
            let args = orca::agent_launch(service.program(), &folder, &text, request.model.as_ref(), effort)
                .map_err(|error| LaunchError::before(error.to_string()))?;
            self.call(&pin, "agent launch", args, &request.folder, self.waits.launch).map_err(|error| {
                if error.started() { LaunchError::AfterEffect(error.to_string()) } else { LaunchError::before(error.to_string()) }
            })
        };
        let result = match (agent_launch(effort), effort) {
            // Refused before anything is created: once more without it.
            (Err(LaunchError::AfterEffect(error)), Some(level)) if error.contains("does not support effort") => {
                warnings.push(format!("effort {} is not supported by Orca for this model: launched without", level.as_str()));
                effort = None;
                agent_launch(None)
            }
            (result, _) => result,
        }?;

        // From here on the session may exist: whatever happens is after an effect.
        let outcome = result.get("outcome");
        let orca_session = ["sessionId", "handle"]
            .iter()
            .find_map(|key| outcome.and_then(|outcome| outcome.get(*key)).and_then(Value::as_str).filter(|id| !id.is_empty()))
            .map(str::to_owned);
        let Some(orca_session) = orca_session else {
            let shown: String = result.to_string().chars().take(300).collect();
            return Err(LaunchError::AfterEffect(format!("orca agent launch returned no session: {shown}")));
        };
        let (session, transcript) = match self.find_session(&service, &request.folder, &before, &marker) {
            Found::Session(session, path) => (Some(session), Some(path.display().to_string())),
            Found::Nothing { quoted } => {
                warnings.push(if quoted {
                    "new session files quote the prompt without starting with it (Orca's naming session?), none is certain: `session` is Orca's id; look for it with `orca search`".to_owned()
                } else {
                    "no session file found: `session` is Orca's id; look for it with `orca search`".to_owned()
                });
                (None, None)
            }
        };
        let log = json!({"started": receipt.started, "launch": result, "warnings": warnings});
        if let Err(error) = fs::write(&files.log, serde_json::to_vec_pretty(&log).unwrap_or_default()) {
            warnings.push(format!("cannot write the log {}: {error}", files.log.display()));
        }
        receipt.effort = effort.map(|level| level.as_str());
        receipt.resume = session.as_deref().map(|session| service.resume(session));
        receipt.session = Some(session.unwrap_or_else(|| orca_session.clone()));
        receipt.transcript_exists = transcript.is_some();
        receipt.transcript = transcript;
        receipt.log = Some(files.log.display().to_string());
        receipt.prompt_file = Some(files.prompt.display().to_string());
        receipt.warnings = warnings;
        receipt.orca_session = Some(orca_session);
        receipt.workspace = result.get("worktreeId").and_then(Value::as_str).map(str::to_owned);
        receipt.visible = outcome.and_then(|outcome| outcome.get("kind")).and_then(Value::as_str).map(str::to_owned);
        receipt.marker = Some(marker);
        Ok(receipt)
    }

    /// Never through Orca: the wake-up is claude's own, as on `Cli`, over the same environment, state
    /// folder and search.
    fn wake(&self) -> WakeOutcome {
        let cli = Cli::new(self.env.clone());
        let cli = match &self.state {
            Some(state) => cli.with_state(state.clone()),
            None => cli,
        };
        match &self.search {
            Some(search) => cli.with_search(search.clone()).wake(),
            None => cli.wake(),
        }
    }
}

/// What Orca gets as the prompt: the prompt itself when it is short and a valid argument, otherwise
/// a sentence that points to the file holding it, byte for byte.
fn argument(prompt: &str, file: &Path) -> Result<Text, LaunchError> {
    if prompt.chars().count() <= PROMPT_IN_ARGV_MAX
        && let Ok(text) = Text::parse(prompt)
    {
        return Ok(text);
    }
    let pointer = format!("Read the file {} and follow the instructions it holds: they are the prompt of this session.", file.display());
    Text::parse(&pointer).map_err(|error| LaunchError::before(format!("the prompt's file cannot be named to Orca: {error}")))
}

/// The start of what the agent got, by which its session file is found.
fn marker(text: &str) -> String {
    text.chars().take(MARKER_CHARS).collect()
}

/// The session files of the agent's folder there are now: claude's for this working folder, codex's
/// of every day.
fn session_files(service: &AgentService, env: &Env, folder: &Path) -> BTreeSet<PathBuf> {
    let Some(home) = service.home(env) else { return BTreeSet::new() };
    let jsonl = |path: &PathBuf| path.extension().is_some_and(|extension| extension == "jsonl");
    match service.id() {
        ServiceId::Claude => children(&home.join("projects").join(crate::claude_slug(folder))).into_iter().filter(jsonl).collect(),
        ServiceId::Codex => children(&home.join("sessions"))
            .iter()
            .flat_map(|year| children(year))
            .flat_map(|month| children(&month))
            .flat_map(|day| children(&day))
            .filter(|path| jsonl(path) && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("rollout-")))
            .collect(),
    }
}

/// The session a file holds: claude's file is named by it, codex's ends with it.
fn session_of(service: &AgentService, path: &Path) -> Option<String> {
    let stem = path.file_name()?.to_str()?.strip_suffix(".jsonl")?;
    match service.id() {
        ServiceId::Claude => Some(stem.to_owned()),
        ServiceId::Codex => {
            let (head, id) = stem.split_at_checked(stem.len().checked_sub(36)?)?;
            let uuid = id.char_indices().all(|(at, c)| if [8, 13, 18, 23].contains(&at) { c == '-' } else { matches!(c, '0'..='9' | 'a'..='f') });
            // `rollout-<anything>-<uuid>`: the dash before the id is not the one of `rollout-`.
            (uuid && head.starts_with("rollout-") && head.len() > "rollout-".len() && head.ends_with('-')).then(|| id.to_owned())
        }
    }
}

/// The first lines of a file, as text.
fn head(path: &Path) -> Vec<String> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    let mut reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut line = Vec::new();
    while lines.len() < HEAD_LINES && reader.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
        lines.push(String::from_utf8_lossy(&line).into_owned());
        line.clear();
    }
    lines
}

/// The marker is in the first lines, as it is or as it reads inside a JSON string.
fn holds(path: &Path, marker: &str) -> bool {
    let quoted = serde_json::to_string(marker).unwrap_or_default();
    let escaped = quoted.get(1..quoted.len().saturating_sub(1)).unwrap_or_default();
    head(path).iter().any(|line| line.contains(marker) || (!escaped.is_empty() && line.contains(escaped)))
}

/// The first message of the user in a claude session file starts with the marker.
fn first_message_starts_with(path: &Path, marker: &str) -> bool {
    for line in head(path) {
        let Ok(Value::Object(event)) = serde_json::from_str::<Value>(&line) else { continue };
        if event.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let content = event.get("message").and_then(|message| message.get("content"));
        let text = match content {
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Array(parts)) => {
                Some(parts.iter().filter_map(|part| part.as_object()).filter_map(|part| part.get("text")?.as_str()).collect())
            }
            _ => None,
        };
        return text.is_some_and(|text| text.starts_with(marker));
    }
    false
}

/// Orca when it can take the launch, otherwise `Cli`, as the prototype's `auto`: the second runs only
/// when Orca failed before any effect, and its receipt says why.
#[derive(Debug, Clone)]
pub struct Auto {
    orca: Orca,
    cli: Cli,
}

impl Auto {
    pub fn new(orca: Orca, cli: Cli) -> Auto {
        Auto { orca, cli }
    }
}

impl LaunchSurface for Auto {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Auto
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        launch_with_fallback(&self.orca, &self.cli, request)
    }

    fn wake(&self) -> WakeOutcome {
        self.cli.wake()
    }
}

/// The surface `kind` names, over `env`, with this process's state folder and programs as setup finds
/// them: what a command or the app launches on.
pub fn surface(kind: SurfaceKind, env: Env) -> Box<dyn LaunchSurface> {
    match kind {
        SurfaceKind::Cli => Box::new(Cli::new(env)),
        SurfaceKind::Orca => Box::new(Orca::new(env)),
        SurfaceKind::Auto => Box::new(Auto::new(Orca::new(env.clone()), Cli::new(env))),
        SurfaceKind::Dry => Box::new(Dry::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_or_invalid_prompt_goes_in_a_file_and_the_marker_is_its_start() {
        let file = std::env::current_dir().unwrap().join("1.prompt.md");
        assert_eq!(argument("do the step", &file).unwrap().as_str(), "do the step");
        for prompt in ["x".repeat(PROMPT_IN_ARGV_MAX + 1), "- a list first".to_owned()] {
            let text = argument(&prompt, &file).unwrap();
            assert!(text.as_str().starts_with("Read the file ") && text.as_str().contains(&file.display().to_string()), "{}", text.as_str());
        }
        assert_eq!(argument(&"è".repeat(PROMPT_IN_ARGV_MAX), &file).unwrap().as_str().chars().count(), PROMPT_IN_ARGV_MAX, "characters, not bytes");
        assert_eq!(marker(&"é".repeat(100)).chars().count(), MARKER_CHARS);
        assert_eq!(marker("short"), "short");
    }

    #[test]
    fn a_codex_session_is_the_uuid_at_the_end_of_its_file() {
        let codex = AgentService::known(ServiceId::Codex);
        let sid = "11111111-2222-3333-4444-555555555555";
        let name = |name: &str| session_of(&codex, Path::new(name));
        assert_eq!(name(&format!("rollout-2026-10-21T12-00-00-{sid}.jsonl")).as_deref(), Some(sid));
        assert_eq!(name(&format!("rollout--{sid}.jsonl")).as_deref(), Some(sid));
        let upper = "ABCDEF11-2222-3333-4444-555555555555";
        for other in [format!("rollout-{sid}.jsonl"), format!("other-x-{sid}.jsonl"), "rollout-x-1111.jsonl".to_owned(), format!("rollout-x-{upper}.jsonl")] {
            assert_eq!(name(&other), None, "{other}");
        }
        assert_eq!(session_of(&AgentService::known(ServiceId::Claude), Path::new(&format!("{sid}.jsonl"))).as_deref(), Some(sid));
    }

    #[test]
    fn every_kind_has_a_surface_and_a_name() {
        for kind in SurfaceKind::ALL {
            assert_eq!(surface(kind, Env::default()).kind(), kind);
            assert_eq!(SurfaceKind::parse(kind.id()), Some(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.id());
        }
        assert_eq!(SurfaceKind::parse("ssh"), None);
    }
}
