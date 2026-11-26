//! `plan`, natively: the graph of the open tasks of a project or under a root (`regia_core::plan`), with the arguments
//! of the facade table (`table/plan.toml`). Not a subcommand of the CLI's parser: while its group runs on the
//! prototype, the facade takes its argv as it is. The JSON is the contract (`docs/contract/commands.md`); the diagram in
//! it is data in the prototype's words, with this command line in its first line. The tree, the line before the
//! diagram and the errors are the catalog's (`locales/*/plan.ftl`).

use clap::{ArgGroup, Parser};
use regia_core::plan::{self, DEFAULT_MAX_NODES, Issues, Note, PlanGraph, Quoted, Scope, TREE_TITLE, TreeLine};
use regia_core::{GraphOrder, Metrics, Task, Tracker, TrackerBinding, graph_order};
use regia_github::GhTracker;
use regia_i18n::Catalog;
use regia_identity::BIN_NAME;

use crate::clock::Clock;
use crate::print_json;
use crate::reads::{parse, python_int};

/// The command line's names, as the table gives them: the first line of a diagram quotes them, and so does the hint
/// of a project's cut.
const COMMAND: &str = "plan";
const PROJECT: &str = "--project";
const ROOT: &str = "--root";
const MERMAID: &str = "--mermaid";
const MAX_NODES: &str = "--max-nodes";

#[derive(Parser)]
#[command(name = "plan", args_override_self = true)]
#[command(group(ArgGroup::new("scope").required(true).args(["project", "root"])))]
struct PlanArgs {
    /// The open tasks of the project, Objectives left out.
    #[arg(long)]
    project: Option<String>,
    /// The open descendants of this issue, an Objective too: the root itself is not drawn.
    #[arg(long, allow_negative_numbers = true)]
    root: Option<String>,
    /// Markdown to paste into a dated comment on the root: a line in bold, then the mermaid block.
    #[arg(long)]
    mermaid: bool,
    /// The cap of drawn nodes, 25 if not given.
    #[arg(long, allow_negative_numbers = true)]
    max_nodes: Option<String>,
    /// Machine-readable output: this is the contract the agents use.
    #[arg(long)]
    json: bool,
}

/// `command` natively, if it is `plan`: its exit code, or the error to print.
pub(crate) fn run(command: &str, args: &[String], catalog: &Catalog) -> Option<Result<u8, String>> {
    (command == COMMAND).then(|| parse::<PlanArgs>(command, args).map(|args| plan(&args, catalog)).unwrap_or_else(Ok))
}

/// A number given to `option`, read as the prototype's parser reads it, with Python's `int()`.
fn number(option: &str, text: &str) -> Result<i64, String> {
    python_int(text).ok_or_else(|| format!("argument {option}: invalid int value: '{text}'"))
}

/// The cap is checked before the tracker is read; the root and the project after, on every issue read.
fn plan(args: &PlanArgs, catalog: &Catalog) -> Result<u8, String> {
    let default = i64::try_from(DEFAULT_MAX_NODES).unwrap_or(i64::MAX);
    let max_nodes = args.max_nodes.as_deref().map_or(Ok(default), |text| number(MAX_NODES, text))?;
    if max_nodes < 1 {
        return Err(catalog.message_with("plan-max-nodes", "option", MAX_NODES));
    }
    let root = args.root.as_deref().map(|text| number(ROOT, text)).transpose()?;
    let config = crate::tracker_config()?;
    let binding = TrackerBinding::new(&config).map_err(|error| error.to_string())?;
    // Every issue, closed ones too: an open sub-issue under a closed parent is still in the root's tree. Each status is
    // read, as the graph counts by status.
    let tasks: Vec<Task> = GhTracker::new(&config)
        .all_issues()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|issue| binding.resolve(issue).map_err(|error| error.to_string()))
        .collect::<Result<_, _>>()?;
    let measured = graph_order(&tasks);
    let issues = Issues::new(&tasks, &measured);
    let scope = match (root, args.project.as_deref()) {
        (Some(root), _) => match u64::try_from(root).ok().filter(|root| issues.task(*root).is_some()) {
            Some(root) => Scope::Root(root),
            None => return Err(catalog.message_with("plan-root-missing", "number", &root.to_string())),
        },
        (None, Some(project)) if issues.has_project(project) => Scope::Project(project),
        (None, Some(project)) => return Err(catalog.message_with("plan-project-missing", "project", project)),
        (None, None) => return Err(format!("{ROOT} or {PROJECT} is required")),
    };
    let graph = plan::plan_graph(&issues, scope, usize::try_from(max_nodes).unwrap_or(usize::MAX));
    let command = command_line(root, args.project.as_deref(), args.mermaid, max_nodes);
    let date = Clock::read()?.today().format("%Y-%m-%d").to_string();
    let diagram = args.mermaid.then(|| plan::diagram(&issues, &graph, &binding, Quoted { command: &command, root_option: ROOT, date: &date }));
    if args.json {
        let output = plan::output(&issues, &graph, command, date, diagram);
        print_json(&serde_json::to_value(&output).map_err(|error| error.to_string())?)?;
        return Ok(0);
    }
    warn_about(&measured, &graph, catalog);
    if graph.nodes.is_empty() && graph.inbox_hidden.is_empty() {
        println!("{}", catalog.message("plan-nothing"));
    } else if let Some(diagram) = diagram {
        print!("{}", markdown(&diagram, &command, &date, catalog));
    } else {
        print!("{}", tree_text(&issues, &graph, catalog));
    }
    Ok(0)
}

