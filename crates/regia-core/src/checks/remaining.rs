//! `remaining-unreconciled` (#180 and #238 of the dossier, `docs/design/contratto-resta.md` §5): each item a worker's
//! `done` report leaves in its `resta` field needs a destination that holds, the one of the last line of
//! reconciliation the orchestrator wrote in `decisioni.md` or its archives, else the one of the last report that
//! lists it. A destination holds when it is an open issue, a closed one closed recently, a discard with its
//! reason, or a done with where; the seven cases of the contract say when it does not.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;

use super::{CheckId, Finding, clean, collapse_spaces, files, found, pattern, python_int, references, remaining_from, rome_date, truncate, why, words};
use crate::model::IssueState;
use crate::pystr;

/// What the check reads of an issue, open or closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueFacts {
    pub state: IssueState,
    /// GitHub's `stateReason`: `COMPLETED` for done, `NOT_PLANNED` for discarded.
    pub state_reason: Option<String>,
    pub closed_at: Option<String>,
    pub url: Option<String>,
}

/// A `done` report as the check reads it (`leggi_report_resta`): its task, the date of its name, whether it has
/// the field; the entries in form `(n, text, destination, line)`; the lines out of form `(line, text, n, why)`; the
/// field that is neither a list nor `nessuno`, `(why, line, text)`; and whether it says nothing is left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRemaining {
    pub task: u64,
    pub date: String,
    pub name: String,
    pub has_field: bool,
    pub entries: Vec<(u64, String, String, usize)>,
    pub defects: Vec<(usize, String, Option<u64>, String)>,
    pub on_field: Option<(String, usize, String)>,
    pub none: bool,
}

/// `strip().strip("\"'")`.
fn unquoted(text: &str) -> &str {
    pystr::strip(text).trim_matches(|c| words().quotes.contains(c))
}

/// `leggi_report_resta`: the report `AAAA-MM-GG-<N>-<slug>.md` read for its `resta`; `None` if it is not a `done`.
/// The head is the front matter, or for a report written as a list (a bullet a field) its lines up to the first
/// `##` title. The task is the head's `task:` when it is a number, else the number of the name.
pub fn read_report(name: &str, text: &str) -> Option<ReportRemaining> {
    let captures = pattern("report_name").captures(name).ok().flatten()?;
    let w = words();
    let text = clean(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let fence = "---";
    let head: Vec<usize> = if lines.first().is_some_and(|line| pystr::strip(line) == fence) {
        let end = (1..lines.len()).find(|i| pystr::strip(lines[*i]) == fence).unwrap_or(lines.len());
        (1..end).collect()
    } else {
        lines.iter().take(60).take_while(|line| !line.starts_with("##")).enumerate().map(|(i, _)| i).collect()
    };
    let (mut kind, mut start, mut value, mut task_field): (Option<String>, Option<usize>, String, Option<u64>) = (None, None, String::new(), None);
    for &i in &head {
        let Ok(Some(field)) = pattern("field").captures(lines[i]) else { continue };
        let (key, rest) = (field.get(1).map_or("", |m| m.as_str()), field.get(2).map_or("", |m| m.as_str()));
        if w.type_fields.iter().any(|name| name == key) && kind.is_none() {
            kind = Some(unquoted(rest).to_owned());
        } else if key == w.remaining_field && start.is_none() {
            (start, value) = (Some(i), pystr::strip(rest).to_owned());
        } else if key == w.task_field && task_field.is_none() && let Some(number) = python_int(unquoted(rest)) {
            task_field = Some(number);
        }
    }
    if kind.as_deref() != Some(w.done.as_str()) {
        return None;
    }
    let mut out = ReportRemaining {
        task: task_field.or_else(|| captures.get(2).and_then(|n| python_int(n.as_str())))?,
        date: captures.get(1).map_or("", |m| m.as_str()).to_owned(),
        name: name.to_owned(),
        has_field: start.is_some(),
        entries: Vec::new(),
        defects: Vec::new(),
        on_field: None,
        none: false,
    };
    let Some(start) = start else { return Some(out) };
    // A YAML comment after the key.
    if value.starts_with('#') {
        value.clear();
    }
    if !value.is_empty() {
        if value.trim_matches(|c| w.quotes.contains(c)) == w.none {
            out.none = true;
        } else {
            out.on_field = Some((why("field_not_a_list", &[]), start + 1, value));
        }
        return Some(out);
    }
    let mut items = 0;
    for &i in head.iter().filter(|i| **i > start) {
        let line = lines[i];
        if pystr::strip(line).is_empty() || line.trim_start_matches(pystr::is_space).starts_with('#') {
            continue;
        }
        if !line.chars().next().is_some_and(pystr::is_space) {
            break;
        }
        items += 1;
        match pattern("entry").captures(line) {
            Ok(Some(entry)) => {
                let group = |k: usize| entry.get(k).map_or("", |m| m.as_str()).to_owned();
                if let Some(n) = python_int(&group(1)) {
                    out.entries.push((n, group(2), group(3), i + 1));
                }
            }
            _ => {
                let n = pattern("entry_number").captures(line).ok().flatten().and_then(|n| n.get(1)).and_then(|n| python_int(n.as_str()));
                out.defects.push((i + 1, pystr::strip(line).to_owned(), n, why("entry_out_of_form", &[])));
            }
        }
    }
    if items == 0 {
        out.on_field = Some((why("field_empty", &[]), start + 1, why("field_empty_text", &[])));
    }
    Some(out)
}

/// A line of reconciliation: the date of the entry it is in (`0000-00-00` before the first), the index of its file,
/// its number from 1, the destination, and the file's path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationRow {
    pub date: String,
    pub file: usize,
    pub line: usize,
    pub destination: String,
    pub path: String,
}

