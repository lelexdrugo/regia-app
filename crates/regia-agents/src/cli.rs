//! The Cli surface (ADR 0001 §6): the service's own CLI, started on its own with a log, its argv
//! fixed and the prompt on stdin. It waits for the session to open in the log and for its
//! transcript, never for the end, and says whether a failure came before the start.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use regia_exec::{AbsolutePath, Detached, Launch, Limits, Pin, Search, claude, codex, secret_shaped};
use regia_identity::{env_var, prototype_env_var, state_dir};
use regia_store::{is_within, temporary_folder};
use serde_json::Value;

use crate::launch::{Label, LaunchError, LaunchRequest, LaunchSurface, Receipt, SurfaceKind};
use crate::{AgentService, Env, ServiceId, WakeOutcome};

/// The folder of the logs in the state folder, by project. It is the prototype's, shared while the
/// two coexist, so a step launched by either is found in one place.
const STEPS: &str = "passi";

/// The variables of the agents' folders. A session may start the other agent, for a review.
const AGENT_FOLDERS: [&str; 2] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/// The prompt of the token wake-up, on stdin; its argv is [`claude::wake`].
pub const WAKE_PROMPT: &str = "Reply only: ok";

/// What a failed wake-up keeps of the text that explains it: the end, this many characters.
const WAKE_ERROR_CHARS: usize = 300;

/// How long the surface waits, and how often it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waits {
    /// For the first line of the stream that names the session.
    pub session: Duration,
    /// For the transcript, while the process lives.
    pub transcript: Duration,
    /// For the token wake-up to end.
    pub wake: Duration,
    pub tick: Duration,
}

impl Default for Waits {
    /// The prototype's: 60 s for the session, 30 s for the transcript, 120 s for the wake-up, which
    /// usually ends in 2 to 5 s.
    fn default() -> Self {
        Waits {
            session: Duration::from_secs(60),
            transcript: Duration::from_secs(30),
            wake: Duration::from_secs(120),
            tick: Duration::from_millis(200),
        }
    }
}

/// The service's CLI as a detached process with its log.
#[derive(Debug, Clone)]
pub struct Cli {
    env: Env,
    state: Option<PathBuf>,
    pins: BTreeMap<ServiceId, Pin>,
    search: Option<Search>,
    waits: Waits,
}

impl Cli {
    /// The surface over `env`, the environment the child's variables come from, with the state
    /// folder of this process and each program found as setup finds it.
    pub fn new(env: Env) -> Cli {
        Cli { env, state: state_dir(), pins: BTreeMap::new(), search: None, waits: Waits::default() }
    }

    /// Another state folder, the root of the logs.
    #[must_use]
    pub fn with_state(mut self, folder: PathBuf) -> Cli {
        self.state = Some(folder);
        self
    }

    /// The program of `service` at a pinned path, as the setup confirmed it or as a test names a fake.
    #[must_use]
    pub fn with_pin(mut self, service: ServiceId, pin: Pin) -> Cli {
        self.pins.insert(service, pin);
        self
    }

    /// Where a program without a pin is looked for, in place of this process's search.
    #[must_use]
    pub fn with_search(mut self, search: Search) -> Cli {
        self.search = Some(search);
        self
    }

    #[must_use]
    pub fn with_waits(mut self, waits: Waits) -> Cli {
        self.waits = waits;
        self
    }

    fn pin(&self, service: &AgentService) -> Result<Pin, LaunchError> {
        let pinned = match (self.pins.get(&service.id()), &self.search) {
            (Some(pin), _) => Ok(pin.clone()),
            (None, Some(search)) => Pin::search(service.program(), search),
            (None, None) => service.detect(),
        };
        pinned.map_err(|error| LaunchError::before(error.to_string()))
    }

