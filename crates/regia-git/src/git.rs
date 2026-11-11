//! One call of the user's git: a typed argv on the pinned program, and what it gave, read as the
//! prototype's `_git` reads it.

use std::path::Path;
use std::time::Duration;

use regia_exec::{Args, DossierPath, ExecError, Limits, Pin, Program, Text};

/// The deadline of every call, the prototype's `GIT_TIMEOUT_S`.
pub const TIMEOUT: Duration = Duration::from_secs(120);

/// The exit codes the prototype gives a call past its deadline, and a git that could not start.
const TIMED_OUT: i32 = 124;
const NOT_RUN: i32 = 127;

/// The user's git at its pinned path.
#[derive(Debug, Clone)]
pub struct Git {
    pin: Pin,
}

/// What a call gave: its exit code, its stdout without the line breaks at its end, and its stderr
/// trimmed. Both are decoded as the prototype decodes them: UTF-8 with invalid bytes replaced, and
/// every line break read as `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// A commit's date as git reads it from `GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE`: seconds since the
/// epoch, and the offset from UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitDate {
    epoch: i64,
    offset_minutes: i32,
}

impl GitDate {
    pub fn new(epoch: i64, offset_seconds: i32) -> Self {
        Self { epoch, offset_minutes: offset_seconds / 60 }
    }

    /// `@<epoch> +hhmm`, git's own form.
    pub fn as_env(&self) -> String {
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let minutes = self.offset_minutes.unsigned_abs();
        format!("@{} {sign}{:02}{:02}", self.epoch, minutes / 60, minutes % 60)
    }
}

impl Git {
    /// `REGIA_GIT` when it is set, otherwise the search of this process (regia-exec).
    pub fn resolve() -> Result<Git, ExecError> {
        Ok(Git { pin: Pin::resolve(Program::Git)? })
    }

    /// The git at `pin`.
    pub fn at(pin: Pin) -> Git {
        Git { pin }
    }

    /// Runs `args` in `folder`. `date`, when given, is the date of the commit the call makes.
    pub(crate) fn run(&self, folder: &Path, args: Args, date: Option<GitDate>) -> Run {
        let subcommand = args.tokens().next().map_or_else(String::new, |token| token.to_string_lossy().into_owned());
        let mut launch = self.pin.launch(args, folder).limits(Limits { timeout: TIMEOUT, ..Limits::default() });
        if let Some(date) = date {
            launch = launch.env("GIT_AUTHOR_DATE", date.as_env()).env("GIT_COMMITTER_DATE", date.as_env());
        }
        match launch.run() {
            Ok(output) => Run {
                code: output.status.code().unwrap_or(-1),
                stdout: decoded(&output.stdout).trim_end_matches(['\r', '\n']).to_owned(),
                stderr: decoded(&output.stderr).trim().to_owned(),
            },
            Err(ExecError::TimedOut { limit, .. }) => Run {
                code: TIMED_OUT,
                stdout: String::new(),
                stderr: format!("git {subcommand}: no answer within {} s", limit.as_secs()),
            },
            Err(error) => Run { code: NOT_RUN, stdout: String::new(), stderr: format!("git cannot run: {error}") },
        }
    }
}

/// Python's text mode: invalid UTF-8 replaced, `\r\n` and `\r` read as `\n`.
fn decoded(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n").replace('\r', "\n")
}

/// The last line of `text` that is not blank, trimmed: the prototype's `_ultima`, the part of git's
/// stderr a reason quotes.
pub(crate) fn last_line(text: &str) -> String {
    text.split(is_line_break).map(str::trim).rfind(|line| !line.is_empty()).unwrap_or("(no message)").to_owned()
}

/// The line boundaries of Python's `str.splitlines`.
fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// The argv of each call, in the prototype's order. Values are typed; paths come after `--`.
pub(crate) mod call {
    use super::*;

    pub fn show_toplevel() -> Args {
        Args::new().word("rev-parse").word("--show-toplevel")
    }

    /// The current branch, short; fails on a detached HEAD.
    pub fn branch() -> Args {
        Args::new().word("symbolic-ref").word("--short").word("-q").word("HEAD")
    }

    pub fn upstream() -> Args {
        Args::new().word("rev-parse").word("--abbrev-ref").word("--symbolic-full-name").word("@{u}")
    }

