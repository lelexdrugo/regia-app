//! Reading the issue list, with its calls in parallel (#572).
//!
//! Every call to GitHub is a process of `gh`, and the list is read through three kinds of them: a page of
//! the list in full (100 issues, with their sub-issues, blockers and Project items: 2 to 4 seconds on the
//! server), a page of only where it ends (under a second), and the rest of an issue's sub-issues or
//! blockers when the list cut them. Read one after the other, they were the whole of the time the brief
//! waited for the tracker. Two things let them overlap without changing what is read:
//!
//! - A cursor of the list is a position (`createdAt` and the id of the issue it follows), so the cursors of
//!   all the pages can be had by walking the list without its nodes, while the full pages are read from
//!   them, each as soon as its cursor is known.
//! - The rest of an issue's sub-issues, and of its blockers, each read after the page the issue came from,
//!   do not depend on any other issue.
//!
//! The result is assembled in the order of the pages and of the nodes, never in the order the calls end.
//! The parallel read is only an acceleration of the sequential one, which stays the rule: where the two
//! could differ, because the tracker moved between the calls, the sequential read is what answers.

use std::thread;

use regia_core::{IssueState, RawIssue, TrackerError};

use super::{IssueConnection, IssueNode, PageInfo, PagedConnection, ProjectItem};

/// Which of an issue's connections is read page by page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Connection {
    /// The list query reads the first 20 `blockedBy`.
    Blockers,
    /// The list query reads the first 50 sub-issues.
    SubIssues,
}

/// The calls a read of the list makes. `GhTracker` makes them with `gh`; a test makes them with a
/// tracker it holds, to answer in any order.
pub(crate) trait Wire: Sync {
    /// A page of the list in full; `after` is the cursor the previous page ended at.
    fn list(&self, states: &str, after: Option<&str>) -> Result<IssueConnection, TrackerError>;

    /// Where the page after `after` ends, and whether another follows, without its issues.
    fn list_ends(&self, states: &str, after: Option<&str>) -> Result<PageInfo, TrackerError>;

    /// A page of an issue's blockers or sub-issues.
    fn connection(
        &self,
        connection: Connection,
        number: u64,
        after: Option<&str>,
    ) -> Result<PagedConnection, TrackerError>;
}

/// What identifies the status of an issue in the configured Project.
pub(crate) struct Project<'a> {
    pub id: &'a str,
    pub status_field_id: &'a str,
}

/// The list, in tracker order, each issue with its blockers and sub-issues read in full where the list
/// cuts them. The parallel read when it can stand for the sequential one, the sequential read otherwise.
pub(crate) fn issues<W: Wire>(
    wire: &W,
    project: &Project,
    states: &str,
) -> Result<Vec<RawIssue>, TrackerError> {
    let read = match in_parallel(wire, project, states)? {
        Some(read) => read,
        None => sequential(wire, project, states)?,
    };
    add_children_and_check(project, read)
}

/// What a read of the list found, before the rules that need all of it.
pub(crate) struct Read {
    pub issues: Vec<RawIssue>,
    /// How many items of the configured Project the issues carried.
    pub items_seen: u64,
}

fn add_children_and_check(project: &Project, read: Read) -> Result<Vec<RawIssue>, TrackerError> {
    let Read {
        mut issues,
        items_seen,
    } = read;
    regia_core::add_children_from_parents(&mut issues);
    // Every task in the Project without a status means the config is wrong, not that the plan is
    // empty: reporting that as an empty brief would be a false all-clear.
    if items_seen > 0 && issues.iter().all(|issue| issue.status_option_id.is_none()) {
        return Err(TrackerError::BadResponse(format!(
            "none of the {items_seen} items in the Project has a value for status field {}: check tracker.json",
            project.status_field_id
        )));
    }
    Ok(issues)
}

