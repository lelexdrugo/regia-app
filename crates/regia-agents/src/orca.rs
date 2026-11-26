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

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use regex::Regex;
use regia_exec::{AbsolutePath, Args, Effort, ExecError, Limits, Pin, Program, Search, Text, neutral_folder, orca};
use regia_identity::state_dir;
use regia_store::WallTime;
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
pub(crate) fn session_files(service: &AgentService, env: &Env, folder: &Path) -> BTreeSet<PathBuf> {
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
pub(crate) fn session_of(service: &AgentService, path: &Path) -> Option<String> {
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
pub(crate) fn holds(path: &Path, marker: &str) -> bool {
    let quoted = serde_json::to_string(marker).unwrap_or_default();
    let escaped = quoted.get(1..quoted.len().saturating_sub(1)).unwrap_or_default();
    head(path).iter().any(|line| line.contains(marker) || (!escaped.is_empty() && line.contains(escaped)))
}

/// The first message of the user in a claude session file starts with the marker.
pub(crate) fn first_message_starts_with(path: &Path, marker: &str) -> bool {
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

// ---------- the workers of the tasks in progress (#210, #641 of the dossier; F275-F277 of its inventory) ----------

/// The thresholds of the workers, from the prototype's `policies/worker.yaml` (F276): a worker silent past `silence_s`
/// seconds wakes a step; past `block_s` its adapter says whether it is blocked. One `key: integer` a line, `#` begins a
/// comment; a key it does not know, or a file that does not read, leaves the defaults, 30 and 180 minutes, and the block
/// is never before the silence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkerThresholds {
    pub silence_s: f64,
    pub block_s: f64,
}

/// The keys of the policy's file, and their defaults in minutes: the owner's decisions of 26/10 (#210) and 10/11 (#213).
const SILENCE_KEY: &str = "silenzio_minuti";
const BLOCK_KEY: &str = "blocco_minuti";
const SILENCE_DEFAULT_MIN: u64 = 30;
const BLOCK_DEFAULT_MIN: u64 = 180;

impl WorkerThresholds {
    /// The policy's file path under the prototype's home.
    pub const FILE: &'static str = "policies/worker.yaml";

    /// The prototype's `carica_soglia_s` and `carica_blocco_s` on the text of the file, `None` when it does not read.
    #[must_use]
    pub fn from_policy(text: Option<&str>) -> WorkerThresholds {
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        for line in text.unwrap_or_default().lines() {
            let before = line.split('#').next().unwrap_or_default();
            if let Some((key, value)) = before.split_once(':')
                && !key.trim().is_empty()
            {
                values.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
        if values.keys().any(|key| key != SILENCE_KEY && key != BLOCK_KEY) {
            values.clear();
        }
        let minutes = |key: &str, default: u64| {
            let value = values.get(key).map(String::as_str).unwrap_or_default();
            let whole = !value.is_empty() && value.chars().all(|c| c.is_ascii_digit());
            whole.then(|| value.parse::<u64>().ok()).flatten().filter(|minutes| *minutes >= 1).unwrap_or(default)
        };
        let silence = minutes(SILENCE_KEY, SILENCE_DEFAULT_MIN);
        WorkerThresholds { silence_s: (silence * 60) as f64, block_s: (minutes(BLOCK_KEY, BLOCK_DEFAULT_MIN).max(silence) * 60) as f64 }
    }
}

/// Why a worker silent past the block's threshold is blocked (#641, decision 1A of #213): it waits for a person (Orca's
/// `agentWait`, or a request of permission as its last line), or its last line holds a call to a tool still open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    WaitingForPerson,
    OpenCall,
}

/// A worker of a task in progress, as the precheck reads it: its task's number, when it last wrote, whether it waits
/// for a person, and why it is blocked. Orca's ids stay in this adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerBeat {
    pub task: u64,
    pub last: WallTime,
    pub waiting: bool,
    pub blocked: Option<Blocked>,
}

/// The pages of `worker-list` the precheck reads, a hundred dispatches each from the most recent; the lines of a page
/// of a transcript, and how many pages at most; the cap of one call.
const WORKER_LIST_PAGES: usize = 3;
const TRANSCRIPT_PAGES: usize = 400;
const PER_CALL: Duration = Duration::from_secs(20);

/// A request of permission of Orca or of the agent as a system line ("Allow Bash?"): the worker waits for a person.
static PERMISSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Allow [\w .:-]{1,40}\?").expect("the pattern of a permission"));

/// The warnings of a session as a system line of a transcript says them (#644 of the dossier, decision 1A): its limit,
/// which passes by itself, and its billing, which only the owner can fix.
static SESSION_LIMIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)session limit|hit your .{0,30}limit").expect("the pattern of a session's limit"));
static BILLING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)payment is past due|can't use your Claude subscription").expect("the pattern of a billing"));

/// Why a worker went silent because of its session (#644 of the dossier, decision 1A): the limit of its session, which
/// passes by itself, or its billing, which only the owner can fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCause {
    Limit,
    Billing,
}

/// The cause of a session as the prototype tells it from the texts of system lines: the billing when one of them says it, before the
/// limit, since a warning can say both and only the billing needs the owner; else the limit when one says it.
#[must_use]
pub fn session_cause<S: AsRef<str>>(texts: &[S]) -> Option<SessionCause> {
    if texts.iter().any(|text| BILLING.is_match(text.as_ref())) {
        Some(SessionCause::Billing)
    } else if texts.iter().any(|text| SESSION_LIMIT.is_match(text.as_ref())) {
        Some(SessionCause::Limit)
    } else {
        None
    }
}

/// The title of a worker's task, `--task-title "<product> #N …"` (protocol §1.3): the task's number.
static TASK_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^\s*{}\s+#(\d+)\b", regex::escape(regia_identity::PRODUCT_NAME))).expect("the pattern of a worker's title"));

