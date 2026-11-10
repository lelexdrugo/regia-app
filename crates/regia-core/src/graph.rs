//! The plan is the `blocked_by` graph between open tasks. Three measures for each open task, objectives
//! excluded: `unblocks` counts the approved work downstream, transitively (approved, in progress or at a
//! decision); `unblocks_total` counts every task downstream that has a status; `chain` is the longest
//! chain of open tasks downstream, itself included. Paths go through any open task, with or without a
//! status. Tasks in a cycle have no measures.
//!
//! An open child counts as a blocker of its parent in these measures, never in readiness (#244 of the
//! dossier, contract of the graph §4.1): the arc child → parent is computed, not written. Computed arcs
//! that close a cycle the written ones do not have all drop, and the brief names them.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::model::{IssueState, Status, Task};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Metrics {
    pub unblocks: Option<u64>,
    pub unblocks_total: Option<u64>,
    pub chain: Option<u64>,
    pub cycle: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphOrder {
    pub metrics: HashMap<u64, Metrics>,
    /// Tasks that block each other, or themselves.
    pub cycles: Vec<Vec<u64>>,
    /// Tasks with more `blocked_by` than were read: the graph is incomplete for them.
    pub truncated: Vec<u64>,
    /// Computed arcs (child, parent) that do not count, because with them a new cycle would close or a
    /// written one would grow.
    pub children_waiting: Vec<(u64, u64)>,
}

/// Strongly connected components (iterative Tarjan), emitted from the sinks to the roots: each
/// component comes after all those it reaches.
fn components(nodes: &[u64], successors: &BTreeMap<u64, Vec<u64>>) -> Vec<Vec<u64>> {
    let mut index: HashMap<u64, usize> = HashMap::new();
    let mut low: HashMap<u64, usize> = HashMap::new();
    let mut stack: Vec<u64> = Vec::new();
    let mut on_stack: BTreeSet<u64> = BTreeSet::new();
    let mut output = Vec::new();
    for &root in nodes {
        if index.contains_key(&root) {
            continue;
        }
        let mut work: Vec<(u64, usize)> = vec![(root, 0)];
        index.insert(root, index.len());
        low.insert(root, index[&root]);
        stack.push(root);
        on_stack.insert(root);
        while let Some(&mut (node, ref mut next)) = work.last_mut() {
            if let Some(&target) = successors[&node].get(*next) {
                *next += 1;
                if !index.contains_key(&target) {
                    index.insert(target, index.len());
                    low.insert(target, index[&target]);
                    stack.push(target);
                    on_stack.insert(target);
                    work.push((target, 0));
                } else if on_stack.contains(&target) {
                    let lowest = low[&node].min(index[&target]);
                    low.insert(node, lowest);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                let lowest = low[&parent].min(low[&node]);
                low.insert(parent, lowest);
            }
            if low[&node] == index[&node] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack.remove(&member);
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                component.sort_unstable();
                output.push(component);
            }
        }
    }
    output
}

/// The tasks each node blocks, for arcs (blocker, blocked). The order does not depend on how the arcs
/// were found.
fn successors(nodes: &[u64], arcs: &BTreeSet<(u64, u64)>) -> BTreeMap<u64, Vec<u64>> {
    let mut out: BTreeMap<u64, Vec<u64>> = nodes.iter().map(|&n| (n, Vec::new())).collect();
    for &(blocker, blocked) in arcs {
        out.get_mut(&blocker).expect("arcs join open tasks").push(blocked);
    }
    out
}

/// The arcs child → parent between open tasks that are not written already. The parent is read from the
/// child (`parent`) and from the parent's list (`sub_issues`): they agree, but data read in part loses
/// neither.
fn computed_arcs(open: &BTreeMap<u64, &Task>, written: &BTreeSet<(u64, u64)>) -> BTreeSet<(u64, u64)> {
    let mut arcs = BTreeSet::new();
    for (&number, task) in open {
        if let Some(parent) = task.parent.filter(|parent| open.contains_key(parent)) {
            arcs.insert((number, parent));
        }
        for &(child, state) in &task.sub_issues {
            if state == IssueState::Open && open.contains_key(&child) {
                arcs.insert((child, number));
            }
        }
    }
    arcs.retain(|&(child, parent)| child != parent && !written.contains(&(child, parent)));
    arcs
}