    /// What the child sees on top of the allow-list of its program (its own folder and the base
    /// variables of regia-exec), from the environment of the surface:
    ///
    /// - `PYTHONUTF8=1`, because the session runs the prototype's tools, and without it Python on
    ///   Windows fails to print an arrow and writes files in the console's code page;
    /// - the prototype's and the product's own variables (`REGIA_*`, `MOVIOLA_*`), which those tools
    ///   read, and never one shaped like a secret;
    /// - the agents' folders as the surface has them, or none, so that the transcript is looked for
    ///   where the child writes it: a session may start the other agent too;
    /// - each secret the user named for this service in the settings, if it is set; nothing else
    ///   shaped like a secret, whatever its prefix.
    ///
    /// The variables that bind a process to the session that started it are removed by name too.
    pub fn child_environment(&self, service: &AgentService, request: &LaunchRequest) -> (Vec<(String, OsString)>, Vec<String>) {
        let prefixes = [prototype_env_var(""), env_var(""), "MOVIOLA_".to_owned()];
        let mut set: Vec<(String, OsString)> = vec![("PYTHONUTF8".to_owned(), "1".into())];
        let mut removed = Vec::new();
        for (name, value) in self.env.pairs() {
            let Some(name) = name.to_str() else { continue };
            let upper = name.to_ascii_uppercase();
            if prefixes.iter().any(|prefix| upper.starts_with(prefix.as_str())) && !secret_shaped(name) && !value.is_empty() {
                set.push((name.to_owned(), value.to_os_string()));
            }
        }
        for folder in AGENT_FOLDERS {
            match self.env.get(folder) {
                Some(value) => set.push((folder.to_owned(), value.to_os_string())),
                None => removed.push(folder.to_owned()),
            }
        }
        for name in &request.secret_env {
            if let Some(value) = self.env.get(name) {
                set.push((name.clone(), value.to_os_string()));
            }
        }
        removed.extend(service.scrub(&self.env));
        (set, removed)
    }

    fn environment(&self, launch: Launch, service: &AgentService, request: &LaunchRequest) -> Launch {
        let (set, removed) = self.child_environment(service, request);
        let launch = removed.iter().fold(launch, |launch, name| launch.without(name));
        set.iter().fold(launch, |launch, (name, value)| launch.env(name, value))
    }

    /// The first line of the stream that names the session. A process that ends before saying it, or
    /// that does not say it in time, did not start: the error carries the tail of the log.
    fn wait_for_session(&self, service: &AgentService, detached: &mut Detached, log: &Path) -> Result<String, LaunchError> {
        let deadline = Instant::now() + self.waits.session;
        let mut read = 0;
        loop {
            if let Some(session) = scan(service, log, &mut read, false) {
                return Ok(session);
            }
            let status = detached.try_wait().map_err(|error| LaunchError::AfterEffect(error.to_string()))?;
            if let Some(status) = status {
                if let Some(session) = scan(service, log, &mut read, true) {
                    return Ok(session);
                }
                let code = status.code().map_or_else(|| status.to_string(), |code| code.to_string());
                return Err(LaunchError::AfterEffect(format!(
                    "{} exited with code {code} without opening a session. Tail of the log {}:\n{}",
                    service.id(),
                    log.display(),
                    tail(log)
                )));
            }
            if Instant::now() > deadline {
                return Err(LaunchError::AfterEffect(format!(
                    "{} did not open a session in {} s (pid {} still running). Tail of the log {}:\n{}",
                    service.id(),
                    self.waits.session.as_secs(),
                    detached.pid(),
                    log.display(),
                    tail(log)
                )));
            }
            thread::sleep(self.waits.tick);
        }
    }

    /// The session's transcript, which the service writes a moment after the first line: waited for
    /// while the process lives. Claude's expected path when it is not there yet; none for codex,
    /// whose file name holds the time the session started.
    fn wait_for_transcript(&self, service: &AgentService, detached: &mut Detached, folder: &Path, session: &str) -> (Option<String>, bool) {
        let deadline = Instant::now() + self.waits.transcript;
        loop {
            if let Some(path) = service.transcript(&self.env, folder, session) {
                return (Some(path.display().to_string()), true);
            }
            let running = matches!(detached.try_wait(), Ok(None));
            if !running || Instant::now() > deadline {
                break;
            }
            thread::sleep(self.waits.tick);
        }
        (service.expected_transcript(&self.env, folder, session).map(|path| path.display().to_string()), false)
    }