/// The list one page after the other, each issue completed before the next: the reading the rules were
/// written against.
pub(crate) fn sequential<W: Wire>(
    wire: &W,
    project: &Project,
    states: &str,
) -> Result<Read, TrackerError> {
    let mut issues: Vec<RawIssue> = Vec::new();
    let mut items_seen = 0;
    let mut after: Option<String> = None;
    loop {
        let connection = wire.list(states, after.as_deref())?;
        for node in connection.nodes {
            let sub_issues_total = node.sub_issues.total_count;
            let mut raw = to_raw(project, node, &mut items_seen);
            complete(wire, &mut raw, sub_issues_total);
            issues.push(raw);
        }
        if !connection.page_info.has_next_page {
            break;
        }
        after = connection.page_info.end_cursor;
    }
    Ok(Read { issues, items_seen })
}

/// A page of the list as the parallel read has it: its issues, completed, and where it ended.
struct Segment {
    read: Read,
    ends: PageInfo,
}

/// The parallel read, or `None` where it cannot be trusted to stand for the sequential one: the walk
/// of the cursors failed, or a full page did not end where the walk said it would (an issue was opened,
/// closed or reopened between the calls). A full page that fails is the failure the sequential read
/// would meet at that page, and the earliest page's is the one that returns.
fn in_parallel<W: Wire>(
    wire: &W,
    project: &Project,
    states: &str,
) -> Result<Option<Read>, TrackerError> {
    let segments = thread::scope(|scope| {
        // The page without a cursor starts at once, with the walk.
        let mut pages = vec![scope.spawn(|| segment(wire, project, states, None))];
        let mut walk = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let Ok(ends) = wire.list_ends(states, after.as_deref()) else {
                return None;
            };
            let next = ends.has_next_page.then(|| ends.end_cursor.clone());
            walk.push(ends);
            let Some(next) = next else { break };
            // A page that is said to follow and has no cursor is a response that does not read.
            let cursor = next?;
            after = Some(cursor.clone());
            pages.push(scope.spawn(move || segment(wire, project, states, Some(cursor))));
        }
        // The pages are taken in their order, which is not the order they end in.
        let pages: Vec<_> = pages
            .into_iter()
            .map(|page| page.join().expect("a page of the list panicked"))
            .collect();
        Some((pages, walk))
    });
    let Some((pages, walk)) = segments else {
        return Ok(None);
    };
    let mut segments = Vec::with_capacity(pages.len());
    for page in pages {
        segments.push(page?);
    }
    // The walk and the full pages saw the same list only if every page ended where the walk said.
    if segments.len() != walk.len()
        || segments
            .iter()
            .zip(&walk)
            .any(|(segment, ends)| segment.ends != *ends)
    {
        return Ok(None);
    }
    let mut issues = Vec::new();
    let mut items_seen = 0;
    for segment in segments {
        items_seen += segment.read.items_seen;
        issues.extend(segment.read.issues);
    }
    Ok(Some(Read { issues, items_seen }))
}

fn segment<W: Wire>(
    wire: &W,
    project: &Project,
    states: &str,
    after: Option<String>,
) -> Result<Segment, TrackerError> {
    let connection = wire.list(states, after.as_deref())?;
    let mut items_seen = 0;
    let mut pending: Vec<(RawIssue, u64)> = connection
        .nodes
        .into_iter()
        .map(|node| {
            let sub_issues_total = node.sub_issues.total_count;
            (to_raw(project, node, &mut items_seen), sub_issues_total)
        })
        .collect();
    // Each issue completes on its own, as soon as the page is here; the issues stay where they are.
    thread::scope(|scope| {
        for (raw, sub_issues_total) in &mut pending {
            if needs_completing(raw, *sub_issues_total) {
                scope.spawn(|| complete(wire, raw, *sub_issues_total));
            }
        }
    });
    let issues = pending.into_iter().map(|(raw, _)| raw).collect();
    Ok(Segment {
        read: Read { issues, items_seen },
        ends: connection.page_info,
    })
}