pub fn graph_order(tasks: &[Task]) -> GraphOrder {
    let open: BTreeMap<u64, &Task> = tasks
        .iter()
        .filter(|task| task.state == IssueState::Open && !task.objective)
        .map(|task| (task.number, task))
        .collect();
    let nodes: Vec<u64> = open.keys().copied().collect();
    let written: BTreeSet<(u64, u64)> = nodes
        .iter()
        .flat_map(|&node| {
            open[&node]
                .blocked_by
                .iter()
                .filter(|(blocker, state)| *state == IssueState::Open && open.contains_key(blocker))
                .map(move |&(blocker, _)| (blocker, node))
        })
        .collect();
    let computed = computed_arcs(&open, &written);

    // Components with the written arcs, then with all of them. A component that is a cycle only in the
    // second loses every computed arc inside it: one at a time is not enough, two can close a cycle together.
    let before: BTreeSet<Vec<u64>> = components(&nodes, &successors(&nodes, &written)).into_iter().collect();
    let all: BTreeSet<(u64, u64)> = written.union(&computed).copied().collect();
    let mut lost: BTreeSet<(u64, u64)> = BTreeSet::new();
    for component in components(&nodes, &successors(&nodes, &all)) {
        if component.len() > 1 && !before.contains(&component) {
            let inside = |n: &u64| component.binary_search(n).is_ok();
            lost.extend(computed.iter().filter(|(child, parent)| inside(child) && inside(parent)));
        }
    }
    let kept: BTreeSet<(u64, u64)> = written.iter().chain(computed.difference(&lost)).copied().collect();
    let successors = successors(&nodes, &kept);

    let comps = components(&nodes, &successors);
    let component_of: HashMap<u64, usize> = comps
        .iter()
        .enumerate()
        .flat_map(|(i, comp)| comp.iter().map(move |&n| (n, i)))
        .collect();
    let mut reached: Vec<BTreeSet<u64>> = vec![BTreeSet::new(); comps.len()];
    let mut chain: Vec<u64> = vec![0; comps.len()];
    // A component's successors have a lower index, so they are already computed.
    for (i, comp) in comps.iter().enumerate() {
        let children: BTreeSet<usize> = comp
            .iter()
            .flat_map(|n| successors[n].iter().map(|m| component_of[m]))
            .filter(|&child| child != i)
            .collect();
        let mut all = BTreeSet::new();
        let mut longest = 0;
        for &child in &children {
            all.extend(reached[child].iter().copied());
            all.extend(comps[child].iter().copied());
            longest = longest.max(chain[child]);
        }
        reached[i] = all;
        chain[i] = comp.len() as u64 + longest;
    }

    let count = |reached: &BTreeSet<u64>, counts: fn(Status) -> bool| {
        reached.iter().filter(|n| open[*n].status.is_some_and(counts)).count() as u64
    };
    let mut metrics = HashMap::new();
    let mut cycles = Vec::new();
    for (i, comp) in comps.iter().enumerate() {
        let cycle = comp.len() > 1 || successors[&comp[0]].contains(&comp[0]);
        if cycle {
            cycles.push(comp.clone());
        }
        for &node in comp {
            let entry = if cycle {
                Metrics { unblocks: None, unblocks_total: None, chain: None, cycle: true }
            } else {
                Metrics {
                    unblocks: Some(count(&reached[i], Status::is_approved_work)),
                    // A task with no status is off the board, so the total leaves it out.
                    unblocks_total: Some(count(&reached[i], |_| true)),
                    chain: Some(chain[i]),
                    cycle: false,
                }
            };
            metrics.insert(node, entry);
        }
    }
    cycles.sort();
    let truncated = nodes
        .iter()
        .copied()
        .filter(|n| open[n].blocked_by_total > open[n].blocked_by.len() as u64)
        .collect();
    GraphOrder { metrics, cycles, truncated, children_waiting: lost.into_iter().collect() }
}

