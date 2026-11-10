//! Agent services and launch surfaces (ADR 0001 §6). Two ports, because the prototype already keeps
//! the two questions apart:
//!
//! - [`AgentService`] says **which agent**: detect it, its sign-in state and guide, describe a launch,
//!   resume, classify the end, and scrub the child's environment.
//! - [`LaunchSurface`] says **where it runs**: `Cli` (a detached process with a log) and [`Dry`]
//!   (starts nothing). They meet at a neutral [`LaunchRequest`], never at an argv.
//!
//! Which service, tier, model and effort a role gets comes from three layers ([`Policy`]): the
//! product's defaults, each service's file, and the user's settings.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use regia_exec::{ExecError, ModelId, Pin, Program};
use serde::{Deserialize, Serialize};

mod gates;
mod launch;
mod policy;

pub use gates::{Gate, GateError, Gates};
pub use launch::{
    Dry, Label, LaunchError, LaunchRequest, LaunchSurface, Receipt, SurfaceKind, launch_with_fallback,
};
pub use policy::{Assignment, Diversity, Fallback, Policy, Role, Settings, SettingsError, Tier};

/// The agent services the product knows. Only these two: every other one needs code of its own,
/// for its argv, its stream, its transcript and its usage (ADR 0001 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceId {
    Claude,
    Codex,
}

impl ServiceId {
    pub const ALL: [ServiceId; 2] = [ServiceId::Claude, ServiceId::Codex];

    pub fn id(self) -> &'static str {
        match self {
            ServiceId::Claude => "claude",
            ServiceId::Codex => "codex",
        }
    }

    pub fn parse(text: &str) -> Option<ServiceId> {
        ServiceId::ALL.into_iter().find(|service| service.id() == text)
    }

    pub fn program(self) -> Program {
        match self {
            ServiceId::Claude => Program::Claude,
            ServiceId::Codex => Program::Codex,
        }
    }
}

impl fmt::Display for ServiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// A snapshot of an environment, which the services read for their folders and the names the user
/// passes on. The product takes this process's; a test hands its own.
#[derive(Debug, Clone, Default)]
pub struct Env(Vec<(OsString, OsString)>);

impl Env {
    pub fn of_this_process() -> Env {
        Env(std::env::vars_os().collect())
    }

    pub fn from_pairs(pairs: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>) -> Env {
        Env(pairs.into_iter().map(|(name, value)| (name.into(), value.into())).collect())
    }

    /// Sets or replaces a variable.
    #[must_use]
    pub fn with(mut self, name: &str, value: impl Into<OsString>) -> Env {
        self.0.retain(|(other, _)| !same_name(other, name));
        self.0.push((name.into(), value.into()));
        self
    }

    /// An empty value counts as unset, as the prototype has it.
    pub fn get(&self, name: &str) -> Option<&std::ffi::OsStr> {
        self.0.iter().find(|(other, value)| same_name(other, name) && !value.is_empty()).map(|(_, value)| value.as_os_str())
    }

    pub fn names(&self) -> impl Iterator<Item = &std::ffi::OsStr> {
        self.0.iter().map(|(name, _)| name.as_os_str())
    }

    fn home(&self) -> Option<PathBuf> {
        let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        self.get(name).map(PathBuf::from)
    }
}

/// Names compare without case on Windows, as its environment does.
fn same_name(name: &std::ffi::OsStr, wanted: &str) -> bool {
    name.to_str().is_some_and(|name| if cfg!(windows) { name.eq_ignore_ascii_case(wanted) } else { name == wanted })
}

/// Prefixes of the variables that bind a process to the session that started it, or to the Orca app
/// that hosts that session. A controller started under an agent must not hand them to its children.
const SESSION_BINDINGS: [&str; 9] = [
    "ORCA_",
    "CLAUDE_CODE_",
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CLAUDE_AGENT_SDK_",
    "CLAUDE_PLUGIN_",
    "CODEX_COMPANION_",
    "AI_AGENT",
];

