//! `plan`: the open tasks of a project, or under a root, as a graph to draw (#239 of the dossier, whose format is
//! its `docs/design/contratto-piano.md` §3), and the two drawings of it. The diagram is data in the prototype's
//! words (`formats/plan_v1.toml`): it goes into a dated comment of the tracker. The indented tree is text for
//! people: this module gives its lines, and the command line writes them in the catalog's words. Pure functions
//! of the issues read and of their measures in the graph (`graph_order`): no second read of the tracker, no clock.
//!
//! The perimeter is a root's open descendants, through its sub-issues at any depth, or a project's open tasks;
//! Objectives never. One step outside it in both directions, the open blockers of a task of the perimeter and the
//! open tasks it blocks, other projects' too, without going further. An Inbox task with no edge to draw stays out
//! of the drawing and is counted in a note. A parent with a drawn child is a box around it, and an edge between a
//! box and what it holds is not drawn: it is in a note. Over the cap the main line stays first, then the tasks
//! with edges, then those that wait for the owner, then the others by chain and by what they unblock.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::graph::{GraphOrder, Metrics};
use crate::model::{IssueState, Maturity, Status, Task};
use crate::pystr;
use crate::v1::TrackerBinding;

const FORMAT: &str = include_str!("../formats/plan_v1.toml");

/// The cap of drawn nodes when the command line asks for none.
pub const DEFAULT_MAX_NODES: usize = 25;

/// How many numbers a note lists before `…`.
pub const NOTE_MAX: usize = 12;

/// The widest title, in characters: of a node, of a box, and of a line of the tree.
pub const NODE_TITLE: usize = 38;
pub const BOX_TITLE: usize = 44;
pub const TREE_TITLE: usize = 60;

/// What a plan draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope<'a> {
    /// The open descendants of an issue, an Objective too; the root itself is never drawn.
    Root(u64),
    /// The open tasks of a project, Objectives left out.
    Project(&'a str),
}

/// The issues a plan reads, by number, each with its measures in the graph. Every issue of the tracker, the closed
/// ones too: an open sub-issue under a closed parent is still in the root's tree. Of two issues with one number,
/// the last read counts.
#[derive(Debug, Clone)]
pub struct Issues<'a> {
    by_number: BTreeMap<u64, (&'a Task, Metrics)>,
}

impl<'a> Issues<'a> {
    /// `tasks` with the measures of `graph`, which is theirs: a task outside the graph has none.
    pub fn new(tasks: &'a [Task], graph: &GraphOrder) -> Self {
        Issues { by_number: tasks.iter().map(|task| (task.number, (task, graph.metrics_of(task.number)))).collect() }
    }

    pub fn task(&self, number: u64) -> Option<&'a Task> {
        self.by_number.get(&number).map(|(task, _)| *task)
    }

    /// The measures of `number`: none, and not in a cycle, for an issue outside the graph.
    pub fn metrics(&self, number: u64) -> Metrics {
        self.by_number
            .get(&number)
            .map_or(Metrics { unblocks: None, unblocks_total: None, chain: None, cycle: false }, |(_, metrics)| *metrics)
    }

    /// Whether any issue read has the label of `project`, closed ones and Objectives too.
    pub fn has_project(&self, project: &str) -> bool {
        self.by_number.values().any(|(task, _)| task.project.as_deref() == Some(project))
    }

    /// Open and not an Objective: a task the graph has.
    fn is_open(&self, number: u64) -> bool {
        self.task(number).is_some_and(|task| task.state == IssueState::Open && !task.objective)
    }

    fn status(&self, number: u64) -> Option<Status> {
        self.task(number).and_then(|task| task.status)
    }
}

/// The graph to draw.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanGraph {
    /// Every drawn node, boxes included.
    pub nodes: BTreeSet<u64>,
    /// The drawn nodes one step outside the perimeter.
    pub external: BTreeSet<u64>,
    /// The longest path on the drawn edges, in its order; empty under two nodes.
    pub main_line: Vec<u64>,
    /// The drawn boxes: each child with the parent it sits in.
    pub boxes: BTreeMap<u64, u64>,
    /// The drawn edges `(blocker, blocked)`, in order.
    pub edges: Vec<(u64, u64)>,
    /// The edges a box hides, and the root's: in the note, in order.
    pub nested: Vec<(u64, u64)>,
    /// The edges the cap cut, between two nodes that would be drawn without it; those a box hides too.
    pub edges_lost: u64,
    /// The Inbox tasks of the perimeter with no edge to draw, in order.
    pub inbox_hidden: Vec<u64>,
    /// The nodes the cap cut, in order.
    pub truncated: Vec<u64>,
    pub root: Option<u64>,
    pub project: Option<String>,
}

