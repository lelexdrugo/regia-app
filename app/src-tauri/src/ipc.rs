//! The types the webview receives, and only those. They are the app's own view of the core, not the CLI's
//! JSON: every key is always present, with `null` for "none", so the TypeScript that `ts-rs` generates
//! from them says exactly what arrives. Their names repeat the CLI contract's where one exists.

use std::collections::BTreeMap;

use regia_core::{Brief, BriefTask, Status, normalize_title};
use serde::Serialize;

/// What the webview needs before it draws anything: the threat model's `get_app_info` (§3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AppInfo {
    pub product_name: String,
    pub version: String,
    /// A language tag of the catalog, such as `en` or `it`.
    pub language: String,
    /// The operating system, as Rust names it: `windows`, `macos`, `linux`.
    pub os: String,
    /// True only in a test build that reports what it rendered (`smoke` feature).
    pub smoke: bool,
}

/// The contract's status ids, as the core names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StatusView {
    Inbox,
    Proposal,
    Approved,
    InProgress,
    Decision,
    Done,
    Discarded,
}

impl From<Status> for StatusView {
    fn from(status: Status) -> Self {
        match status {
            Status::Inbox => StatusView::Inbox,
            Status::Proposal => StatusView::Proposal,
            Status::Approved => StatusView::Approved,
            Status::InProgress => StatusView::InProgress,
            Status::Decision => StatusView::Decision,
            Status::Done => StatusView::Done,
            Status::Discarded => StatusView::Discarded,
        }
    }
}

/// A task as the queue shows it, with the measures and the due date the core computed: the view orders nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskView {
    pub number: u64,
    pub title: String,
    pub status: Option<StatusView>,
    /// The project's id, from its label: `null` without one, or with an empty one.
    pub project: Option<String>,
    /// The graph's measures, on the sections it orders; `null` in a cycle and in `in_progress`.
    pub unblocks: Option<u64>,
    pub unblocks_total: Option<u64>,
    pub chain: Option<u64>,
    pub cycle: bool,
    /// The due date the spec declares, `YYYY-MM-DD`, when the core could read one.
    pub due: Option<String>,
    pub due_soon: bool,
    pub overdue: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GraphView {
    pub cycles: Vec<Vec<u64>>,
    pub truncated: Vec<u64>,
}

/// A due date the core could not read, as it was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct IgnoredDueView {
    pub number: u64,
    pub value: String,
}

/// The brief's sections in the core's order, its warnings, and the sections this build does not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BriefView {
    pub waiting_on_owner: Vec<TaskView>,
    /// The tasks of `waiting_on_owner` that need a working session with the owner, in the same order.
    pub session: Vec<TaskView>,
    pub ready: Vec<TaskView>,
    pub in_progress: Vec<TaskView>,
    pub graph: Option<GraphView>,
    pub due_ignored: Vec<IgnoredDueView>,
    pub controller: Option<ControllerView>,
    pub dossier: Vec<DossierView>,
    /// Keyed by project.
    pub outside_tracker: BTreeMap<String, OutsideTrackerView>,
    /// The sections of the contract this build does not produce: the view says so, instead of an all-clear.
    pub not_ported: Vec<String>,
}

pub use sections::*;

/// The three sections the brief reads from this machine, in the contract's shape (`brief.toml` of the facade).
/// The core produces none of them yet, and names them in `not_ported`; #450 (F15) ports them. The view keeps only
/// the fields it shows.
#[expect(dead_code, reason = "nothing builds these until the core reads the sections, #450 (F15)")]
mod sections {
    use serde::Serialize;

    /// What the controller left for the owner. `last_round` says how old it is: a warning of a controller
    /// stopped for days does not read as current.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[cfg_attr(test, derive(ts_rs::TS))]
    pub struct ControllerView {
        pub last_round: Option<String>,
        pub stale: bool,
        pub budget: Option<ControllerBudgetView>,
        pub to_open: Vec<ControllerToOpenView>,
        pub to_resume: Vec<ControllerToResumeView>,
    }

    /// The budget nobody could read for `rounds` rounds: nothing starts by itself until it is read.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[cfg_attr(test, derive(ts_rs::TS))]
    pub struct ControllerBudgetView {
        pub rounds: u64,
        pub since: Option<String>,
        pub reason: Option<String>,
    }

    /// An operation of today the controller could not open by itself.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[cfg_attr(test, derive(ts_rs::TS))]
    pub struct ControllerToOpenView {
        pub name: String,
        pub folder: Option<String>,
        pub prompt: Option<String>,
        pub reason: Option<String>,
    }