/// A service's file, layer 2 of the policy.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceFile {
    service: ServiceId,
    launch: String,
    stream: String,
    transcript: String,
    home_env: String,
    home_default: String,
    credentials: String,
    login: Vec<String>,
    skills: String,
    tiers: std::collections::BTreeMap<Tier, String>,
}

/// The rules the code has for each service, which its file must name.
fn rules(service: ServiceId) -> [&'static str; 3] {
    match service {
        ServiceId::Claude => ["claude_print", "claude_stream_json", "claude_projects"],
        ServiceId::Codex => ["codex_exec", "codex_json", "codex_sessions"],
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("the file of {service} does not parse: {reason}")]
    Parse { service: ServiceId, reason: String },
    #[error("the file of {service} names rules the code does not have for it")]
    Rules { service: ServiceId },
    #[error("the file of {service} maps a tier to an invalid model")]
    Model { service: ServiceId },
}

/// Whether the user has signed in to a service, judged by the presence of its credentials file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    SignedIn,
    SignedOut,
    Unknown,
}

/// How a run ended, from its exit code and whether it opened a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "end", content = "code")]
pub enum End {
    /// Exit 0 after opening a session.
    Finished,
    /// A session was opened, and the run then failed.
    Failed(Option<i32>),
    /// The run ended without ever opening a session: it did not start.
    NotStarted(Option<i32>),
}

/// One agent service: what the code knows about it, plus its file.
#[derive(Debug, Clone)]
pub struct AgentService {
    file: ServiceFile,
    tiers: std::collections::BTreeMap<Tier, ModelId>,
}