/// The command line that draws this plan again, in this command line's words: the scope, `--mermaid`, the cap only
/// when it is not the default, never `--json`.
fn command_line(root: Option<i64>, project: Option<&str>, mermaid: bool, max_nodes: i64) -> String {
    let scope = match root {
        Some(root) => format!("{ROOT} {root}"),
        None => format!("{PROJECT} {}", project.unwrap_or_default()),
    };
    let mut line = format!("{BIN_NAME} {COMMAND} {scope}");
    if mermaid {
        line.push(' ');
        line.push_str(MERMAID);
    }
    if usize::try_from(max_nodes).ok() != Some(DEFAULT_MAX_NODES) {
        line.push_str(&format!(" {MAX_NODES} {max_nodes}"));
    }
    line
}

/// The graph's warnings on what the plan draws, on stderr, beside its text.
fn warn_about(measured: &GraphOrder, graph: &PlanGraph, catalog: &Catalog) {
    let numbers = |list: &[u64]| list.iter().map(|number| format!("#{number}")).collect::<Vec<_>>().join(", ");
    for cycle in measured.cycles.iter().filter(|cycle| cycle.iter().any(|number| graph.nodes.contains(number))) {
        eprintln!("  ! {}", catalog.message_with("brief-cycle", "reason", &numbers(cycle)));
    }
    let truncated: Vec<u64> = measured.truncated.iter().copied().filter(|number| graph.nodes.contains(number)).collect();
    if !truncated.is_empty() {
        eprintln!("  ! {}", catalog.message_with("brief-truncated", "reason", &numbers(&truncated)));
    }
    let waiting: Vec<String> = measured
        .children_waiting
        .iter()
        .filter(|(child, parent)| graph.nodes.contains(child) || graph.nodes.contains(parent))
        .map(|(child, parent)| format!("#{child} → #{parent}"))
        .collect();
    if !waiting.is_empty() {
        eprintln!("  ! {}", catalog.message_with("plan-children-waiting", "edges", &waiting.join(", ")));
    }
}

/// The markdown to paste: a line in bold with the day and the command line, then the fenced diagram.
fn markdown(diagram: &str, command: &str, date: &str, catalog: &Catalog) -> String {
    let head = catalog.message_with("plan-markdown-head", "date", date);
    format!("**{head}** (`{command}`; {})\n\n```mermaid\n{diagram}```\n", catalog.message("plan-markdown-legend"))
}

/// What a task unblocks and its chain after a line of the tree, or that it is in a cycle; nothing if nothing is
/// downstream.
fn measures_text(metrics: &Metrics, catalog: &Catalog) -> String {
    if metrics.cycle {
        return format!("  · {}", catalog.message("plan-cycle"));
    }
    let Some(shown) = plan::measures(metrics) else { return String::new() };
    let total = if shown.total == shown.unblocks {
        String::new()
    } else {
        format!(" {}", catalog.message_with("plan-unblocks-total", "total", &shown.total.to_string()))
    };
    let unblocks = catalog.message_with("plan-unblocks", "unblocks", &shown.unblocks.to_string());
    format!("  · {unblocks}{total} · {}", catalog.message_with("plan-chain", "chain", &shown.chain.to_string()))
}

