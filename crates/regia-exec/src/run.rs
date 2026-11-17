//! A run: environment, folder, input, limits, and the process tree (threat model §3.6, rules 3–6).

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::{Args, ExecError, Pin, Program, tree};

/// Inherited by every program, when the parent has them: what a tool needs to find its own files,
/// the user's locale and the network's proxies. Compared without case on Windows.
const BASE: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LANGUAGE", "LC_ALL", "LC_CTYPE", "LC_MESSAGES", "TZ", "TMPDIR",
    "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS", "SYSTEMROOT", "SYSTEMDRIVE", "WINDIR", "COMSPEC", "PATHEXT", "TEMP", "TMP",
    "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "PROGRAMFILES",
    "PROGRAMFILES(X86)", "PROGRAMW6432", "COMMONPROGRAMFILES", "COMMONPROGRAMFILES(X86)", "COMMONPROGRAMW6432",
    "OS", "PROCESSOR_ARCHITECTURE", "NUMBER_OF_PROCESSORS", "USERNAME", "USERDOMAIN", "COMPUTERNAME",
    "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "ALL_PROXY", "https_proxy", "http_proxy", "no_proxy", "all_proxy",
    "SSL_CERT_FILE", "SSL_CERT_DIR",
];

/// What the prototype reads besides its own `REGIA_*` and `MOVIOLA_*`: the agents' folders and the
/// session ids its declarations use.
const PROTOTYPE: &[&str] =
    &["CLAUDE_CONFIG_DIR", "CODEX_HOME", "CLAUDE_CODE_SESSION_ID", "ORCA_AGENT_SESSION_ID", "CODEX_THREAD_ID"];

/// Set for `gh`, and for the prototype, which calls it (T22): its own host, no prompt, no update
/// check, no pager. `GH_TOKEN`, `GITHUB_TOKEN`, `GH_DEBUG`, `GH_BROWSER`, `GH_EDITOR` and `GH_PATH`
/// are simply not on the list.
const GH: &[(&str, &str)] =
    &[("GH_HOST", "github.com"), ("GH_PROMPT_DISABLED", "1"), ("GH_NO_UPDATE_NOTIFIER", "1"), ("GH_PAGER", "cat")];

/// Set for every `git` call: no prompt, for the user or for a credential helper.
const GIT: &[(&str, &str)] = &[("GIT_TERMINAL_PROMPT", "0"), ("GCM_INTERACTIVE", "never")];

/// Set for the `git` calls that read, and for those only (rule 8, and the prototype's `_git`): no
/// optional lock, literal pathspecs and no fsmonitor. `GIT_CONFIG_*` turns fsmonitor off without a `-c`
/// in argv. A write ([`Launch::writing`]) does not get them, because they would reach the user's hooks
/// and every `git` a hook starts: a hook with a glob would change its meaning.
const GIT_READS: &[(&str, &str)] = &[
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_LITERAL_PATHSPECS", "1"),
    ("GIT_CONFIG_COUNT", "1"),
    ("GIT_CONFIG_KEY_0", "core.fsmonitor"),
    ("GIT_CONFIG_VALUE_0", "false"),
];

/// Inherited by `git` and by no other program: the agent's socket, which a push over SSH asks, and the
/// folder of gpg, which a signed commit reads. A socket and a folder are not secrets (rule 4).
const GIT_USER: &[&str] = &["SSH_AUTH_SOCK", "GNUPGHOME"];

/// A secret passes only when a caller names it with [`Launch::env`], as the settings of an agent
/// do (rule 4). A caller that forwards variables by prefix leaves these out too.
pub fn secret_shaped(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    ["_TOKEN", "_KEY", "_SECRET", "_PASSWORD"].iter().any(|suffix| name.ends_with(suffix))
}

fn key(name: &OsStr) -> Option<String> {
    name.to_str().map(|name| if cfg!(windows) { name.to_ascii_uppercase() } else { name.to_owned() })
}

fn same(a: &OsStr, b: &OsStr) -> bool {
    key(a).is_some_and(|a| key(b).is_some_and(|b| a == b))
}