    /// The prototype's `sveglia_claude`: [`claude::wake`] in the state folder, with claude's allow-list
    /// and its folder as the surface has it, waited for up to [`Waits::wake`]. The token's expiry is
    /// not read here ([`WakeOutcome::with_expiry`]).
    fn wake_up(&self) -> Result<WakeOutcome, String> {
        let service = AgentService::known(ServiceId::Claude);
        let pin = self.pin(&service).map_err(|error| error.to_string())?;
        let folder = state_folder(self.state.as_deref()).map_err(|error| error.to_string())?;
        fs::create_dir_all(&folder).map_err(|error| format!("cannot create {}: {error}", folder.display()))?;
        let launch = pin.launch(claude::wake(), &folder).stdin(WAKE_PROMPT).limits(Limits { timeout: self.waits.wake, ..Limits::default() });
        let launch = match self.env.get(service.home_env()) {
            Some(home) => launch.env(service.home_env(), home),
            None => launch.without(service.home_env()),
        };
        let launch = service.scrub(&self.env).iter().fold(launch, |launch, name| launch.without(name));
        let output = launch.run().map_err(|error| error.to_string())?;
        Ok(wake_outcome(output.status.code(), &String::from_utf8_lossy(&output.stdout), &String::from_utf8_lossy(&output.stderr)))
    }
}

impl LaunchSurface for Cli {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Cli
    }

    fn launch(&self, request: &LaunchRequest) -> Result<Receipt, LaunchError> {
        let service = AgentService::known(request.service);
        let pin = self.pin(&service)?;
        let files = Files::reserve(&steps_folder(self.state.as_deref(), &request.label)?, &request.prompt)?;
        let args = match request.service {
            ServiceId::Claude => claude::print(request.model.as_ref(), request.effort),
            ServiceId::Codex => {
                let absolute = |path: &Path| AbsolutePath::new(path).map_err(|error| LaunchError::before(error.to_string()));
                codex::exec(&absolute(&request.folder)?, &absolute(&files.last)?, request.model.as_ref(), request.effort)
            }
        };
        let launch = self.environment(pin.launch(args, &request.folder), &service, request);
        let mut receipt = Receipt::new(SurfaceKind::Cli, request);
        let mut detached = launch.detach(&files.prompt, &files.log).map_err(|error| LaunchError::before(error.to_string()))?;
        // From here on something runs: a failure is after an effect.
        let session = self.wait_for_session(&service, &mut detached, &files.log)?;
        let (transcript, exists) = self.wait_for_transcript(&service, &mut detached, &request.folder, &session);
        receipt.resume = Some(service.resume(&session));
        receipt.session = Some(session);
        receipt.transcript = transcript;
        receipt.transcript_exists = exists;
        receipt.pid = Some(detached.pid());
        receipt.log = Some(files.log.display().to_string());
        receipt.prompt_file = Some(files.prompt.display().to_string());
        Ok(receipt)
    }

    /// The real wake-up. What fails before the run, such as claude missing, is in the outcome too.
    fn wake(&self) -> WakeOutcome {
        let started = Instant::now();
        let mut outcome = self.wake_up().unwrap_or_else(|error| WakeOutcome::failed(error.chars().take(WAKE_ERROR_CHARS).collect::<String>()));
        outcome.duration_s = Some((started.elapsed().as_secs_f64() * 10.0).round() / 10.0);
        outcome
    }
}

/// The wake-up's run as the prototype reads it: the answer is the last line of stdout that is a JSON
/// object, and the wake-up worked on exit 0 with `is_error` false. Otherwise the error is the end of
/// the answer's `result`, of stderr or of stdout, the first that is not empty.
fn wake_outcome(code: Option<i32>, stdout: &str, stderr: &str) -> WakeOutcome {
    let answer = stdout
        .lines()
        .rev()
        .filter(|line| line.starts_with('{'))
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
        .and_then(|answer| answer.as_object().cloned())
        .unwrap_or_default();
    let ok = code == Some(0) && answer.get("is_error") == Some(&Value::Bool(false));
    let error = (!ok).then(|| {
        let result = answer.get("result").and_then(Value::as_str).unwrap_or_default();
        let text = [result, stderr, stdout].into_iter().find(|text| !text.is_empty()).map_or_else(
            || format!("exit {}", code.map_or_else(|| "without a code".to_owned(), |code| code.to_string())),
            str::to_owned,
        );
        let text = text.trim();
        text.chars().skip(text.chars().count().saturating_sub(WAKE_ERROR_CHARS)).collect()
    });
    WakeOutcome { ok, exit: code, cost_usd: answer.get("total_cost_usd").and_then(Value::as_f64), error, ..WakeOutcome::default() }
}