/// An instant as Orca writes one, as the prototype's `_istante` reads it: milliseconds or seconds since the epoch, or an
/// ISO text, in UTC when it has no offset. `None` for anything else.
fn orca_instant(value: Option<&Value>) -> Option<WallTime> {
    match value? {
        Value::Number(number) => {
            let number = number.as_f64()?;
            WallTime::from_unix_secs_f64(if number > 1e11 { number / 1000.0 } else { number })
        }
        Value::String(text) => WallTime::parse_iso(text.trim()),
        _ => None,
    }
}

/// The fields of an object; anything else, which the prototype would fail on, fails the read.
fn object<'a>(value: &'a Value, what: &str) -> Result<&'a serde_json::Map<String, Value>, String> {
    value.as_object().ok_or_else(|| format!("orca: {what} is not an object"))
}

/// The objects of a list, those that are; none when it is no list.
fn objects(value: Option<&Value>) -> impl Iterator<Item = &serde_json::Map<String, Value>> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object)
}

fn text_of<'a>(map: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str).filter(|text| !text.is_empty())
}

/// The blocks of a line that are objects.
fn blocks(line: &serde_json::Map<String, Value>) -> Vec<&serde_json::Map<String, Value>> {
    objects(line.get("blocks")).collect()
}

/// A line of the assistant with a call to a tool that has no result and is not `completed`: still open.
fn open_call(line: &serde_json::Map<String, Value>) -> bool {
    let blocks = blocks(line);
    let results: Vec<Option<&Value>> = blocks.iter().filter(|block| block.get("type").and_then(Value::as_str) == Some("tool-result")).map(|block| block.get("callId")).collect();
    blocks.iter().any(|block| {
        block.get("type").and_then(Value::as_str) == Some("tool-call")
            && block.get("state").and_then(Value::as_str) != Some("completed")
            && !results.contains(&block.get("callId"))
    })
}

/// The start of the text of a system line, its first eighty characters; empty for any other line.
fn system_text(line: &serde_json::Map<String, Value>) -> String {
    if line.get("role").and_then(Value::as_str) != Some("system") {
        return String::new();
    }
    let texts: Vec<String> = blocks(line)
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .map(|block| match block.get("text") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
        })
        .collect();
    texts.join(" ").trim().chars().take(80).collect()
}

/// A line of a transcript, or a block of one: Orca's object.
type Line = serde_json::Map<String, Value>;

/// What a transcript says of its worker: when it last wrote, whether its last line of the assistant has an open call,
/// whether its last line asks for a permission, and the cause of a session its system lines at the end say (#644 of the
/// dossier), which only the command of the workers reads.
#[derive(Debug, Default)]
struct Transcript {
    last: Option<WallTime>,
    open_call: bool,
    permission: bool,
    cause: Option<SessionCause>,
}

/// One call of the Orca adapter: the `result` of `orca orchestration <words> --json`, or why there is none.
type OrcaCall<'a> = dyn FnMut(&[&str], Instant) -> Result<Value, String> + 'a;

/// The prototype's `_leggi_transcript`: page after page, until one is empty or names no next; a line of a person does not
/// count, of a line the later of `timestamp` and `completedAt`.
fn read_transcript(call: &mut OrcaCall<'_>, deadline: Instant, dispatch: &str) -> Result<Transcript, String> {
    let (mut cursor, mut last): (Option<String>, Option<WallTime>) = (None, None);
    let (mut assistant, mut final_line): (Option<Line>, Option<Line>) = (None, None);
    // The texts of the system lines after the last line that is not one: a warning of the session counts only when it is
    // the last thing the worker wrote.
    let mut tail: Vec<String> = Vec::new();
    let outcome = |last, assistant: &Option<Line>, final_line: &Option<Line>, tail: &[String]| Transcript {
        last,
        open_call: assistant.as_ref().is_some_and(open_call),
        permission: final_line.as_ref().is_some_and(|line| PERMISSION.is_match(&system_text(line))),
        cause: session_cause(tail),
    };
    for _ in 0..TRANSCRIPT_PAGES {
        let mut words = vec!["worker-read", "--dispatch", dispatch, "--limit", "50"];
        if let Some(cursor) = &cursor {
            words.extend(["--cursor", cursor.as_str()]);
        }
        let answer = call(&words, deadline)?;
        let Some(transcript) = object(&answer, "worker-read's result")?.get("transcript").and_then(Value::as_object) else {
            return Ok(Transcript::default());
        };
        let lines = transcript.get("messages").and_then(Value::as_array).cloned().unwrap_or_default();
        if lines.is_empty() {
            return Ok(outcome(last, &assistant, &final_line, &tail));
        }
        for line in &lines {
            let line = object(line, "a line of a transcript")?;
            let role = line.get("role").and_then(Value::as_str);
            if role == Some("user") {
                continue;
            }
            for at in [orca_instant(line.get("timestamp")), orca_instant(line.get("completedAt"))].into_iter().flatten() {
                if last.is_none_or(|last| at > last) {
                    last = Some(at);
                }
            }
            final_line = Some(line.clone());
            if role == Some("system") {
                tail.push(system_text(line));
            } else {
                tail.clear();
            }
            if role == Some("assistant") {
                assistant = Some(line.clone());
            }
        }
        cursor = transcript.get("nextCursor").and_then(Value::as_str).or_else(|| answer.get("cursor").and_then(Value::as_str)).filter(|c| !c.is_empty()).map(str::to_owned);
        if cursor.is_none() {
            return Ok(outcome(last, &assistant, &final_line, &tail));
        }
    }
    Err("orca: a transcript without end".to_owned())
}