fn inherits(program: Program, name: &OsStr) -> bool {
    let Some(name) = key(name) else { return false };
    if secret_shaped(&name) {
        return false;
    }
    let prefix = regia_identity::prototype_env_var("");
    BASE.contains(&name.as_str())
        || match program {
            Program::Gh => name == "GH_CONFIG_DIR",
            Program::Python => {
                name.starts_with(&prefix) || name.starts_with("MOVIOLA_") || PROTOTYPE.contains(&name.as_str())
            }
            Program::Claude => name == "CLAUDE_CONFIG_DIR",
            Program::Codex => name == "CODEX_HOME",
            Program::Git => GIT_USER.contains(&name.as_str()),
            Program::Orca => false,
        }
}

fn forced(program: Program, writes: bool) -> Vec<(&'static str, &'static str)> {
    let list: &[&[(&str, &str)]] = match program {
        Program::Gh | Program::Python => &[GH],
        Program::Git if writes => &[GIT],
        Program::Git => &[GIT, GIT_READS],
        Program::Claude | Program::Codex | Program::Orca => &[],
    };
    list.concat()
}

/// Deadline and output caps of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub timeout: Duration,
    pub stdout: usize,
    pub stderr: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { timeout: Duration::from_secs(120), stdout: 32 << 20, stderr: 1 << 20 }
    }
}

pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Lengths only: what a program printed can be private, and `{:?}` ends up in logs (T10).
impl fmt::Debug for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Output")
            .field("status", &self.status)
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .finish()
    }
}

#[derive(Debug, Clone)]
enum Target {
    Program(Pin),
    #[cfg(feature = "harness")]
    Harness(PathBuf),
}

/// A run, described and not started. [`Launch::run`] starts it.
#[derive(Clone)]
pub struct Launch {
    target: Target,
    args: Vec<OsString>,
    cwd: PathBuf,
    stdin: Option<Vec<u8>>,
    set: Vec<(OsString, OsString)>,
    removed: Vec<OsString>,
    limits: Limits,
    writes: bool,
    parent: Option<Vec<(OsString, OsString)>>,
}

impl Launch {
    pub(crate) fn program(pin: Pin, args: Args, cwd: &Path) -> Self {
        Launch::new(Target::Program(pin), args.0, cwd)
    }

    #[cfg(feature = "harness")]
    pub(crate) fn harness(binary: &Path, args: &[String], cwd: &Path) -> Self {
        Launch::new(Target::Harness(binary.to_path_buf()), args.iter().map(OsString::from).collect(), cwd)
    }

    #[cfg(feature = "harness")]
    pub(crate) fn untyped(pin: Pin, args: &[String], cwd: &Path) -> Self {
        Launch::new(Target::Program(pin), args.iter().map(OsString::from).collect(), cwd)
    }

    fn new(target: Target, args: Vec<OsString>, cwd: &Path) -> Self {
        Launch {
            target,
            args,
            cwd: cwd.to_path_buf(),
            stdin: None,
            set: Vec::new(),
            removed: Vec::new(),
            limits: Limits::default(),
            writes: false,
            parent: None,
        }
    }

    /// The input, such as a prompt or a request body: never in argv. Without it stdin is empty.
    #[must_use]
    pub fn stdin(mut self, input: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(input.into());
        self
    }

    /// A variable named by the caller, set on top of the allow-list.
    #[must_use]
    pub fn env(mut self, name: &str, value: impl AsRef<OsStr>) -> Self {
        self.set.push((name.into(), value.as_ref().to_os_string()));
        self
    }

    /// A variable the program must not see, even if the allow-list would pass it.
    #[must_use]
    pub fn without(mut self, name: &str) -> Self {
        self.removed.push(name.into());
        self
    }

    #[must_use]
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// A `git` call that writes: commit, merge, push. It does not get the settings of a read (no optional
    /// lock, no literal pathspecs, no fsmonitor off), which would reach the user's hooks. Without it a
    /// call is a read. No other program reads it.
    #[must_use]
    pub fn writing(mut self) -> Self {
        self.writes = true;
        self
    }

