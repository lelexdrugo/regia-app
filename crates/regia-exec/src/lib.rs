//! The only crate that starts a process (threat model §3.6, ADR 0001 §2). The rest of the workspace
//! cannot: `clippy.toml` forbids `std::process::Command::new` and a guard test greps for it.
//!
//! - **Which program:** a closed list, [`Program`], each at an absolute path resolved once and
//!   pinned with what its file looked like ([`Pin`]). A changed file is refused (T06).
//! - **Arguments:** [`Args`] takes literals of the code and the typed values of this crate, never a
//!   `String`. Options are `--name=value`; values never start with a dash (T04).
//! - **Environment:** built from an allow-list per program, not inherited (T03, T22).
//! - **Folder, input, limits:** the folder is always explicit, input goes on stdin, a run has a
//!   deadline and a cap on its output, and nothing it started outlives it ([`Launch::run`]). An
//!   agent's session is the exception: it is started on its own, with a log ([`Launch::detach`]).

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

mod run;
mod search;
mod tree;
mod values;

pub use run::{Detached, Launch, Limits, Output, secret_shaped};
pub use search::{Pin, Search, usual_folders};
pub use values::{
    AbsolutePath, ArgValue, DossierPath, Effort, GitRef, InvalidValue, IssueNumber, ModelId, RepoSlug, Text,
};

/// The programs regia may start, and nothing else. No shell is on the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Program {
    Gh,
    Git,
    Python,
    Claude,
    Codex,
    Orca,
}

impl Program {
    pub const ALL: [Program; 6] =
        [Program::Gh, Program::Git, Program::Python, Program::Claude, Program::Codex, Program::Orca];