/// Each issue's children, from its own `sub_issues` and from the `parent` of the others: the read of the list has
/// at most the first sub-issues of a parent, and the `parent` of each child completes them.
fn children(issues: &Issues) -> BTreeMap<u64, BTreeSet<u64>> {
    let mut out: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for (&number, (task, _)) in &issues.by_number {
        if let Some(parent) = task.parent.filter(|parent| issues.by_number.contains_key(parent)) {
            out.entry(parent).or_default().insert(number);
        }
        for &(child, _) in &task.sub_issues {
            if issues.by_number.contains_key(&child) {
                out.entry(number).or_default().insert(child);
            }
        }
    }
    out
}

/// Every issue under `root`, at any depth, in any state: the root never, even where the data loops back to it.
fn descendants(issues: &Issues, root: u64) -> BTreeSet<u64> {
    let children = children(issues);
    let (mut seen, mut stack) = (BTreeSet::new(), vec![root]);
    while let Some(node) = stack.pop() {
        for &child in children.get(&node).into_iter().flatten() {
            if child != root && seen.insert(child) {
                stack.push(child);
            }
        }
    }
    seen
}

/// The boxes (child → parent), the edges they hide, and the edges to draw.
type Containment = (BTreeMap<u64, u64>, BTreeSet<(u64, u64)>, BTreeSet<(u64, u64)>);

/// The boxes drawn among `nodes` (child → parent: neither one step outside), the edges a box hides because one of
/// their ends holds the other at some depth, and the edges left to draw. Only the `parent` of a node makes a box,
/// so a parent that is closed, cut or outside hides nothing.
fn containment(
    issues: &Issues,
    nodes: &BTreeSet<u64>,
    external: &BTreeSet<u64>,
    edges: &BTreeSet<(u64, u64)>,
) -> Containment {
    let boxes: BTreeMap<u64, u64> = nodes
        .iter()
        .filter(|node| !external.contains(node))
        .filter_map(|&node| {
            let parent = issues.task(node)?.parent?;
            (nodes.contains(&parent) && !external.contains(&parent)).then_some((node, parent))
        })
        .collect();
    // Whether `outer` holds `inner` at some depth; a loop in the data holds nothing.
    let holds = |outer: u64, inner: u64| {
        let mut seen = BTreeSet::new();
        let mut at = boxes.get(&inner).copied();
        while let Some(parent) = at {
            if !seen.insert(parent) {
                return false;
            }
            if parent == outer {
                return true;
            }
            at = boxes.get(&parent).copied();
        }
        false
    };
    let within: BTreeSet<(u64, u64)> = edges.iter().copied().filter(|(from, to)| nodes.contains(from) && nodes.contains(to)).collect();
    let hidden: BTreeSet<(u64, u64)> = within.iter().copied().filter(|&(from, to)| holds(from, to) || holds(to, from)).collect();
    let drawn = within.difference(&hidden).copied().collect();
    (boxes, hidden, drawn)
}

fn ends(edges: &BTreeSet<(u64, u64)>) -> BTreeSet<u64> {
    edges.iter().flat_map(|&(from, to)| [from, to]).collect()
}

/// The chain and what a node unblocks, none as 0: the order of the main line and of the cap after the length.
fn rank(issues: &Issues, number: u64) -> (u64, u64) {
    let metrics = issues.metrics(number);
    (metrics.chain.unwrap_or(0), metrics.unblocks.unwrap_or(0))
}