    /// Test only: the environment the allow-list reads, in place of this process's (V18).
    #[cfg(feature = "harness")]
    #[must_use]
    pub fn parent_env(mut self, vars: Vec<(OsString, OsString)>) -> Self {
        self.parent = Some(vars);
        self
    }

    pub fn program_name(&self) -> String {
        match &self.target {
            Target::Program(pin) => pin.program().name().to_owned(),
            #[cfg(feature = "harness")]
            Target::Harness(path) => path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        }
    }

    /// The environment of an external program: the allow-list of its kind, then what it must have,
    /// then the caller's removals and additions.
    fn environment(&self, program: Program) -> Vec<(OsString, OsString)> {
        let parent = self.parent.clone().unwrap_or_else(|| env::vars_os().collect());
        let mut out: Vec<(OsString, OsString)> = parent.into_iter().filter(|(name, _)| inherits(program, name)).collect();
        let mut put = |name: &OsStr, value: Option<&OsStr>| {
            out.retain(|(other, _)| !same(other, name));
            if let Some(value) = value {
                out.push((name.to_os_string(), value.to_os_string()));
            }
        };
        for (name, value) in forced(program, self.writes) {
            put(OsStr::new(name), Some(OsStr::new(value)));
        }
        for name in &self.removed {
            put(name, None);
        }
        for (name, value) in &self.set {
            put(name, Some(value));
        }
        out
    }

    /// The program checked against its pin, with its arguments, folder and environment. The
    /// caller sets the pipes and the tree: [`tree::prepare`] for a run it follows, a detached start
    /// otherwise.
    fn command(&self) -> Result<Command, ExecError> {
        let (path, program) = match &self.target {
            Target::Program(pin) => {
                pin.check()?;
                (pin.path().to_path_buf(), Some(pin.program()))
            }
            #[cfg(feature = "harness")]
            Target::Harness(path) => (path.clone(), None),
        };
        if batch(&path) && !self.args.iter().all(|arg| safe_for_batch(arg)) {
            return Err(ExecError::UnsafeForBatch(self.program_name()));
        }
        let mut command = new_command(&path);
        command.args(&self.args).current_dir(&self.cwd);
        match program {
            Some(program) => {
                command.env_clear().envs(self.environment(program));
            }
            None => {
                for name in &self.removed {
                    command.env_remove(name);
                }
                command.envs(self.set.iter().map(|(name, value)| (name, value)));
            }
        }
        Ok(command)
    }

    /// Starts the program and waits for it. On the deadline, or past an output cap, the whole tree
    /// is killed; when the program ends, what it left running is killed too, except a process that
    /// left the tree on purpose (a new session on Unix, a breakaway on Windows).
    pub fn run(self) -> Result<Output, ExecError> {
        let label = self.program_name();
        let mut command = self.command()?;
        tree::prepare(&mut command);
        command
            .stdin(if self.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().map_err(|source| ExecError::Spawn { program: label.clone(), source })?;
        supervise(child, self.stdin, self.limits, label)
    }

    /// Starts the program on its own and returns at once, for an agent's session (ADR 0001 §6):
    /// stdin is the file `input`, and stdout and stderr go to the file `log`, created or emptied. It
    /// leaves the caller's console and terminal: on Windows no window, a process group of its own,
    /// and out of the caller's job where that job allows it; on Unix a session of its own. No
    /// deadline or output cap applies, a [`Launch::stdin`] is not used, and nothing kills the
    /// program when it ends or when the handle is dropped: [`Detached::kill`] does, on request.
    ///
    /// An error means that no process is running: everything that can fail is checked before the
    /// start, and a tree that cannot be set up afterwards leaves [`Detached::kill`] with the program
    /// alone.
    pub fn detach(self, input: &Path, log: &Path) -> Result<Detached, ExecError> {
        let label = self.program_name();
        let failed = |source| ExecError::Spawn { program: label.clone(), source };
        let mut command = self.command()?;
        let input = File::open(input).map_err(failed)?;
        let output = File::create(log).map_err(failed)?;
        let errors = output.try_clone().map_err(failed)?;
        command.stdin(input).stdout(output).stderr(errors);
        let mut child = tree::spawn_detached(&mut command).map_err(failed)?;
        let tree = match tree::Tree::adopt_detached(&child) {
            Ok(tree) => tree,
            Err(source) => {
                // Not started (on Windows it is still suspended): nothing is left running.
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed(source));
            }
        };
        Ok(Detached { child, tree, label })
    }
}

