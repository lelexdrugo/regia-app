//! The budget policy, natively (#464): `policy show`, `set`, `reset` and `proposals`, as the prototype's
//! `tools/politica_budget.py` has them. They are subcommands of the CLI's parser, as `budget` is, with no
//! row in the facade table: the facade starts only the prototype's `tools/regia.py`, which has no such
//! command. The policy file is the dossier's, else the compiled defaults; the local override is the state
//! folder's, where the prototype's precheck reads it; the clock is `REGIA_NOW`'s, in the machine's time
//! zone. The JSON is the contract; the text is the catalog's, a line for each of the prototype's.

use std::fmt::Display;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::{DateTime, Local, TimeZone, Utc};
use clap::Subcommand;
use regia_core::policy::{self, read_local, set_local};
use regia_core::{LocalStatus, Origin, Policy, Room, Tracker, TrackerBinding, TrackerConfigV1, Values};
use regia_github::GhTracker;
use regia_i18n::Catalog;
use regia_runtime::dossier_dir;
use regia_store::{default_folder, read_state_file, remove_state_file, write_state_file};
use serde_json::Value;

use crate::clock::Clock;
use crate::{numbers, python_g};

/// The exit of what the command refuses: an override it does not write, a project it does not know.
const REFUSED: u8 = 2;

#[derive(Subcommand)]
pub(crate) enum PolicyCommand {
    /// The policy in use: its values, their source, and the local override with its expiry.
    Show {
        /// Machine-readable output: this is the contract the agents use.
        #[arg(long)]
        json: bool,
    },
    /// For the orchestrator, read only: the project's open Proposals, those of the last 7 days, the caps,
    /// and whether there is room for a new one.
    Proposals {
        #[arg(long)]
        project: String,
        /// Machine-readable output: this is the contract the agents use.
        #[arg(long)]
        json: bool,
    },
    /// Changes values of the policy on this machine, over those in use, then prints the policy.
    Set {
        /// `key=value`, with the keys of the JSON: `automatic_max_pct=80`, `proposal_caps=off`, `backfill=off`.
        #[arg(required = true)]
        pairs: Vec<String>,
        /// `12h`, `3d`, `YYYY-MM-DD` (to the end of that day, local time) or an ISO 8601 instant. Without
        /// it: the expiry of the values in use.
        #[arg(long)]
        until: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Removes the local override, then prints the policy.
    Reset,
}

/// `policy`, with the clock, the dossier and the state folder of this process.
pub(crate) fn run(command: PolicyCommand, catalog: &Catalog) -> Result<ExitCode, String> {
    let sources = Sources {
        file: dossier_dir().map(|dir| dir.join(policy::file_path())),
        folder: default_folder().map_err(|error| error.to_string())?,
        now: Clock::read()?.now.with_timezone(&Utc),
        zone: Local,
    };
    let printed = match command {
        PolicyCommand::Show { json } => sources.show(json, catalog)?,
        PolicyCommand::Set { pairs, until, note } => sources.set(&pairs, until.as_deref(), note.as_deref(), catalog)?,
        PolicyCommand::Reset => sources.reset(catalog)?,
        PolicyCommand::Proposals { project, json } => {
            let config: TrackerConfigV1 = crate::tracker_config()?;
            let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
            let tracker = GhTracker::new(&config);
            proposals(&tracker, &binding, &project, &sources.policy(), sources.now, json, catalog)?
        }
    };
    printed.emit()
}

/// What a command prints, and its exit code.
#[derive(Debug)]
struct Printed {
    stdout: String,
    stderr: String,
    exit: u8,
}

impl Printed {
    fn lines(lines: &[String]) -> Printed {
        Printed { stdout: lines.iter().map(|line| format!("{line}\n")).collect(), stderr: String::new(), exit: 0 }
    }

    fn json(value: &Value) -> Result<Printed, String> {
        let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
        Ok(Printed { stdout: format!("{text}\n"), stderr: String::new(), exit: 0 })
    }

    /// A refusal: one line on stderr and nothing on stdout, exit 2.
    fn refused(line: &str) -> Printed {
        Printed { stdout: String::new(), stderr: format!("{line}\n"), exit: REFUSED }
    }