/// `<state>/passi/<project>`, created; never inside the temporary folder, which Storage Sense
/// empties. The refusal comes before anything is created.
pub(crate) fn steps_folder(state: Option<&Path>, label: &Label) -> Result<PathBuf, LaunchError> {
    let folder = state_folder(state)?.join(STEPS).join(label.as_str());
    fs::create_dir_all(&folder).map_err(|error| LaunchError::before(format!("cannot create {}: {error}", folder.display())))?;
    Ok(folder)
}

/// The state folder, absolute, so the log and codex's last message name one place whatever the
/// child's folder; refused inside the temporary folder.
pub(crate) fn state_folder(state: Option<&Path>) -> Result<PathBuf, LaunchError> {
    let variable = prototype_env_var("RUNTIME");
    let Some(state) = state else {
        return Err(LaunchError::before(format!("no state folder: set {variable}")));
    };
    let temporary = temporary_folder();
    if is_within(state, &temporary) {
        return Err(LaunchError::before(format!(
            "the state folder {} is inside the temporary folder {}, which Storage Sense empties: choose another one with {variable}",
            state.display(),
            temporary.display()
        )));
    }
    Ok(std::path::absolute(state).unwrap_or_else(|_| state.to_path_buf()))
}

/// The files of a launch, named by the local time to the second, as the prototype names them; a
/// second launch in the same second takes `-2`, then `-3`. Creating the prompt file reserves the
/// name, so two launches never share one.
pub(crate) struct Files {
    pub(crate) log: PathBuf,
    /// The prompt, byte for byte, so no line break changes: the child's stdin.
    pub(crate) prompt: PathBuf,
    /// Codex's last message.
    pub(crate) last: PathBuf,
}