/// The lines `#<task>/V<n> → <destination>` by `(task, n)`, and the tasks with `#<task>/resta → nessuno`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reconciliations {
    pub rows: BTreeMap<(u64, u64), Vec<ReconciliationRow>>,
    pub none: BTreeSet<u64>,
}

/// `leggi_riconciliazioni`: the decisions' files `(path, text)` in order, archives first. A destination ends at the
/// first `;`, ` · ` or at the end of the line, and bold does not count; a short `; V2 → #275` after a whole one
/// belongs to the task of the last whole reference of its line.
pub fn read_reconciliations(files: &[(String, String)]) -> Reconciliations {
    let mut out = Reconciliations::default();
    let split = words().destination_split;
    for (k, (path, text)) in files.iter().enumerate() {
        let mut date = "0000-00-00".to_owned();
        for (i, line) in clean(text).split('\n').enumerate() {
            if let Ok(Some(found)) = pattern("line_date").captures(line)
                && let Some(day) = found.get(1)
            {
                date = day.as_str().to_owned();
            }
            let mut current: Option<u64> = None;
            for segment in line.split(';') {
                let mut matches: Vec<(u64, u64, String)> = pattern("reconciliation")
                    .captures_iter(segment)
                    .flatten()
                    .filter_map(|c| Some((python_int(c.get(1)?.as_str())?, python_int(c.get(2)?.as_str())?, c.get(3)?.as_str().to_owned())))
                    .collect();
                if matches.is_empty()
                    && let Some(task) = current
                    && let Ok(Some(short)) = pattern("reconciliation_short").captures(segment)
                    && let (Some(n), Some(destination)) = (short.get(1).and_then(|n| python_int(n.as_str())), short.get(2))
                {
                    matches.push((task, n, destination.as_str().to_owned()));
                }
                for (task, n, destination) in matches {
                    current = Some(task);
                    let destination = destination.replace('*', "");
                    let destination = cut_at_split(&destination, split);
                    let row = ReconciliationRow { date: date.clone(), file: k, line: i + 1, destination: pystr::strip(destination).to_owned(), path: path.clone() };
                    out.rows.entry((task, n)).or_default().push(row);
                }
            }
            for captures in pattern("reconciliation_none").captures_iter(line).flatten() {
                out.none.extend(captures.get(1).and_then(|task| python_int(task.as_str())));
            }
        }
    }
    out
}

