//! #444: `plan` in the binary, natively, against the prototype (`tools/regia.py` of the dossier, #239).
//!
//! - **The prototype's own tests** (`tools/tests/test_piano_mermaid.py`, 63): `oracle/plan_capture.py` ran them with the
//!   functions of the plan wrapped, and `tests/plan/vectors.json` holds each call with the prototype's answer: the graph,
//!   the diagram, its labels and its notes. Each is answered here by the port, and must be the same, byte for byte. The
//!   diagram and the notes are given the prototype's command line and the name of its option of a root, from the facade
//!   table: they quote what they are given.
//! - **Graphs at random** from a fixed seed, captured the same way, each with its graph and its diagram.
//! - **Properties** of the cap (`conformita.md` §9.3 of the dossier): at most the cap drawn, every drawn edge between two
//!   drawn nodes, the cut and the drawn nodes together the nodes drawn without a cap, the edges lost counted on those.
//! - **The fixture cases** of `plan`, run natively: below.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use proptest::prelude::*;
use regia_conformance::{Case, CliRun, Recorded, fixtures_dir, json_diff, lf, run_case, translate_python, workspace_root};
use regia_core::graph_order;
use regia_core::plan::{self, Issues, PlanGraph, Quoted, Scope};
use regia_core::{IssueState, Maturity, Metrics, Status, Task, TrackerBinding, TrackerConfigV1};
use regia_facade::Table;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Vectors {
    defaults: Value,
    tests: usize,
    vectors: Vec<Vector>,
}

#[derive(Debug, Clone, Deserialize)]
struct Vector {
    #[serde(rename = "fn")]
    function: String,
    args: Value,
    out: Value,
}

fn vectors_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("plan").join("vectors.json")
}

fn vectors() -> Vectors {
    serde_json::from_str(&fs::read_to_string(vectors_path()).expect("the plan's vectors")).expect("the vectors are JSON")
}

fn binding() -> TrackerBinding {
    let config = TrackerConfigV1::read(&fixtures_dir().join("tracker.json")).expect("the fixtures' tracker.json");
    TrackerBinding::new(&config).expect("it binds every status")
}

/// The prototype's option of a root, as the facade table translates the product's: what its notes name.
fn prototype_root_option() -> String {
    let argv = Table::embedded().python_argv("plan", &["--root".to_owned(), "1".to_owned()]).expect("the table knows plan");
    argv[1].clone()
}

fn pairs(value: &Value) -> Vec<(u64, IssueState)> {
    let state = |value: &Value| serde_json::from_value::<IssueState>(value.clone()).expect("a GitHub state");
    value.as_array().expect("pairs").iter().map(|pair| (pair[0].as_u64().expect("a number"), state(&pair[1]))).collect()
}

/// A task of the vectors: the keys left out have the value of `defaults`.
fn task(value: &Value, defaults: &Value) -> Task {
    let get = |key: &str| value.get(key).unwrap_or(&defaults[key]).clone();
    let blocked_by = pairs(&get("blocked_by"));
    Task {
        number: value["number"].as_u64().expect("a number"),
        title: value["title"].as_str().expect("a title").to_owned(),
        state: serde_json::from_value(get("state")).expect("a state"),
        state_reason: None,
        status: serde_json::from_value::<Option<Status>>(get("status")).expect("a status"),
        project: get("project").as_str().map(str::to_owned),
        project_field: None,
        maturity: serde_json::from_value::<Option<Maturity>>(get("maturity")).expect("a maturity"),
        objective: get("objective").as_bool().expect("a boolean"),
        labels: Vec::new(),
        parent: get("parent").as_u64(),
        sub_issues: pairs(&get("sub")),
        blocked_by_total: value.get("blocked_by_total").and_then(Value::as_u64).unwrap_or(blocked_by.len() as u64),
        blocked_by,
        url: String::new(),
        updated_at: String::new(),
        created_at: None,
        closed_at: None,
        due: None,
    }
}