/// The prototype's `leggi` with its Orca as `call` (the tests' fake, or the CLI): the workers of the dispatches in
/// progress whose task's title names one of `wanted`, at `now`. A worker that does not read is skipped; when the
/// deadline comes the read ends with the workers already read in full, never one half read. An error only when Orca
/// does not answer the list of the dispatches or of the tasks.
///
/// # Errors
///
/// When Orca does not answer `worker-list` or `task-list`, or answers them with something that is not one.
pub fn read_workers(call: &mut OrcaCall<'_>, wanted: &BTreeSet<u64>, thresholds: WorkerThresholds, now: WallTime, deadline: Instant) -> Result<Vec<WorkerBeat>, String> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    // The dispatches in progress, the most recent first, on at most three pages.
    let mut active: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..WORKER_LIST_PAGES {
        let mut words = vec!["worker-list", "--full", "--limit", "100"];
        if let Some(cursor) = &cursor {
            words.extend(["--cursor", cursor.as_str()]);
        }
        let answer = call(&words, deadline)?;
        let result = object(&answer, "worker-list's result")?;
        if result.get("scope").and_then(Value::as_object).and_then(|scope| scope.get("source")).is_some_and(|source| !source.is_null() && source.as_str() != Some("all")) {
            return Err("orca: worker-list is limited to a run".to_owned());
        }
        let listed = result.get("workers").is_some_and(truthy);
        for worker in objects(result.get("workers")) {
            let outcome = worker.get("projection").and_then(Value::as_object).and_then(|projection| projection.get("outcome")).and_then(Value::as_str);
            if outcome == Some("in_progress")
                && let Some(dispatch) = text_of(worker, "dispatchId")
            {
                active.push((dispatch.to_owned(), text_of(worker, "taskId").map(str::to_owned), text_of(worker, "runId").map(str::to_owned)));
            }
        }
        let page = result.get("page").and_then(Value::as_object);
        cursor = page.and_then(|page| text_of(page, "nextCursor")).or_else(|| text_of(result, "nextCursor")).map(str::to_owned);
        if cursor.is_none() || page.and_then(|page| page.get("hasMore")) == Some(&Value::Bool(false)) || !listed {
            break;
        }
    }
    // The task's number of each of Orca's tasks, from the titles of the tasks of their runs.
    let runs: BTreeSet<&str> = active.iter().filter_map(|(_, _, run)| run.as_deref()).collect();
    let mut numbers: BTreeMap<String, u64> = BTreeMap::new();
    for run in runs {
        let answer = call(&["task-list", "--run", run], deadline)?;
        for task in objects(object(&answer, "task-list's result")?.get("tasks")) {
            let title = task.get("title").and_then(Value::as_str).unwrap_or_default();
            if let (Some(found), Some(id)) = (TASK_TITLE.captures(title), text_of(task, "id"))
                && let Ok(number) = found[1].parse::<u64>()
            {
                numbers.insert(id.to_owned(), number);
            }
        }
    }
    let mut beats = Vec::new();
    let elapsed = |at: WallTime| (now.unix_micros() - at.unix_micros()) as f64 / 1e6;
    for (dispatch, task, _) in &active {
        let Some(number) = task.as_ref().and_then(|task| numbers.get(task)).copied().filter(|number| wanted.contains(number)) else { continue };
        let read = (|| -> Result<(Option<WallTime>, Transcript, bool), String> {
            let transcript = read_transcript(call, deadline, dispatch)?;
            let mut last = transcript.last;
            let mut waiting = false;
            // Past the threshold: Orca's own beat, when it gives one, and whether a person is awaited.
            if last.is_none_or(|last| elapsed(last) > thresholds.silence_s.min(thresholds.block_s)) {
                let shown = call(&["worker-show", "--dispatch", dispatch, "--full"], deadline)?;
                let shown = object(&shown, "worker-show's result")?;
                let of_dispatch = shown.get("dispatch").filter(|value| !value.is_null());
                let field = |key: &str| of_dispatch.and_then(|dispatch| dispatch.get(key));
                let beat = orca_instant(field("lastHeartbeatAt"));
                last = match (last, beat) {
                    (Some(line), Some(beat)) => Some(line.max(beat)),
                    (Some(line), None) => Some(line),
                    (None, Some(beat)) => Some(beat),
                    (None, None) => orca_instant(field("dispatchedAt")),
                };
                waiting = shown.get("observation").and_then(|observation| observation.get("agentWait")).is_some_and(truthy);
            }
            Ok((last, transcript, waiting))
        })();
        let (last, transcript, waiting) = match read {
            Ok(read) => read,
            Err(_) if Instant::now() + Duration::from_secs(1) >= deadline => break,
            Err(_) => continue,
        };
        if let Some(last) = last {
            let blocked = if elapsed(last) <= thresholds.block_s {
                None
            } else if waiting || transcript.permission {
                Some(Blocked::WaitingForPerson)
            } else if transcript.open_call {
                Some(Blocked::OpenCall)
            } else {
                None
            };
            beats.push(WorkerBeat { task: number, last, waiting, blocked });
        }
    }
    Ok(beats)
}

/// The JSON truth of a value, as Python's `bool` reads it.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

// ---------- the command of the workers (#698 and #744 of the dossier; F277 of its inventory) ----------

/// The pages of `worker-list` the command reads at most, to the end of the list, a hundred dispatches each: a dispatch
/// forgotten for months is at its bottom. A list that does not end within them is no answer.
const ALL_PAGES: usize = 30;

/// Orca's activities (`projection.stage.activity`) that mean a person is awaited (#698 of the dossier): `blocked`, seen on
/// a record, and `needs-input`, the reason of Orca's own notice, taken too. Only the command reads them.
pub const WAITING_ACTIVITIES: [&str; 2] = ["blocked", "needs-input"];

/// A dispatch in progress of one of the tasks asked, as the command reads it: Orca's ids, which it prints only when asked,
/// its activity, and what its transcript and `worker-show` say, `None` when it does not read.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchState {
    pub dispatch_id: String,
    pub run_id: Option<String>,
    /// `projection.stage.activity`, lowercase: the list's, or `worker-show`'s when the list has none and the dispatch read.
    pub activity: Option<String>,
    pub read: Option<DispatchRead>,
}