    /// A stopped session the controller will not resume by itself. The contract's command for a worker's tab is
    /// left out: the app's sources do not name that tool (regia-agents keeps it), and `how` says what to do.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[cfg_attr(test, derive(ts_rs::TS))]
    pub struct ControllerToResumeView {
        pub project: Option<String>,
        pub session: Option<String>,
        pub resumes: Option<String>,
        pub how: Option<String>,
        pub reason: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[cfg_attr(test, derive(ts_rs::TS))]
    pub struct DossierView {
        pub project: String,
        pub folder: String,
        pub problem: DossierProblem,
        pub branch: Option<String>,
        pub ahead: Option<u64>,
        pub detail: Option<String>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    #[cfg_attr(test, derive(ts_rs::TS))]
    #[serde(rename_all = "snake_case")]
    pub enum DossierProblem {
        FolderMissing,
        NotARepo,
        NoRemote,
        NoUpstream,
        DetachedHead,
        GitFailed,
        NotPushed,
    }

    /// A project's commitments that seem to be outside the tracker: the first `items`, of `total`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[cfg_attr(test, derive(ts_rs::TS))]
    pub struct OutsideTrackerView {
        pub total: u64,
        pub items: Vec<OutsideTrackerItemView>,
        pub more: u64,
    }

    /// A finding of `check`: `text` and `why` are prose of whoever wrote the check, not the catalog's.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[cfg_attr(test, derive(ts_rs::TS))]
    pub struct OutsideTrackerItemView {
        pub check: String,
        pub project: String,
        #[serde(rename = "where")]
        pub place: Option<String>,
        pub text: String,
        pub why: Option<String>,
    }
}

/// One line, capped, nothing hidden: titles and labels are written by people and by agents (threat model T19).
fn line(text: &str) -> String {
    normalize_title(text).text
}

fn task(entry: &BriefTask) -> TaskView {
    let metrics = entry.metrics;
    TaskView {
        number: entry.task.number,
        title: line(&entry.task.title),
        status: entry.task.status.map(StatusView::from),
        project: entry.task.project.as_deref().filter(|project| !project.is_empty()).map(line),
        unblocks: metrics.and_then(|metrics| metrics.unblocks),
        unblocks_total: metrics.and_then(|metrics| metrics.unblocks_total),
        chain: metrics.and_then(|metrics| metrics.chain),
        cycle: metrics.is_some_and(|metrics| metrics.cycle),
        due: entry.due.due.map(|date| date.to_string()),
        due_soon: entry.due.due_soon,
        overdue: entry.due.overdue,
    }
}

fn tasks(section: &[BriefTask]) -> Vec<TaskView> {
    section.iter().map(task).collect()
}

impl From<&Brief> for BriefView {
    fn from(brief: &Brief) -> Self {
        BriefView {
            waiting_on_owner: tasks(&brief.waiting_on_owner),
            session: tasks(&brief.session),
            ready: tasks(&brief.ready),
            in_progress: tasks(&brief.in_progress),
            graph: brief
                .graph
                .as_ref()
                .map(|graph| GraphView { cycles: graph.cycles.clone(), truncated: graph.truncated.clone() }),
            due_ignored: brief
                .due_ignored
                .iter()
                .map(|ignored| IgnoredDueView { number: ignored.number, value: line(&ignored.value) })
                .collect(),
            controller: None,
            dossier: Vec::new(),
            outside_tracker: BTreeMap::new(),
            not_ported: brief.not_ported.iter().map(|&section| section.to_owned()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use regia_core::markdown::{Block, Document, Inline, corpus};
    use regia_core::{LinkRefusal, LinkTarget, parse_document};
    use ts_rs::{Config, TS};

    use super::*;
    use crate::settings::{Density, Direction, LanguageTag, Settings, SettingsChange};

    const HEADER: &str = "// Generated from app/src-tauri/src/ipc.rs by its tests: do not edit by hand.\n\
                          // After changing a type: cargo test -p regia-app -- --ignored write_bindings\n";

    fn bindings_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui/src/lib/bindings.ts")
    }

    /// One file, one declaration per type. Issue numbers fit a JavaScript number, and `serde_json` sends
    /// them as numbers, so `u64` is `number` and not ts-rs's default `bigint`. The markdown tree is the
    /// core's own type (`regia-core`, feature `ts`): it was made for the view.
    fn bindings() -> String {
        let config = Config::new().with_large_int("number");
        let declarations = [
            AppInfo::decl(&config),
            BriefView::decl(&config),
            TaskView::decl(&config),
            StatusView::decl(&config),
            GraphView::decl(&config),
            IgnoredDueView::decl(&config),
            ControllerView::decl(&config),
            ControllerBudgetView::decl(&config),
            ControllerToOpenView::decl(&config),
            ControllerToResumeView::decl(&config),
            DossierView::decl(&config),
            DossierProblem::decl(&config),
            OutsideTrackerView::decl(&config),
            OutsideTrackerItemView::decl(&config),
            Document::decl(&config),
            Block::decl(&config),
            Inline::decl(&config),
            LinkTarget::decl(&config),
            LinkRefusal::decl(&config),
            Settings::decl(&config),
            SettingsChange::decl(&config),
            Direction::decl(&config),
            Density::decl(&config),
            LanguageTag::decl(&config),
        ];
        let mut out = HEADER.to_owned();
        for declaration in declarations {
            out.push_str("\nexport ");
            out.push_str(&declaration);
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_frontend_has_the_types_of_this_build() {
        let committed = std::fs::read_to_string(bindings_path()).expect("app/ui/src/lib/bindings.ts");
        assert_eq!(committed.replace("\r\n", "\n"), bindings(), "stale bindings: run the ignored test write_bindings");
    }

    #[test]
    #[ignore = "writes app/ui/src/lib/bindings.ts"]
    fn write_bindings() {
        std::fs::write(bindings_path(), bindings()).unwrap();
    }

    fn harness_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui/harness").join(name)
    }

    fn read_harness(name: &str) -> String {
        std::fs::read_to_string(harness_path(name)).expect("a file of app/ui/harness").replace("\r\n", "\n")
    }

    /// The hostile corpus as the core parses it, for the frontend's DOM proof (threat model V07), with the
    /// markdown it came from for the proof's deliberately unsafe renderer, and the text the DOM must show. The
    /// 1 MiB case stays out: the app's smoke run renders it, from the core.
    fn harness_corpus() -> String {
        let cases: Vec<serde_json::Value> = corpus::cases()
            .into_iter()
            .filter(|case| case.repeat.is_none())
            .map(|case| {
                let document = parse_document(&case.markdown);
                serde_json::json!({ "name": case.name, "markdown": case.markdown, "document": document, "shows": case.shows })
            })
            .collect();
        serde_json::to_string_pretty(&cases).unwrap() + "\n"
    }

    /// The documents the views' proofs render (`harness/documents.json`): their markdown is written by hand,
    /// their tree is the core's, so a proof sees what a view would get.
    fn harness_documents(committed: &str) -> String {
        let mut documents: Vec<serde_json::Value> = serde_json::from_str(committed).expect("harness/documents.json");
        for document in &mut documents {
            let markdown = document["markdown"].as_str().expect("each document has its markdown").to_owned();
            document["document"] = serde_json::to_value(parse_document(&markdown)).unwrap();
        }
        serde_json::to_string_pretty(&documents).unwrap() + "\n"
    }

    #[test]
    fn the_harness_has_the_trees_of_this_build() {
        let stale = "stale harness: run the ignored test write_harness";
        assert_eq!(read_harness("corpus.json"), harness_corpus(), "{stale}");
        let documents = read_harness("documents.json");
        assert_eq!(documents, harness_documents(&documents), "{stale}");
    }

    #[test]
    #[ignore = "writes app/ui/harness/corpus.json and documents.json"]
    fn write_harness() {
        std::fs::write(harness_path("corpus.json"), harness_corpus()).unwrap();
        let documents = harness_documents(&read_harness("documents.json"));
        std::fs::write(harness_path("documents.json"), documents).unwrap();
    }

    #[test]
    fn a_title_reaches_the_webview_on_one_line_and_without_hidden_characters() {
        let title = "Approve\u{202E} now\nand \u{1B}[31mthen";
        assert_eq!(normalize_title(title).text, "Approve‹U+202E› now and then");
    }

    #[test]
    fn numbers_and_absent_values_have_the_types_the_webview_reads() {
        let out = bindings();
        assert!(!out.contains("bigint"), "{out}");
        assert!(out.contains("graph: GraphView | null"), "{out}");
        assert!(out.contains("controller: ControllerView | null"), "{out}");
        // The contract's key, which is a keyword in Rust.
        assert!(out.contains("where: string | null"), "{out}");
    }

    fn entry(number: u64, title: &str, status: Status, project: Option<&str>) -> BriefTask {
        let task = regia_core::Task {
            number,
            title: title.to_owned(),
            state: regia_core::IssueState::Open,
            state_reason: None,
            status: Some(status),
            project: project.map(str::to_owned),
            project_field: None,
            maturity: None,
            objective: false,
            labels: vec![],
            parent: None,
            sub_issues: vec![],
            blocked_by: vec![],
            blocked_by_total: 0,
            url: String::new(),
            updated_at: String::new(),
            created_at: None,
            closed_at: None,
            due: None,
        };
        BriefTask { task, metrics: None, due: regia_core::DueMark::default() }
    }

    fn measured(mut entry: BriefTask, unblocks: Option<u64>, total: Option<u64>, chain: Option<u64>) -> BriefTask {
        entry.metrics = Some(regia_core::Metrics { unblocks, unblocks_total: total, chain, cycle: chain.is_none() });
        entry
    }

    fn due(mut entry: BriefTask, day: &str, soon: bool, overdue: bool) -> BriefTask {
        entry.due = regia_core::DueMark { due: regia_core::parse_date(day), due_soon: soon, overdue };
        entry
    }

    /// The core's brief, with entries in an order no field of theirs gives: by number, by title, by due date.
    fn brief() -> Brief {
        let session = measured(entry(50, "needs a session", Status::Decision, Some("beta")), Some(0), Some(0), Some(1));
        Brief {
            waiting_on_owner: vec![
                due(measured(entry(60, "b", Status::Decision, Some("alpha")), Some(1), Some(1), Some(2)), "2026-01-12", true, false),
                session.clone(),
                measured(entry(21, "a", Status::Proposal, Some("")), Some(0), Some(2), Some(2)),
                due(measured(entry(53, "c", Status::Decision, None), Some(0), Some(0), Some(1)), "2026-01-09", false, true),
                measured(entry(18, "cycle", Status::Proposal, Some("alpha")), None, None, None),
            ],
            ready: vec![due(measured(entry(52, "far", Status::Approved, Some("gamma")), Some(0), Some(0), Some(1)), "2026-01-14", false, false)],
            in_progress: vec![entry(9, "running", Status::InProgress, Some("alpha\nnext\u{202E}"))],
            by_project: BTreeMap::new(),
            session: vec![session],
            graph: None,
            due_ignored: vec![regia_core::IgnoredDue { number: 56, value: "soon\u{1B}[31m".to_owned() }],
            not_ported: regia_core::brief::NOT_PORTED,
        }
    }

    #[test]
    fn the_view_keeps_the_cores_order_and_its_measures() {
        let core = brief();
        let view = BriefView::from(&core);
        let numbers = |tasks: &[TaskView]| tasks.iter().map(|task| task.number).collect::<Vec<_>>();
        assert_eq!(numbers(&view.waiting_on_owner), [60, 50, 21, 53, 18]);
        assert_eq!(numbers(&view.session), [50]);
        assert_eq!(numbers(&view.ready), [52]);
        assert_eq!(numbers(&view.in_progress), [9]);

        let first = &view.waiting_on_owner[0];
        assert_eq!((first.status, first.project.as_deref()), (Some(StatusView::Decision), Some("alpha")));
        assert_eq!((first.unblocks, first.unblocks_total, first.chain, first.cycle), (Some(1), Some(1), Some(2), false));
        assert_eq!((first.due.as_deref(), first.due_soon, first.overdue), (Some("2026-01-12"), true, false));
        let upstream = &view.waiting_on_owner[2];
        assert_eq!((upstream.unblocks, upstream.unblocks_total), (Some(0), Some(2)), "unblocks next to its total");
        assert_eq!(upstream.project, None, "an empty project label is no project");
        let overdue = &view.waiting_on_owner[3];
        assert_eq!((overdue.due.as_deref(), overdue.due_soon, overdue.overdue), (Some("2026-01-09"), false, true));
        let cycle = &view.waiting_on_owner[4];
        assert_eq!((cycle.unblocks, cycle.chain, cycle.cycle), (None, None, true));
        assert_eq!(view.ready[0].due.as_deref(), Some("2026-01-14"), "a date far away is there, not soon");
        assert!(!view.ready[0].due_soon && !view.ready[0].overdue);
        let running = &view.in_progress[0];
        assert_eq!((running.unblocks, running.chain, running.cycle), (None, None, false), "no graph keys in progress");
        assert_eq!(running.project.as_deref(), Some("alpha next‹U+202E›"), "a label is one line, nothing hidden");
        assert_eq!(view.due_ignored, [IgnoredDueView { number: 56, value: "soon".to_owned() }]);
    }

    #[test]
    fn the_sections_the_core_does_not_read_are_named_not_empty() {
        let view = BriefView::from(&brief());
        assert_eq!(view.not_ported, ["controller", "dossier", "outside_tracker"]);
        assert_eq!(view.controller, None);
        assert!(view.dossier.is_empty() && view.outside_tracker.is_empty());
        let json = serde_json::to_value(&view).unwrap();
        for key in ["controller", "dossier", "outside_tracker", "not_ported", "session", "due_ignored"] {
            assert!(json.get(key).is_some(), "every key is always there: {key}");
        }
        assert_eq!(json["waiting_on_owner"][0]["status"], "decision");
    }
}