/// The longest path on the drawn edges that starts in the perimeter, never through a task in a cycle; its steps
/// are chosen on the drawn edges, not on the chain of the graph, which can count edges that are not drawn. Between
/// paths of one length: the longer chain, then more unblocked, then the lower number, for the start and for each
/// next step. Fewer than two nodes: no line. No recursion, so a chain of any length is fine.
fn main_line(issues: &Issues, nodes: &BTreeSet<u64>, drawn: &BTreeSet<(u64, u64)>, perimeter: &BTreeSet<u64>) -> Vec<u64> {
    let fine: BTreeSet<u64> = nodes.iter().copied().filter(|&node| !issues.metrics(node).cycle).collect();
    let mut next: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut incoming: BTreeMap<u64, usize> = fine.iter().map(|&node| (node, 0)).collect();
    for &(from, to) in drawn {
        if fine.contains(&from) && fine.contains(&to) {
            next.entry(from).or_default().push(to);
            *incoming.entry(to).or_default() += 1;
        }
    }
    // Kahn's order: the tasks outside a cycle have none among them.
    let mut order = Vec::new();
    let mut queue: Vec<u64> = fine.iter().copied().filter(|node| incoming[node] == 0).collect();
    while let Some(node) = queue.pop() {
        order.push(node);
        for &to in next.get(&node).into_iter().flatten() {
            let count = incoming.entry(to).or_default();
            *count -= 1;
            if *count == 0 {
                queue.push(to);
            }
        }
    }
    // From each node down: the length of its longest path, and the step after it.
    let mut longest: BTreeMap<u64, (usize, Option<u64>)> = BTreeMap::new();
    for &node in order.iter().rev() {
        let best = next
            .get(&node)
            .into_iter()
            .flatten()
            .filter_map(|&to| longest.get(&to).map(|&(length, _)| (length, to)))
            .max_by_key(|&(length, to)| {
                let (chain, unblocks) = rank(issues, to);
                (length, chain, unblocks, Reverse(to))
            });
        longest.insert(node, best.map_or((1, None), |(length, to)| (length + 1, Some(to))));
    }
    let start = fine.iter().copied().filter(|node| perimeter.contains(node) && longest.contains_key(node)).max_by_key(|&node| {
        let (chain, unblocks) = rank(issues, node);
        (longest[&node].0, chain, unblocks, Reverse(node))
    });
    let Some(start) = start.filter(|start| longest[start].0 >= 2) else { return Vec::new() };
    let mut line = vec![start];
    let mut at = longest[&start].1;
    while let Some(node) = at {
        line.push(node);
        at = longest[&node].1;
    }
    line
}

