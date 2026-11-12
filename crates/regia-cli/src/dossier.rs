//! `checkpoint`, natively, and the dossier's block that `step finish` shares with it: the named files of
//! the project's folder committed and pushed with the user's git (`regia-git`), in the folder given or in
//! the one the project's registry names. A failure is an outcome with its reason, never an error.

use std::cell::OnceCell;

use chrono::Utc;
use clap::Parser;
use regia_git::{Checkpoint, DossierSync, GitDate, Outcome};
use regia_runtime::project_folder;
use serde_json::json;

use crate::clock::Clock;
use crate::reads::parse;
use crate::{flat, print_json};

/// `checkpoint`, natively: the arguments of the facade table (`table/checkpoint.toml`). It is not a
/// subcommand of the CLI's parser: while its group runs on the prototype, the facade takes its argv as it is.
#[derive(Parser)]
#[command(name = "checkpoint")]
struct CheckpointArgs {
    #[arg(long)]
    project: String,
    /// A file to commit besides the named ones, relative to the project's folder. Repeatable.
    #[arg(long)]
    file: Vec<String>,
    /// The project's folder, in place of the one its registry names.
    #[arg(long)]
    folder: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// The project's folder for the dossier's block: the one given, an empty one being none as for the
/// prototype, else the one the project's registry names, read from the tracker once.
pub(crate) struct ProjectFolder<'a> {
    project: &'a str,
    given: Option<&'a str>,
    read: OnceCell<Result<String, String>>,
}

impl<'a> ProjectFolder<'a> {
    pub(crate) fn new(project: &'a str, given: Option<&'a str>) -> Self {
        Self { project, given: given.filter(|folder| !folder.is_empty()), read: OnceCell::new() }
    }

    /// The folder, or why it was not read.
    pub(crate) fn get(&self) -> Result<&str, &str> {
        if let Some(given) = self.given {
            return Ok(given);
        }
        let read = self.read.get_or_init(|| project_folder(self.project).map_err(|error| error.to_string()));
        read.as_deref().map_err(String::as_str)
    }
}

/// The dossier's outcome for `checkpoint` and `step finish`, on the folder, with `message` and the files
/// given besides the named ones.
pub(crate) fn sync(folder: &ProjectFolder, message: &str, files: &[String], date: Option<GitDate>) -> DossierSync {
    match folder.get() {
        Ok(folder) => regia_git::sync(&Checkpoint { folder, message, files, date }),
        Err(error) => DossierSync::unavailable(None, format!("the project's folder was not read: {error}")),
    }
}

/// `checkpoint`, natively: the named files of the dossier committed and pushed, with no lock. Its exit
/// is the prototype's: 1 when it could not even try, a project whose folder could not be read, and for a
/// wrong argument; 0 otherwise, a failed push included.
pub(crate) fn checkpoint(args: &[String]) -> Result<u8, String> {
    let args = match parse::<CheckpointArgs>("checkpoint", args) {
        Ok(args) => args,
        Err(exit) => return Ok(exit),
    };
    let clock = Clock::read()?;
    let message = format!("{}: checkpoint {}", args.project, clock.now.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%SZ"));
    let dossier = sync(&ProjectFolder::new(&args.project, args.folder.as_deref()), &message, &args.file, clock.commit_date());
    let ok = dossier.outcome != Outcome::Unavailable;
    let exit = u8::from(!ok && dossier.folder.is_none());
    let value = json!({"ok": ok, "project": args.project, "dossier": dossier});
    if args.json {
        print_json(&value)?;
    } else {
        for line in flat(&value) {
            println!("{line}");
        }
    }
    Ok(exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `checkpoint` in text is the contract's keys, one leaf a line, as `budget` prints them: the
    /// prototype's sentence (`checkpoint alpha: niente da committare`) would need a catalog.
    #[test]
    fn the_text_of_checkpoint_is_the_contract_s_keys() {
        let dossier = DossierSync::unavailable(None, "the project's folder was not read: project 'alpha' is unknown".to_owned());
        let lines = flat(&json!({"ok": false, "project": "alpha", "dossier": dossier}));
        assert_eq!(
            lines,
            [
                "dossier.ahead: -",
                "dossier.branch: -",
                "dossier.files: []",
                "dossier.folder: -",
                "dossier.outcome: unavailable",
                "dossier.reason: the project's folder was not read: project 'alpha' is unknown",
                "dossier.sha: -",
                "ok: false",
                "project: alpha",
            ]
        );
    }

    /// A wrong argument exits 1, as in the prototype, whose parser exits 1 and not argparse's 2; help
    /// exits 0. None of them reads the clock, the tracker or a repo.
    #[test]
    fn a_wrong_argument_to_checkpoint_exits_1() {
        let run = |argv: &[&str]| checkpoint(&argv.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>()).unwrap();
        assert_eq!(run(&["--project", "alpha", "--no-push"]), 1, "checkpoint has no --no-push");
        assert_eq!(run(&["--file", "a.md"]), 1, "--project is required");
        assert_eq!(run(&["--project"]), 1);
        assert_eq!(run(&["--help"]), 0);
        assert!(CheckpointArgs::try_parse_from(["checkpoint", "--project", "alpha", "--file", "a", "--file=b", "--folder", "x", "--json"]).is_ok());
    }

    /// The folder given wins, an empty one is none, and the registry is read at most once.
    #[test]
    fn the_folder_given_wins_and_an_empty_one_is_none() {
        assert_eq!(ProjectFolder::new("alpha", Some("/x")).get(), Ok("/x"));
        assert!(ProjectFolder::new("alpha", Some("")).given.is_none());
    }
}