/// A program started with [`Launch::detach`]. Dropping the handle neither kills nor reaps it: a
/// caller that lives on reaps it with [`Detached::try_wait`].
pub struct Detached {
    child: Child,
    tree: Option<tree::Tree>,
    label: String,
}

impl Detached {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The exit status once the program has ended, `None` while it runs.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, ExecError> {
        self.child.try_wait().map_err(|source| ExecError::Spawn { program: self.label.clone(), source })
    }

    /// Kills the program and what it started: its job on Windows, its session's group on Unix, or
    /// the program alone when the tree could not be set up. Then reaps it.
    pub fn kill(&mut self) -> Result<ExitStatus, ExecError> {
        match &self.tree {
            Some(tree) => tree.kill(),
            None => {
                let _ = self.child.kill();
            }
        }
        self.child.wait().map_err(|source| ExecError::Spawn { program: self.label.clone(), source })
    }
}

impl fmt::Debug for Detached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Detached")
            .field("program", &self.label)
            .field("pid", &self.pid())
            .field("tree", &self.tree.is_some())
            .finish()
    }
}

/// Names and lengths only: a prompt, a body or a variable set by name never reaches a log through
/// `{:?}` (T10).
impl fmt::Debug for Launch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Launch")
            .field("program", &self.program_name())
            .field("args", &self.args.len())
            .field("cwd", &self.cwd)
            .field("stdin_bytes", &self.stdin.as_ref().map(Vec::len))
            .field("env_set", &self.set.iter().map(|(name, _)| name).collect::<Vec<_>>())
            .field("env_removed", &self.removed)
            .finish_non_exhaustive()
    }
}

/// The only `Command::new` of the workspace (V12).
#[allow(clippy::disallowed_methods)]
pub(crate) fn new_command(path: &Path) -> Command {
    Command::new(path)
}

/// A `.cmd` or `.bat` runs through `cmd.exe`, which reads its arguments again.
fn batch(path: &Path) -> bool {
    cfg!(windows)
        && path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"))
}

/// The alphabet of threat model §3.6, rule 9. Rust refuses more on its own (CVE-2024-24576).
fn safe_for_batch(arg: &OsStr) -> bool {
    arg.to_str().is_some_and(|arg| {
        !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '=' | '-'))
    })
}

const TICK: Duration = Duration::from_millis(10);

/// How long the pipes may stay open after the program ends: a process that left the tree on
/// purpose can hold them, and the run does not wait for it.
const GRACE: Duration = Duration::from_secs(2);

enum Chunk {
    Out(Vec<u8>),
    Err(Vec<u8>),
    End,
}