/// The graph of `scope`, at most `max_nodes` nodes drawn.
pub fn plan_graph(issues: &Issues, scope: Scope, max_nodes: usize) -> PlanGraph {
    let root = match scope {
        Scope::Root(root) => Some(root),
        Scope::Project(_) => None,
    };
    let perimeter: BTreeSet<u64> = match scope {
        Scope::Root(root) => descendants(issues, root).into_iter().filter(|&node| issues.is_open(node)).collect(),
        Scope::Project(project) => issues
            .by_number
            .iter()
            .filter(|(number, (task, _))| task.project.as_deref() == Some(project) && issues.is_open(**number))
            .map(|(&number, _)| number)
            .collect(),
    };
    let all: BTreeSet<(u64, u64)> = issues
        .by_number
        .iter()
        .filter(|(number, _)| issues.is_open(**number))
        .flat_map(|(&number, (task, _))| {
            task.blocked_by
                .iter()
                .filter(|(blocker, state)| *state == IssueState::Open && issues.is_open(*blocker))
                .map(move |&(blocker, _)| (blocker, number))
        })
        .filter(|(from, to)| perimeter.contains(from) || perimeter.contains(to))
        .collect();
    // The root holds the whole perimeter: its edges go in the note, and it is never a node.
    let root_edges: BTreeSet<(u64, u64)> = all.iter().copied().filter(|&(from, to)| root.is_some_and(|root| root == from || root == to)).collect();
    let edges: BTreeSet<(u64, u64)> = all.difference(&root_edges).copied().collect();
    let external: BTreeSet<u64> = ends(&edges).difference(&perimeter).copied().collect();

    let mut nodes: BTreeSet<u64> = perimeter.union(&external).copied().collect();
    let linked = ends(&containment(issues, &nodes, &external, &edges).2);
    let inbox_hidden: Vec<u64> =
        perimeter.iter().copied().filter(|&node| issues.status(node) == Some(Status::Inbox) && !linked.contains(&node)).collect();
    for node in &inbox_hidden {
        nodes.remove(node);
    }
    let (mut boxes, mut hidden, mut drawn) = containment(issues, &nodes, &external, &edges);
    let linked = ends(&drawn);
    let line = main_line(issues, &nodes, &drawn, &perimeter);
    let mut truncated = Vec::new();
    let mut edges_lost = 0;
    if nodes.len() > max_nodes {
        let position: BTreeMap<u64, usize> = line.iter().enumerate().map(|(at, &node)| (node, at)).collect();
        let priority = |node: u64| {
            let (chain, unblocks) = rank(issues, node);
            let waits = issues.status(node).is_some_and(Status::waits_on_owner);
            (position.get(&node).copied().unwrap_or(line.len()), !linked.contains(&node), !waits, Reverse(chain), Reverse(unblocks), node)
        };
        let before = nodes;
        let mut ordered: Vec<u64> = before.iter().copied().collect();
        ordered.sort_by_key(|&node| priority(node));
        nodes = ordered.into_iter().take(max_nodes).collect();
        (boxes, hidden, drawn) = containment(issues, &nodes, &external, &edges);
        // One step outside with no edge left, a node explains nothing.
        let alone: Vec<u64> = nodes.intersection(&external).copied().filter(|node| !drawn.iter().any(|(from, to)| from == node || to == node)).collect();
        for node in alone {
            nodes.remove(&node);
        }
        truncated = before.difference(&nodes).copied().collect();
        edges_lost = edges
            .iter()
            .filter(|(from, to)| before.contains(from) && before.contains(to) && !(nodes.contains(from) && nodes.contains(to)))
            .count() as u64;
    }
    let kept = |node: u64| nodes.contains(&node) || Some(node) == root;
    let nested: BTreeSet<(u64, u64)> = hidden.iter().copied().chain(root_edges.iter().copied().filter(|&(from, to)| kept(from) && kept(to))).collect();
    PlanGraph {
        external: external.intersection(&nodes).copied().collect(),
        main_line: line.into_iter().filter(|node| nodes.contains(node)).collect(),
        boxes,
        edges: drawn.into_iter().collect(),
        nested: nested.into_iter().collect(),
        edges_lost,
        inbox_hidden,
        truncated,
        root,
        project: match scope {
            Scope::Project(project) => Some(project.to_owned()),
            Scope::Root(_) => None,
        },
        nodes,
    }
}

/// `[#138 B] title` as `B · title`: the letter of a Plan, readable.
fn letter_prefix() -> &'static Regex {
    static PREFIX: OnceLock<Regex> = OnceLock::new();
    PREFIX.get_or_init(|| {
        let pattern = format!(r"^\[#\d+[ .]([{word}.]+)\]{space}*", word = pystr::WORD, space = pystr::SPACE);
        Regex::new(&pattern).expect("the prefix's pattern")
    })
}

/// `#12;`, which mermaid reads as the character of code 12.
fn number_and_semicolon() -> &'static Regex {
    static ENTITY: OnceLock<Regex> = OnceLock::new();
    ENTITY.get_or_init(|| Regex::new(r"(#\d+);").expect("the entity's pattern"))
}

/// A task's title as a plan shows it: its blanks as single spaces, a prefix `[#138 B]` as `B · `, the prefix of the
/// task's own project dropped (others' stay), at most `width` characters with `…`. `escaped`, for a label of the
/// diagram between quotes: `"`, `<` and `>` as their entities, and `#12;` as `#12#59;`.
pub fn title_label(task: &Task, width: usize, escaped: bool) -> String {
    let words: Vec<&str> = task.title.split(pystr::is_space).filter(|word| !word.is_empty()).collect();
    let mut title = letter_prefix().replace(&words.join(" "), "${1} · ").into_owned();
    if let Some(project) = task.project.as_deref().filter(|project| !project.is_empty())
        && let Some(rest) = title.strip_prefix(&format!("[{project}]"))
    {
        title = rest.trim_start_matches(pystr::is_space).to_owned();
    }
    if title.chars().count() > width {
        let kept: String = title.chars().take(width.saturating_sub(1)).collect();
        title = format!("{}…", pystr::rstrip(&kept));
    }
    if !escaped {
        return title;
    }
    number_and_semicolon().replace_all(&title, "${1}#59;").replace('"', "#quot;").replace('<', "#lt;").replace('>', "#gt;")
}

