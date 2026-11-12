//! One issue through `gh`'s own subcommands, as the prototype calls them (G4, G5 of the dossier's
//! conformance study): its labels, which a comment reads its issue's project from, and a comment, whose
//! body goes on stdin and never in argv. Each value is a typed argument of `regia-exec`, as `name=value`.

use regia_core::TrackerError;
use regia_exec::{Args, IssueNumber, RepoSlug};
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

    /// `issue <verb> <n> --repo=<owner/repo>`.
    fn issue(&self, verb: &'static str, number: u64) -> Result<Args, TrackerError> {
        let invalid = |error: regia_exec::InvalidValue| TrackerError::Unavailable(error.to_string());
        let number = IssueNumber::new(number).map_err(invalid)?;
        let repo = RepoSlug::parse(&format!("{}/{}", self.owner, self.repo)).map_err(invalid)?;
        Ok(Args::new().word("issue").word(verb).value(&number).opt("--repo", &repo))
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