impl GraphOrder {
    /// Longest chain first, then most unblocked, then lowest number. Cycles and tasks outside the
    /// graph go last, by number.
    pub fn sort_key(&self, number: u64) -> (u8, i64, i64, u64) {
        match self.metrics.get(&number) {
            Some(Metrics { unblocks: Some(unblocks), chain: Some(chain), .. }) => {
                (0, -(*chain as i64), -(*unblocks as i64), number)
            }
            _ => (1, 0, 0, number),
        }
    }

    pub fn metrics_of(&self, number: u64) -> Metrics {
        self.metrics
            .get(&number)
            .copied()
            .unwrap_or(Metrics { unblocks: None, unblocks_total: None, chain: None, cycle: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Spec {
        number: u64,
        blocked_by: Vec<u64>,
        parent: Option<u64>,
        status: Option<Status>,
        open: bool,
        objective: bool,
    }

    fn spec(number: u64, blocked_by: &[u64]) -> Spec {
        Spec { number, blocked_by: blocked_by.to_vec(), parent: None, status: Some(Status::Approved), open: true, objective: false }
    }

    impl Spec {
        fn parent(mut self, parent: u64) -> Self {
            self.parent = Some(parent);
            self
        }
        fn status(mut self, status: Option<Status>) -> Self {
            self.status = status;
            self
        }
        fn closed(mut self) -> Self {
            self.open = false;
            self
        }
        fn objective(mut self) -> Self {
            self.objective = true;
            self
        }
    }

    fn task_of(spec: &Spec) -> Task {
        Task {
            number: spec.number,
            title: String::new(),
            state: if spec.open { IssueState::Open } else { IssueState::Closed },
            state_reason: None,
            status: spec.status,
            project: None,
            project_field: None,
            maturity: None,
            objective: spec.objective,
            labels: vec![],
            parent: spec.parent,
            sub_issues: vec![],
            blocked_by: spec.blocked_by.iter().map(|&b| (b, IssueState::Open)).collect(),
            blocked_by_total: spec.blocked_by.len() as u64,
            url: String::new(),
            updated_at: String::new(),
            created_at: None,
            closed_at: None,
            due: None,
        }
    }

    /// The tasks with `sub_issues` filled from the `parent` fields, as the tracker reports them.
    fn graph(specs: Vec<Spec>) -> Vec<Task> {
        let mut tasks: Vec<Task> = specs.iter().map(task_of).collect();
        for spec in &specs {
            let state = if spec.open { IssueState::Open } else { IssueState::Closed };
            if let Some(parent) = tasks.iter_mut().find(|t| Some(t.number) == spec.parent) {
                parent.sub_issues.push((spec.number, state));
            }
        }
        tasks
    }

    fn task(number: u64, blocked_by: &[u64]) -> Task {
        task_of(&spec(number, blocked_by))
    }

    /// (unblocks, chain) of every node.
    fn measures(tasks: &[Task]) -> BTreeMap<u64, (Option<u64>, Option<u64>)> {
        graph_order(tasks).metrics.into_iter().map(|(n, m)| (n, (m.unblocks, m.chain))).collect()
    }

    fn order(tasks: &[Task], among: &[u64]) -> Vec<u64> {
        let graph = graph_order(tasks);
        let mut numbers: Vec<u64> = among.to_vec();
        numbers.sort_by_key(|&n| graph.sort_key(n));
        numbers
    }

    #[test]
    fn chain_and_unblocks_on_a_diamond() {
        let graph = graph_order(&[task(1, &[]), task(2, &[1]), task(3, &[1]), task(4, &[2, 3])]);
        assert_eq!(graph.metrics[&1].unblocks, Some(3));
        assert_eq!(graph.metrics[&1].chain, Some(3));
        assert_eq!(graph.metrics[&4].unblocks, Some(0));
        assert_eq!(graph.metrics[&4].chain, Some(1));
    }

    #[test]
    fn cycles_have_no_measures_and_upstream_counts_their_nodes() {
        let graph = graph_order(&[task(1, &[]), task(2, &[1, 3]), task(3, &[2]), task(4, &[4])]);
        assert_eq!(graph.cycles, vec![vec![2, 3], vec![4]]);
        assert!(graph.metrics[&2].cycle);
        assert_eq!(graph.metrics[&2].chain, None);
        assert_eq!(graph.metrics[&1].unblocks, Some(2));
        assert_eq!(graph.metrics[&1].chain, Some(3));
    }

    #[test]
    fn a_deep_chain_does_not_overflow_the_stack() {
        let tasks: Vec<Task> =
            (1..=3_000u64).map(|n| if n == 1 { task(1, &[]) } else { task(n, &[n - 1]) }).collect();
        let graph = graph_order(&tasks);
        assert_eq!(graph.metrics[&1].chain, Some(3_000));
    }

    // The arc child → parent, ported from the prototype's tests of #244 and #407.

    #[test]
    fn an_open_child_counts_as_a_blocker_of_its_parent() {
        let without = graph(vec![spec(10, &[]), spec(11, &[]), spec(20, &[10])]);
        let with = graph(vec![spec(10, &[]), spec(11, &[]).parent(10), spec(20, &[10])]);
        assert_eq!(measures(&without)[&11], (Some(0), Some(1)));
        assert_eq!(measures(&with)[&11], (Some(2), Some(3)));
        assert_eq!(measures(&with)[&10], (Some(1), Some(2)), "the parent does not change");
    }

    #[test]
    fn the_parent_field_or_the_list_of_sub_issues_is_enough() {
        let only_parent = vec![task(10, &[]), task_of(&spec(11, &[]).parent(10)), task(20, &[10])];
        let mut only_list = vec![task(10, &[]), task(11, &[]), task(20, &[10])];
        only_list[0].sub_issues.push((11, IssueState::Open));
        for tasks in [only_parent, only_list] {
            assert_eq!(measures(&tasks)[&11], (Some(2), Some(3)));
        }
    }

    #[test]
    fn the_arc_is_transitive_over_levels_of_children() {
        let tasks = graph(vec![spec(1, &[]), spec(2, &[]).parent(1), spec(3, &[]).parent(2), spec(9, &[1])]);
        let expected: BTreeMap<u64, (Option<u64>, Option<u64>)> = [
            (1, (Some(1), Some(2))),
            (2, (Some(2), Some(3))),
            (3, (Some(3), Some(4))),
            (9, (Some(0), Some(1))),
        ]
        .into();
        assert_eq!(measures(&tasks), expected);
    }

    #[test]
    fn a_closed_child_or_an_objective_parent_makes_no_arc() {
        let closed = graph(vec![spec(10, &[]), spec(11, &[]).parent(10).closed(), spec(20, &[10])]);
        let expected: BTreeMap<u64, (Option<u64>, Option<u64>)> =
            [(10, (Some(1), Some(2))), (20, (Some(0), Some(1)))].into();
        assert_eq!(measures(&closed), expected);
        let epic = graph(vec![spec(1, &[]).objective(), spec(2, &[]).parent(1), spec(3, &[2])]);
        let expected: BTreeMap<u64, (Option<u64>, Option<u64>)> = [(2, (Some(1), Some(2))), (3, (Some(0), Some(1)))].into();
        assert_eq!(measures(&epic), expected);
    }

    #[test]
    fn the_children_rise_above_a_shorter_chain() {
        let tasks = graph(vec![
            spec(100, &[]),
            spec(101, &[]).parent(100),
            spec(102, &[]).parent(100),
            spec(200, &[100]),
            spec(201, &[200]),
            spec(30, &[]),
            spec(31, &[30]),
        ]);
        assert_eq!(order(&tasks, &[101, 102, 30]), [101, 102, 30]);
        let without = graph(vec![spec(100, &[]), spec(101, &[]), spec(102, &[]), spec(200, &[100]), spec(201, &[200]), spec(30, &[]), spec(31, &[30])]);
        assert_eq!(order(&without, &[101, 102, 30]), [30, 101, 102]);
    }

    #[test]
    fn a_child_its_parent_blocks_does_not_close_a_cycle() {
        // The prototype's #69 and #209: the arc 209 → 69 would close a cycle, so it does not count.
        let tasks = graph(vec![
            spec(69, &[]).status(Some(Status::Decision)),
            spec(209, &[69]).parent(69).status(Some(Status::Inbox)),
            spec(70, &[]),
            spec(71, &[70]),
        ]);
        let graph = graph_order(&tasks);
        assert!(graph.cycles.is_empty());
        assert!(!graph.metrics[&69].cycle);
        assert_eq!(graph.children_waiting, [(209, 69)]);
        assert_eq!((graph.metrics[&69].unblocks, graph.metrics[&69].unblocks_total), (Some(0), Some(1)));
        assert_eq!(order(&tasks, &[69, 70]), [70, 69]);
        let all = order(&tasks, &[69, 70, 71, 209]);
        assert!(all.iter().position(|&n| n == 69) < all.iter().position(|&n| n == 71), "not last, with the cycles");
    }

    #[test]
    fn two_computed_arcs_that_close_a_cycle_together_both_drop() {
        let tasks = graph(vec![spec(1, &[]), spec(2, &[]), spec(3, &[1]).parent(2), spec(4, &[2]).parent(1)]);
        let nodes = [1, 2, 3, 4];
        let written: BTreeSet<(u64, u64)> = [(1, 3), (2, 4)].into();
        for alone in [(3, 2), (4, 1)] {
            let arcs: BTreeSet<(u64, u64)> = written.iter().copied().chain([alone]).collect();
            assert!(components(&nodes, &successors(&nodes, &arcs)).iter().all(|c| c.len() == 1), "{alone:?}");
        }
        let both: BTreeSet<(u64, u64)> = written.iter().copied().chain([(3, 2), (4, 1)]).collect();
        assert_eq!(components(&nodes, &successors(&nodes, &both)), [vec![1, 2, 3, 4]]);
        let graph = graph_order(&tasks);
        assert!(graph.cycles.is_empty());
        assert_eq!(graph.children_waiting, [(3, 2), (4, 1)]);
        let without = vec![task(1, &[]), task(2, &[]), task(3, &[1]), task(4, &[2])];
        assert_eq!(graph.metrics, graph_order(&without).metrics);
    }

    #[test]
    fn three_computed_arcs_that_close_a_cycle_only_together_all_drop() {
        // A rule that adds one arc at a time would keep all three, and the cycle would be back.
        let tasks = graph(vec![
            spec(1, &[]),
            spec(2, &[]),
            spec(3, &[]),
            spec(4, &[1]).parent(2),
            spec(5, &[2]).parent(3),
            spec(6, &[3]).parent(1),
        ]);
        let nodes = [1, 2, 3, 4, 5, 6];
        let written: BTreeSet<(u64, u64)> = [(1, 4), (2, 5), (3, 6)].into();
        let computed: BTreeSet<(u64, u64)> = [(4, 2), (5, 3), (6, 1)].into();
        for left_out in &computed {
            let arcs: BTreeSet<(u64, u64)> = written.iter().chain(computed.iter().filter(|a| *a != left_out)).copied().collect();
            assert!(components(&nodes, &successors(&nodes, &arcs)).iter().all(|c| c.len() == 1), "{left_out:?}");
        }
        let graph = graph_order(&tasks);
        assert!(graph.cycles.is_empty());
        assert_eq!(graph.children_waiting, [(4, 2), (5, 3), (6, 1)]);
        let without = vec![task(1, &[]), task(2, &[]), task(3, &[]), task(4, &[1]), task(5, &[2]), task(6, &[3])];
        assert_eq!(graph.metrics, graph_order(&without).metrics);
    }

    #[test]
    fn a_written_cycle_stays_and_loses_nothing() {
        let tasks = graph(vec![spec(2, &[3]), spec(3, &[2]), spec(4, &[]).parent(2)]);
        let graph = graph_order(&tasks);
        assert_eq!(graph.cycles, [vec![2, 3]]);
        assert!(graph.children_waiting.is_empty());
        assert_eq!(graph.metrics[&4], Metrics { unblocks: Some(2), unblocks_total: Some(2), chain: Some(3), cycle: false });
    }

    #[test]
    fn a_computed_arc_that_grows_a_written_cycle_drops() {
        let tasks = graph(vec![spec(2, &[3]), spec(3, &[2]), spec(5, &[3]).parent(2)]);
        let graph = graph_order(&tasks);
        assert_eq!(graph.cycles, [vec![2, 3]]);
        assert_eq!(graph.children_waiting, [(5, 2)]);
        assert!(!graph.metrics[&5].cycle);
    }

    #[test]
    fn the_arcs_of_other_cycles_still_count() {
        let tasks = graph(vec![spec(1, &[]), spec(3, &[1]).parent(1), spec(10, &[]), spec(11, &[]).parent(10), spec(20, &[10])]);
        let graph = graph_order(&tasks);
        assert_eq!(graph.children_waiting, [(3, 1)]);
        assert_eq!(graph.metrics[&11].unblocks, Some(2));
    }

    // `unblocks` counts the approved work.

    #[test]
    fn unblocks_counts_approved_in_progress_and_decision_and_the_total_every_status() {
        let statuses = [Status::Approved, Status::InProgress, Status::Decision, Status::Inbox, Status::Proposal];
        let mut tasks = vec![task(1, &[])];
        tasks.extend(statuses.iter().zip(10..).map(|(&status, n)| task_of(&spec(n, &[1]).status(Some(status)))));
        let one = graph_order(&tasks).metrics[&1];
        assert_eq!((one.unblocks, one.unblocks_total, one.chain), (Some(3), Some(5), Some(2)));
    }

    #[test]
    fn paths_go_through_inbox_and_proposals_and_through_tasks_with_no_status() {
        let tasks = vec![
            task(1, &[]),
            task_of(&spec(2, &[1]).status(Some(Status::Inbox))),
            task_of(&spec(3, &[2]).status(Some(Status::Proposal))),
            task(4, &[3]),
        ];
        let one = graph_order(&tasks).metrics[&1];
        assert_eq!((one.unblocks, one.unblocks_total, one.chain), (Some(1), Some(3), Some(4)));
        let unset = vec![
            task(1, &[]),
            task_of(&spec(2, &[1]).status(Some(Status::Inbox))),
            task_of(&spec(3, &[1]).status(None)),
            task_of(&spec(4, &[3]).status(Some(Status::Proposal))),
        ];
        let one = graph_order(&unset).metrics[&1];
        assert_eq!((one.unblocks, one.unblocks_total, one.chain), (Some(0), Some(2), Some(3)));
    }

    #[test]
    fn an_inbox_downstream_does_not_lift_who_comes_before_it() {
        let tasks = vec![
            task(5, &[]),
            task_of(&spec(51, &[5]).status(Some(Status::Inbox))),
            task_of(&spec(52, &[5]).status(Some(Status::Inbox))),
            task(6, &[]),
            task(61, &[6]),
        ];
        assert_eq!(order(&tasks, &[5, 6]), [6, 5]);
    }

    #[test]
    fn a_node_of_a_cycle_downstream_counts_once_if_approved() {
        let tasks = vec![task(1, &[]), task(2, &[1, 3]), task_of(&spec(3, &[2]).status(Some(Status::Inbox)))];
        let one = graph_order(&tasks).metrics[&1];
        assert_eq!((one.unblocks, one.unblocks_total), (Some(1), Some(2)));
    }

    // Properties (conformance study §9.3), on random graphs from a fixed seed.

    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, bound: u64) -> u64 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % bound
        }
    }