/// What a dispatch that reads says: when its worker last wrote; whether it waits for a person (Orca's `agentWait`, or an
/// activity of [`WAITING_ACTIVITIES`], under the thresholds too); why it is blocked, past the block's threshold; and why
/// its session stopped it, only past the silence's threshold and never while it waits for a person.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchRead {
    pub last: WallTime,
    pub waiting: bool,
    pub blocked: Option<Blocked>,
    pub cause: Option<SessionCause>,
}

/// A task asked, with its dispatches in progress in the order of Orca's list, the most recent first.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskDispatches {
    pub task: u64,
    pub dispatches: Vec<DispatchState>,
}

/// A dispatch of the list: its id, its task and its run in Orca, and its activity.
struct Listed {
    dispatch: String,
    task: Option<String>,
    run: Option<String>,
    activity: Option<String>,
}

/// `projection.stage.activity` of a record of `worker-list --full` or `worker-show --full`, trimmed and lowercase; `None`
/// when it is missing, empty or not a text.
fn activity_of(record: &serde_json::Map<String, Value>) -> Option<String> {
    let activity = record.get("projection")?.get("stage")?.get("activity")?.as_str()?.trim();
    (!activity.is_empty()).then(|| activity.to_lowercase())
}

/// The prototype's list of the dispatches with their activity (#698): every dispatch in progress, the most recent first, to the end of
/// `worker-list` within [`ALL_PAGES`] pages, each with its activity.
fn all_in_progress(call: &mut OrcaCall<'_>, deadline: Instant) -> Result<Vec<Listed>, String> {
    let mut found = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..ALL_PAGES {
        let mut words = vec!["worker-list", "--full", "--limit", "100"];
        if let Some(cursor) = &cursor {
            words.extend(["--cursor", cursor.as_str()]);
        }
        let answer = call(&words, deadline)?;
        let result = object(&answer, "worker-list's result")?;
        if result.get("scope").and_then(Value::as_object).and_then(|scope| scope.get("source")).is_some_and(|source| !source.is_null() && source.as_str() != Some("all")) {
            return Err("orca: worker-list is limited to a run".to_owned());
        }
        let listed = result.get("workers").is_some_and(truthy);
        for worker in objects(result.get("workers")) {
            let outcome = worker.get("projection").and_then(Value::as_object).and_then(|projection| projection.get("outcome")).and_then(Value::as_str);
            if outcome == Some("in_progress")
                && let Some(dispatch) = text_of(worker, "dispatchId")
            {
                let (task, run) = (text_of(worker, "taskId").map(str::to_owned), text_of(worker, "runId").map(str::to_owned));
                found.push(Listed { dispatch: dispatch.to_owned(), task, run, activity: activity_of(worker) });
            }
        }
        let page = result.get("page").and_then(Value::as_object);
        cursor = page.and_then(|page| text_of(page, "nextCursor")).or_else(|| text_of(result, "nextCursor")).map(str::to_owned);
        if cursor.is_none() || page.and_then(|page| page.get("hasMore")) == Some(&Value::Bool(false)) || !listed {
            return Ok(found);
        }
    }
    Err("orca: worker-list without end".to_owned())
}

/// The prototype's `_numeri_dei_task`: the task's number of each of Orca's tasks of `runs`, from its title.
fn task_numbers(call: &mut OrcaCall<'_>, deadline: Instant, runs: &BTreeSet<&str>) -> Result<BTreeMap<String, u64>, String> {
    let mut numbers = BTreeMap::new();
    for run in runs {
        let answer = call(&["task-list", "--run", run], deadline)?;
        for task in objects(object(&answer, "task-list's result")?.get("tasks")) {
            let title = task.get("title").and_then(Value::as_str).unwrap_or_default();
            if let (Some(found), Some(id)) = (TASK_TITLE.captures(title), text_of(task, "id"))
                && let Ok(number) = found[1].parse::<u64>()
            {
                numbers.insert(id.to_owned(), number);
            }
        }
    }
    Ok(numbers)
}

/// The prototype's `_leggi_dispatch`: a dispatch read as [`read_workers`] reads a worker, with the wait of its activity,
/// which the list's record holds already, under the thresholds too. `None` when there is no instant; otherwise what it
/// read, with its activity, the list's or else `worker-show`'s.
fn read_dispatch(call: &mut OrcaCall<'_>, deadline: Instant, listed: &Listed, thresholds: WorkerThresholds, now: WallTime) -> Result<Option<(DispatchRead, Option<String>)>, String> {
    let elapsed = |at: WallTime| (now.unix_micros() - at.unix_micros()) as f64 / 1e6;
    let transcript = read_transcript(call, deadline, &listed.dispatch)?;
    let (mut last, mut activity, mut agent_wait) = (transcript.last, listed.activity.clone(), false);
    if last.is_none_or(|last| elapsed(last) > thresholds.silence_s.min(thresholds.block_s)) {
        let shown = call(&["worker-show", "--dispatch", &listed.dispatch, "--full"], deadline)?;
        let shown = object(&shown, "worker-show's result")?;
        let of_dispatch = shown.get("dispatch").filter(|value| !value.is_null());
        let field = |key: &str| of_dispatch.and_then(|dispatch| dispatch.get(key));
        let beat = orca_instant(field("lastHeartbeatAt"));
        last = match (last, beat) {
            (Some(line), Some(beat)) => Some(line.max(beat)),
            (Some(line), None) => Some(line),
            (None, Some(beat)) => Some(beat),
            (None, None) => orca_instant(field("dispatchedAt")),
        };
        agent_wait = shown.get("observation").and_then(|observation| observation.get("agentWait")).is_some_and(truthy);
        activity = activity.or_else(|| activity_of(shown));
    }
    let Some(last) = last else { return Ok(None) };
    let waiting = agent_wait || activity.as_deref().is_some_and(|activity| WAITING_ACTIVITIES.contains(&activity));
    // The cause of a session holds over an open call, and a wait for a person over the cause (#644 of the dossier).
    let cause = transcript.cause.filter(|_| elapsed(last) > thresholds.silence_s && !(waiting || transcript.permission));
    let blocked = if elapsed(last) <= thresholds.block_s {
        None
    } else if waiting || transcript.permission {
        Some(Blocked::WaitingForPerson)
    } else if transcript.open_call {
        Some(Blocked::OpenCall)
    } else {
        None
    };
    Ok(Some((DispatchRead { last, waiting, blocked, cause }, activity)))
}