/// `re.split(r"\s·\s", text)[0]`: the text before the first `·` with a blank on each side.
fn cut_at_split(text: &str, split: char) -> &str {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for window in chars.windows(3) {
        if pystr::is_space(window[0].1) && window[1].1 == split && pystr::is_space(window[2].1) {
            return &text[..window[0].0];
        }
    }
    text
}

/// What a destination is (contract-resta §5, case 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Issue(u64),
    /// `?`: nobody reconciled it.
    Unanswered,
    Discarded,
    Done,
    Invalid,
}

/// What a destination is: `#N`, `?`, a discard with its reason, or a done with a sha, a `#N`, a path or a round;
/// anything else is invalid.
pub fn classify_destination(destination: &str) -> Destination {
    let text = pystr::strip(destination);
    if let Ok(Some(issue)) = pattern("destination_issue").captures(text)
        && let Some(n) = issue.get(1).and_then(|n| python_int(n.as_str()))
    {
        return Destination::Issue(n);
    }
    if text == "?" {
        return Destination::Unanswered;
    }
    if found("destination_discarded", text) {
        return Destination::Discarded;
    }
    if let Ok(Some(done)) = pattern("destination_done").captures(text)
        && let Some(place) = done.get(1).map(|m| m.as_str())
        && (found("sha", place) || !references(place).is_empty() || found("path", place) || found("round", place))
    {
        return Destination::Done;
    }
    Destination::Invalid
}

fn date_of(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// `None` if the destination holds, else why (cases 3 to 7). `line_date` is the date of the
/// report or of the entry of the decisions that names the destination.
fn destination_problem(destination: &str, task: u64, line_date: &str, issues: &BTreeMap<u64, IssueFacts>) -> Option<String> {
    let n = match classify_destination(destination) {
        Destination::Unanswered => return Some(why("destination_unanswered", &[])),
        Destination::Invalid => return Some(why("destination_invalid", &[("destination", &truncate(destination, 8))])),
        Destination::Discarded | Destination::Done => return None,
        Destination::Issue(n) => n,
    };
    let number = n.to_string();
    if n == task {
        return Some(why("destination_itself", &[("n", &number)]));
    }
    let Some(issue) = issues.get(&n) else { return Some(why("destination_missing", &[("n", &number)])) };
    if issue.state != IssueState::Closed {
        return None;
    }
    if issue.state_reason.as_deref() == Some("NOT_PLANNED") {
        return Some(why("destination_discarded", &[("n", &number)]));
    }
    let closed = rome_date(issue.closed_at.as_deref());
    let line = (line_date > "0001").then(|| date_of(line_date)).flatten();
    match (closed, line) {
        (Some(closed), Some(line)) if (line - closed).num_days() >= words().closed_destination_days => {
            let (closed, line) = (closed.format("%d/%m").to_string(), line.format("%d/%m").to_string());
            Some(why("destination_stale", &[("n", &number), ("closed", &closed), ("line", &line)]))
        }
        _ => None,
    }
}

/// The item of `elencate`: the normalized text, the text, the destination, the report and the line.
struct Listed<'a> {
    normalized: String,
    text: String,
    destination: String,
    report: &'a ReportRemaining,
    line: usize,
}

