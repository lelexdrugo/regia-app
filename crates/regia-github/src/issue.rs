//! One issue through `gh`'s own subcommands, as the prototype calls them (G2, G4, G5 of the dossier's
//! conformance study): its labels, which a comment reads its issue's project from; a comment, whose body
//! goes on stdin, or in argv for a change of status, as the prototype writes it; a new issue; a change of
//! its labels. Each value is a typed argument of `regia-exec`, as `name=value`.

use regia_core::TrackerError;
use regia_exec::{Args, IssueNumber, RepoSlug, Text};
use serde::Deserialize;

use crate::GhTracker;

/// What `gh issue view <n> --json labels` prints.
#[derive(Deserialize)]
struct View {
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

impl GhTracker {
    /// `gh issue view <n> --repo=<owner/repo> --json=labels`: the names of the issue's labels.
    pub(crate) fn issue_labels(&self, number: u64) -> Result<Vec<String>, TrackerError> {
        let args = self.issue("view", number)?.word("--json=labels");
        let printed = self.run_gh("issue view", args, None)?;
        let view: View = serde_json::from_str(&printed).map_err(|error| TrackerError::BadResponse(error.to_string()))?;
        Ok(view.labels.into_iter().map(|label| label.name).collect())
    }

    /// `gh issue comment <n> --repo=<owner/repo> --body-file=-`, with the body on stdin: what `gh` printed,
    /// the comment's URL.
    pub(crate) fn issue_comment(&self, number: u64, body: &str) -> Result<String, TrackerError> {
        let args = self.issue("comment", number)?.word("--body-file=-");
        self.run_gh("issue comment", args, Some(body))
    }

    /// `gh issue comment <n> --repo=<owner/repo> --body=<body>`: the body in argv, and nothing read back,
    /// as a change of status with a comment writes it (G4).
    pub(crate) fn issue_comment_inline(&self, number: u64, body: &str) -> Result<(), TrackerError> {
        let args = self.issue("comment", number)?.opt("--body", &text(body)?);
        self.run_gh("issue comment", args, None).map(|_| ())
    }

    /// `gh issue create --repo=<owner/repo> --title=<title> --body-file=- --label=<label>…`, with the body on
    /// stdin: what `gh` printed, the issue's URL last (G2).
    pub(crate) fn issue_create(&self, title: &str, body: &str, labels: &[String]) -> Result<String, TrackerError> {
        let mut args = Args::new().word("issue").word("create").opt("--repo", &self.repo_slug()?).opt("--title", &text(title)?);
        args = args.word("--body-file=-");
        for label in labels {
            args = args.opt("--label", &text(label)?);
        }
        self.run_gh("issue create", args, Some(body))
    }

    /// `gh issue edit <n> --repo=<owner/repo> --add-label=<add> [--remove-label=<remove>]` (G5).
    pub(crate) fn issue_edit_labels(&self, number: u64, add: &str, remove: Option<&str>) -> Result<(), TrackerError> {
        let mut args = self.issue("edit", number)?.opt("--add-label", &text(add)?);
        if let Some(remove) = remove {
            args = args.opt("--remove-label", &text(remove)?);
        }
        self.run_gh("issue edit", args, None).map(|_| ())
    }

    /// `issue <verb> <n> --repo=<owner/repo>`.
    fn issue(&self, verb: &'static str, number: u64) -> Result<Args, TrackerError> {
        let number = IssueNumber::new(number).map_err(invalid)?;
        Ok(Args::new().word("issue").word(verb).value(&number).opt("--repo", &self.repo_slug()?))
    }

    fn repo_slug(&self) -> Result<RepoSlug, TrackerError> {
        RepoSlug::parse(&format!("{}/{}", self.owner, self.repo)).map_err(invalid)
    }

    /// Runs `gh`; a failure is its stderr, or else its stdout, as the prototype reports it.
    fn run_gh(&self, name: &str, args: Args, stdin: Option<&str>) -> Result<String, TrackerError> {
        let gh = self.gh.as_ref().map_err(|error| TrackerError::Unavailable(error.clone()))?;
        let mut launch = gh.launch(args, &self.work_dir);
        if let Some(stdin) = stdin {
            launch = launch.stdin(stdin.as_bytes().to_vec());
        }
        let output = launch.run().map_err(|error| TrackerError::Unavailable(error.to_string()))?;
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_owned();
        if !output.status.success() {
            let stderr = text(&output.stderr);
            let said = if stderr.is_empty() { text(&output.stdout) } else { stderr };
            return Err(TrackerError::Unavailable(format!("gh {name}: {said}")));
        }
        String::from_utf8(output.stdout).map_err(|error| TrackerError::BadResponse(error.to_string()))
    }
}

/// A value `gh` reads as data: a title, a label, a body in argv.
fn text(value: &str) -> Result<Text, TrackerError> {
    Text::parse(value).map_err(invalid)
}

fn invalid(error: regia_exec::InvalidValue) -> TrackerError {
    TrackerError::Unavailable(error.to_string())
}