/// The prototype's `osserva_dispatch`, its reading (#698 of the dossier): for each of `tasks`, in their order, every
/// dispatch in progress whose task's title names it, whatever the tracker says of the task. A dispatch that does not read
/// stays in its task's list, unread; once a read fails with the deadline near, the dispatches after it are not read
/// either, and stay listed. It changes nothing of [`read_workers`], which the precheck reads.
///
/// # Errors
///
/// When Orca does not answer `worker-list` or `task-list`, answers them with something that is not one, or with a list
/// that does not end.
pub fn read_dispatches(call: &mut OrcaCall<'_>, tasks: &[u64], thresholds: WorkerThresholds, now: WallTime, deadline: Instant) -> Result<Vec<TaskDispatches>, String> {
    let listed = all_in_progress(call, deadline)?;
    let runs: BTreeSet<&str> = listed.iter().filter_map(|dispatch| dispatch.run.as_deref()).collect();
    let numbers = task_numbers(call, deadline, &runs)?;
    let wanted: BTreeSet<u64> = tasks.iter().copied().collect();
    let mut by_task: BTreeMap<u64, Vec<DispatchState>> = BTreeMap::new();
    let mut expired = false;
    for dispatch in &listed {
        let Some(number) = dispatch.task.as_ref().and_then(|task| numbers.get(task)).copied().filter(|number| wanted.contains(number)) else { continue };
        let mut found = None;
        if !expired {
            match read_dispatch(call, deadline, dispatch, thresholds, now) {
                Ok(read) => found = read,
                Err(_) => expired = Instant::now() + Duration::from_secs(1) >= deadline,
            }
        }
        let (read, activity) = match found {
            Some((read, activity)) => (Some(read), activity),
            None => (None, dispatch.activity.clone()),
        };
        by_task.entry(number).or_default().push(DispatchState { dispatch_id: dispatch.dispatch.clone(), run_id: dispatch.run.clone(), activity, read });
    }
    Ok(tasks.iter().map(|task| TaskDispatches { task: *task, dispatches: by_task.get(task).cloned().unwrap_or_default() }).collect())
}

/// The workers of Orca on `orca` as setup finds it, or pinned: `orca orchestration <words> --json` for each call, with no
/// stdin, each within its cap and the read's deadline, in a neutral folder and the environment of its allow-list.
#[derive(Debug, Clone, Default)]
pub struct OrcaWorkers {
    pin: Option<Pin>,
}

impl OrcaWorkers {
    #[must_use]
    pub fn new() -> OrcaWorkers {
        OrcaWorkers::default()
    }

    #[must_use]
    pub fn with_pin(mut self, pin: Pin) -> OrcaWorkers {
        self.pin = Some(pin);
        self
    }

    /// [`read_workers`] on this machine's Orca.
    ///
    /// # Errors
    ///
    /// When `orca` cannot be found, or does not answer the lists.
    pub fn read(&self, wanted: &BTreeSet<u64>, thresholds: WorkerThresholds, now: WallTime, deadline: Instant) -> Result<Vec<WorkerBeat>, String> {
        let pin = self.pin()?;
        read_workers(&mut orca_call(&pin), wanted, thresholds, now, deadline)
    }

    /// [`read_dispatches`] on this machine's Orca: what the command of the workers shows.
    ///
    /// # Errors
    ///
    /// When `orca` cannot be found, or does not answer the lists.
    pub fn read_dispatches(&self, tasks: &[u64], thresholds: WorkerThresholds, now: WallTime, deadline: Instant) -> Result<Vec<TaskDispatches>, String> {
        let pin = self.pin()?;
        read_dispatches(&mut orca_call(&pin), tasks, thresholds, now, deadline)
    }

    fn pin(&self) -> Result<Pin, String> {
        match &self.pin {
            Some(pin) => Ok(pin.clone()),
            None => Pin::resolve(Program::Orca).map_err(|error| format!("orca is not available: {error}")),
        }
    }
}

/// One call of the adapter on `orca` at `pin`: `orca orchestration <words> --json`, the `result` of its answer, or why
/// there is none.
fn orca_call(pin: &Pin) -> impl FnMut(&[&str], Instant) -> Result<Value, String> + '_ {
    move |words: &[&str], deadline: Instant| -> Result<Value, String> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left <= Duration::from_secs(1) {
            return Err("orca: the deadline ran out".to_owned());
        }
        let args = orchestration(words)?;
        let output = pin.launch(args, &neutral_folder()).limits(Limits { timeout: PER_CALL.min(left), ..Limits::default() }).run().map_err(|error| format!("orca {}: {error}", words[0]))?;
        let answer: Value = serde_json::from_slice(&output.stdout).map_err(|_| format!("orca {}: an answer that is not JSON", words[0]))?;
        if !answer.get("ok").is_some_and(truthy) {
            let code = answer.get("error").and_then(|error| error.get("code")).and_then(Value::as_str).unwrap_or("error");
            return Err(format!("orca {}: {code}", words[0]));
        }
        Ok(answer.get("result").filter(|result| truthy(result)).cloned().unwrap_or_else(|| json!({})))
    }
}

/// The workers of the tasks in progress as this machine's app of agents shows them, with `orca` as setup finds it:
/// [`OrcaWorkers::read`]. The rest of the product names none of this, as it asks a [`crate::ChatHost`] for the chats.
///
/// # Errors
///
/// When the app cannot be found, or does not answer the lists.
pub fn running_workers(wanted: &BTreeSet<u64>, thresholds: WorkerThresholds, now: WallTime, deadline: Instant) -> Result<Vec<WorkerBeat>, String> {
    OrcaWorkers::new().read(wanted, thresholds, now, deadline)
}