fn tasks(value: &Value, defaults: &Value) -> Vec<Task> {
    value.as_array().expect("tasks").iter().map(|value| task(value, defaults)).collect()
}

fn graph_json(graph: &PlanGraph) -> Value {
    json!({
        "nodes": graph.nodes, "external": graph.external, "main_line": graph.main_line,
        "boxes": graph.boxes.iter().map(|(child, parent)| [child, parent]).collect::<Vec<_>>(),
        "edges": graph.edges, "nested": graph.nested, "edges_lost": graph.edges_lost, "inbox_hidden": graph.inbox_hidden,
        "truncated": graph.truncated, "root": graph.root, "project": graph.project,
    })
}

fn graph_of(value: &Value) -> PlanGraph {
    let numbers = |key: &str| -> Vec<u64> { value[key].as_array().expect("numbers").iter().map(|n| n.as_u64().expect("a number")).collect() };
    let edges = |key: &str| -> Vec<(u64, u64)> {
        value[key].as_array().expect("edges").iter().map(|e| (e[0].as_u64().expect("a number"), e[1].as_u64().expect("a number"))).collect()
    };
    PlanGraph {
        nodes: numbers("nodes").into_iter().collect(),
        external: numbers("external").into_iter().collect(),
        main_line: numbers("main_line"),
        boxes: edges("boxes").into_iter().collect(),
        edges: edges("edges"),
        nested: edges("nested"),
        edges_lost: value["edges_lost"].as_u64().expect("a count"),
        inbox_hidden: numbers("inbox_hidden"),
        truncated: numbers("truncated"),
        root: value["root"].as_u64(),
        project: value["project"].as_str().map(str::to_owned),
    }
}

/// The graph of `args`, natively, the measures computed from its tasks.
fn native_graph(args: &Value, tasks: &[Task]) -> PlanGraph {
    let graph = graph_order(tasks);
    let issues = Issues::new(tasks, &graph);
    let cap = usize::try_from(args["max_nodes"].as_u64().expect("a cap")).expect("a cap");
    let scope = match (args["root"].as_u64(), args["project"].as_str()) {
        (Some(root), _) => Scope::Root(root),
        (None, Some(project)) => Scope::Project(project),
        (None, None) => panic!("a vector without a scope: {args}"),
    };
    plan::plan_graph(&issues, scope, cap)
}

fn quoted<'a>(args: &'a Value, root_option: &'a str) -> Quoted<'a> {
    Quoted { command: args["command"].as_str().expect("a command line"), root_option, date: args["date"].as_str().expect("a date") }
}

/// The port's answer to a vector.
fn answer(vector: &Vector, defaults: &Value, binding: &TrackerBinding, root_option: &str) -> Value {
    let args = &vector.args;
    match vector.function.as_str() {
        "graph" => graph_json(&native_graph(args, &tasks(&args["tasks"], defaults))),
        "diagram" => {
            let tasks = tasks(&args["tasks"], defaults);
            let measured = graph_order(&tasks);
            json!(plan::diagram(&Issues::new(&tasks, &measured), &graph_of(&args["graph"]), binding, quoted(args, root_option)))
        }
        "title" => {
            let width = usize::try_from(args["width"].as_u64().expect("a width")).expect("a width");
            json!(plan::title_label(&task(&args["task"], defaults), width, args["escaped"].as_bool().expect("a boolean")))
        }
        "status" => {
            let value = &args["task"];
            let metrics = Metrics {
                unblocks: value["unblocks"].as_u64(),
                unblocks_total: value["unblocks_total"].as_u64(),
                chain: value["chain"].as_u64(),
                cycle: value["cycle"].as_bool().expect("a boolean"),
            };
            json!(plan::status_line(binding, &task(value, defaults), &metrics, args["external"].as_bool().expect("a boolean")))
        }
        "notes" => {
            let notes = plan::notes(&graph_of(&args["graph"]), args["root_hint"].as_bool().expect("a boolean"));
            json!(notes.iter().map(|note| plan::diagram_note(note, root_option)).collect::<Vec<_>>())
        }
        "random" => {
            let tasks = tasks(&args["tasks"], defaults);
            let graph = native_graph(args, &tasks);
            let measured = graph_order(&tasks);
            let diagram = plan::diagram(&Issues::new(&tasks, &measured), &graph, binding, quoted(args, root_option));
            json!({"graph": graph_json(&graph), "diagram": diagram})
        }
        other => panic!("a vector of no function the port has: {other}"),
    }
}

