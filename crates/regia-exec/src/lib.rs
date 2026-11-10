//! The only crate that starts a process (threat model §3.6, ADR 0001 §2). The rest of the workspace
//! cannot: `clippy.toml` forbids `std::process::Command::new` and a guard test greps for it.
//!
//! - **Which program:** a closed list, [`Program`], each at an absolute path resolved once and
//!   pinned with what its file looked like ([`Pin`]). A changed file is refused (T06).
//! - **Arguments:** [`Args`] takes literals of the code and the typed values of this crate, never a
//!   `String`. Options are `--name=value`; values never start with a dash (T04).
//! - **Environment:** built from an allow-list per program, not inherited (T03, T22).
//! - **Folder, input, limits:** the folder is always explicit, input goes on stdin, a run has a
//!   deadline and a cap on its output, and nothing it started outlives it ([`Launch::run`]).

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

mod run;
mod search;
mod tree;
mod values;

pub use run::{Launch, Limits, Output};
pub use search::{Pin, Search, usual_folders};
pub use values::{ArgValue, DossierPath, Effort, GitRef, InvalidValue, IssueNumber, ModelId, RepoSlug, Text};

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
}

/// Read-only `git`. The environment of [`Program::Git`](crate::Program::Git) adds no prompt, no
/// optional lock, literal pathspecs and no fsmonitor, without a `-c` in argv.
pub mod git {
    use crate::{Args, DossierPath, GitRef};

    /// `git rev-parse --verify --end-of-options <rev>`.
    pub fn rev_parse(rev: &GitRef) -> Args {
        Args::new().word("rev-parse").word("--verify").word("--end-of-options").value(rev)
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
    use crate::{Args, DossierPath, InvalidValue, Text};

    /// `python -X utf8 -E -s <script> <argv…>`. `-E` and `-s` keep the `PYTHON*` variables and the
    /// user's site packages out (threat model §3.2, T06). `-I` would also leave the script's folder
    /// out of the imports, which the prototype's modules have not been proven without. UTF-8 mode
    /// is explicit because `-E` ignores the `PYTHONUTF8` the prototype's own sessions set.
    ///
    /// Each element of `argv` is an option name from the facade table (`--json`) or a value, and a
    /// value never starts with a dash: the facade passes values untouched, so this is where a
    /// hostile one stops.
    pub fn prototype(script: &DossierPath, argv: &[String]) -> Result<Args, InvalidValue> {
        let mut args = Args::new().word("-X").word("utf8").word("-E").word("-s").value(script);
        for item in argv {
            if let Some(name) = item.strip_prefix("--") {
                let valid = !name.is_empty()
                    && name.len() <= 64
                    && name.starts_with(|c: char| c.is_ascii_lowercase())
                    && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                if !valid {
                    return Err(InvalidValue { kind: "option name", reason: "is not --lowercase-words" });
                }
                args.0.push(item.into());
            } else {
                args = args.value(&Text::parse(item)?);
            }
        }
        Ok(args)
    }
}

#[cfg(feature = "harness")]
pub mod harness {
    //! Dev only, for regia-conformance: this workspace's own binaries and the probe. They inherit
    //! the environment as a shell would, minus what [`Launch::without`] removes, plus what
    //! [`Launch::env`] sets. The allow-list is a property of the product starting external tools
    //! (V18), and a test app needs the display variables a desktop session has.

    use std::ffi::OsStr;
    use std::io;
    use std::path::Path;
    use std::process::Stdio;

    use crate::Launch;

    pub fn launch(binary: &Path, args: &[String], cwd: &Path) -> Launch {
        Launch::harness(binary, args, cwd)
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