/// What a task unblocks and its chain, as a plan shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measures {
    pub unblocks: u64,
    /// Every task with a status downstream; the same as `unblocks` when nothing else is there.
    pub total: u64,
    pub chain: u64,
}

/// The measures a plan shows: none for a task that unblocks nothing, or that is in a cycle.
pub fn measures(metrics: &Metrics) -> Option<Measures> {
    if metrics.cycle {
        return None;
    }
    let unblocks = metrics.unblocks.unwrap_or(0);
    let total = metrics.unblocks_total.filter(|total| *total != 0).unwrap_or(unblocks);
    (total != 0).then(|| Measures { unblocks, total, chain: metrics.chain.unwrap_or(0) })
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Class {
    Waiting,
    InProgress,
    Draft,
    Ready,
    Inbox,
    External,
    Note,
    MainLine,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Format {
    diagram: DiagramWords,
    notes: NoteWords,
    classes: BTreeMap<String, String>,
    styles: Vec<Style>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagramWords {
    head: String,
    direction: String,
    hand: String,
    unknown: String,
    draft: String,
    cycle: String,
    unblocks: String,
    unblocks_total: String,
    note_node: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteWords {
    inbox: String,
    nested: String,
    truncated: String,
    root_hint: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Style {
    class: Class,
    style: String,
}

fn format() -> &'static Format {
    static PARSED: OnceLock<Format> = OnceLock::new();
    PARSED.get_or_init(|| toml::from_str(FORMAT).expect("the embedded plan format is valid"))
}

impl Class {
    /// The product's id of the class, the key of `[classes]`.
    fn id(self) -> &'static str {
        match self {
            Class::Waiting => "waiting",
            Class::InProgress => "in_progress",
            Class::Draft => "draft",
            Class::Ready => "ready",
            Class::Inbox => "inbox",
            Class::External => "external",
            Class::Note => "note",
            Class::MainLine => "main_line",
        }
    }
}

impl Format {
    /// The diagram's name of a class.
    fn class(&self, class: Class) -> &str {
        self.classes.get(class.id()).map_or("", String::as_str)
    }
}

/// `template` with each `{name}` of `values` in its place, in one pass: a value is never read again as a template.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let named = after.find('}').and_then(|close| values.iter().find(|(name, _)| *name == &after[..close]).map(|(_, value)| (close, value)));
        match named {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The second line of a node's label, and the end of a box's title: the status, the word of a draft for an approved
/// task in draft, then in a cycle or the measures when something is downstream, then the project of a node outside.
pub fn status_line(binding: &TrackerBinding, task: &Task, metrics: &Metrics, external: bool) -> String {
    let words = &format().diagram;
    let mut parts = vec![task.status.and_then(|status| binding.status_name(status)).unwrap_or(&words.unknown).to_owned()];
    if task.status == Some(Status::Approved) && task.maturity == Some(Maturity::Draft) {
        parts.push(words.draft.clone());
    }
    if metrics.cycle {
        parts.push(words.cycle.clone());
    } else if let Some(shown) = measures(metrics) {
        let (unblocks, total, chain) = (shown.unblocks.to_string(), shown.total.to_string(), shown.chain.to_string());
        let template = if shown.total == shown.unblocks { &words.unblocks } else { &words.unblocks_total };
        parts.push(fill(template, &[("unblocks", unblocks.as_str()), ("total", total.as_str()), ("chain", chain.as_str())]));
    }
    if external {
        parts.push(task.project.clone().filter(|project| !project.is_empty()).unwrap_or_else(|| words.unknown.clone()));
    }
    parts.join(" · ")
}

/// A note at the bottom of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// The Inbox tasks with no edge to draw.
    InboxHidden(Vec<u64>),
    /// The edges between a box and what it holds, and the root's.
    Nested(Vec<(u64, u64)>),
    /// The nodes the cap cut and the edges lost with them; `root_hint` for a project's plan, which a root narrows.
    Truncated { numbers: Vec<u64>, edges_lost: u64, root_hint: bool },
}

/// The notes of `graph`, in their order; `root_hint` with the cut of a project's plan.
pub fn notes(graph: &PlanGraph, root_hint: bool) -> Vec<Note> {
    let mut out = Vec::new();
    if !graph.inbox_hidden.is_empty() {
        out.push(Note::InboxHidden(graph.inbox_hidden.clone()));
    }
    if !graph.nested.is_empty() {
        out.push(Note::Nested(graph.nested.clone()));
    }
    if !graph.truncated.is_empty() {
        out.push(Note::Truncated { numbers: graph.truncated.clone(), edges_lost: graph.edges_lost, root_hint });
    }
    out
}

/// `#1, #2, …`: the first [`NOTE_MAX`] of `items`, then ` …` if there are more.
fn capped<T>(items: &[T], show: impl Fn(&T) -> String) -> String {
    let shown: Vec<String> = items.iter().take(NOTE_MAX).map(show).collect();
    format!("{}{}", shown.join(", "), if items.len() > NOTE_MAX { " …" } else { "" })
}

/// The numbers of a note: `#20, #21`, the first [`NOTE_MAX`] then ` …`.
pub fn numbers(list: &[u64]) -> String {
    capped(list, |number| format!("#{number}"))
}

/// The edges of a note: `#10→#11, #10→#12`, the first [`NOTE_MAX`] then ` …`.
pub fn edge_numbers(list: &[(u64, u64)]) -> String {
    capped(list, |(from, to)| format!("#{from}→#{to}"))
}

/// A note in the diagram's words; `root_option` is the command line's option of a root.
pub fn diagram_note(note: &Note, root_option: &str) -> String {
    let words = &format().notes;
    match note {
        Note::InboxHidden(list) => fill(&words.inbox, &[("count", list.len().to_string().as_str()), ("numbers", numbers(list).as_str())]),
        Note::Nested(list) => fill(&words.nested, &[("edges", edge_numbers(list).as_str())]),
        Note::Truncated { numbers: list, edges_lost, root_hint } => {
            let (count, lost, shown) = (list.len().to_string(), edges_lost.to_string(), numbers(list));
            let text = fill(&words.truncated, &[("count", count.as_str()), ("lost", lost.as_str()), ("numbers", shown.as_str())]);
            if *root_hint { text + &fill(&words.root_hint, &[("option", root_option)]) } else { text }
        }
    }
}

/// Each box with its children, in order.
fn box_children(graph: &PlanGraph) -> BTreeMap<u64, Vec<u64>> {
    let mut out: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for (&child, &parent) in &graph.boxes {
        out.entry(parent).or_default().push(child);
    }
    out
}

/// The command line and the day a diagram names, and the option of a root its note can name: the command line's,
/// in its own words.
#[derive(Debug, Clone, Copy)]
pub struct Quoted<'a> {
    pub command: &'a str,
    pub root_option: &'a str,
    pub date: &'a str,
}

/// The mermaid diagram of `graph`, its comment line first: data in the prototype's words, with the status of each
/// task by the name of its option in the Project.
pub fn diagram(issues: &Issues, graph: &PlanGraph, binding: &TrackerBinding, quoted: Quoted) -> String {
    let format = format();
    let words = &format.diagram;
    let line_edges: BTreeSet<(u64, u64)> = graph.main_line.windows(2).map(|pair| (pair[0], pair[1])).collect();
    let children = box_children(graph);
    let mut lines = vec![fill(&words.head, &[("command", quoted.command), ("date", quoted.date)]), words.direction.clone()];
    enum Step {
        Node(u64, usize),
        End(usize),
    }
    let mut steps: Vec<Step> = graph.nodes.iter().rev().filter(|node| !graph.boxes.contains_key(node)).map(|&node| Step::Node(node, 1)).collect();
    while let Some(step) = steps.pop() {
        let (node, depth) = match step {
            Step::End(depth) => {
                lines.push(format!("{}end", "  ".repeat(depth)));
                continue;
            }
            Step::Node(node, depth) => (node, depth),
        };
        let indent = "  ".repeat(depth);
        let Some(task) = issues.task(node) else { continue };
        let metrics = issues.metrics(node);
        let waits = task.status.is_some_and(Status::waits_on_owner);
        if let Some(inside) = children.get(&node) {
            let hand = if waits { format!("{} ", words.hand) } else { String::new() };
            let title = title_label(task, BOX_TITLE, true);
            lines.push(format!("{indent}subgraph n{node} [\"{hand}#{node} {title} · {}\"]", status_line(binding, task, &metrics, false)));
            steps.push(Step::End(depth));
            steps.extend(inside.iter().rev().map(|&child| Step::Node(child, depth + 1)));
            continue;
        }
        let text = format!(
            "#{node} {}<br/>{}",
            title_label(task, NODE_TITLE, true),
            status_line(binding, task, &metrics, graph.external.contains(&node))
        );
        lines.push(if waits { format!("{indent}n{node}{{{{\"{} {text}\"}}}}", words.hand) } else { format!("{indent}n{node}[\"{text}\"]") });
    }
    for &(from, to) in &graph.edges {
        lines.push(format!("  n{from} {} n{to}", if line_edges.contains(&(from, to)) { "==>" } else { "-->" }));
    }
    let notes: Vec<String> = notes(graph, graph.root.is_none()).iter().map(|note| diagram_note(note, quoted.root_option)).collect();
    if !notes.is_empty() {
        lines.push(format!("  {}[\"{}\"]", words.note_node, notes.join("<br/>")));
        lines.push(format!("  class {} {}", words.note_node, format.class(Class::Note)));
    }
    let order = [Class::Waiting, Class::InProgress, Class::Draft, Class::Ready, Class::Inbox, Class::External, Class::MainLine];
    let mut classes: BTreeMap<usize, BTreeSet<u64>> = BTreeMap::new();
    let mut add = |class: Class, node: u64| {
        let at = order.iter().position(|known| *known == class).unwrap_or(order.len());
        classes.entry(at).or_default().insert(node);
    };
    for &node in &graph.nodes {
        let Some(task) = issues.task(node).filter(|_| !children.contains_key(&node)) else { continue };
        let class = if graph.external.contains(&node) {
            Some(Class::External)
        } else if task.status.is_some_and(Status::waits_on_owner) {
            Some(Class::Waiting)
        } else if task.status == Some(Status::InProgress) {
            Some(Class::InProgress)
        } else if task.status == Some(Status::Inbox) {
            Some(Class::Inbox)
        } else if task.maturity == Some(Maturity::Draft) {
            Some(Class::Draft)
        } else if task.status == Some(Status::Approved) && task.maturity == Some(Maturity::Ready) {
            Some(Class::Ready)
        } else {
            None
        };
        if let Some(class) = class {
            add(class, node);
        }
        if graph.main_line.contains(&node) {
            add(Class::MainLine, node);
        }
    }
    lines.extend(format.styles.iter().map(|style| format!("  classDef {} {}", format.class(style.class), style.style)));
    for (at, nodes) in classes {
        let Some(&class) = order.get(at) else { continue };
        let list: Vec<String> = nodes.iter().map(|node| format!("n{node}")).collect();
        lines.push(format!("  class {} {}", list.join(","), format.class(class)));
    }
    lines.join("\n") + "\n"
}

/// A line of the indented tree, for the command line to write in its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeLine<'a> {
    Task(TreeTask<'a>),
    /// Before the nodes one step outside the perimeter.
    Outside,
    Note(Note),
    /// The main line, in its order, last.
    MainLine(Vec<u64>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeTask<'a> {
    /// 0 for a node of the perimeter that no box holds, one more inside each box; 1 for a node outside.
    pub depth: usize,
    pub on_main_line: bool,
    pub task: &'a Task,
    pub metrics: Metrics,
    /// One step outside the perimeter: the tree names its project.
    pub external: bool,
    /// The blockers of the node among the edges drawn and those in the note, in order.
    pub waits_for: Vec<u64>,
}

/// The tree of `graph`: the nodes of the perimeter by box, then those one step outside, then the notes, then the
/// main line.
pub fn tree<'a>(issues: &Issues<'a>, graph: &PlanGraph) -> Vec<TreeLine<'a>> {
    let children = box_children(graph);
    let mut blockers: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for &(from, to) in graph.edges.iter().chain(&graph.nested) {
        blockers.entry(to).or_default().insert(from);
    }
    let mut out = Vec::new();
    let walk = |top: Vec<(u64, usize)>, out: &mut Vec<TreeLine<'a>>| {
        let mut stack: Vec<(u64, usize)> = top.into_iter().rev().collect();
        while let Some((node, depth)) = stack.pop() {
            let Some(task) = issues.task(node) else { continue };
            out.push(TreeLine::Task(TreeTask {
                depth,
                on_main_line: graph.main_line.contains(&node),
                task,
                metrics: issues.metrics(node),
                external: graph.external.contains(&node),
                waits_for: blockers.get(&node).map(|set| set.iter().copied().collect()).unwrap_or_default(),
            }));
            stack.extend(children.get(&node).into_iter().flatten().rev().map(|&child| (child, depth + 1)));
        }
    };
    let top: Vec<(u64, usize)> =
        graph.nodes.iter().copied().filter(|node| !graph.external.contains(node) && !graph.boxes.contains_key(node)).map(|node| (node, 0)).collect();
    walk(top, &mut out);
    if !graph.external.is_empty() {
        out.push(TreeLine::Outside);
        walk(graph.external.iter().map(|&node| (node, 1)).collect(), &mut out);
    }
    out.extend(notes(graph, graph.root.is_none()).into_iter().map(TreeLine::Note));
    if !graph.main_line.is_empty() {
        out.push(TreeLine::MainLine(graph.main_line.clone()));
    }
    out
}