/// Every vector the port answers otherwise, with both answers.
fn differences(vectors: &[Vector], defaults: &Value) -> Vec<String> {
    let (binding, root_option) = (binding(), prototype_root_option());
    let mut out = Vec::new();
    for (at, vector) in vectors.iter().enumerate() {
        let rust = answer(vector, defaults, &binding, &root_option);
        if rust != vector.out {
            out.push(format!("vector {at} ({}): {}\n  rust:   {rust}\n  python: {}", vector.function, vector.args, vector.out));
        }
    }
    out
}

/// Acceptance and conformance of #444: on the calls of the prototype's 63 tests, the port gives the prototype's graph,
/// diagram, labels and notes, byte for byte.
#[test]
fn the_prototype_s_tests_get_the_same_answers_from_the_port() {
    let vectors = vectors();
    assert_eq!(vectors.tests, 63, "the vectors come from the prototype's 63 tests");
    let tested: Vec<Vector> = vectors.vectors.iter().filter(|vector| vector.function != "random").cloned().collect();
    for (function, at_least) in [("graph", 40), ("diagram", 15), ("title", 15), ("status", 4), ("notes", 3)] {
        let count = tested.iter().filter(|vector| vector.function == function).count();
        assert!(count >= at_least, "{count} vectors of {function}");
    }
    let diff = differences(&tested, &vectors.defaults);
    assert!(diff.is_empty(), "{} of {} vectors differ:\n{}", diff.len(), tested.len(), diff.join("\n"));
}

/// On graphs drawn at random, with cycles, closed parents, Objectives, other projects and every cap, the port draws the
/// prototype's graph and writes its diagram, byte for byte.
#[test]
fn random_graphs_get_the_prototype_s_graph_and_diagram() {
    let vectors = vectors();
    let random: Vec<Vector> = vectors.vectors.iter().filter(|vector| vector.function == "random").cloned().collect();
    assert!(random.len() >= 100, "{} random graphs", random.len());
    let cut = random.iter().filter(|vector| !vector.out["graph"]["truncated"].as_array().expect("a list").is_empty()).count();
    let cycles = random.iter().filter(|vector| vector.out["diagram"].as_str().is_some_and(|text| text.contains("in ciclo"))).count();
    assert!(cut >= 20 && cycles >= 10, "{cut} with a cut, {cycles} with a cycle");
    let diff = differences(&random, &vectors.defaults);
    assert!(diff.is_empty(), "{} of {} random graphs differ:\n{}", diff.len(), random.len(), diff.join("\n"));
}

/// The comparison can fail: one byte of a diagram, a title of an input, a node of a graph altered are each noticed.
#[test]
fn an_altered_vector_is_noticed() {
    let vectors = vectors();
    let first = |function: &str| vectors.vectors.iter().find(|vector| vector.function == function).cloned().expect(function);
    let mut diagram = first("diagram");
    let text = diagram.out.as_str().expect("a diagram").replacen("flowchart LR", "flowchart LR ", 1);
    diagram.out = json!(text);
    let mut title = first("diagram");
    title.args["tasks"][0]["title"] = json!("another title");
    let mut graph = first("graph");
    graph.out["nodes"].as_array_mut().expect("nodes").push(json!(99_999));
    for (what, vector) in [("a byte of a diagram", diagram), ("a title of an input", title), ("a node of a graph", graph)] {
        assert_eq!(differences(std::slice::from_ref(&vector), &vectors.defaults).len(), 1, "{what} went unnoticed");
    }
}