fn note_text(note: &Note, catalog: &Catalog) -> String {
    match note {
        Note::InboxHidden(list) => format!("{} {}", catalog.message_with("plan-note-inbox", "count", &list.len().to_string()), plan::numbers(list)),
        Note::Nested(list) => format!("{} {}", catalog.message("plan-note-nested"), plan::edge_numbers(list)),
        Note::Truncated { numbers, edges_lost, root_hint } => {
            let cut = catalog.message_with("plan-note-truncated", "count", &numbers.len().to_string());
            let lost = catalog.message_with("plan-note-lost", "lost", &edges_lost.to_string());
            let hint = if *root_hint { format!(" · {}", catalog.message_with("plan-note-hint", "option", ROOT)) } else { String::new() };
            format!("{cut}, {lost} {}{hint}", plan::numbers(numbers))
        }
    }
}

/// The indented tree: a line per node, `*` before those of the main line, its blockers after an arrow; then the nodes
/// one step outside, the notes and the main line.
fn tree_text(issues: &Issues, graph: &PlanGraph, catalog: &Catalog) -> String {
    let mut out = String::new();
    for line in plan::tree(issues, graph) {
        let text = match line {
            TreeLine::Task(entry) => {
                let task = entry.task;
                let status = task.status.map_or_else(|| "—".to_owned(), |status| catalog.message(&format!("state-{}", status.id())));
                let project = task.project.as_deref().filter(|project| !project.is_empty()).unwrap_or("?");
                let outside = if entry.external { format!("  [{project}]") } else { String::new() };
                let waits = if entry.waits_for.is_empty() {
                    String::new()
                } else {
                    let list: Vec<String> = entry.waits_for.iter().map(|number| format!("#{number}")).collect();
                    format!("  ← {}", catalog.message_with("plan-waits", "numbers", &list.join(",")))
                };
                format!(
                    "{}{}#{} [{status}] {}{outside}{}{waits}",
                    "  ".repeat(entry.depth),
                    if entry.on_main_line { '*' } else { ' ' },
                    task.number,
                    plan::title_label(task, TREE_TITLE, false),
                    measures_text(&entry.metrics, catalog)
                )
            }
            TreeLine::Outside => catalog.message("plan-outside"),
            TreeLine::Note(note) => format!("({})", note_text(&note, catalog)),
            TreeLine::MainLine(line) => {
                let line: Vec<String> = line.iter().map(|number| format!("#{number}")).collect();
                format!("(* {})", catalog.message_with("plan-main-line", "line", &line.join(" → ")))
            }
        };
        out.push_str(&text);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use regia_i18n::Language;

    use super::*;

    fn run_with(argv: &[&str]) -> Option<u8> {
        let args: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        match run("plan", &args, &Catalog::new(Language::En))? {
            Ok(code) => Some(code),
            Err(_) => Some(1),
        }
    }

    /// The prototype's parser: one of `--project` and `--root`, never both; numbers as `int()` reads them; a cap under
    /// 1 is an error before the tracker is read. Each exits 1, help 0.
    #[test]
    fn a_wrong_argument_exits_1_and_help_0() {
        for argv in [&[][..], &["--root", "1", "--project", "p"], &["--root", "one"], &["--project", "p", "--max-nodes", "0"], &["--project", "p", "--max-nodes", "-3"], &["--root", "1", "--max-nodes", "2.5"], &["--project"]] {
            assert_eq!(run_with(argv), Some(1), "{argv:?}");
        }
        assert_eq!(run_with(&["--help"]), Some(0));
        assert_eq!(run("brief", &[], &Catalog::new(Language::En)).map(|_| ()), None, "not a command of this module");
    }

    /// The command line in this command line's words: the scope first, `--mermaid`, the cap only when it is not 25.
    #[test]
    fn the_command_line_is_this_one_s() {
        assert_eq!(command_line(Some(7), None, true, 25), format!("{BIN_NAME} plan --root 7 --mermaid"));
        assert_eq!(command_line(None, Some("alpha"), false, 3), format!("{BIN_NAME} plan --project alpha --max-nodes 3"));
        assert_eq!(command_line(Some(-2), None, false, 25), format!("{BIN_NAME} plan --root -2"));
    }
}