/// A node of the plan's JSON: the issue's keys the plan names, its measures, and whether it is one step outside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanNode {
    pub number: u64,
    pub title: String,
    pub status: Option<Status>,
    pub project: Option<String>,
    pub parent: Option<u64>,
    pub unblocks: Option<u64>,
    pub chain: Option<u64>,
    pub cycle: bool,
    pub external: bool,
}

/// The plan's JSON: the contract the agents read (`docs/contract/commands.md`, `plan`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanOutput {
    /// The command line that drew it, in the command line's own words.
    pub command: String,
    pub date: String,
    pub root: Option<u64>,
    pub project: Option<String>,
    pub nodes: Vec<PlanNode>,
    pub edges: Vec<(u64, u64)>,
    pub nested: Vec<(u64, u64)>,
    pub main_line: Vec<u64>,
    pub inbox_hidden: Vec<u64>,
    pub truncated: Vec<u64>,
    pub edges_lost: u64,
    /// The diagram, with `--mermaid`.
    pub mermaid: Option<String>,
}

/// The JSON of `graph`, its nodes in order.
pub fn output(issues: &Issues, graph: &PlanGraph, command: String, date: String, mermaid: Option<String>) -> PlanOutput {
    let nodes = graph
        .nodes
        .iter()
        .filter_map(|&number| {
            let task = issues.task(number)?;
            let metrics = issues.metrics(number);
            Some(PlanNode {
                number,
                title: task.title.clone(),
                status: task.status,
                project: task.project.clone(),
                parent: task.parent,
                unblocks: metrics.unblocks,
                chain: metrics.chain,
                cycle: metrics.cycle,
                external: graph.external.contains(&number),
            })
        })
        .collect();
    PlanOutput {
        command,
        date,
        root: graph.root,
        project: graph.project.clone(),
        nodes,
        edges: graph.edges.clone(),
        nested: graph.nested.clone(),
        main_line: graph.main_line.clone(),
        inbox_hidden: graph.inbox_hidden.clone(),
        truncated: graph.truncated.clone(),
        edges_lost: graph.edges_lost,
        mermaid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every class has a name and a style, once each, and the format reads.
    #[test]
    fn the_format_names_every_class_and_styles_each_once() {
        let format = format();
        let all = [Class::Waiting, Class::InProgress, Class::Draft, Class::Ready, Class::Inbox, Class::External, Class::Note, Class::MainLine];
        for class in all {
            assert!(!format.class(class).is_empty(), "{class:?}");
            assert_eq!(format.styles.iter().filter(|style| style.class == class).count(), 1, "{class:?}");
        }
        assert_eq!(format.classes.len(), all.len());
    }

    #[test]
    fn a_template_is_filled_once() {
        assert_eq!(fill("{a} and {b}", &[("a", "{b}"), ("b", "2")]), "{b} and 2");
        assert_eq!(fill("{c} {a", &[("a", "1")]), "{c} {a");
    }

    #[test]
    fn capped_lists_end_with_an_ellipsis() {
        let list: Vec<u64> = (1..=13).collect();
        assert_eq!(numbers(&list[..2]), "#1, #2");
        assert!(numbers(&list).ends_with("#12 …"));
        assert_eq!(edge_numbers(&[(1, 2)]), "#1→#2");
    }
}