/// A random graph: up to 14 tasks numbered 1 to 30, in two projects or none, with parents among those before, blockers
/// among all, some closed, some Objectives; then a root among them or a project, and a cap.
fn random_tasks() -> impl Strategy<Value = (Vec<Task>, Option<u64>, Option<String>, usize)> {
    let one = (0..9usize, 0..4usize, 0..3usize, any::<bool>(), 0..13u8, 0..30u64, proptest::collection::vec(1..31u64, 0..4), 0..4u8);
    (proptest::collection::btree_set(1..31u64, 2..15), proptest::collection::vec(one, 14), 0..32u64, any::<bool>(), 1..30usize).prop_map(
        |(numbers, draws, root, by_root, cap)| {
            let numbers: Vec<u64> = numbers.into_iter().collect();
            let statuses = [None, Some(Status::Inbox), Some(Status::Proposal), Some(Status::Approved), Some(Status::Approved),
                Some(Status::InProgress), Some(Status::Decision), Some(Status::Done), Some(Status::Discarded)];
            let projects = [Some("p"), Some("p"), Some("q"), None];
            let maturities = [Some(Maturity::Ready), Some(Maturity::Draft), None];
            let closed: BTreeSet<u64> = numbers.iter().zip(&draws).filter(|(_, draw)| draw.4 == 0).map(|(n, _)| *n).collect();
            let state = |n: u64| if closed.contains(&n) { IssueState::Closed } else { IssueState::Open };
            let mut tasks: Vec<Task> = numbers
                .iter()
                .zip(&draws)
                .enumerate()
                .map(|(at, (&number, draw))| {
                    let (status, project, maturity, has_parent, objective, parent, blockers, extra) = draw.clone();
                    let blocked_by: Vec<(u64, IssueState)> =
                        blockers.into_iter().filter(|b| *b != number && numbers.contains(b)).collect::<BTreeSet<_>>().into_iter().map(|b| (b, state(b))).collect();
                    Task {
                        number,
                        title: format!("t{number}"),
                        state: state(number),
                        state_reason: None,
                        status: statuses[status],
                        project: projects[project].map(str::to_owned),
                        project_field: None,
                        maturity: maturities[maturity],
                        objective: objective == 1,
                        labels: Vec::new(),
                        parent: (has_parent && at > 0).then(|| numbers[usize::try_from(parent).unwrap_or(0) % at]),
                        sub_issues: Vec::new(),
                        blocked_by_total: blocked_by.len() as u64 + u64::from(extra == 0 && !blocked_by.is_empty()),
                        blocked_by,
                        url: String::new(),
                        updated_at: String::new(),
                        created_at: None,
                        closed_at: None,
                        due: None,
                    }
                })
                .collect();
            let children: Vec<(u64, u64, IssueState)> = tasks.iter().filter_map(|task| task.parent.map(|parent| (parent, task.number, task.state))).collect();
            for (parent, child, child_state) in children {
                if let Some(parent) = tasks.iter_mut().find(|task| task.number == parent) {
                    parent.sub_issues.push((child, child_state));
                }
            }
            let root = by_root.then(|| numbers[usize::try_from(root).unwrap_or(0) % numbers.len()]);
            let project = (!by_root).then(|| if root.is_none() && cap % 2 == 0 { "q" } else { "p" }.to_owned());
            (tasks, root, project, cap)
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// §9.3 of the dossier's conformance study: the cap's invariants, against the graph drawn without one.
    #[test]
    fn the_cap_keeps_its_invariants((tasks, root, project, cap) in random_tasks()) {
        let measured = graph_order(&tasks);
        let issues = Issues::new(&tasks, &measured);
        let scope = match (root, project.as_deref()) {
            (Some(root), _) => Scope::Root(root),
            (None, Some(project)) => Scope::Project(project),
            (None, None) => unreachable!("one of the two"),
        };
        let capped = plan::plan_graph(&issues, scope, cap);
        let whole = plan::plan_graph(&issues, scope, usize::MAX);
        prop_assert!(capped.nodes.len() <= cap, "{} nodes over a cap of {cap}", capped.nodes.len());
        for (from, to) in &capped.edges {
            prop_assert!(capped.nodes.contains(from) && capped.nodes.contains(to), "edge {from}→{to} without both ends");
        }
        for (from, to) in &capped.nested {
            let kept = |node: &u64| capped.nodes.contains(node) || Some(*node) == root;
            prop_assert!(kept(from) && kept(to), "nested {from}→{to} without both ends");
        }
        let together: BTreeSet<u64> = capped.nodes.iter().chain(&capped.truncated).copied().collect();
        prop_assert_eq!(&together, &whole.nodes, "the cut and the drawn nodes are the nodes drawn without a cap");
        prop_assert!(capped.nodes.iter().all(|node| !capped.truncated.contains(node)));
        prop_assert_eq!(&capped.inbox_hidden, &whole.inbox_hidden);
        let lost = whole
            .edges
            .iter()
            .chain(&whole.nested)
            .filter(|(from, to)| Some(*from) != root && Some(*to) != root)
            .filter(|(from, to)| !(capped.nodes.contains(from) && capped.nodes.contains(to)))
            .count() as u64;
        prop_assert_eq!(capped.edges_lost, lost, "the edges lost are those of the whole graph with an end cut");
        prop_assert!(capped.main_line.iter().all(|node| capped.nodes.contains(node)));
        for pair in capped.main_line.windows(2) {
            prop_assert!(capped.edges.contains(&(pair[0], pair[1])), "the main line breaks at {}→{}", pair[0], pair[1]);
        }
        // One node is left of a line only by a cut, which keeps the line's start.
        prop_assert!(capped.main_line.len() != 1 || !capped.truncated.is_empty(), "a main line of one node");
        if whole.nodes.len() <= cap {
            prop_assert_eq!(&capped, &whole, "under the cap nothing is cut");
        }
        let mut reversed = tasks.clone();
        reversed.reverse();
        let measured_again = graph_order(&reversed);
        prop_assert_eq!(plan::plan_graph(&Issues::new(&reversed, &measured_again), scope, cap), capped, "the order of the issues counts");
    }
}

/// The product's names of `plan` and of its options, each with the prototype's, as the facade table translates them.
fn names() -> Vec<(String, String)> {
    let table = Table::embedded();
    let mut out = Vec::new();
    for argv in [&["--root", "1", "--mermaid", "--max-nodes", "3"][..], &["--project", "p"]] {
        let english: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let python = table.python_argv("plan", &english).expect("the table knows plan");
        out.push(("plan".to_owned(), python[0].clone()));
        out.extend(english.iter().zip(&python[1..]).filter(|(arg, _)| arg.starts_with("--")).map(|(arg, theirs)| (arg.clone(), theirs.clone())));
    }
    out.sort();
    out.dedup();
    out
}

/// `text` with each of `from` as a whole word, between blanks, backticks or the ends, as `to`.
fn replace_word(text: &str, from: &str, to: &str) -> String {
    let edge = |c: Option<char>| c.is_none_or(|c| c.is_whitespace() || c == '`');
    let mut out = String::new();
    let mut rest = text;
    // The character of the text before `rest`.
    let mut previous: Option<char> = None;
    while let Some(at) = rest.find(from) {
        let before = if at == 0 { previous } else { rest[..at].chars().last() };
        let after = rest[at + from.len()..].chars().next();
        out.push_str(&rest[..at]);
        out.push_str(if edge(before) && edge(after) { to } else { from });
        previous = from.chars().last();
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

/// The one normalization of the cases, declared: the native plan quotes its own command line, in the first line of the
/// diagram and in the line before it, and names its own option of a root in the hint of a project's cut
/// (`docs/contract/commands.md`, `comando`); the prototype quotes its own. Here the product's names become the
/// prototype's, by the table. The recordings never hold the product's names that differ, so nothing else can change.
fn in_the_prototype_s_names(text: &str) -> String {
    let names = names();
    let command = names.iter().find(|(ours, _)| ours == "plan").map(|(_, theirs)| theirs.clone()).expect("the command's name");
    let bin = regia_identity::BIN_NAME;
    let mut out = replace_word(text, &format!("{bin} plan"), &format!("{bin} {command}"));
    for (ours, theirs) in names.iter().filter(|(ours, _)| ours.starts_with("--")) {
        out = replace_word(&out, ours, theirs);
    }
    out
}

/// The normalization changes only the product's names, as whole words.
#[test]
fn the_names_change_and_nothing_else() {
    let bin = regia_identity::BIN_NAME;
    let line = format!("%% {bin} plan --root 1 --mermaid --max-nodes 3 · per il resto usa --root su `{bin} plan --project p`");
    let theirs = in_the_prototype_s_names(&line);
    assert!(!theirs.contains("--root") && !theirs.contains("--max-nodes") && !theirs.contains("--project") && !theirs.contains(" plan "), "{theirs}");
    assert_eq!(in_the_prototype_s_names(&theirs), theirs, "the prototype's names stay");
    for kept in ["--rooted", "x--root", "planned", "a --mermaid b"] {
        assert_eq!(in_the_prototype_s_names(kept), kept);
    }
}

fn plan_cases() -> Vec<Case> {
    Case::all().into_iter().filter(|case| case.command_name() == "plan").collect()
}

/// The binary on `case`, natively (no prototype), in Italian: the language the recordings were made in.
fn native(label: &str, case: &Case) -> CliRun {
    run_case(label, case, |_| vec![(regia_identity::env_var("LANG"), "it".into())])
}

/// Every way the native run differs from what the prototype printed for `case`, with `python` in place of the recording
/// when it is given. JSON whole, text byte for byte, both after the names; an error's words are each side's own, so
/// for an exit other than 0 both print nothing on stdout and something on stderr; otherwise stderr is empty on both
/// sides or on neither (the graph's warnings are the catalog's here).
fn case_differences(case: &Case, run: &CliRun, python: Option<&Recorded>) -> Vec<String> {
    let mut out = Vec::new();
    if run.exit != Some(i32::from(case.spec.exit)) {
        out.push(format!("exit: rust {:?}, python {}; stderr: {}", run.exit, case.spec.exit, run.stderr.trim()));
    }
    let recorded = python.cloned().unwrap_or_else(|| case.recorded());
    let python_text = match &recorded {
        Recorded::Text(text) => text.clone(),
        Recorded::Json(value) => value.to_string(),
    };
    // The names that differ: those the normalization changes.
    let ours: Vec<String> = names().into_iter().filter(|(ours, theirs)| ours.starts_with("--") && ours != theirs).map(|(ours, _)| ours).collect();
    for name in &ours {
        assert!(!python_text.contains(name.as_str()), "{}: the recording holds the product's name {name}", case.name);
    }
    let stdout = lf(&String::from_utf8_lossy(&run.stdout));
    if case.spec.exit != 0 {
        if !stdout.is_empty() || !python_text.trim().is_empty() {
            out.push(format!("stdout of an error: rust {stdout:?}, python {python_text:?}"));
        }
        if run.stderr.trim().is_empty() || case.recorded_stderr().trim().is_empty() {
            out.push(format!("stderr of an error: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
        }
        return out;
    }
    match &recorded {
        Recorded::Json(python) => match (serde_json::from_str::<Value>(&stdout), translate_python("plan", python)) {
            (Ok(mut rust), Ok(python)) => {
                // The two strings that quote the command line, each as a whole.
                for key in ["command", "mermaid"] {
                    if let Some(text) = rust[key].as_str() {
                        rust[key] = json!(in_the_prototype_s_names(text));
                    }
                }
                out.extend(json_diff(&rust, &python).into_iter().map(|line| format!("stdout {line}")));
                if rust["mermaid"] != python["mermaid"] {
                    out.push(format!("mermaid, byte for byte:\n--- rust\n{}\n--- python\n{}", rust["mermaid"].as_str().unwrap_or("null"), python["mermaid"].as_str().unwrap_or("null")));
                }
            }
            (Err(error), _) => out.push(format!("stdout: rust printed no JSON ({error}): {stdout}")),
            (_, Err(error)) => out.push(format!("stdout: the table does not cover the prototype's output: {error}")),
        },
        Recorded::Text(python) => {
            let stdout = in_the_prototype_s_names(&stdout);
            if &stdout != python {
                out.push(format!("stdout, byte for byte:\n--- rust\n{stdout}--- python\n{python}"));
            }
        }
    }
    if run.stderr.trim().is_empty() != case.recorded_stderr().trim().is_empty() {
        out.push(format!("stderr: rust {:?}, python {:?}", run.stderr, case.recorded_stderr()));
    }
    out
}

/// Acceptance 1 and 2: every case of `plan` natively gives what the prototype printed, the diagram byte for byte, the
/// tree and the markdown in Italian byte for byte; it writes nothing to the tracker.
#[test]
fn every_plan_case_natively_gives_the_prototype_s_output() {
    let cases = plan_cases();
    assert!(cases.len() >= 4, "the plan's cases are missing: {}", cases.len());
    let mut failed = Vec::new();
    for case in &cases {
        let run = native(&format!("native-{}", case.name), case);
        let mut diff = case_differences(case, &run, None);
        if !run.writes.is_empty() {
            diff.push(format!("writes: {:?}", run.writes));
        }
        if !diff.is_empty() {
            failed.push(format!("case {}:\n{}", case.name, diff.join("\n")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}

/// The cases show what Acceptance 1 asks: a plan cut over the default cap, one cut with edges lost and the hint of a
/// root, plans with cycles, boxes in boxes, nodes one step outside, the Inbox left out, a root's edge in the note.
#[test]
fn the_plan_cases_show_what_they_claim() {
    let json = |name: &str| translate_python("plan", &Case::named(name).python_output()).expect("the table covers the plan");
    let wide = json("graph-wide-root-json");
    assert_eq!((wide["nodes"].as_array().map(Vec::len), wide["truncated"].as_array().map(Vec::len)), (Some(25), Some(7)), "cut at 25");
    let project = json("graph-wide-project-json");
    assert!(project["edges_lost"].as_u64() > Some(0) && project["truncated"].as_array().is_some_and(|cut| !cut.is_empty()));
    let hint = prototype_root_option();
    assert!(project["mermaid"].as_str().is_some_and(|text| text.contains(&format!(" {hint} "))), "the hint of a root");
    for name in ["graph-cycles-json", "graph-wide-all-json"] {
        let plan = json(name);
        let cycle = plan["nodes"].as_array().expect("nodes").iter().filter(|node| node["cycle"] == true).count();
        assert!(cycle >= 2 && plan["truncated"].as_array().is_some_and(Vec::is_empty), "{name}: {cycle} in a cycle, no cut");
    }
    let all = json("graph-wide-all-json");
    let mermaid = all["mermaid"].as_str().expect("the diagram");
    assert!(mermaid.contains("\n    subgraph "), "a box in a box");
    assert!(all["nodes"].as_array().expect("nodes").iter().any(|node| node["external"] == true));
    assert!(all["nodes"].as_array().expect("nodes").iter().any(|node| node["status"].is_null()), "a task with no status");
    assert_eq!(all["inbox_hidden"], json!([70, 71]));
    let under = json("graph-wide-box-json");
    assert!(under["nested"].as_array().expect("nested").contains(&json!([20, 22])), "the root's edge in the note");
    assert!(under["nodes"].as_array().expect("nodes").iter().all(|node| node["number"] != 20), "the root is not drawn");
    let text = |name: &str| match Case::named(name).recorded() {
        Recorded::Text(text) => text,
        Recorded::Json(_) => panic!("{name} is text"),
    };
    assert!(text("graph-wide-project-text").starts_with("**") && text("graph-wide-project-text").contains("```mermaid\n%% "));
    assert!(!Case::named("graph-cycles-text").recorded_stderr().is_empty(), "the warnings of the cycles");
}

/// The comparison of the cases can fail: a byte of a diagram, a title of a node, a number of a tree and an exit, altered
/// in the recordings, are each a difference.
#[test]
fn an_altered_recording_of_a_plan_is_noticed() {
    let case = Case::named("graph-wide-all-json");
    let run = native("negative-plan-json", &case);
    assert!(case_differences(&case, &run, None).is_empty());
    let Recorded::Json(python) = case.recorded() else { panic!("graph-wide-all-json is JSON") };
    let mut diagram = python.clone();
    diagram["mermaid"] = json!(diagram["mermaid"].as_str().expect("a diagram").replacen("-->", "==>", 1));
    let mut title = python.clone();
    title["nodi"][0]["title"] = json!("another title");
    for (what, altered) in [("a byte of a diagram", diagram), ("a title of a node", title)] {
        assert!(!case_differences(&case, &run, Some(&Recorded::Json(altered))).is_empty(), "{what} went unnoticed");
    }
    let tree = Case::named("graph-tree-text");
    let run = native("negative-plan-text", &tree);
    let Recorded::Text(python) = tree.recorded() else { panic!("graph-tree-text is text") };
    assert!(!case_differences(&tree, &run, Some(&Recorded::Text(python.replacen("#13", "#14", 1)))).is_empty(), "a number of a tree");
    let mut wrong = tree.clone();
    wrong.spec.exit = 1;
    assert!(!case_differences(&wrong, &run, None).is_empty(), "an exit");
}

/// The text for people is the catalog's: in English the tree and the line before the diagram are English, while the
/// diagram, which is data, is the same in both languages.
#[test]
fn the_tree_speaks_the_catalog_s_language_and_the_diagram_stays_data() {
    let english = |label: &str, name: &str| run_case(label, &Case::named(name), |_| vec![(regia_identity::env_var("LANG"), "en".into())]);
    let tree = english("english-tree", "graph-tree-text");
    let text = String::from_utf8_lossy(&tree.stdout).into_owned();
    let catalog = regia_i18n::Catalog::new(regia_i18n::Language::En);
    assert_eq!(tree.exit, Some(0), "{}", tree.stderr);
    assert!(text.contains(&catalog.message("plan-outside")) && text.contains(&catalog.message("state-approved")), "{text}");
    let markdown = english("english-markdown", "graph-command-mermaid-text");
    let head = catalog.message_with("plan-markdown-head", "date", regia_conformance::FIXTURE_TODAY);
    assert!(String::from_utf8_lossy(&markdown.stdout).starts_with(&format!("**{head}**")));
    let json = |run: &CliRun| serde_json::from_slice::<Value>(&run.stdout).expect("JSON")["mermaid"].clone();
    let case = Case::named("graph-wide-all-json");
    assert_eq!(json(&english("english-json", "graph-wide-all-json")), json(&native("italian-json", &case)), "the diagram is data");
}

/// The vectors are the answers of today's prototype: the capture, run again on the dossier named by `REGIA_HOME`, writes
/// the same file.
///
///     REGIA_HOME=<dossier> cargo test -p regia-conformance --test plan -- --ignored
#[test]
#[ignore = "needs the dossier's prototype and Python"]
fn the_vectors_are_today_s_prototype() {
    let home = PathBuf::from(std::env::var(regia_identity::prototype_env_var("HOME")).expect("REGIA_HOME names the dossier"));
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plan-vectors.json");
    let script = workspace_root().join("crates").join("regia-conformance").join("oracle").join("plan_capture.py");
    let python = regia_facade::PythonPrototype::in_dossier(&home, None).interpreter().expect("a Python interpreter");
    let args: Vec<String> = ["-I".into(), script.display().to_string(), "--dossier".into(), home.display().to_string(), "--out".into(), out.display().to_string()].into();
    let output = regia_exec::harness::program(&python, &args, &workspace_root()).run().expect("the capture runs");
    assert!(output.status.success(), "the capture failed: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(&out).expect("the new vectors"), fs::read_to_string(vectors_path()).expect("the vectors"));
}