fn read_into(mut pipe: impl Read + Send + 'static, wrap: fn(Vec<u8>) -> Chunk, sender: Sender<Chunk>) {
    thread::spawn(move || {
        let mut buffer = vec![0; 64 * 1024];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if sender.send(wrap(buffer[..n].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = sender.send(Chunk::End);
    });
}

/// The tree of a program that just started. Without one the program does not go on.
fn adopt(child: &mut Child, label: &str) -> Result<tree::Tree, ExecError> {
    tree::Tree::adopt(child).map_err(|source| {
        let _ = child.kill();
        let _ = child.wait();
        ExecError::Spawn { program: label.to_owned(), source }
    })
}

fn supervise(mut child: Child, input: Option<Vec<u8>>, limits: Limits, label: String) -> Result<Output, ExecError> {
    let tree = adopt(&mut child, &label)?;
    let writer: Option<JoinHandle<()>> = input.map(|bytes| {
        let mut pipe = child.stdin.take().expect("stdin is piped when there is input");
        thread::spawn(move || {
            // A program that exits without reading closes the pipe: that is its answer, not an error.
            let _ = pipe.write_all(&bytes);
        })
    });
    let (sender, receiver) = mpsc::channel();
    let stdout: ChildStdout = child.stdout.take().expect("stdout is piped");
    let stderr: ChildStderr = child.stderr.take().expect("stderr is piped");
    read_into(stdout, Chunk::Out, sender.clone());
    read_into(stderr, Chunk::Err, sender);

    let started = Instant::now();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut open = 2;
    let mut exited: Option<(ExitStatus, Instant)> = None;
    let mut take = |chunk: Chunk, open: &mut i32| -> Option<ExecError> {
        let (length, limit, stream) = match chunk {
            Chunk::Out(bytes) => {
                out.extend_from_slice(&bytes);
                (out.len(), limits.stdout, "stdout")
            }
            Chunk::Err(bytes) => {
                err.extend_from_slice(&bytes);
                (err.len(), limits.stderr, "stderr")
            }
            Chunk::End => {
                *open -= 1;
                return None;
            }
        };
        (length > limit).then(|| ExecError::TooMuchOutput { program: label.clone(), stream, limit })
    };
    let failure = loop {
        if exited.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Nothing it started outlives it.
                    tree.kill();
                    exited = Some((status, Instant::now()));
                }
                Ok(None) => {}
                Err(source) => break Some(ExecError::Spawn { program: label.clone(), source }),
            }
        }
        if let Some((_, at)) = exited
            && (open == 0 || at.elapsed() >= GRACE)
        {
            break None;
        }
        if started.elapsed() >= limits.timeout {
            break Some(ExecError::TimedOut { program: label.clone(), limit: limits.timeout });
        }
        match receiver.recv_timeout(TICK) {
            Ok(chunk) => {
                if let Some(error) = take(chunk, &mut open) {
                    break Some(error);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => open = 0,
        }
    };
    tree.kill();
    let status = match exited {
        Some((status, _)) => status,
        None => {
            let _ = child.kill();
            child.wait().map_err(|source| ExecError::Spawn { program: label.clone(), source })?
        }
    };
    if failure.is_none() {
        while let Ok(chunk) = receiver.try_recv() {
            if let Some(error) = take(chunk, &mut open) {
                return Err(error);
            }
        }
    }
    drop(receiver);
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(Output { status, stdout: out, stderr: err }),
    }
}

#[cfg(feature = "harness")]
pub use running::Running;

#[cfg(feature = "harness")]
mod running {
    use std::io::{self, BufRead, BufReader, Write};
    use std::process::{Child, ChildStdin, ChildStdout, ExitStatus, Stdio};

    use super::{Launch, adopt};
    use crate::ExecError;
    use crate::tree::Tree;

    impl Launch {
        /// See [`crate::harness::start`].
        pub(crate) fn start(self) -> Result<Running, ExecError> {
            let label = self.program_name();
            let failed = |source| ExecError::Spawn { program: label.clone(), source };
            let mut command = self.command()?;
            crate::tree::prepare(&mut command);
            command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut child = command.spawn().map_err(failed)?;
            let tree = adopt(&mut child, &label)?;
            let mut stdin = child.stdin.take();
            if let (Some(pipe), Some(input)) = (stdin.as_mut(), &self.stdin) {
                pipe.write_all(input).map_err(failed)?;
            }
            let stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
            Ok(Running { child, tree, stdin, stdout, label })
        }
    }

    /// Dev only: a run the test drives while it goes on, from [`crate::harness::start`]. Its
    /// stdout is read a line at a time and its stderr is discarded; its tree dies with the handle.
    pub struct Running {
        child: Child,
        tree: Tree,
        stdin: Option<ChildStdin>,
        stdout: BufReader<ChildStdout>,
        label: String,
    }