fn needs_completing(issue: &RawIssue, sub_issues_total: u64) -> bool {
    issue.blocked_by_total > issue.blocked_by.len() as u64
        || sub_issues_total > issue.sub_issues.len() as u64
}

/// The list query reads the first 20 `blockedBy` and the first 50 sub-issues. Issues above the limits read
/// the rest here, blockers first. If a page fails the total stays above what was read, and the rules treat
/// the task as incomplete; the open children in the list still reach their parent
/// (`add_children_from_parents`).
fn complete<W: Wire>(wire: &W, issue: &mut RawIssue, sub_issues_total: u64) {
    if issue.blocked_by_total > issue.blocked_by.len() as u64 {
        read_pages(
            wire,
            Connection::Blockers,
            issue.number,
            &mut issue.blocked_by,
        );
    }
    if sub_issues_total > issue.sub_issues.len() as u64 {
        read_pages(
            wire,
            Connection::SubIssues,
            issue.number,
            &mut issue.sub_issues,
        );
    }
}

/// Reads one connection of an issue from its first page, and adds the nodes the list lacks. A page that
/// fails stops the reading and keeps what came before it. Every page needs the cursor of the one before,
/// so one issue's pages are read one after the other.
fn read_pages<W: Wire>(
    wire: &W,
    connection: Connection,
    number: u64,
    list: &mut Vec<(u64, IssueState)>,
) {
    let mut after: Option<String> = None;
    loop {
        let Ok(page) = wire.connection(connection, number, after.as_deref()) else {
            return;
        };
        let seen: Vec<u64> = list.iter().map(|(number, _)| *number).collect();
        list.extend(
            page.nodes
                .into_iter()
                .filter(|node| !seen.contains(&node.number))
                .map(|n| (n.number, n.state)),
        );
        if !page.page_info.has_next_page {
            return;
        }
        after = page.page_info.end_cursor;
    }
}