    fn random_tasks(random: &mut Lcg) -> Vec<Task> {
        let count = 2 + random.below(9);
        let statuses = [None, Some(Status::Inbox), Some(Status::Proposal), Some(Status::Approved), Some(Status::Decision)];
        (1..=count)
            .map(|n| {
                let blocked_by: Vec<u64> = (1..=count).filter(|_| random.below(5) == 0).collect();
                let mut spec = spec(n, &blocked_by).status(statuses[random.below(5) as usize]);
                if random.below(3) == 0 {
                    spec = spec.parent(1 + random.below(count));
                }
                if random.below(10) == 0 {
                    spec = spec.closed();
                }
                task_of(&spec)
            })
            .collect()
    }

    /// Every node `from` reaches through `arcs`: itself only if a cycle leads back.
    fn reach(from: u64, arcs: &BTreeSet<(u64, u64)>) -> BTreeSet<u64> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![from];
        while let Some(node) = todo.pop() {
            for &(_, next) in arcs.range((node, 0)..=(node, u64::MAX)) {
                if seen.insert(next) {
                    todo.push(next);
                }
            }
        }
        seen
    }

    /// The nodes that reach `node` and that it reaches, itself included.
    fn component_of(node: u64, arcs: &BTreeSet<(u64, u64)>) -> BTreeSet<u64> {
        let mut out: BTreeSet<u64> = reach(node, arcs).into_iter().filter(|m| reach(*m, arcs).contains(&node)).collect();
        out.insert(node);
        out
    }

    /// The most nodes on a path from `node`, a component counting all its nodes.
    fn longest(node: u64, arcs: &BTreeSet<(u64, u64)>) -> u64 {
        let own = component_of(node, arcs);
        let next: BTreeSet<u64> = arcs.iter().filter(|(a, b)| own.contains(a) && !own.contains(b)).map(|&(_, b)| b).collect();
        own.len() as u64 + next.into_iter().map(|m| longest(m, arcs)).max().unwrap_or(0)
    }

    #[test]
    fn on_random_graphs_the_measures_match_brute_force() {
        let mut random = Lcg(0x5eed);
        for _ in 0..500 {
            let tasks = random_tasks(&mut random);
            let graph = graph_order(&tasks);
            let open: BTreeMap<u64, &Task> =
                tasks.iter().filter(|t| t.state == IssueState::Open).map(|t| (t.number, t)).collect();
            let written: BTreeSet<(u64, u64)> = open
                .values()
                .flat_map(|t| t.blocked_by.iter().filter(|(b, _)| open.contains_key(b)).map(|&(b, _)| (b, t.number)))
                .collect();
            let computed = computed_arcs(&open, &written);
            let all: BTreeSet<(u64, u64)> = written.union(&computed).copied().collect();

            // A computed arc drops when its ends share a component that the written arcs alone do not make.
            let dropped: BTreeSet<(u64, u64)> = computed
                .iter()
                .copied()
                .filter(|&(child, parent)| {
                    let component = component_of(child, &all);
                    component.contains(&parent) && component != component_of(child, &written)
                })
                .collect();
            assert_eq!(graph.children_waiting.iter().copied().collect::<BTreeSet<_>>(), dropped);

            // So the cycles that remain are those of the written arcs alone.
            let cyclic = |arcs: &BTreeSet<(u64, u64)>, n: u64| reach(n, arcs).contains(&n);
            let expected: BTreeSet<u64> = open.keys().copied().filter(|&n| cyclic(&written, n)).collect();
            assert_eq!(graph.cycles.iter().flatten().copied().collect::<BTreeSet<_>>(), expected);

            let kept: BTreeSet<(u64, u64)> = all.difference(&dropped).copied().collect();
            for &n in open.keys() {
                let metrics = graph.metrics[&n];
                assert_eq!(metrics.cycle, cyclic(&kept, n), "#{n}");
                if metrics.cycle {
                    assert_eq!((metrics.unblocks, metrics.unblocks_total, metrics.chain), (None, None, None));
                    continue;
                }
                let downstream = reach(n, &kept);
                let approved = downstream.iter().filter(|m| open[*m].status.is_some_and(Status::is_approved_work)).count();
                let total = downstream.iter().filter(|m| open[*m].status.is_some()).count();
                assert_eq!(metrics.unblocks, Some(approved as u64), "#{n}");
                assert_eq!(metrics.unblocks_total, Some(total as u64), "#{n}");
                assert_eq!(metrics.chain, Some(longest(n, &kept)), "#{n}");
            }
            // The order of the issues does not change the outcome.
            let mut reversed = tasks.clone();
            reversed.reverse();
            assert_eq!(graph_order(&reversed), graph);
        }
    }
}