impl AgentService {
    fn text(service: ServiceId) -> &'static str {
        match service {
            ServiceId::Claude => include_str!("../services/claude.toml"),
            ServiceId::Codex => include_str!("../services/codex.toml"),
        }
    }

    /// The service with its compiled file.
    pub fn known(service: ServiceId) -> AgentService {
        AgentService::from_toml(service, AgentService::text(service)).expect("the compiled service files are valid")
    }

    /// A service from the text of its file, which must be the file of that service.
    pub fn from_toml(service: ServiceId, text: &str) -> Result<AgentService, ServiceError> {
        let file: ServiceFile =
            toml::from_str(text).map_err(|error| ServiceError::Parse { service, reason: error.message().to_owned() })?;
        let [launch, stream, transcript] = rules(service);
        if file.service != service || file.launch != launch || file.stream != stream || file.transcript != transcript {
            return Err(ServiceError::Rules { service });
        }
        let mut tiers = std::collections::BTreeMap::new();
        for (tier, model) in &file.tiers {
            tiers.insert(*tier, ModelId::parse(model).map_err(|_| ServiceError::Model { service })?);
        }
        Ok(AgentService { file, tiers })
    }

    pub fn id(&self) -> ServiceId {
        self.file.service
    }

    pub fn program(&self) -> Program {
        self.id().program()
    }

    /// Detects the service: its program, resolved once and pinned. Nothing is started.
    pub fn detect(&self) -> Result<Pin, ExecError> {
        Pin::resolve(self.program())
    }

    /// The model the service's file maps a tier to, if any.
    pub fn model_for(&self, tier: Tier) -> Option<&ModelId> {
        self.tiers.get(&tier)
    }

    /// The variable that moves the service's folder, as the child must see it.
    pub fn home_env(&self) -> &str {
        &self.file.home_env
    }

    /// The service's folder: its variable when set, otherwise its folder in the home.
    pub fn home(&self, env: &Env) -> Option<PathBuf> {
        env.get(&self.file.home_env).map(PathBuf::from).or_else(|| env.home().map(|home| home.join(&self.file.home_default)))
    }

    /// Where the user's skills for this service live.
    pub fn skills_dir(&self, env: &Env) -> Option<PathBuf> {
        self.home(env).map(|home| home.join(&self.file.skills))
    }

    /// Signed in when the credentials file is there. Its contents are never read.
    pub fn auth_state(&self, env: &Env) -> AuthState {
        if self.id() == ServiceId::Claude && cfg!(target_os = "macos") {
            return AuthState::Unknown;
        }
        match self.home(env) {
            Some(home) if home.join(&self.file.credentials).is_file() => AuthState::SignedIn,
            Some(_) => AuthState::SignedOut,
            None => AuthState::Unknown,
        }
    }

    /// What the user runs to sign in, word by word, for a guide to show. Nothing runs it.
    pub fn login_guide(&self) -> &[String] {
        &self.file.login
    }

    /// The command that resumes a session, run in the folder of the launch.
    pub fn resume(&self, session: &str) -> String {
        match self.id() {
            ServiceId::Claude => format!("claude --resume {session}"),
            ServiceId::Codex => format!("codex resume {session}"),
        }
    }

    /// How a run ended.
    pub fn classify_end(&self, code: Option<i32>, opened_a_session: bool) -> End {
        match (opened_a_session, code) {
            (false, code) => End::NotStarted(code),
            (true, Some(0)) => End::Finished,
            (true, code) => End::Failed(code),
        }
    }

    /// The variables of `env` the child must not see: those binding it to the session that started
    /// it. The allow-list of regia-exec already leaves them out; this keeps them out by name too.
    pub fn scrub(&self, env: &Env) -> Vec<String> {
        env.names()
            .filter_map(|name| name.to_str())
            .filter(|name| {
                let upper = name.to_ascii_uppercase();
                SESSION_BINDINGS.iter().any(|prefix| upper.starts_with(prefix))
            })
            .map(str::to_owned)
            .collect()
    }

    /// The session id a line of the stream carries, if any.
    pub fn session_from_line(&self, line: &str) -> Option<String> {
        let event: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        let id = match self.id() {
            ServiceId::Claude => event.get("session_id"),
            ServiceId::Codex if event.get("type").and_then(|kind| kind.as_str()) == Some("thread.started") => {
                event.get("thread_id")
            }
            ServiceId::Codex => None,
        };
        id.and_then(|id| id.as_str()).filter(|id| !id.is_empty()).map(str::to_owned)
    }

    /// Where the transcript of a session is, if it is there yet.
    pub fn transcript(&self, env: &Env, folder: &Path, session: &str) -> Option<PathBuf> {
        let home = self.home(env)?;
        match self.id() {
            ServiceId::Claude => {
                let expected = self.expected_transcript(env, folder, session)?;
                if expected.is_file() {
                    return Some(expected);
                }
                let wanted = format!("{session}.jsonl");
                children(&home.join("projects")).into_iter().map(|dir| dir.join(&wanted)).find(|path| path.is_file())
            }
            ServiceId::Codex => {
                let suffix = format!("-{session}.jsonl");
                let mut found = None;
                for year in children(&home.join("sessions")) {
                    for month in children(&year) {
                        for day in children(&month) {
                            found = found.or_else(|| {
                                children(&day).into_iter().find(|file| {
                                    file.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
                                        name.starts_with("rollout-") && name.ends_with(&suffix)
                                    })
                                })
                            });
                        }
                    }
                }
                found
            }
        }
    }

    /// Claude's transcript path for a session, before the file exists. Codex has none: its name holds
    /// the time the session started.
    pub fn expected_transcript(&self, env: &Env, folder: &Path, session: &str) -> Option<PathBuf> {
        (self.id() == ServiceId::Claude)
            .then(|| self.home(env))
            .flatten()
            .map(|home| home.join("projects").join(claude_slug(folder)).join(format!("{session}.jsonl")))
    }
}