/// The dispatches in progress of the tasks asked as this machine's app of agents shows them, with `orca` as setup finds
/// it: [`OrcaWorkers::read_dispatches`]. The command of the workers names none of this, as the precheck does not.
///
/// # Errors
///
/// When the app cannot be found, or does not answer the lists.
pub fn running_dispatches(tasks: &[u64], thresholds: WorkerThresholds, now: WallTime, deadline: Instant) -> Result<Vec<TaskDispatches>, String> {
    OrcaWorkers::new().read_dispatches(tasks, thresholds, now, deadline)
}

/// What the resume of a session a usage limit stopped sends it (#576): a status message, from a sender, to a step's
/// chat or a worker's dispatch, with the worker's task and dispatch when it is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusMessage<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    pub ids: Option<(&'a str, &'a str)>,
}

impl OrcaWorkers {
    /// The status message that wakes a session of the app (#576, F290 of the dossier's inventory): `orca orchestration
    /// send --from=… --to=… --type=status --subject=… --body=… [--task-id=… --dispatch-id=…] --json`. The id Orca gave
    /// the message. Orca refuses one without a sender (`no_active_sender_terminal`), and takes any sender, also a
    /// label that names no terminal (the dossier's `.regia/prove/576/prova-mittente.md`).
    ///
    /// # Errors
    ///
    /// When `orca` cannot be found, or does not take the message: its code, as the other calls give it.
    pub fn send_status(&self, message: &StatusMessage<'_>, deadline: Instant) -> Result<Option<String>, String> {
        let pin = self.pin()?;
        let mut words = vec!["send", "--from", message.from, "--to", message.to, "--type", "status", "--subject", message.subject, "--body", message.body];
        if let Some((task, dispatch)) = message.ids {
            words.extend(["--task-id", task, "--dispatch-id", dispatch]);
        }
        let result = orca_call(&pin)(&words, deadline)?;
        Ok(result.get("messageId").and_then(Value::as_str).map(str::to_owned))
    }
}

/// `orca orchestration <words> --json`, each flag with its value as one token, the values typed. The reads, and the one
/// message the product sends: a status (#576), never another type, never an outcome or a payload.
fn orchestration(words: &[&str]) -> Result<Args, String> {
    let mut args = Args::new().word("orchestration");
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        args = match *word {
            "worker-list" => args.word("worker-list"),
            "worker-read" => args.word("worker-read"),
            "worker-show" => args.word("worker-show"),
            "task-list" => args.word("task-list"),
            "send" => args.word("send"),
            "--type" => match rest.next() {
                Some(&"status") => args.word("--type=status"),
                other => return Err(format!("orca: --type {} is not sent", other.copied().unwrap_or_default())),
            },
            flag @ ("--from" | "--to" | "--subject" | "--body" | "--task-id" | "--dispatch-id") => {
                let value = Text::parse(rest.next().ok_or_else(|| format!("orca: {flag} without a value"))?).map_err(|error| format!("orca: {flag}: {error}"))?;
                let name = match flag {
                    "--from" => "--from",
                    "--to" => "--to",
                    "--subject" => "--subject",
                    "--body" => "--body",
                    "--task-id" => "--task-id",
                    _ => "--dispatch-id",
                };
                args.opt(name, &value)
            }
            "--full" => args.word("--full"),
            flag @ ("--limit" | "--cursor" | "--dispatch" | "--run") => {
                let value = Text::parse(rest.next().ok_or_else(|| format!("orca: {flag} without a value"))?).map_err(|error| format!("orca: {flag}: {error}"))?;
                let name = match flag {
                    "--limit" => "--limit",
                    "--cursor" => "--cursor",
                    "--dispatch" => "--dispatch",
                    _ => "--run",
                };
                args.opt(name, &value)
            }
            other => return Err(format!("orca: no word {other}")),
        };
    }
    Ok(args.word("--json"))
}