    impl Running {
        /// The next line on stdout, without its line break; empty once stdout is closed.
        pub fn read_line(&mut self) -> Result<String, ExecError> {
            let mut line = String::new();
            self.stdout.read_line(&mut line).map_err(|source| self.failed(source))?;
            Ok(line.trim_end_matches(['\n', '\r']).to_owned())
        }

        /// Closes stdin, as a caller with nothing more to say, and waits for the program to end.
        pub fn finish(mut self) -> Result<ExitStatus, ExecError> {
            drop(self.stdin.take());
            self.child.wait().map_err(|source| self.failed(source))
        }

        /// Kills the whole tree, as a crash would, and reaps the program.
        pub fn kill(mut self) -> Result<ExitStatus, ExecError> {
            self.tree.kill();
            self.child.wait().map_err(|source| self.failed(source))
        }

        fn failed(&self, source: io::Error) -> ExecError {
            ExecError::Spawn { program: self.label.clone(), source }
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.tree.kill();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(env: &[(OsString, OsString)]) -> Vec<String> {
        env.iter().map(|(name, _)| key(name).unwrap()).collect()
    }

    #[test]
    fn the_allow_list_drops_tokens_and_foreign_hosts() {
        let parent: Vec<(OsString, OsString)> = [
            ("PATH", "/bin"),
            ("GH_TOKEN", "canary"),
            ("GITHUB_TOKEN", "canary"),
            ("GH_HOST", "evil.example"),
            ("GH_DEBUG", "api"),
            ("ANTHROPIC_API_KEY", "canary"),
            ("REGIA_HOME", "/d"),
            ("REGIA_X_TOKEN", "canary"),
            ("MOVIOLA_TASKS", "t"),
        ]
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .to_vec();
        let pin = Pin::at(Program::Gh, &std::env::current_exe().unwrap()).unwrap();
        let launch = || Launch::new(Target::Program(pin.clone()), Vec::new(), Path::new(".")).env("EXPLICIT", "1");
        let mut gh = launch();
        gh.parent = Some(parent.clone());
        let env = gh.environment(Program::Gh);
        let gh_names = names(&env);
        for absent in ["GH_TOKEN", "GITHUB_TOKEN", "GH_DEBUG", "ANTHROPIC_API_KEY", "REGIA_HOME", "MOVIOLA_TASKS"] {
            assert!(!gh_names.contains(&absent.to_owned()), "gh sees {absent}");
        }
        let host = env.iter().find(|(name, _)| key(name).unwrap() == "GH_HOST").unwrap();
        assert_eq!(host.1, "github.com");
        assert!(gh_names.contains(&"PATH".to_owned()) && gh_names.contains(&"EXPLICIT".to_owned()));

        let mut python = launch();
        python.parent = Some(parent);
        let python_names = names(&python.environment(Program::Python));
        assert!(python_names.contains(&"REGIA_HOME".to_owned()) && python_names.contains(&"MOVIOLA_TASKS".to_owned()));
        assert!(!python_names.contains(&"REGIA_X_TOKEN".to_owned()), "a secret by name never passes by prefix");
    }

    /// #571/V4: what `git` sees. A read has the settings of rule 8; a write has none of them, so the user's
    /// hooks do not either, and both have what a push over SSH and a signed commit need.
    #[test]
    fn git_reads_have_the_settings_and_writes_do_not_and_both_keep_the_agent_and_the_gpg_folder() {
        let parent: Vec<(OsString, OsString)> = [
            ("PATH", "/bin"),
            ("SSH_AUTH_SOCK", "/run/agent.sock"),
            ("GNUPGHOME", "/home/u/gnupg"),
            ("GITHUB_TOKEN", "canary"),
            ("MY_SIGNING_KEY", "canary"),
            ("GIT_DIR", "/elsewhere"),
        ]
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .to_vec();
        let pin = Pin::at(Program::Git, &std::env::current_exe().unwrap()).unwrap();
        let seen = |writes: bool| {
            let mut launch = pin.launch(Args::new(), Path::new("."));
            launch.parent = Some(parent.clone());
            if writes {
                launch = launch.writing();
            }
            names(&launch.environment(Program::Git))
        };
        let settings = ["GIT_OPTIONAL_LOCKS", "GIT_LITERAL_PATHSPECS", "GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0"];
        let (reads, writes) = (seen(false), seen(true));
        for name in settings {
            assert!(reads.contains(&name.to_owned()), "a read lacks {name}");
            assert!(!writes.contains(&name.to_owned()), "a write has {name}");
        }
        for list in [&reads, &writes] {
            for name in ["GIT_TERMINAL_PROMPT", "GCM_INTERACTIVE", "SSH_AUTH_SOCK", "GNUPGHOME", "PATH"] {
                assert!(list.contains(&name.to_owned()), "{name} is missing");
            }
            for name in ["GITHUB_TOKEN", "MY_SIGNING_KEY", "GIT_DIR"] {
                assert!(!list.contains(&name.to_owned()), "{name} came through");
            }
        }
    }

    #[test]
    fn only_git_inherits_the_agent_socket_and_the_gpg_folder() {
        for program in Program::ALL {
            for name in GIT_USER {
                assert_eq!(inherits(program, OsStr::new(name)), program == Program::Git, "{program} and {name}");
            }
        }
    }

    #[test]
    fn debug_output_shows_no_input_secret_or_output() {
        let pin = Pin::at(Program::Claude, &std::env::current_exe().unwrap()).unwrap();
        let launch = pin.launch(Args::new(), Path::new(".")).stdin("the secret prompt").env("SOME_TOKEN", "canary");
        let text = format!("{launch:?}");
        assert!(!text.contains("secret prompt") && !text.contains("canary"), "{text}");
        assert!(text.contains("SOME_TOKEN") && text.contains("17"), "names and lengths stay: {text}");
    }

    /// A detached start, on this test binary asked for the list of its tests: it returns at once, the
    /// output lands in the log, and a pin that no longer matches starts nothing and writes no log.
    #[test]
    fn a_detached_program_writes_its_log_and_a_changed_pin_starts_nothing() {
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().unwrap().join("detach-unit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (input, log) = (dir.join("input.txt"), dir.join("out.log"));
        std::fs::write(&input, "unused").unwrap();
        let pin = Pin::at(Program::Claude, &exe).unwrap();
        let mut detached = pin.launch(Args::new().word("--list"), &dir).detach(&input, &log).unwrap();
        assert!(detached.pid() > 0 && format!("{detached:?}").contains("pid"));
        let started = std::time::Instant::now();
        let status = loop {
            if let Some(status) = detached.try_wait().unwrap() {
                break status;
            }
            assert!(started.elapsed() < Duration::from_secs(60), "the listing did not end");
            thread::sleep(TICK);
        };
        assert!(status.success());
        let listed = std::fs::read_to_string(&log).unwrap();
        assert!(listed.contains("a_detached_program_writes_its_log_and_a_changed_pin_starts_nothing"), "{listed}");
        assert!(detached.kill().unwrap().success(), "killing an ended program only reaps it");

        let copy = dir.join(exe.file_name().unwrap());
        std::fs::copy(&exe, &copy).unwrap();
        let stale = Pin::at(Program::Claude, &copy).unwrap();
        std::fs::write(&copy, b"another file").unwrap();
        let other_log = dir.join("never.log");
        let refused = stale.launch(Args::new(), &dir).detach(&input, &other_log).unwrap_err();
        assert!(matches!(refused, ExecError::Changed { .. }), "{refused:?}");
        assert!(!other_log.exists(), "nothing was started, and nothing written");
    }

    #[test]
    fn a_batch_file_takes_a_narrow_alphabet() {
        for safe in ["exec", "--json", "--model=gpt-5.4", "a/b:c_d.e"] {
            assert!(safe_for_batch(OsStr::new(safe)), "{safe}");
        }
        for unsafe_arg in ["", "a b", "%PATH%", "a&b", "a|b", "a^b", "\"", "a\nb", "@x", "é"] {
            assert!(!safe_for_batch(OsStr::new(unsafe_arg)), "{unsafe_arg:?}");
        }
    }
}