    pub fn name(self) -> &'static str {
        match self {
            Program::Gh => "gh",
            Program::Git => "git",
            Program::Python => "python",
            Program::Claude => "claude",
            Program::Codex => "codex",
            Program::Orca => "orca",
        }
    }

    /// The file names it is looked up by, without extension, in order.
    fn file_names(self) -> &'static [&'static str] {
        match self {
            Program::Gh => &["gh"],
            Program::Git => &["git"],
            Program::Python if cfg!(windows) => &["python"],
            Program::Python => &["python3", "python"],
            Program::Claude => &["claude"],
            Program::Codex => &["codex"],
            Program::Orca => &["orca"],
        }
    }

    /// `REGIA_GH`, `REGIA_PYTHON`…: an explicit path, which wins over the search.
    pub fn env_name(self) -> String {
        regia_identity::env_var(&self.name().to_ascii_uppercase())
    }
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The argv of a program, built only from literals of the code and typed values (threat model
/// §3.6, rule 2). There is no way to add a `String`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args(Vec<OsString>);

impl Args {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fixed token: a subcommand, a flag, `--`.
    #[must_use]
    pub fn word(mut self, word: &'static str) -> Self {
        self.0.push(word.into());
        self
    }

    /// `name=value` as one token, so the tool never reads the value as an option of its own.
    #[must_use]
    pub fn opt(mut self, name: &'static str, value: &impl ArgValue) -> Self {
        let mut token = OsString::from(name);
        token.push("=");
        token.push(value.as_arg());
        self.0.push(token);
        self
    }

    /// A value as a token of its own. Values never start with a dash, and where the tool allows it
    /// the builder puts `--` before them.
    #[must_use]
    pub fn value(mut self, value: &impl ArgValue) -> Self {
        self.0.push(value.as_arg().into());
        self
    }

    /// A path of the dossier as a pathspec that git reads literally, whatever the environment says:
    /// `:(literal)<path>`. For the calls that write, which do not get `GIT_LITERAL_PATHSPECS` (it would
    /// reach the user's hooks). The alphabet of a [`DossierPath`] has no glob anyway: this is the second lock.
    #[must_use]
    pub fn pathspec(mut self, path: &DossierPath) -> Self {
        let mut token = OsString::from(":(literal)");
        token.push(path.as_arg());
        self.0.push(token);
        self
    }

    pub fn tokens(&self) -> impl Iterator<Item = &OsStr> {
        self.0.iter().map(OsString::as_os_str)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error(transparent)]
    InvalidValue(#[from] InvalidValue),
    #[error("{program} not found: install it, or set {variable} to its absolute path")]
    NotFound { program: Program, variable: String },
    #[error("{program}: {} is not an executable file at an absolute path", path.display())]
    NotAFile { program: Program, path: PathBuf },
    #[error("{program} at {} changed after it was resolved: it will not start until the new file is confirmed", path.display())]
    Changed { program: Program, path: PathBuf },
    #[error("{0} is a .cmd or .bat file, and an argument outside [A-Za-z0-9._:/=-] cannot reach it safely")]
    UnsafeForBatch(String),
    #[error("cannot start {program}: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error("{program} did not finish within {} s", limit.as_secs())]
    TimedOut { program: String, limit: Duration },
    #[error("{program} wrote more than {limit} bytes to {stream}")]
    TooMuchOutput { program: String, stream: &'static str, limit: usize },
}

/// A folder for a tool whose result does not depend on where it runs (`gh api`): the user's home,
/// which always exists.
pub fn neutral_folder() -> PathBuf {
    directories::BaseDirs::new().map_or_else(std::env::temp_dir, |dirs| dirs.home_dir().to_path_buf())
}

/// `gh`, as the tracker adapter starts it.
pub mod gh {
    use crate::Args;

    /// `gh api graphql --input=-`. The request, query and typed variables, goes on stdin, so no
    /// value reaches argv and `-F`, which reads a file for `@path`, is never needed (threat model
    /// §3.6, rule 2).
    pub fn graphql() -> Args {
        Args::new().word("api").word("graphql").word("--input=-")
    }

    /// `gh auth token --hostname=github.com`: the token of the account signed in to github.com, on
    /// stdout. The updater reads it into memory for one round of requests to `api.github.com` and
    /// never writes it anywhere (the dossier's `rilascio.md` §4.4, point 1). The environment gives
    /// gh no `GH_TOKEN`, so the token is the one gh keeps, not one the caller's shell exported.
    pub fn auth_token() -> Args {
        Args::new().word("auth").word("token").word("--hostname=github.com")
    }
}

/// Read-only `git`. The environment of a read of [`Program::Git`](crate::Program::Git) adds no prompt, no
/// optional lock, literal pathspecs and no fsmonitor, without a `-c` in argv; a write does not get the
/// last three ([`Launch::writing`](crate::Launch::writing)).
pub mod git {
    use crate::{Args, DossierPath, GitRef};

    /// `git rev-parse --verify --end-of-options <rev>`.
    pub fn rev_parse(rev: &GitRef) -> Args {
        Args::new().word("rev-parse").word("--verify").word("--end-of-options").value(rev)
    }

    /// `git status --porcelain -- <path>`: what the working copy changed under `path`. The
    /// environment keeps it from refreshing the index.
    pub fn status(path: &DossierPath) -> Args {
        Args::new().word("status").word("--porcelain").word("--").value(path)
    }

    /// `git log -1` on `rev`, or on the last commit of `rev` that touched `path`.
    pub fn last_commit(field: CommitField, rev: &GitRef, path: Option<&DossierPath>) -> Args {
        let args = Args::new().word("log").word("-1").word(field.format()).word("--end-of-options").value(rev);
        match path {
            Some(path) => args.word("--").value(path),
            None => args,
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CommitField {
        Hash,
        Date,
    }

    impl CommitField {
        fn format(self) -> &'static str {
            match self {
                CommitField::Hash => "--format=%H",
                CommitField::Date => "--format=%cs",
            }
        }
    }
}

/// The Python prototype, for the facade and the recorder.
pub mod python {
    use crate::{AbsolutePath, Args, DossierPath, InvalidValue, Text};

    /// The interpreter's flags for the prototype. `-I` keeps the `PYTHON*` variables, the user's site
    /// packages and the script's folder out (threat model §3.2, T06). The prototype loads its modules
    /// by path, or puts their folder on `sys.path` itself, and runs the same without it: #440 compared
    /// output, exit code, writes and activity log with `-E -s`. UTF-8 mode is explicit because `-I`
    /// ignores the `PYTHONUTF8` the prototype's own sessions set.
    pub const FLAGS: [&str; 3] = ["-X", "utf8", "-I"];

    /// `python -X utf8 -I <script> <argv…>`, with [`FLAGS`].
    ///
    /// Each element of `argv` is an option name from the facade table (`--json`), an option with its
    /// value joined (`--corpo=- a list item`) or a value. A separate value never starts with a dash:
    /// the facade passes values untouched, so this is where a hostile one stops.
    ///
    /// A joined option is the form the threat model asks for (§3.6, rule 2), and the one way a value
    /// that starts with a dash gets through: the prototype's parser takes what follows the first `=`
    /// as the value, whatever it starts with, so it never reads it as an option. The name before the
    /// `=` has the rule of a bare option name. The value after it has the rules of a [`Text`] except
    /// the leading dash: not empty, no control character but tab and line breaks, 64 KiB at most. An
    /// empty joined value (`--corpo=`) is refused as an empty separate one is: it would not reach any
    /// option, but the prototype has no use for it, and a value is refused the same wherever it stands
    /// (#568/V2).
    pub fn prototype(script: &DossierPath, argv: &[String]) -> Result<Args, InvalidValue> {
        with_argv(FLAGS.into_iter().fold(Args::new(), Args::word).value(script), argv)
    }

    /// [`prototype`] with the script at an absolute path, for a run that does not start in the dossier's root: the
    /// facade starts `path` with no `--project` in the caller's folder, which is what that command reads (#701).
    /// The argv has the same rules; only the script, which `-I` keeps off `sys.path` anyway, is named in full.
    pub fn prototype_at(script: &AbsolutePath, argv: &[String]) -> Result<Args, InvalidValue> {
        with_argv(FLAGS.into_iter().fold(Args::new(), Args::word).value(script), argv)
    }

    /// `args`, which end with the script, then the facade's argv: each element judged as [`prototype`] says.
    fn with_argv(mut args: Args, argv: &[String]) -> Result<Args, InvalidValue> {
        for item in argv {
            if let Some(option) = item.strip_prefix("--") {
                let (name, joined) = match option.split_once('=') {
                    Some((name, value)) => (name, Some(value)),
                    None => (option, None),
                };
                let valid = !name.is_empty()
                    && name.len() <= 64
                    && name.starts_with(|c: char| c.is_ascii_lowercase())
                    && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                if !valid {
                    return Err(InvalidValue { kind: "option name", reason: "is not --lowercase-words" });
                }
                if let Some(value) = joined {
                    // The rules of `Text` with a dash first allowed: that one dash becomes a letter of
                    // the same length, and `Text` judges the rest.
                    let probe = value.strip_prefix('-').map_or_else(|| value.to_owned(), |rest| format!("x{rest}"));
                    Text::parse(&probe)?;
                }
                args.0.push(item.into());
            } else {
                args = args.value(&Text::parse(item)?);
            }
        }
        Ok(args)
    }
}

/// `claude` as the Cli surface starts it (ADR 0001 §6): fixed flags, the prompt on stdin.
pub mod claude {
    use crate::{Args, Effort, ModelId, Text};

    /// `claude -p --output-format=stream-json --verbose --dangerously-skip-permissions [--model=…]
    /// [--effort=…]`: a session with nobody at the keyboard, which prints its events on stdout, one
    /// JSON object a line. The prompt goes on stdin, never in argv.
    pub fn print(model: Option<&ModelId>, effort: Option<Effort>) -> Args {
        let args = Args::new()
            .word("-p")
            .word("--output-format=stream-json")
            .word("--verbose")
            .word("--dangerously-skip-permissions");
        let args = match model {
            Some(model) => args.opt("--model", model),
            None => args,
        };
        match effort {
            Some(effort) => args.opt("--effort", &effort),
            None => args,
        }
    }

    /// The token wake-up of #190, `claude -p --model=haiku --effort=low --safe-mode --tools=
    /// --system-prompt=Reply-with-one-word. --no-session-persistence --max-turns=1
    /// --output-format=json`: the smallest session that signs in. No tool, and no hook, plugin, MCP
    /// or `CLAUDE.md` (`--safe-mode`, which unlike `--bare` still reads the sign-in); nothing kept;
    /// one turn and one JSON answer. The prompt goes on stdin, as every other. Every token is in the
    /// alphabet a `.cmd` takes (threat model §3.6, rule 9): npm installs `claude.cmd`.
    pub fn wake() -> Args {
        Args::new()
            .word("-p")
            .word("--model=haiku")
            .word("--effort=low")
            .word("--safe-mode")
            .word("--tools=")
            .word("--system-prompt=Reply-with-one-word.")
            .word("--no-session-persistence")
            .word("--max-turns=1")
            .word("--output-format=json")
    }

    /// `claude -p --resume=<session> --output-format=stream-json --verbose --dangerously-skip-permissions [--model=…]
    /// [--effort=…]`: a session the usage limit stopped, resumed with nobody at the keyboard (the dossier's #132). What
    /// it is told goes on stdin, as for [`print`]. The session is untrusted (read from a registry or a file name): its
    /// caller checks its shape before it becomes a [`Text`], and the joined form keeps it from reading as an option.
    pub fn resume(session: &Text, model: Option<&ModelId>, effort: Option<Effort>) -> Args {
        let args = Args::new()
            .word("-p")
            .opt("--resume", session)
            .word("--output-format=stream-json")
            .word("--verbose")
            .word("--dangerously-skip-permissions");
        let args = match model {
            Some(model) => args.opt("--model", model),
            None => args,
        };
        match effort {
            Some(effort) => args.opt("--effort", &effort),
            None => args,
        }
    }
}

/// `codex` as the Cli surface starts it (ADR 0001 §6): fixed flags, the prompt on stdin.
pub mod codex {
    use crate::{AbsolutePath, Args, Effort, ModelId, Text};

    /// `codex exec --json --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check
    /// [--cd=<folder>] --output-last-message=<file> [--model=…] [--config=model_reasoning_effort=…] -`.
    /// The last `-` makes it read the prompt from stdin. On Windows codex is `codex.cmd`, so the
    /// paths are written with `/` ([`AbsolutePath`]).
    ///
    /// `folder` is `None` when the program is started in that folder and takes no `--cd`: codex works
    /// in its own current folder when it has none. That is how a folder a `.cmd` cannot receive as an
    /// argument ([`AbsolutePath::fits_batch`]) still reaches codex, whole.
    pub fn exec(folder: Option<&AbsolutePath>, last_message: &AbsolutePath, model: Option<&ModelId>, effort: Option<Effort>) -> Args {
        let mut args = Args::new().word("exec").word("--json").word("--dangerously-bypass-approvals-and-sandbox").word("--skip-git-repo-check");
        if let Some(folder) = folder {
            args = args.opt("--cd", folder);
        }
        args = args.opt("--output-last-message", last_message);
        if let Some(model) = model {
            args = args.opt("--model", model);
        }
        if let Some(effort) = effort {
            // A key of codex's configuration and a level of the closed list: one token.
            args = args.push(format!("--config=model_reasoning_effort={}", effort.as_str()).into());
        }
        args.word("-")
    }

    /// `codex exec resume --json --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check [--model=…]
    /// [--config=model_reasoning_effort=…] <session> -`: a session the usage limit stopped, resumed (the dossier's
    /// #132), started in its own folder. The last `-` reads what it is told from stdin. The session is a positional
    /// that its caller checked: a `.cmd` takes only the narrow alphabet, which an id of codex keeps.
    pub fn resume(session: &Text, model: Option<&ModelId>, effort: Option<Effort>) -> Args {
        let mut args =
            Args::new().word("exec").word("resume").word("--json").word("--dangerously-bypass-approvals-and-sandbox").word("--skip-git-repo-check");
        if let Some(model) = model {
            args = args.opt("--model", model);
        }
        if let Some(effort) = effort {
            args = args.push(format!("--config=model_reasoning_effort={}", effort.as_str()).into());
        }
        args.value(session).word("-")
    }
}

/// `orca`, for the Orca surface of #462: its builders only. Every command asks for `--json`.
pub mod orca {
    use crate::{AbsolutePath, Args, Effort, InvalidValue, ModelId, Program, Text};

    /// `orca status --json`: whether the runtime is reachable.
    pub fn status() -> Args {
        Args::new().word("status").word("--json")
    }

    /// `orca worktree show --worktree=path:<folder> --json`: whether a folder is a workspace of Orca.
    pub fn worktree_show(folder: &AbsolutePath) -> Args {
        Args::new().word("worktree").word("show").push(selector(folder)).word("--json")
    }

    /// `orca agent launch --agent=<claude|codex> --worktree=path:<folder> --prompt=<text>
    /// [--model=…] [--effort=…] --json`. Orca takes the prompt as an argument, so its surface keeps
    /// it short and writes a long one to a file, as the prototype does.
    pub fn agent_launch(
        agent: Program,
        folder: &AbsolutePath,
        prompt: &Text,
        model: Option<&ModelId>,
        effort: Option<Effort>,
    ) -> Result<Args, InvalidValue> {
        let agent = match agent {
            Program::Claude => "--agent=claude",
            Program::Codex => "--agent=codex",
            _ => return Err(InvalidValue { kind: "agent", reason: "is not an agent service" }),
        };
        let mut args = Args::new().word("agent").word("launch").word(agent).push(selector(folder)).opt("--prompt", prompt);
        if let Some(model) = model {
            args = args.opt("--model", model);
        }
        if let Some(effort) = effort {
            args = args.opt("--effort", &effort);
        }
        Ok(args.word("--json"))
    }

    /// `orca search --query=<text> --path=<folder> --limit=100 [--cursor=<c>] --json`: a page of the sessions Orca
    /// indexed under a folder whose text holds `query` (the dossier's #132). The folder is the one the registry names,
    /// as it is: Orca matches it literally. The cursor is the one the previous page printed.
    pub fn search(query: &Text, folder: &Text, cursor: Option<&Text>) -> Args {
        let args = Args::new().word("search").opt("--query", query).opt("--path", folder).word("--limit=100");
        let args = match cursor {
            Some(cursor) => args.opt("--cursor", cursor),
            None => args,
        };
        args.word("--json")
    }

    /// `--worktree=path:<folder>`, the folder with `/`, as the prototype's selector.
    fn selector(folder: &AbsolutePath) -> std::ffi::OsString {
        format!("--worktree=path:{}", folder.as_str()).into()
    }
}

impl Args {
    /// A token this crate built from a literal and a typed value.
    fn push(mut self, token: std::ffi::OsString) -> Self {
        self.0.push(token);
        self
    }
}

#[cfg(feature = "harness")]
pub mod harness {
    //! Dev only, for regia-conformance; a guard test keeps every call to it there.
    //!
    //! - [`launch`]: this workspace's own binaries and the probe. They inherit the environment as a
    //!   shell would, minus what [`Launch::without`] removes, plus what [`Launch::env`] sets. The
    //!   allow-list is a property of the product starting external tools (V18), and a test app
    //!   needs the display variables a desktop session has.
    //! - [`program`]: an external program of the closed list as the product starts it, at its
    //!   pinned path with the allow-list of its kind, on the harness's own arguments: an oracle
    //!   script, the prototype through a driver, the prototype's controller. The arguments are not
    //!   typed because they are the test's, not data.
    //! - [`start`]: a run the test drives while it goes on.

    use std::ffi::OsStr;
    use std::io;
    use std::path::Path;
    use std::process::Stdio;

    pub use crate::run::Running;
    use crate::{ExecError, Launch, Pin};

    pub fn launch(binary: &Path, args: &[String], cwd: &Path) -> Launch {
        Launch::harness(binary, args, cwd)
    }

    pub fn program(pin: &Pin, args: &[String], cwd: &Path) -> Launch {
        Launch::untyped(pin.clone(), args, cwd)
    }

    /// Starts a run and hands it over, for a process that holds something until it is told to
    /// stop. The input, if any, is written first, and stdin stays open until [`Running::finish`].
    /// No deadline applies: the test ends the run, and dropping the handle kills its tree.
    pub fn start(launch: Launch) -> Result<Running, ExecError> {
        launch.start()
    }

    /// For the probe's `tree` mode: a process with no pipes that stays in the caller's job or
    /// process group, and is left running.
    pub fn spawn_background(binary: &Path, args: &[&str], env: &[(&str, &OsStr)]) -> io::Result<()> {
        let mut command = crate::run::new_command(binary);
        command
            .args(args)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.spawn().map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(args: &Args) -> Vec<String> {
        args.tokens().map(|token| token.to_string_lossy().into_owned()).collect()
    }

    fn absolute(text: &str) -> AbsolutePath {
        AbsolutePath::new(std::path::Path::new(text)).unwrap()
    }

    /// The prototype's argv (`claude -p --output-format stream-json …`, `codex exec … -C <folder> …`) in
    /// the `--name=value` form of T04: same options, same values, same order of the fixed part.
    #[test]
    fn the_agents_builders_spell_the_prototype_argv() {
        let opus = ModelId::parse("opus").unwrap();
        assert_eq!(
            tokens(&claude::print(Some(&opus), Some(Effort::High))),
            ["-p", "--output-format=stream-json", "--verbose", "--dangerously-skip-permissions", "--model=opus", "--effort=high"]
        );
        assert_eq!(tokens(&claude::print(None, None)).len(), 4, "no model, no effort: nothing more");

        let (folder, last) = if cfg!(windows) {
            (absolute(r"C:\work\trial"), absolute(r"C:\state\steps\trial\1.last.txt"))
        } else {
            (absolute("/work/trial"), absolute("/state/steps/trial/1.last.txt"))
        };
        let model = ModelId::parse("gpt-x").unwrap();
        let args = tokens(&codex::exec(Some(&folder), &last, Some(&model), Some(Effort::High)));
        let root = if cfg!(windows) { "C:/" } else { "/" };
        assert_eq!(
            args,
            [
                "exec".to_owned(),
                "--json".to_owned(),
                "--dangerously-bypass-approvals-and-sandbox".to_owned(),
                "--skip-git-repo-check".to_owned(),
                format!("--cd={root}work/trial"),
                format!("--output-last-message={root}state/steps/trial/1.last.txt"),
                "--model=gpt-x".to_owned(),
                "--config=model_reasoning_effort=high".to_owned(),
                "-".to_owned(),
            ]
        );
        assert_eq!(tokens(&codex::exec(Some(&folder), &last, None, None)).last().map(String::as_str), Some("-"));
        // Without a folder: the same argv without `--cd`, which codex does not need when it starts in it.
        let without = tokens(&codex::exec(None, &last, Some(&model), Some(Effort::High)));
        let with: Vec<String> = args.iter().filter(|token| !token.starts_with("--cd=")).cloned().collect();
        assert_eq!(without, with);
        assert_eq!(without.len(), args.len() - 1);
    }

    /// The prototype's `SVEGLIA_ARGOMENTI` in the `--name=value` form, without the prompt, which goes
    /// on stdin, and with a system prompt a `.cmd` can take.
    #[test]
    fn the_wake_up_is_the_smallest_session_in_the_alphabet_of_a_batch_file() {
        let args = tokens(&claude::wake());
        assert_eq!(
            args,
            [
                "-p",
                "--model=haiku",
                "--effort=low",
                "--safe-mode",
                "--tools=",
                "--system-prompt=Reply-with-one-word.",
                "--no-session-persistence",
                "--max-turns=1",
                "--output-format=json"
            ]
        );
        assert!(!args.iter().any(|arg| arg == "--bare"), "--bare does not read the sign-in: the wake-up would renew nothing");
        for arg in &args {
            let batch = arg.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '=' | '-'));
            assert!(batch, "{arg}");
        }
    }

    /// The token's host is fixed in argv as well as in the environment (`GH_HOST`), and no token is
    /// an argument: the command only prints one.
    #[test]
    fn the_token_is_asked_of_github_com_only() {
        assert_eq!(tokens(&gh::auth_token()), ["auth", "token", "--hostname=github.com"]);
    }

    /// #568/V2: `--<name>=<value>` is one option of the facade with its value joined, which is how the
    /// prototype's parser takes a value that starts with a dash (`--corpo=- a list item`). The name
    /// keeps today's rule and the value is a [`Text`]; a value that is separate and starts with a dash
    /// stays refused, and so does a name that is not lowercase words, before the `=`.
    #[test]
    fn the_prototype_takes_a_joined_option_with_its_value() {
        let script = DossierPath::parse("tools/regia.py").unwrap();
        let argv = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<_>>();
        let tokens_of = |items: &[&str]| python::prototype(&script, &argv(items)).map(|args| tokens(&args));
        let fixed = ["-X", "utf8", "-I", "tools/regia.py"].map(str::to_owned);

        let joined = tokens_of(&["--json", "commento", "12", "--corpo=- a list item"]).unwrap();
        assert_eq!(joined[..4], fixed);
        assert_eq!(joined[4..], ["--json", "commento", "12", "--corpo=- a list item"]);
        let many = tokens_of(&["--report-elaborati=a.md", "--corpo=line one\nline two", "--n1-x=y=z"]).unwrap();
        assert_eq!(many[4..], ["--report-elaborati=a.md", "--corpo=line one\nline two", "--n1-x=y=z"], "the value keeps what follows its own `=`");

        // What stays out: the value's own rules, the name's rule before the `=`, a separate value.
        for hostile in [
            "--corpo=",          // `Text` takes no empty value, joined or not
            "--=x",              // no name
            "--Corpo=x",         // a name of lowercase words
            "--corpo x=y",       // a space is not in a name
            "--1corpo=x",        // a name starts with a letter
            "--corpo=a\u{1b}[31m", // a control character in the value
            "--corpo-é=x",       // an ASCII name
            "-x=y",              // one dash is no option name
        ] {
            assert!(tokens_of(&["brief", hostile]).is_err(), "the facade passed {hostile:?}");
        }
        let too_long = format!("--corpo={}", "x".repeat(64 * 1024 + 1));
        assert!(tokens_of(&["brief", &too_long]).is_err(), "a value is at most 64 KiB, joined or not");
        let long_name = format!("--{}=x", "a".repeat(65));
        assert!(tokens_of(&["brief", &long_name]).is_err(), "a name is at most 64 characters");
        assert!(tokens_of(&["commento", "12", "--corpo", "-x"]).is_err(), "a separate value that starts with a dash stays refused");
        let dashes = tokens_of(&["brief", "--corpo=-", "--corpo=--upload-pack=x"]).unwrap();
        assert_eq!(dashes[5..], ["--corpo=-", "--corpo=--upload-pack=x"], "a joined value is never read as an option");
    }

    #[test]
    fn the_orca_builders_ask_for_json_and_take_only_agents() {
        let folder = absolute(if cfg!(windows) { r"C:\work\trial" } else { "/work/trial" });
        let path = if cfg!(windows) { "C:/work/trial" } else { "/work/trial" };
        assert_eq!(tokens(&orca::status()), ["status", "--json"]);
        assert_eq!(tokens(&orca::worktree_show(&folder)), ["worktree", "show", &format!("--worktree=path:{path}"), "--json"]);
        let prompt = Text::parse("do the step\nline two").unwrap();
        let haiku = ModelId::parse("haiku").unwrap();
        let args = orca::agent_launch(Program::Claude, &folder, &prompt, Some(&haiku), Some(Effort::Low)).unwrap();
        assert_eq!(
            tokens(&args),
            [
                "agent",
                "launch",
                "--agent=claude",
                &format!("--worktree=path:{path}"),
                "--prompt=do the step\nline two",
                "--model=haiku",
                "--effort=low",
                "--json"
            ]
        );
        assert!(orca::agent_launch(Program::Gh, &folder, &prompt, None, None).is_err());
        let (query, here, cursor) = (Text::parse("hit your session limit").unwrap(), Text::parse(r"C:\work\trial").unwrap(), Text::parse("c1").unwrap());
        assert_eq!(
            tokens(&orca::search(&query, &here, Some(&cursor))),
            ["search", "--query=hit your session limit", r"--path=C:\work\trial", "--limit=100", "--cursor=c1", "--json"]
        );
        assert_eq!(tokens(&orca::search(&query, &here, None)).len(), 5, "no cursor on the first page");
    }

    /// The prototype's argv of a resume (`sospese.py`) in the `--name=value` form: the session joined to `--resume` for claude, a
    /// positional before the `-` of stdin for codex, the model and the effort as for a launch.
    #[test]
    fn the_resume_builders_spell_the_prototype_argv() {
        let session = Text::parse("11111111-2222-3333-4444-555555555555").unwrap();
        let haiku = ModelId::parse("haiku").unwrap();
        assert_eq!(
            tokens(&claude::resume(&session, Some(&haiku), Some(Effort::Low))),
            [
                "-p",
                "--resume=11111111-2222-3333-4444-555555555555",
                "--output-format=stream-json",
                "--verbose",
                "--dangerously-skip-permissions",
                "--model=haiku",
                "--effort=low"
            ]
        );
        assert_eq!(tokens(&claude::resume(&session, None, None)).len(), 5);
        let codex = tokens(&codex::resume(&session, Some(&haiku), Some(Effort::High)));
        assert_eq!(codex[..5], ["exec", "resume", "--json", "--dangerously-bypass-approvals-and-sandbox", "--skip-git-repo-check"]);
        assert_eq!(codex[5..], ["--model=haiku", "--config=model_reasoning_effort=high", "11111111-2222-3333-4444-555555555555", "-"]);
        assert_eq!(tokens(&codex::resume(&session, None, None))[5..], ["11111111-2222-3333-4444-555555555555", "-"]);
    }
}