/// The surface `kind` names, over `env`, with this process's state folder and programs as setup finds
/// them: what a command or the app launches on.
pub fn surface(kind: SurfaceKind, env: Env) -> Box<dyn LaunchSurface + Send + Sync> {
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

    /// The argv of the resume's message, as the probe of #576 sent it to the real Orca: one token a flag, `--json` last;
    /// a type other than a status, an outcome, or a value that reads as a flag is refused before `orca` starts.
    #[test]
    fn the_resume_s_message_is_the_only_send_and_a_status() {
        let tokens = |words: &[&str]| orchestration(words).map(|args| args.tokens().map(|token| token.to_string_lossy().into_owned()).collect::<Vec<_>>());
        let words = [
            "send", "--from", "app-x", "--to", "dispatch:ctx_1", "--type", "status", "--subject", "Continue where you left off", "--body",
            "The limit's reset is past: café", "--task-id", "task_1", "--dispatch-id", "ctx_1",
        ];
        assert_eq!(
            tokens(&words).unwrap(),
            [
                "orchestration", "send", "--from=app-x", "--to=dispatch:ctx_1", "--type=status", "--subject=Continue where you left off",
                "--body=The limit's reset is past: café", "--task-id=task_1", "--dispatch-id=ctx_1", "--json",
            ]
        );
        for refused in [&["send", "--type", "question"][..], &["send", "--type"], &["send", "--outcome", "succeeded"], &["send", "--body", "-x"], &["send", "--payload", "{}"]] {
            assert!(tokens(refused).is_err(), "{refused:?}");
        }
    }

    /// `test_worker_orca.py` of the prototype, on the policy's file: one key a line, the defaults for a file that does not
    /// read or has a key it does not know, and the block never before the silence.
    #[test]
    fn the_thresholds_come_from_the_policy_s_file() {
        let read = |text: Option<&str>| {
            let found = WorkerThresholds::from_policy(text);
            (found.silence_s, found.block_s)
        };
        assert_eq!(read(None), (1800.0, 10800.0));
        assert_eq!(read(Some("# a comment\nsilenzio_minuti: 45  # why\nblocco_minuti: 90\n")), (2700.0, 5400.0));
        assert_eq!(read(Some("silenzio_minuti: 200\nblocco_minuti: 90\n")), (12000.0, 12000.0), "never before the silence");
        assert_eq!(read(Some("silenzio_minuti: 45\naltro: 3\n")), (1800.0, 10800.0), "a key it does not know");
        assert_eq!(read(Some("silenzio_minuti: 0\nblocco_minuti: -5\n")), (1800.0, 10800.0), "at least one minute, digits only");
    }

    /// The prototype's `leggi` against an injected Orca: a silent worker, one with an open call and one asking a
    /// permission past the block, one waiting for a person, a task not wanted, a title of another kind, a transcript
    /// that fails; and a list limited to one run, which is no answer.
    #[test]
    fn the_workers_read_from_orca_as_the_prototype_reads_them() {
        let now = WallTime::parse_iso("2026-01-10T09:00:00Z").unwrap();
        let ms = |minutes_ago: i64| json!(now.unix_micros() / 1000 - minutes_ago * 60_000);
        let line = |minutes_ago: i64, blocks: Value| json!({"role": "assistant", "timestamp": ms(minutes_ago), "blocks": blocks});
        let transcripts = BTreeMap::from([
            ("d1", vec![line(40, json!([{"type": "text", "text": "working"}])), json!({"role": "user", "timestamp": ms(1)})]),
            ("d2", vec![line(200, json!([{"type": "tool-call", "callId": "c", "state": "running", "name": "Bash"}]))]),
            ("d3", vec![line(200, json!([{"type": "tool-call", "callId": "c", "state": "completed"}])), json!({"role": "system", "timestamp": ms(200), "blocks": [{"type": "text", "text": "Allow Bash?"}]})]),
            ("d4", vec![line(50, json!([]))]),
            ("d5", vec![line(10, json!([]))]),
        ]);
        let workers: Vec<Value> = ["d1", "d2", "d3", "d4", "d5", "d6", "d7"]
            .iter()
            .enumerate()
            .map(|(at, dispatch)| json!({"dispatchId": dispatch, "taskId": format!("t{}", at + 1), "runId": "r", "projection": {"outcome": "in_progress"}}))
            .chain([json!({"dispatchId": "d8", "taskId": "t1", "runId": "r", "projection": {"outcome": "done"}})])
            .collect();
        let tasks: Vec<Value> =
            (1..=7).map(|n| json!({"id": format!("t{n}"), "title": if n == 6 { "other #6".to_owned() } else { format!("{} #{n} work", regia_identity::PRODUCT_NAME) }})).collect();
        let mut asked: Vec<String> = Vec::new();
        let mut call = |words: &[&str], _: Instant| -> Result<Value, String> {
            asked.push(words.join(" "));
            match words[0] {
                "worker-list" => Ok(json!({"workers": workers, "page": {"hasMore": false}})),
                "task-list" => Ok(json!({"tasks": tasks})),
                "worker-read" if words[2] == "d7" => Err("orca worker-read: error".to_owned()),
                "worker-read" if words.contains(&"--cursor") => Ok(json!({"transcript": {"messages": []}})),
                "worker-read" => Ok(json!({"transcript": {"messages": transcripts[words[2]], "nextCursor": "page-1"}})),
                "worker-show" => Ok(json!({"dispatch": {"lastHeartbeatAt": null}, "observation": {"agentWait": words[2] == "d4"}})),
                other => Err(format!("unexpected {other}")),
            }
        };
        let wanted: BTreeSet<u64> = [1, 2, 3, 4, 6, 7].into();
        let thresholds = WorkerThresholds::from_policy(None);
        let beats = read_workers(&mut call, &wanted, thresholds, now, Instant::now() + Duration::from_secs(60)).unwrap();
        let summary: Vec<(u64, i64, bool, Option<Blocked>)> =
            beats.iter().map(|beat| (beat.task, (now.unix_micros() - beat.last.unix_micros()) / 60_000_000, beat.waiting, beat.blocked)).collect();
        assert_eq!(summary, [(1, 40, false, None), (2, 200, false, Some(Blocked::OpenCall)), (3, 200, false, Some(Blocked::WaitingForPerson)), (4, 50, true, None)]);
        assert!(!asked.iter().any(|call| call.contains("d5") || call.contains("d6") || call.contains("d8")), "not wanted, another title, not in progress: {asked:?}");
        let mut limited = |_: &[&str], _: Instant| Ok(json!({"scope": {"source": "run"}, "workers": []}));
        assert!(read_workers(&mut limited, &wanted, thresholds, now, Instant::now() + Duration::from_secs(60)).is_err());
        assert_eq!(orca_instant(Some(&json!("2026-01-10 08:00:00"))), WallTime::parse_iso("2026-01-10T08:00:00Z"));
        assert_eq!(orca_instant(Some(&json!(true))), None);
    }

    /// The prototype's cause of a session and its tests of #644: the billing before the limit, in any of the texts; the
    /// limit in both its forms; a billing whose first eighty characters still say it.
    #[test]
    fn the_cause_of_a_session_is_the_prototype_s() {
        let limit = "You've hit your session limit · resets 3:30pm (Europe/Rome)";
        let billing: String = "Claude Code can't use your Claude subscription, usually because a payment is past due".chars().take(80).collect();
        assert_eq!(session_cause(&[limit]), Some(SessionCause::Limit));
        assert_eq!(session_cause(&["Claude hit its session limit."]), Some(SessionCause::Limit));
        assert_eq!(session_cause(&[billing.as_str()]), Some(SessionCause::Billing));
        assert_eq!(session_cause(&[limit, billing.as_str()]), Some(SessionCause::Billing));
        assert_eq!(session_cause(&["Allow Bash?", "x"]), None);
        assert_eq!(session_cause::<&str>(&[]), None);
    }

    /// The prototype's `osserva_dispatch`, its reading, on an injected Orca: every dispatch in progress of a task, in the
    /// list's order, a concluded one left out; an open call with a session's limit after it, both said; the list's
    /// activity, a wait under the thresholds too, or `worker-show`'s; a dispatch that does not read stays, unread; and
    /// once a read fails with the deadline near, the dispatches after it are listed and not read.
    #[test]
    fn the_dispatches_read_as_the_prototype_s_command_reads_them() {
        let now = WallTime::parse_iso("2026-01-10T09:00:00Z").unwrap();
        let ms = |minutes_ago: i64| json!(now.unix_micros() / 1000 - minutes_ago * 60_000);
        let line = |minutes_ago: i64, role: &str, blocks: Value| json!({"role": role, "timestamp": ms(minutes_ago), "blocks": blocks});
        let transcripts = BTreeMap::from([
            ("d1", vec![line(2, "assistant", json!([{"type": "text", "text": "ok"}]))]),
            ("d2", vec![line(240, "assistant", json!([{"type": "tool-call", "callId": "c", "state": "running"}])), line(240, "system", json!([{"type": "text", "text": "Claude hit its session limit."}]))]),
            ("d4", vec![line(5, "assistant", json!([]))]),
            ("d5", vec![line(240, "assistant", json!([]))]),
        ]);
        let record = |dispatch: &str, task: &str, outcome: &str, activity: Option<&str>| {
            let mut record = json!({"dispatchId": dispatch, "taskId": task, "runId": "r", "projection": {"outcome": outcome}});
            if let Some(activity) = activity {
                record["projection"]["stage"] = json!({"activity": activity});
            }
            record
        };
        let workers = json!([
            record("d1", "t1", "in_progress", None), record("d2", "t1b", "in_progress", None), record("d3", "t1", "succeeded", None),
            record("d4", "t2", "in_progress", Some(" Blocked ")), record("d5", "t3", "in_progress", None), record("d6", "t3", "in_progress", None),
        ]);
        let title = |n: u64| format!("{} #{n} work", regia_identity::PRODUCT_NAME);
        let tasks = json!([{"id": "t1", "title": title(1)}, {"id": "t1b", "title": title(1)}, {"id": "t2", "title": title(2)}, {"id": "t3", "title": title(3)}]);
        let failing = std::cell::Cell::new("d6");
        let asked = std::cell::RefCell::new(Vec::new());
        let mut call = |words: &[&str], _: Instant| -> Result<Value, String> {
            asked.borrow_mut().push(words.join(" "));
            match words[0] {
                "worker-list" => Ok(json!({"workers": workers, "page": {"hasMore": false}})),
                "task-list" => Ok(json!({"tasks": tasks})),
                "worker-read" if words[2] == failing.get() => Err("orca worker-read: source_changed".to_owned()),
                "worker-read" if words.contains(&"--cursor") => Ok(json!({"transcript": {"messages": []}})),
                "worker-read" => Ok(json!({"transcript": {"messages": transcripts[words[2]], "nextCursor": "page-1"}})),
                "worker-show" if words[2] == "d5" => Ok(json!({"dispatch": {}, "projection": {"stage": {"activity": "needs-input"}}})),
                "worker-show" => Ok(json!({"dispatch": {}, "observation": {}})),
                other => Err(format!("unexpected {other}")),
            }
        };
        let thresholds = WorkerThresholds::from_policy(None);
        let summary = |found: &[TaskDispatches]| -> Vec<String> {
            let minutes = |read: &DispatchRead| (now.unix_micros() - read.last.unix_micros()) / 60_000_000;
            found
                .iter()
                .flat_map(|task| task.dispatches.iter().map(move |dispatch| (task.task, dispatch)))
                .map(|(task, dispatch)| {
                    let read = dispatch.read.as_ref().map(|read| (minutes(read), read.waiting, read.blocked, read.cause));
                    format!("{task} {} {:?} {read:?}", dispatch.dispatch_id, dispatch.activity)
                })
                .collect()
        };
        let found = read_dispatches(&mut call, &[1, 2, 3, 9], thresholds, now, Instant::now() + Duration::from_secs(60)).unwrap();
        assert_eq!(found.iter().map(|task| (task.task, task.dispatches.len())).collect::<Vec<_>>(), [(1, 2), (2, 1), (3, 2), (9, 0)]);
        assert_eq!(
            summary(&found),
            [
                "1 d1 None Some((2, false, None, None))",
                "1 d2 None Some((240, false, Some(OpenCall), Some(Limit)))",
                "2 d4 Some(\"blocked\") Some((5, true, None, None))",
                "3 d5 Some(\"needs-input\") Some((240, true, Some(WaitingForPerson), None))",
                "3 d6 None None",
            ]
        );
        assert!(!asked.borrow().iter().any(|call| call.contains("worker-show --dispatch d4")), "a recent dispatch is not shown: {:?}", asked.borrow());
        failing.set("d5");
        asked.borrow_mut().clear();
        let found = read_dispatches(&mut call, &[3], thresholds, now, Instant::now() + Duration::from_millis(500)).unwrap();
        assert_eq!(summary(&found), ["3 d5 None None", "3 d6 None None"]);
        assert!(!asked.borrow().iter().any(|call| call.contains("d6")), "past the deadline nothing more is read: {:?}", asked.borrow());
        let mut endless = |_: &[&str], _: Instant| Ok(json!({"workers": [record("d1", "t1", "in_progress", None)], "page": {"hasMore": true, "nextCursor": "more"}}));
        assert!(read_dispatches(&mut endless, &[1], thresholds, now, Instant::now() + Duration::from_secs(60)).is_err(), "a list without end is no answer");
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