    fn emit(&self) -> Result<ExitCode, String> {
        std::io::stderr().write_all(self.stderr.as_bytes()).map_err(|error| error.to_string())?;
        std::io::stdout().write_all(self.stdout.as_bytes()).map_err(|error| error.to_string())?;
        Ok(ExitCode::from(self.exit))
    }
}

/// Where the policy is read from: the dossier's file, if there is a dossier, the state folder that holds
/// the local override, and the instant with its time zone.
struct Sources<Tz: TimeZone> {
    file: Option<PathBuf>,
    folder: PathBuf,
    now: DateTime<Utc>,
    zone: Tz,
}

impl<Tz: TimeZone> Sources<Tz>
where
    Tz::Offset: Display,
{
    /// The policy file alone: the prototype's `_carica_file`. Without a dossier, the product's defaults.
    fn file_policy(&self) -> Policy {
        match &self.file {
            Some(path) => Policy::from_file(std::fs::read(path)),
            None => Policy::defaults(),
        }
    }

    fn local(&self) -> (Values, Option<regia_core::Local>) {
        read_local(read_state_file(&self.folder, policy::local_file_name()), self.now, &self.zone)
    }

    /// The policy in use, the override over the file: the prototype's `carica`.
    fn policy(&self) -> Policy {
        self.file_policy().with_local(self.local())
    }

    fn show(&self, json: bool, catalog: &Catalog) -> Result<Printed, String> {
        let policy = self.policy();
        if json { Printed::json(&policy.to_json()) } else { Ok(Printed::lines(&policy_lines(&policy, &self.zone, catalog))) }
    }

    /// `policy set`: the override written whole, then the policy in use. Values that do not fit, alone or
    /// with the file, write nothing.
    fn set(&self, pairs: &[String], until: Option<&str>, note: Option<&str>, catalog: &Catalog) -> Result<Printed, String> {
        let text = match set_local(&self.file_policy(), &self.local(), pairs, until, note, self.now, &self.zone) {
            Ok(text) => text,
            Err(error) => return Ok(Printed::refused(&catalog.message_with("policy-not-written", "reason", &error.to_string()))),
        };
        write_state_file(&self.folder, policy::local_file_name(), &policy::file_bytes(&text))
            .map_err(|error| format!("{}: {error}", self.folder.join(policy::local_file_name()).display()))?;
        Ok(Printed::lines(&policy_lines(&self.policy(), &self.zone, catalog)))
    }

    /// `policy reset`: whether there was an override to remove, then the policy in use.
    fn reset(&self, catalog: &Catalog) -> Result<Printed, String> {
        let removed = remove_state_file(&self.folder, policy::local_file_name())
            .map_err(|error| format!("{}: {error}", self.folder.join(policy::local_file_name()).display()))?;
        let mut lines = vec![catalog.message(if removed { "policy-removed" } else { "policy-nothing-to-remove" })];
        lines.extend(policy_lines(&self.policy(), &self.zone, catalog));
        Ok(Printed::lines(&lines))
    }
}

/// The policy as the prototype's `_stampa_politica` prints it, in the catalog's words, an expiry in `zone`.
/// The warning on the file shows only when the file did not read: without a dossier the defaults are the
/// product's own.
fn policy_lines<Tz: TimeZone>(policy: &Policy, zone: &Tz, catalog: &Catalog) -> Vec<String>
where
    Tz::Offset: Display,
{
    let source = match (policy.origin, policy.with_local) {
        (Origin::File, false) => "policy-source-file",
        (Origin::File, true) => "policy-source-file-local",
        (Origin::Default, false) => "policy-source-default",
        (Origin::Default, true) => "policy-source-default-local",
    };
    let backfill = if policy.backfill {
        catalog.message_with("policy-backfill", "pct", &python_g(policy.backfill_max_pct))
    } else {
        catalog.message("policy-backfill-off")
    };
    let drain = if policy.drain { catalog.message("policy-drain-on") } else { catalog.message_with("policy-drain-off", "file", policy::file_path()) };
    let caps = if policy.proposal_caps {
        let parts = [
            catalog.message_with("policy-caps-open", "n", &policy.open_per_project.to_string()),
            catalog.message_with("policy-caps-new", "n", &policy.new_per_week.to_string()),
        ];
        catalog.message_with("policy-caps", "caps", &parts.join(", "))
    } else {
        catalog.message("policy-caps-off")
    };
    let items = [
        catalog.message_with("policy-automatic", "pct", &python_g(policy.automatic_max_pct)),
        backfill,
        drain,
        caps,
        local_line(policy.local.as_ref(), zone, catalog),
    ];
    let mut lines = vec![catalog.message_with("policy-title", "source", &catalog.message(source))];
    lines.extend(items.into_iter().map(|item| format!("- {item}")));
    if policy.reason.is_some() {
        lines.push(catalog.message_with("policy-defaults", "file", policy::file_path()));
    }
    lines
}

/// The local override's line: active until a local minute, or until it is reset, with its note; otherwise
/// its status, with the reason or the expiry.
fn local_line<Tz: TimeZone>(local: Option<&regia_core::Local>, zone: &Tz, catalog: &Catalog) -> String
where
    Tz::Offset: Display,
{
    let Some(local) = local else {
        return catalog.message("policy-local-none");
    };
    let until = local.until.as_ref().and_then(Value::as_str).filter(|until| !until.is_empty());
    let id = match local.status {
        LocalStatus::Active => None,
        LocalStatus::Expired => Some("policy-local-expired"),
        LocalStatus::Unreadable => Some("policy-local-unreadable"),
        LocalStatus::Ignored => Some("policy-local-ignored"),
    };
    if let Some(id) = id {
        let detail = local.reason.as_deref().filter(|reason| !reason.is_empty()).or(until).unwrap_or_default();
        return catalog.message_with(id, "detail", detail);
    }
    let mut line = match until.and_then(|text| policy::instant(text, zone)) {
        Some(at) => catalog.message_with("policy-local-until", "when", &at.with_timezone(zone).format("%Y-%m-%d %H:%M").to_string()),
        None => catalog.message("policy-local-unbounded"),
    };
    if let Some(note) = local.note.as_ref().and_then(Value::as_str).filter(|note| !note.is_empty()) {
        line.push_str(" · ");
        line.push_str(note);
    }
    line
}

/// `policy proposals`: the room for a new Proposal in `project`, from its open issues and the changes of
/// Status of the last 7 days. A project with no open Objective is refused with 2; changes that do not read
/// leave only the cap on the open ones, and stderr says why, as the prototype's fallback does.
fn proposals(
    tracker: &dyn Tracker,
    binding: &TrackerBinding,
    project: &str,
    policy: &Policy,
    now: DateTime<Utc>,
    json: bool,
    catalog: &Catalog,
) -> Result<Printed, String> {
    let mut tasks = Vec::new();
    for mut issue in tracker.open_issues().map_err(|error| error.to_string())? {
        // The prototype compares the options' names: one the config does not bind is no Proposal, not an error.
        let option = issue.status_option_id.take();
        let mut task = binding.resolve(issue).map_err(|error| error.to_string())?;
        task.status = binding.status(task.number, option.as_deref()).ok().flatten();
        tasks.push(task);
    }
    if !tasks.iter().any(|task| task.objective && task.project.as_deref() == Some(project)) {
        return Ok(Printed::refused(&catalog.message_with("policy-unknown-project", "project", project)));
    }
    let open = policy::open_proposals(&tasks, project);
    let since = now - policy::week();
    let weekly = tracker
        .status_histories(&binding.project_label(project), since)
        .and_then(|histories| policy::new_proposals(&histories, binding, since));
    let (new, warning) = match weekly {
        Ok(numbers) => (Some(numbers), String::new()),
        Err(error) => (None, format!("{}\n", catalog.message_with("policy-weekly-unavailable", "reason", &error.to_string()))),
    };
    let room = Room::new(policy, open, new);
    let mut printed = if json { Printed::json(&room.to_json(project, policy))? } else { Printed::lines(&room_lines(&room, project, policy, catalog)) };
    printed.stderr = warning;
    Ok(printed)
}

/// The room as the prototype's `_stampa_proposte` prints it, in the catalog's words.
fn room_lines(room: &Room, project: &str, policy: &Policy, catalog: &Catalog) -> Vec<String> {
    let count = |n: usize, cap: u64| format!("{n} {}", catalog.message_with("policy-out-of", "cap", &cap.to_string()));
    let list = if room.open.is_empty() { String::new() } else { format!(" ({})", numbers(&room.open)) };
    let open = catalog.message_with("policy-proposals-open", "count", &format!("{}{list}", count(room.open.len(), room.open_cap)));
    let new = room.new.as_ref().map_or_else(|| catalog.message("policy-new-unknown"), |new| count(new.len(), room.new_cap));
    let mut lines = vec![format!("{project}: {open}; {}.", catalog.message_with("policy-proposals-new", "count", &new))];
    let verdict = if !room.caps {
        "policy-room-caps-off"
    } else if room.room_left {
        "policy-room-left"
    } else {
        "policy-room-none"
    };
    lines.push(catalog.message(verdict));
    if policy.reason.is_some() {
        lines.push(catalog.message_with("policy-defaults", "file", policy::file_path()));
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use chrono::{Duration, FixedOffset};
    use regia_core::{IssueState, RawIssue, Status, StatusHistory, TrackerError};
    use regia_i18n::Language;

    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-10T09:00:00Z").unwrap().with_timezone(&Utc)
    }

    fn binding() -> TrackerBinding {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/brief/tracker.json");
        TrackerBinding::new(&TrackerConfigV1::read(std::path::Path::new(path)).unwrap()).unwrap()
    }

    /// The tracker of `TestComando`'s `RegiaFinta`: the open issues, and the changes of Status or the error
    /// they give. It records what it was asked.
    struct Double {
        issues: Vec<RawIssue>,
        histories: Result<Vec<StatusHistory>, String>,
        asked: RefCell<Vec<(String, DateTime<Utc>)>>,
    }

    impl Tracker for Double {
        fn open_issues(&self) -> Result<Vec<RawIssue>, TrackerError> {
            Ok(self.issues.clone())
        }

        fn status_histories(&self, label: &str, since: DateTime<Utc>) -> Result<Vec<StatusHistory>, TrackerError> {
            self.asked.borrow_mut().push((label.to_owned(), since));
            self.histories.clone().map_err(TrackerError::Unavailable)
        }
    }

    /// An open issue of `p` with `status`; the Objective, approved, with `objective`.
    fn issue(binding: &TrackerBinding, number: u64, status: Status, objective: bool) -> RawIssue {
        let mut labels = vec![binding.project_label("p")];
        if objective {
            labels.push(binding.objective_label().to_owned());
        }
        RawIssue {
            number,
            title: format!("t{number}"),
            state: IssueState::Open,
            state_reason: None,
            body: String::new(),
            url: String::new(),
            updated_at: String::new(),
            created_at: None,
            closed_at: None,
            labels,
            parent: None,
            sub_issues: vec![],
            blocked_by: vec![],
            blocked_by_total: 0,
            status_option_id: binding.option_id(status).map(str::to_owned),
            field_values: vec![],
        }
    }

    /// `base(*proposte(…))` of the prototype's tests: the Objective of `p` and Proposals.
    fn double(binding: &TrackerBinding, proposals: &[u64], histories: Result<Vec<StatusHistory>, String>) -> Double {
        let mut issues = vec![issue(binding, 1, Status::Approved, true)];
        issues.extend(proposals.iter().map(|number| issue(binding, *number, Status::Proposal, false)));
        Double { issues, histories, asked: RefCell::new(Vec::new()) }
    }

    fn run(tracker: &Double, project: &str, json: bool, language: Language) -> Printed {
        proposals(tracker, &binding(), project, &Policy::defaults(), t0(), json, &Catalog::new(language)).unwrap()
    }

    /// `test_comando_json`, with the label and the window the tracker was asked for.
    #[test]
    fn the_command_in_json() {
        let binding = binding();
        let two_days_ago = (t0() - Duration::days(2)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let proposal = binding.status_name(Status::Proposal).map(str::to_owned);
        let history = StatusHistory { number: 1, labels: vec![binding.project_label("p")], changes: vec![(proposal, two_days_ago)] };
        let tracker = double(&binding, &[10, 11], Ok(vec![history]));
        let printed = run(&tracker, "p", true, Language::En);
        let json: Value = serde_json::from_str(&printed.stdout).unwrap();
        assert_eq!((printed.exit, printed.stderr.as_str()), (0, ""));
        assert_eq!(
            (&json["project"], &json["open"], &json["open_numbers"], &json["new_7_days"], &json["room_left"]),
            (&Value::from("p"), &Value::from(2), &serde_json::json!([10, 11]), &Value::from(1), &Value::Bool(true))
        );
        assert_eq!(tracker.asked.borrow().as_slice(), [(binding.project_label("p"), t0() - Duration::days(7))]);
    }

    /// `test_comando_testo_senza_posto`, in the prototype's words.
    #[test]
    fn the_command_in_text_with_no_room() {
        let tracker = double(&binding(), &[10, 11, 12], Ok(vec![]));
        let printed = run(&tracker, "p", false, Language::It);
        assert_eq!(printed.exit, 0);
        assert!(printed.stdout.contains("Proposte aperte 3 su 3 (#10, #11, #12)"), "{}", printed.stdout);
        assert!(printed.stdout.contains("Non resta posto"), "{}", printed.stdout);
        let english = run(&tracker, "p", false, Language::En).stdout;
        assert_eq!(english, "p: open Proposals 3 of 3 (#10, #11, #12); new in the last 7 days 0 of 5.\nNo room left: the idea stays in Inbox (protocol §1.2 point 6).\n");
    }

    /// `test_fonte_settimanale_guasta_non_blocca_il_conteggio_delle_aperte`: the cap on the open ones holds,
    /// and stderr says why the weekly count is missing.
    #[test]
    fn a_weekly_source_that_fails_does_not_stop_the_open_count() {
        let tracker = double(&binding(), &[10], Err("gh is down".to_owned()));
        let printed = run(&tracker, "p", true, Language::En);
        let json: Value = serde_json::from_str(&printed.stdout).unwrap();
        assert_eq!((printed.exit, &json["open"], &json["new_7_days"], &json["room_left"]), (0, &Value::from(1), &Value::Null, &Value::Bool(true)));
        assert!(printed.stderr.starts_with("weekly count not available: ") && printed.stderr.contains("gh is down"), "{}", printed.stderr);
        let text = run(&tracker, "p", false, Language::En).stdout;
        assert!(text.contains("new in the last 7 days not available."), "{text}");
    }

    /// The prototype's test of an unknown project (the last of `TestComando`): exit 2, and the weekly
    /// source is not asked.
    #[test]
    fn an_unknown_project_is_refused_with_2() {
        let tracker = double(&binding(), &[10], Ok(vec![]));
        let printed = run(&tracker, "zzz", false, Language::En);
        assert_eq!((printed.exit, printed.stdout.as_str(), printed.stderr.as_str()), (2, "", "unknown project: zzz\n"));
        assert!(tracker.asked.borrow().is_empty());
        // Only an open Objective makes a project known.
        let without = Double { issues: vec![issue(&binding(), 10, Status::Proposal, false)], histories: Ok(vec![]), asked: RefCell::new(Vec::new()) };
        assert_eq!(run(&without, "p", true, Language::En).exit, 2);
    }

    /// A status option the config does not bind is no Proposal, and not an error: the prototype compares
    /// the options' names.
    #[test]
    fn an_option_the_config_does_not_bind_is_no_proposal() {
        let binding = binding();
        let mut tracker = double(&binding, &[10, 11], Ok(vec![]));
        tracker.issues[2].status_option_id = Some("opt-unknown".to_owned());
        let json: Value = serde_json::from_str(&run(&tracker, "p", true, Language::En).stdout).unwrap();
        assert_eq!(json["open_numbers"], serde_json::json!([10]));
    }

    /// The policy's text, line by line the prototype's, here in English: the Italian one is compared with
    /// the prototype's own in `regia-conformance`.
    #[test]
    fn the_text_of_the_policy() {
        let en = Catalog::new(Language::En);
        let utc = FixedOffset::east_opt(0).unwrap();
        assert_eq!(
            policy_lines(&Policy::defaults(), &utc, &en),
            [
                "Budget policy in use (source: default):",
                "- automatic work: below 75% of Claude's 5-hour window",
                "- backfill (steps started only by new ideas in Inbox): below 50%",
                "- backlog to drain (Inbox and drafts): on",
                "- Proposal caps: 3 open per project, 5 new in 7 days",
                "- local override: none",
            ]
        );
        // An override of 3 days with a note, read an hour east of UTC: its expiry is local time.
        let east = FixedOffset::east_opt(3600).unwrap();
        let pairs = ["proposal_caps=off".to_owned(), "backfill=off".to_owned()];
        let written = |until: &str| set_local(&Policy::defaults(), &(Values::default(), None), &pairs, Some(until), Some("only here"), t0(), &east).unwrap();
        let active = Policy::defaults().with_local(read_local(Ok(Some(written("3d").into_bytes())), t0(), &east));
        let lines = policy_lines(&active, &east, &en);
        assert_eq!(lines[0], "Budget policy in use (source: default+local):");
        assert_eq!(lines[2], "- backfill: no threshold of its own, the automatic work's applies");
        assert_eq!(lines[4], "- Proposal caps: off");
        assert_eq!(lines[5], "- local override: active until 2026-01-13 10:00 · only here");
        // Expired, it shows its expiry as the file has it; unreadable, its reason.
        let expired = Policy::defaults().with_local(read_local(Ok(Some(written("12h").into_bytes())), t0() + Duration::hours(13), &east));
        assert_eq!(policy_lines(&expired, &east, &en)[5], "- local override: expired (2026-01-10T21:00:00+00:00)");
        let unreadable = Policy::defaults().with_local(read_local(Ok(Some(b"{".to_vec())), t0(), &east));
        assert!(policy_lines(&unreadable, &east, &en)[5].starts_with("- local override: unreadable (not JSON: "));
        let broken = Policy::from_file(Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
        assert_eq!(policy_lines(&broken, &utc, &en).last().unwrap(), "Warning: policies/budget.yaml does not read, the code's defaults apply.");
    }
}