fn to_raw(project: &Project, node: IssueNode, items_seen: &mut u64) -> RawIssue {
    // The status field is found by its id, so its name can be anything, in any language. The last
    // item of the configured Project wins; other Projects and other fields do not count. The other
    // single-select values keep their field's name, which gives them a meaning in the core.
    let items: Vec<ProjectItem> = node
        .project_items
        .nodes
        .into_iter()
        .filter(|item| item.project.id == project.id)
        .collect();
    *items_seen += items.len() as u64;
    let values: Vec<_> = items
        .into_iter()
        .flat_map(|item| item.field_values.nodes)
        .collect();
    let status_option_id = values
        .iter()
        .filter(|value| {
            value
                .field
                .as_ref()
                .is_some_and(|field| field.id.as_deref() == Some(project.status_field_id))
        })
        .filter_map(|value| value.option_id.clone())
        .next_back();
    let field_values = values
        .into_iter()
        .filter_map(|value| Some((value.field?.name?, value.name?)))
        .collect();
    RawIssue {
        number: node.number,
        title: node.title,
        state: node.state,
        state_reason: node.state_reason,
        body: node.body,
        url: node.url,
        updated_at: node.updated_at,
        created_at: node.created_at,
        closed_at: node.closed_at,
        labels: node
            .labels
            .nodes
            .into_iter()
            .map(|label| label.name)
            .collect(),
        parent: node.parent.map(|parent| parent.number),
        sub_issues: node
            .sub_issues
            .nodes
            .into_iter()
            .map(|n| (n.number, n.state))
            .collect(),
        blocked_by_total: node.blocked_by.total_count,
        blocked_by: node
            .blocked_by
            .nodes
            .into_iter()
            .map(|n| (n.number, n.state))
            .collect(),
        status_option_id,
        field_values,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::*;

    const STATUS_FIELD: &str = "field-status";
    const PROJECT: Project<'static> = Project {
        id: "project",
        status_field_id: STATUS_FIELD,
    };

    /// A tracker of synthetic issues that answers every call from what it holds, and makes the pages
    /// at the start of the list answer last, so that the order the calls end in is the reverse of the
    /// order the list is in. What it logs is only the kind and the position of each call.
    struct Fake {
        issues: usize,
        /// How many issues a full page holds, and how many the walk of the cursors counts to a page.
        page: usize,
        walk_page: usize,
        /// The issue whose sub-issues (or blockers) the list cuts, and how many it has in all. A page of
        /// its connection holds two.
        sub_issues_of: Option<(u64, u64)>,
        blockers_of: Option<(u64, u64)>,
        /// The page of the list (counted from 0) that fails.
        failing_page: Option<usize>,
        /// The issue whose sub-issues fail after their second page.
        failing_sub_issues_of: Option<u64>,
        delay: Duration,
        log: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new(issues: usize, page: usize) -> Self {
            Self {
                issues,
                page,
                walk_page: page,
                sub_issues_of: None,
                blockers_of: None,
                failing_page: None,
                failing_sub_issues_of: None,
                delay: Duration::from_millis(25),
                log: Mutex::new(Vec::new()),
            }
        }

        fn pages(&self) -> usize {
            self.issues.div_ceil(self.page)
        }

        fn start(after: Option<&str>) -> usize {
            after.map_or(0, |cursor| {
                cursor
                    .strip_prefix('c')
                    .and_then(|n| n.parse().ok())
                    .expect("a cursor of this fake")
            })
        }

        fn note(&self, entry: String) {
            self.log.lock().unwrap().push(entry);
        }

        /// The k-th child of an issue: alternating open and closed.
        fn child(number: u64, k: u64) -> (u64, &'static str) {
            (
                1000 + number * 10 + k,
                if k.is_multiple_of(2) {
                    "OPEN"
                } else {
                    "CLOSED"
                },
            )
        }

        fn total(of: Option<(u64, u64)>, number: u64) -> u64 {
            of.filter(|(n, _)| *n == number)
                .map_or(0, |(_, total)| total)
        }

        fn node(&self, number: u64) -> Value {
            // The list holds the first `first` of what an issue has.
            let cut = |of: Option<(u64, u64)>, first: u64| {
                let total = Self::total(of, number);
                let nodes: Vec<Value> = (0..total.min(first))
                    .map(|k| {
                        let (child, state) = Self::child(number, k);
                        json!({"number": child, "state": state})
                    })
                    .collect();
                json!({"totalCount": total, "nodes": nodes})
            };
            json!({
                "number": number, "title": format!("task {number}"), "state": "OPEN", "stateReason": null,
                "body": "", "url": format!("u/{number}"), "updatedAt": "d", "createdAt": null, "closedAt": null,
                "labels": {"nodes": [{"name": "l"}]}, "parent": null,
                "subIssues": cut(self.sub_issues_of, 2), "blockedBy": cut(self.blockers_of, 1),
                "projectItems": {"nodes": [{"project": {"id": "project"}, "fieldValues": {"nodes": [
                    {"optionId": format!("option-{number}"), "name": "Todo", "field": {"id": STATUS_FIELD, "name": "Status"}}
                ]}}]},
            })
        }

        fn ends(&self, start: usize, size: usize) -> PageInfo {
            let end = (start + size).min(self.issues);
            PageInfo {
                has_next_page: end < self.issues,
                end_cursor: Some(format!("c{end}")),
            }
        }
    }

    impl Wire for Fake {
        fn list(
            &self,
            _states: &str,
            after: Option<&str>,
        ) -> Result<IssueConnection, TrackerError> {
            let start = Self::start(after);
            let index = start / self.page;
            self.note(format!("list {index}"));
            // The first pages are the slowest, the last the quickest.
            thread::sleep(self.delay * (self.pages() - index) as u32);
            if self.failing_page == Some(index) {
                return Err(TrackerError::Unavailable(format!("page {index}")));
            }
            let end = (start + self.page).min(self.issues);
            let nodes: Vec<Value> = (start..end).map(|n| self.node(n as u64 + 1)).collect();
            let page_info = self.ends(start, self.page);
            Ok(serde_json::from_value(json!({
                "pageInfo": {"hasNextPage": page_info.has_next_page, "endCursor": page_info.end_cursor},
                "nodes": nodes,
            }))
            .expect("a page"))
        }

        fn list_ends(&self, _states: &str, after: Option<&str>) -> Result<PageInfo, TrackerError> {
            let start = Self::start(after);
            self.note(format!("ends {}", start / self.walk_page));
            thread::sleep(self.delay / 5);
            Ok(self.ends(start, self.walk_page))
        }

        fn connection(
            &self,
            connection: Connection,
            number: u64,
            after: Option<&str>,
        ) -> Result<PagedConnection, TrackerError> {
            let start = Self::start(after);
            self.note(format!("{connection:?} {number} {start}"));
            // The issues' own pages are not in step with each other.
            thread::sleep(self.delay * (1 + number as u32 % 3));
            if connection == Connection::SubIssues
                && self.failing_sub_issues_of == Some(number)
                && start > 2
            {
                return Err(TrackerError::Unavailable("sub-issues".to_owned()));
            }
            let of = if connection == Connection::SubIssues {
                self.sub_issues_of
            } else {
                self.blockers_of
            };
            let total = Self::total(of, number) as usize;
            let end = (start + 2).min(total);
            let nodes: Vec<Value> = (start..end)
                .map(|k| {
                    let (child, state) = Self::child(number, k as u64);
                    json!({"number": child, "state": state})
                })
                .collect();
            Ok(serde_json::from_value(json!({
                "pageInfo": {"hasNextPage": end < total, "endCursor": format!("c{end}")},
                "nodes": nodes,
            }))
            .expect("a connection page"))
        }
    }

    fn numbers(issues: &[RawIssue]) -> Vec<u64> {
        issues.iter().map(|issue| issue.number).collect()
    }

    fn issue(issues: &[RawIssue], number: u64) -> &RawIssue {
        issues
            .iter()
            .find(|issue| issue.number == number)
            .expect("the issue")
    }

    fn children(issues: &[RawIssue], number: u64) -> Vec<u64> {
        issue(issues, number)
            .sub_issues
            .iter()
            .map(|(n, _)| *n)
            .collect()
    }

    /// A tracker of 10 issues in pages of 3, whose first pages answer last, where #2 has 7 sub-issues (the list
    /// holds two, the rest comes in pages of two) and #8 has 4 blockers (the list holds one): the issues come
    /// in the order of the list, each issue's children in the order of their pages, and all of it is what the
    /// sequential read finds.
    #[test]
    fn what_the_parallel_read_assembles_is_what_the_sequential_read_reads() {
        let mut fake = Fake::new(10, 3);
        fake.sub_issues_of = Some((2, 7));
        fake.blockers_of = Some((8, 4));
        let parallel = in_parallel(&fake, &PROJECT, "")
            .expect("no failure")
            .expect("the walk agrees with the pages");
        assert_eq!(
            numbers(&parallel.issues),
            (1..=10).collect::<Vec<u64>>(),
            "in the order of the list, not of the answers"
        );
        assert_eq!(
            children(&parallel.issues, 2),
            (0..7).map(|k| 1020 + k).collect::<Vec<u64>>(),
            "in the order of the pages"
        );
        assert_eq!(
            issue(&parallel.issues, 8).blocked_by,
            (0..4)
                .map(|k| (
                    1080 + k,
                    if k % 2 == 0 {
                        IssueState::Open
                    } else {
                        IssueState::Closed
                    }
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(parallel.items_seen, 10);
        let sequential = sequential(&fake, &PROJECT, "").expect("sequential");
        assert_eq!(parallel.issues, sequential.issues);
        assert_eq!(parallel.items_seen, sequential.items_seen);
    }

    /// The read goes through the walk and the pages in parallel, not through the sequential read that
    /// answers where they disagree: every page is read once, and the walk of the cursors once more.
    #[test]
    fn the_parallel_read_asks_each_page_once_and_walks_the_cursors() {
        let fake = Fake::new(10, 3);
        issues(&fake, &PROJECT, "").expect("a read");
        let mut log = fake.log.lock().unwrap().clone();
        log.sort();
        assert_eq!(
            log,
            [
                "ends 0", "ends 1", "ends 2", "ends 3", "list 0", "list 1", "list 2", "list 3"
            ]
        );
    }

    /// A page that does not end where the walk said it would (the tracker moved between the calls) does not
    /// stand for the sequential read: the sequential read answers, and the result is what it reads.
    #[test]
    fn pages_that_end_elsewhere_than_the_walk_said_give_way_to_the_sequential_read() {
        let mut fake = Fake::new(10, 3);
        fake.walk_page = 4;
        assert!(
            in_parallel(&fake, &PROJECT, "")
                .expect("no failure")
                .is_none()
        );
        fake.log.lock().unwrap().clear();
        let read = issues(&fake, &PROJECT, "").expect("a read");
        assert_eq!(numbers(&read), (1..=10).collect::<Vec<u64>>());
        let log = fake.log.lock().unwrap().clone();
        assert!(
            log.iter().filter(|entry| entry.starts_with("list")).count() > 4,
            "the sequential read came after: {log:?}"
        );
    }

    /// A page of the list that fails is the failure of the earliest page that fails, as in the sequential
    /// read, whatever order the pages answered in.
    #[test]
    fn the_earliest_failing_page_is_the_failure() {
        let mut fake = Fake::new(10, 3);
        fake.failing_page = Some(1);
        let error = issues(&fake, &PROJECT, "").expect_err("a page fails");
        assert!(
            matches!(&error, TrackerError::Unavailable(message) if message == "page 1"),
            "{error:?}"
        );
        let sequential_error = sequential(&fake, &PROJECT, "")
            .err()
            .expect("the sequential read fails too");
        assert_eq!(error.to_string(), sequential_error.to_string());
    }

    /// A page of sub-issues that fails keeps what came before it, as in the sequential read, and the other
    /// issues are not touched by it.
    #[test]
    fn a_failing_page_of_sub_issues_keeps_the_ones_before_it() {
        let mut fake = Fake::new(10, 3);
        fake.sub_issues_of = Some((2, 7));
        fake.failing_sub_issues_of = Some(2);
        let parallel = issues(&fake, &PROJECT, "").expect("a read");
        let mut sequential = sequential(&fake, &PROJECT, "").expect("sequential").issues;
        regia_core::add_children_from_parents(&mut sequential);
        assert_eq!(
            children(&parallel, 2),
            vec![1020, 1021, 1022, 1023],
            "what came before the page that failed"
        );
        assert_eq!(parallel, sequential);
    }

    /// A tracker with one page, and one with none, are read as before.
    #[test]
    fn a_small_tracker_reads_in_one_page() {
        let fake = Fake::new(2, 3);
        assert_eq!(
            numbers(&issues(&fake, &PROJECT, "").expect("a read")),
            vec![1, 2]
        );
        let empty = Fake::new(0, 3);
        assert!(issues(&empty, &PROJECT, "").expect("a read").is_empty());
    }

    /// Items of the Project without any status in the whole list is a configuration that is wrong, not
    /// an empty plan, in the parallel read as in the sequential one.
    #[test]
    fn issues_without_a_status_are_refused_as_before() {
        let fake = Fake::new(4, 3);
        let other = Project {
            id: "project",
            status_field_id: "another-field",
        };
        let error = issues(&fake, &other, "").expect_err("no status anywhere");
        assert!(
            error.to_string().contains(
                "none of the 4 items in the Project has a value for status field another-field"
            ),
            "{error}"
        );
    }
}