/// `resta_non_riconciliato`: the items of `resta` with no destination that holds. `reports`: `(name, text)` of
/// `.regia/report/`; `decisions`: `(path, text)`, the archives then `decisioni.md`; `issues`: every issue, open and
/// closed. Only the `done` reports dated from `from` (the format's date by default) whose task closed as done
/// count. A malformed field or line counts as a report with no field, and any line of reconciliation of the task
/// clears it; the lines out of form only when the items reconciled are at least as many as the report's lines.
pub fn remaining_unreconciled(
    reports: &[(String, String)],
    decisions: &[(String, String)],
    issues: &BTreeMap<u64, IssueFacts>,
    project: &str,
    from: Option<&str>,
) -> Vec<Finding> {
    let from = from.unwrap_or_else(|| remaining_from());
    let mut sorted: Vec<&(String, String)> = reports.iter().collect();
    sorted.sort();
    let mut per_task: BTreeMap<u64, Vec<ReportRemaining>> = BTreeMap::new();
    for (name, text) in sorted {
        if name.chars().take(10).collect::<String>().as_str() >= from
            && let Some(report) = read_report(name, text)
        {
            per_task.entry(report.task).or_default().push(report);
        }
    }
    let reconciliations = read_reconciliations(decisions);
    let reports_folder = &files().reports;
    let mut out = Vec::new();
    for (task, listed_reports) in &per_task {
        let task = *task;
        let Some(issue) = issues.get(&task) else { continue };
        if issue.state != IssueState::Closed || issue.state_reason.as_deref() != Some("COMPLETED") {
            continue;
        }
        let finding = |location: String, text: &str, reason: &str| {
            let closed = rome_date(issue.closed_at.as_deref()).map(|date| why("remaining_closed_on", &[("date", &date.format("%d/%m").to_string())]));
            let reason = why("remaining_task", &[("task", &task.to_string()), ("closed", closed.as_deref().unwrap_or_default()), ("why", reason)]);
            Finding::new(CheckId::RemainingUnreconciled, project, location, text, reason).with_url(issue.url.as_deref())
        };
        let task_rows: BTreeSet<u64> = reconciliations.rows.keys().filter(|(t, _)| *t == task).map(|(_, n)| *n).collect();
        let with_row = reconciliations.none.contains(&task) || !task_rows.is_empty();
        if !listed_reports.iter().any(|report| report.has_field) {
            if !with_row {
                let last = &listed_reports[listed_reports.len() - 1];
                let text = why("no_field_text", &[("task", &task.to_string())]);
                out.push(finding(format!("{reports_folder}/{}:1", last.name), &text, &why("no_field", &[])));
            }
            continue;
        }
        let numbered: BTreeSet<u64> = listed_reports.iter().flat_map(|report| report.entries.iter().map(|entry| entry.0)).collect();
        let items = numbered.len() + listed_reports.iter().map(|report| report.defects.len()).sum::<usize>();
        let listing_done = task_rows.len() >= items;
        let mut listed: BTreeMap<u64, Listed> = BTreeMap::new();
        for report in listed_reports {
            if let Some((reason, line, text)) = &report.on_field
                && !with_row
            {
                out.push(finding(format!("{reports_folder}/{}:{line}", report.name), text, reason));
            }
            for (line, text, _, reason) in &report.defects {
                if !(with_row && listing_done) {
                    out.push(finding(format!("{reports_folder}/{}:{line}", report.name), text, reason));
                }
            }
            for (n, text, destination, line) in &report.entries {
                let normalized = pystr::strip(&collapse_spaces(text)).to_owned();
                if let Some(before) = listed.get(n)
                    && before.normalized != normalized
                    && !reconciliations.rows.contains_key(&(task, *n))
                {
                    let shown = format!("V{n}: {text}");
                    out.push(finding(format!("{reports_folder}/{}:{line}", report.name), &shown, &why("two_texts", &[("n", &n.to_string())])));
                }
                listed.insert(*n, Listed { normalized, text: text.clone(), destination: destination.clone(), report, line: *line });
            }
        }
        let numbers: BTreeSet<u64> = listed.keys().copied().chain(task_rows.iter().copied()).collect();
        for n in numbers {
            let entry = listed.get(&n);
            let (location, line_date, text, destination) = match (reconciliations.rows.get(&(task, n)), entry) {
                (Some(rows), _) => {
                    // The last by date, then file, then line; the first of equals, as Python's `max`.
                    let mut last = &rows[0];
                    for row in &rows[1..] {
                        if (row.date.as_str(), row.file, row.line) > (last.date.as_str(), last.file, last.line) {
                            last = row;
                        }
                    }
                    let text = entry.map_or_else(|| last.destination.clone(), |entry| entry.text.clone());
                    (format!("{}:{}", last.path, last.line), last.date.clone(), text, last.destination.clone())
                }
                (None, Some(entry)) => (
                    format!("{reports_folder}/{}:{}", entry.report.name, entry.line),
                    entry.report.date.clone(),
                    entry.text.clone(),
                    entry.destination.clone(),
                ),
                (None, None) => continue,
            };
            if let Some(reason) = destination_problem(&destination, task, &line_date, issues) {
                out.push(finding(location, &format!("V{n}: {text}"), &reason));
            }
        }
    }
    out
}
