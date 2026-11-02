//! The plan is the `blocked_by` graph between open tasks. Two measures for each open task, objectives
//! excluded: `unblocks` counts the open tasks downstream, transitively; `chain` is the longest chain
//! of open tasks downstream, itself included. Tasks in a cycle have neither.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::model::{IssueState, Task};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Metrics {
    pub unblocks: Option<u64>,
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

pub fn graph_order(tasks: &[Task]) -> GraphOrder {
    let open: BTreeMap<u64, &Task> = tasks
        .iter()
        .filter(|task| task.state == IssueState::Open && !task.objective)
        .map(|task| (task.number, task))
        .collect();
    let nodes: Vec<u64> = open.keys().copied().collect();
    let mut successors: BTreeMap<u64, Vec<u64>> = nodes.iter().map(|&n| (n, Vec::new())).collect();
    for &node in &nodes {
        for &(blocker, state) in &open[&node].blocked_by {
            if state == IssueState::Open && open.contains_key(&blocker) {
                let list = successors.get_mut(&blocker).expect("every open task is a node");
                if !list.contains(&node) {
                    list.push(node);
                }
            }
        }
    }

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

    let mut metrics = HashMap::new();
    let mut cycles = Vec::new();
    for (i, comp) in comps.iter().enumerate() {
        let cycle = comp.len() > 1 || successors[&comp[0]].contains(&comp[0]);
        if cycle {
            cycles.push(comp.clone());
        }
        for &node in comp {
            let entry = if cycle {
                Metrics { unblocks: None, chain: None, cycle: true }
            } else {
                Metrics { unblocks: Some(reached[i].len() as u64), chain: Some(chain[i]), cycle: false }
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
    GraphOrder { metrics, cycles, truncated }
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
            .unwrap_or(Metrics { unblocks: None, chain: None, cycle: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(number: u64, blocked_by: &[u64]) -> Task {
        Task {
            number,
            title: String::new(),
            state: IssueState::Open,
            state_reason: None,
            status: None,
            project: None,
            maturity: None,
            objective: false,
            labels: vec![],
            parent: None,
            sub_issues: vec![],
            blocked_by: blocked_by.iter().map(|&b| (b, IssueState::Open)).collect(),
            blocked_by_total: blocked_by.len() as u64,
            url: String::new(),
            updated_at: String::new(),
            created_at: None,
            closed_at: None,
        }
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
}