    pub fn fetch() -> Args {
        Args::new().word("fetch")
    }

    /// How many commits of the upstream the branch lacks.
    pub fn behind() -> Args {
        Args::new().word("rev-list").word("--count").word("HEAD..@{u}")
    }

    /// How many commits of the branch the upstream lacks.
    pub fn ahead() -> Args {
        Args::new().word("rev-list").word("--count").word("@{u}..HEAD")
    }

    pub fn fast_forward() -> Args {
        Args::new().word("merge").word("--ff-only").word("@{u}")
    }

    /// What changed under `paths`, untracked files one by one, separated by NUL.
    pub fn status(paths: &[DossierPath]) -> Args {
        with_paths(Args::new().word("status").word("--porcelain=v1").word("-z").word("--untracked-files=all"), paths)
    }

    pub fn add(paths: &[DossierPath]) -> Args {
        with_paths(Args::new().word("add"), paths)
    }

    /// A commit of `paths` alone, whatever else is staged.
    pub fn commit(message: &Text, paths: &[DossierPath]) -> Args {
        with_paths(Args::new().word("commit").word("--only").opt("--message", message), paths)
    }

    pub fn head() -> Args {
        Args::new().word("rev-parse").word("HEAD")
    }

    pub fn remotes() -> Args {
        Args::new().word("remote")
    }

    /// The value of a configuration key, such as `branch.main.remote`.
    pub fn config(key: &Text) -> Args {
        Args::new().word("config").value(key)
    }

    /// `HEAD` to `refspec` of `remote`: never `--force`, never a `+` (the refspec starts with `HEAD:`).
    pub fn push(remote: &Text, refspec: &Text) -> Args {
        Args::new().word("push").word("--end-of-options").value(remote).value(refspec)
    }

    fn with_paths(args: Args, paths: &[DossierPath]) -> Args {
        paths.iter().fold(args.word("--"), Args::value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(args: &Args) -> Vec<String> {
        args.tokens().map(|token| token.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn a_date_is_git_s_own_form() {
        assert_eq!(GitDate::new(1_768_035_600, 0).as_env(), "@1768035600 +0000");
        assert_eq!(GitDate::new(1_768_035_600, 5400).as_env(), "@1768035600 +0130");
        assert_eq!(GitDate::new(1_768_035_600, -3600).as_env(), "@1768035600 -0100");
    }

    #[test]
    fn the_last_line_is_the_last_that_is_not_blank() {
        assert_eq!(last_line("fatal: one\n\n  and the repository exists.  \n\n"), "and the repository exists.");
        assert_eq!(last_line("a\rb"), "b", "a bare \\r breaks a line, as in Python");
        assert_eq!(last_line(" \n "), "(no message)");
        assert_eq!(last_line(""), "(no message)");
    }

    /// The push names its remote and `HEAD:<ref>` and nothing that forces; the commit takes the named
    /// paths after `--` and the message as one token.
    #[test]
    fn the_writes_never_force_and_name_only_their_paths() {
        let push = tokens(&call::push(&Text::parse("origin").unwrap(), &Text::parse("HEAD:refs/heads/main").unwrap()));
        assert_eq!(push, ["push", "--end-of-options", "origin", "HEAD:refs/heads/main"]);
        assert!(!push.iter().any(|token| token == "-f" || token.starts_with("--force") || token.starts_with('+')));
        let paths = [DossierPath::parse(".regia/journal").unwrap(), DossierPath::parse("docs/plan.md").unwrap()];
        let commit = tokens(&call::commit(&Text::parse("alpha: checkpoint 2026-01-10T09:00:00Z").unwrap(), &paths));
        assert_eq!(commit, ["commit", "--only", "--message=alpha: checkpoint 2026-01-10T09:00:00Z", "--", ".regia/journal", "docs/plan.md"]);
        assert_eq!(tokens(&call::add(&paths[..1])), ["add", "--", ".regia/journal"]);
        let status = tokens(&call::status(&paths[1..]));
        assert_eq!(status, ["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "docs/plan.md"]);
    }

    #[test]
    fn decoding_is_python_s_text_mode() {
        assert_eq!(decoded(b"a\r\nb\rc\xff"), "a\nb\nc\u{fffd}");
    }
}