/// Claude Code's folder name for a working folder: every character outside `[A-Za-z0-9]` becomes `-`.
pub fn claude_slug(folder: &Path) -> String {
    folder.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn children(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> =
        std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).map(|entry| entry.path()).collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiled_service_files_parse_and_name_their_own_rules() {
        for service in ServiceId::ALL {
            let agent = AgentService::known(service);
            assert_eq!(agent.id(), service);
            assert_eq!(agent.program().name(), service.id());
        }
        assert_eq!(AgentService::known(ServiceId::Claude).model_for(Tier::Deep).unwrap().as_str(), "opus");
        assert!(AgentService::known(ServiceId::Codex).model_for(Tier::Standard).is_none());
        // A file cannot borrow another service's rules: those are code.
        let swapped = AgentService::text(ServiceId::Claude).replace("claude_print", "codex_exec");
        assert!(matches!(AgentService::from_toml(ServiceId::Claude, &swapped), Err(ServiceError::Rules { .. })));
        let other = AgentService::text(ServiceId::Codex);
        assert!(matches!(AgentService::from_toml(ServiceId::Claude, other), Err(ServiceError::Rules { .. })));
        let bad_model = AgentService::text(ServiceId::Claude).replace("\"opus\"", "\"--x\"");
        assert!(matches!(AgentService::from_toml(ServiceId::Claude, &bad_model), Err(ServiceError::Model { .. })));
    }

    #[test]
    fn the_stream_gives_the_session_as_the_prototype_reads_it() {
        let claude = AgentService::known(ServiceId::Claude);
        let codex = AgentService::known(ServiceId::Codex);
        let sid = "11111111-2222-3333-4444-555555555555";
        let init = format!(r#"{{"type":"system","subtype":"init","session_id":"{sid}"}}"#);
        assert_eq!(claude.session_from_line(&init).as_deref(), Some(sid));
        assert_eq!(claude.session_from_line("Error: not JSON"), None);
        assert_eq!(claude.session_from_line(r#"{"session_id":""}"#), None);
        assert_eq!(claude.session_from_line("[1]"), None);
        let started = format!(r#"{{"type":"thread.started","thread_id":"{sid}"}}"#);
        assert_eq!(codex.session_from_line(&started).as_deref(), Some(sid));
        assert_eq!(codex.session_from_line(&format!(r#"{{"type":"turn.started","thread_id":"{sid}"}}"#)), None);
        assert_eq!(claude.resume(sid), format!("claude --resume {sid}"));
        assert_eq!(codex.resume(sid), format!("codex resume {sid}"));
    }

    #[test]
    fn the_end_of_a_run_is_classified() {
        let claude = AgentService::known(ServiceId::Claude);
        assert_eq!(claude.classify_end(Some(0), true), End::Finished);
        assert_eq!(claude.classify_end(Some(1), true), End::Failed(Some(1)));
        assert_eq!(claude.classify_end(Some(2), false), End::NotStarted(Some(2)));
        assert_eq!(claude.classify_end(None, true), End::Failed(None));
    }

    #[test]
    fn the_scrub_names_what_binds_a_child_to_its_parent_session() {
        let env = Env::from_pairs([
            ("ORCA_TERMINAL_HANDLE", "x"),
            ("CLAUDE_CODE_SESSION_ID", "parent"),
            ("CLAUDECODE", "1"),
            ("CODEX_COMPANION_X", "1"),
            ("CLAUDE_CONFIG_DIR", "/home/x/.claude"),
            ("PATH", "/bin"),
        ]);
        let mut scrubbed = AgentService::known(ServiceId::Claude).scrub(&env);
        scrubbed.sort();
        assert_eq!(scrubbed, ["CLAUDECODE", "CLAUDE_CODE_SESSION_ID", "CODEX_COMPANION_X", "ORCA_TERMINAL_HANDLE"]);
    }

    #[test]
    fn folders_and_sign_in_follow_the_variable_then_the_home() {
        let root = std::env::current_dir().unwrap().join("no-such-home");
        let claude = AgentService::known(ServiceId::Claude);
        let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let env = Env::from_pairs([(home_var, root.as_os_str())]);
        assert_eq!(claude.home(&env), Some(root.join(".claude")));
        let moved = env.clone().with("CLAUDE_CONFIG_DIR", root.join("moved"));
        assert_eq!(claude.home(&moved), Some(root.join("moved")));
        assert_eq!(claude.home(&env.clone().with("CLAUDE_CONFIG_DIR", "")), Some(root.join(".claude")));
        let codex = AgentService::known(ServiceId::Codex);
        assert_eq!(codex.auth_state(&env), AuthState::SignedOut);
        assert_eq!(codex.auth_state(&Env::default()), AuthState::Unknown);
        assert_eq!(codex.login_guide(), ["codex", "login"]);
        let slug = claude_slug(Path::new("C:\\Users\\a b\\x"));
        assert_eq!(slug, "C--Users-a-b-x");
    }
}