impl Files {
    pub(crate) fn reserve(folder: &Path, prompt_text: &str) -> Result<Files, LaunchError> {
        let failed = |path: &Path, error: std::io::Error| LaunchError::before(format!("cannot write {}: {error}", path.display()));
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        for n in 1..1000 {
            let name = if n == 1 { stamp.clone() } else { format!("{stamp}-{n}") };
            let log = folder.join(format!("{name}.log"));
            let prompt = folder.join(format!("{name}.prompt.md"));
            if log.exists() {
                continue;
            }
            match OpenOptions::new().write(true).create_new(true).open(&prompt) {
                Ok(mut file) => {
                    file.write_all(prompt_text.as_bytes()).map_err(|error| failed(&prompt, error))?;
                    return Ok(Files { log, prompt, last: folder.join(format!("{name}.ultimo.txt")) });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(failed(&prompt, error)),
            }
        }
        Err(LaunchError::before(format!("no free name for a log in {}", folder.display())))
    }
}

/// The session named by the complete lines of the log past `read`, which advances; with `last`, the
/// line still without its line break counts too, because the process has ended.
fn scan(service: &AgentService, log: &Path, read: &mut usize, last: bool) -> Option<String> {
    let bytes = fs::read(log).ok()?;
    let fresh = bytes.get(*read..)?;
    let complete = if last { fresh.len() } else { fresh.iter().rposition(|byte| *byte == b'\n').map_or(0, |at| at + 1) };
    let found = String::from_utf8_lossy(&fresh[..complete]).lines().find_map(|line| service.session_from_line(line));
    *read += complete;
    found
}

/// The last 15 lines of the log, 300 characters each at most, as the prototype quotes them.
fn tail(log: &Path) -> String {
    let text = fs::read(log).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(15);
    lines[start..].iter().map(|line| line.chars().take(300).collect::<String>()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTF-8 mode always, the prototype's variables without their secrets, a secret only by name, the
    /// folders as the surface has them, and the bindings to the parent session out by name.
    #[test]
    fn the_child_environment_follows_its_rule() {
        let env = Env::from_pairs([
            ("REGIA_HOME", "/d"),
            ("REGIA_X_TOKEN", "canary"),
            ("MOVIOLA_TASKS", "t"),
            ("ANTHROPIC_API_KEY", "canary"),
            ("OPENAI_API_KEY", "named"),
            ("CODEX_HOME", "/home/x/.codex"),
            ("CLAUDE_CODE_SESSION_ID", "parent"),
        ]);
        let cli = Cli::new(env);
        let folder = std::env::current_dir().unwrap();
        let request = LaunchRequest::new(ServiceId::Codex, "trial", &folder, "p").unwrap().secret_env(vec!["OPENAI_API_KEY".to_owned()]);
        let (set, removed) = cli.child_environment(&AgentService::known(ServiceId::Codex), &request);
        let set: BTreeMap<&str, &OsString> = set.iter().map(|(name, value)| (name.as_str(), value)).collect();
        assert_eq!(set.get("PYTHONUTF8").map(|value| value.to_str()), Some(Some("1")));
        for passed in ["REGIA_HOME", "MOVIOLA_TASKS", "CODEX_HOME", "OPENAI_API_KEY"] {
            assert!(set.contains_key(passed), "{passed}");
        }
        for absent in ["REGIA_X_TOKEN", "ANTHROPIC_API_KEY", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CONFIG_DIR"] {
            assert!(!set.contains_key(absent), "{absent}");
        }
        assert!(removed.contains(&"CLAUDE_CONFIG_DIR".to_owned()), "a folder the surface does not have is not inherited");
        assert!(removed.contains(&"CLAUDE_CODE_SESSION_ID".to_owned()));
    }

    /// The last JSON object of stdout is the answer: a line after it that does not parse, a note
    /// before it, CRLF and a cost that is not a number change nothing. A failure keeps the last 300
    /// characters of the first text that explains it.
    #[test]
    fn the_answer_of_the_wake_up_is_its_last_json_line() {
        let ok = r#"{"type":"result","is_error":false,"result":"ok","total_cost_usd":0.00015}"#;
        let outcome = wake_outcome(Some(0), &format!("a note\r\n{{\"is_error\":true}}\r\n{ok}\r\n{{not json\r\n"), "");
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!((outcome.exit, outcome.cost_usd, outcome.error), (Some(0), Some(0.00015), None));
        assert!(!wake_outcome(Some(1), ok, "").ok, "an exit other than 0 is a failure");
        assert!(!wake_outcome(Some(0), r#"{"result":"ok"}"#, "").ok, "is_error must be false, not missing");
        assert!(!wake_outcome(Some(0), "[1]\n\"ok\"", "").ok, "only an object is an answer");
        assert_eq!(wake_outcome(Some(0), r#"{"is_error":false,"total_cost_usd":true}"#, "").cost_usd, None);

        let long = format!("{}{}", "x".repeat(400), "é".repeat(100));
        let expired = serde_json::json!({"is_error": true, "result": format!("  {long}  ")}).to_string();
        let error = wake_outcome(Some(1), &expired, "on stderr").error.unwrap();
        assert_eq!(error.chars().count(), 300);
        assert!(error.ends_with(&"é".repeat(100)) && error.starts_with('x'), "the end, trimmed: {error}");
        assert_eq!(wake_outcome(Some(1), "on stdout\n", "  on stderr\n").error.as_deref(), Some("on stderr"));
        assert_eq!(wake_outcome(Some(1), "on stdout\n", "").error.as_deref(), Some("on stdout"));
        assert_eq!(wake_outcome(Some(3), "", "").error.as_deref(), Some("exit 3"));
        assert_eq!(wake_outcome(None, "", "").error.as_deref(), Some("exit without a code"));
    }

    /// The rule is `regia-store`'s, not a copy: the state folder is refused wherever the store's own
    /// rule says it is in the temporary folder, even before it exists and through dots, and accepted
    /// outside it.
    #[test]
    fn the_state_folder_follows_the_stores_temporary_folder_rule() {
        let temporary = temporary_folder();
        for inside in [temporary.join("not-yet").join("deeper"), temporary.clone(), temporary.join("a").join("..").join("b")] {
            let refused = state_folder(Some(&inside)).unwrap_err();
            assert!(refused.before_effect() && refused.to_string().contains("temporary folder"), "{}: {refused}", inside.display());
        }
        let here = std::env::current_dir().unwrap();
        assert!(!is_within(&here, &temporary), "the test must not run in the temporary folder");
        assert_eq!(state_folder(Some(&here)).unwrap(), here);
        assert!(state_folder(None).unwrap_err().to_string().contains("no state folder"));
    }
}
