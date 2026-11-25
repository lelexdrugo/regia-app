//! The checks of the dossier's texts: the state file (`state-without-issue`, `state-lines-removed`) and
//! `docs/open-questions.md` (`open-questions-without-issue`). A marked line splits into segments on `;`, `·` and
//! every hand outside a parenthesis, and each segment is covered by an issue it cites: an open one for work, one
//! that waits for the owner (Proposal or Decision) for the owner. Lines split on `\n` only, as the prototype's.

use std::collections::{BTreeMap, BTreeSet};

use super::{CheckId, Finding, clean, found, has_word, lowercase, normalized_line, pattern, references, why, without_bullet, words};
use crate::model::Status;
use crate::pystr;

/// `segmenti`: the segments of a line, without its bullet; those with no letter or digit besides the hand do not
/// count. The end of a sentence does not split, and a hand inside a parenthesis does not open a segment.
pub fn segments(line: &str) -> Vec<String> {
    let hand = words().hand;
    let text = without_bullet(&clean(line));
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth: i64 = 0;
    for c in text.chars() {
        if c == ';' || c == '·' || (c == hand && depth <= 0 && !current.is_empty()) {
            out.push(std::mem::take(&mut current));
            if c == hand {
                current.push(c);
            }
            // A parenthesis opened and never closed does not cover the rest of the line.
            depth = 0;
            continue;
        }
        depth += i64::from(c == '(') - i64::from(c == ')');
        current.push(c);
    }
    out.push(current);
    out.into_iter().filter(|segment| has_word(&segment.replace(hand, ""))).map(|segment| pystr::strip(&segment).to_owned()).collect()
}

/// The markers of a line: for the owner, for work, and those that count only for a removed line.
fn markers(line: &str) -> (bool, bool, bool) {
    let lower = lowercase(line);
    (found("owner", &lower), found("work", &lower), found("extra", &lower))
}

/// The length of a line's indentation, in characters.
fn indentation(line: &str) -> usize {
    line.chars().count() - line.trim_start_matches(pystr::is_space).chars().count()
}

/// `_righe_marcate`: the marked lines, `(number from 1, line, for the owner)`, past the headings and the ticked
/// boxes. With `inherit` a marked line that ends with `:` and has indented lines below is a heading: it gives no
/// finding and passes its markers on to them.
fn marked_lines(text: &str, with_extra: bool, inherit: bool) -> Vec<(usize, String, bool)> {
    let text = clean(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    // (indentation, owner, work, extra, heading)
    let mut stack: Vec<(usize, bool, bool, bool, bool)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if pystr::strip(line).is_empty() || found("heading", line) {
            continue;
        }
        let indent = indentation(line);
        let (mut owner, mut work, mut extra) = markers(line);
        let mut heading = false;
        if inherit {
            while stack.last().is_some_and(|top| top.0 >= indent) {
                stack.pop();
            }
            if let Some(&(_, top_owner, top_work, top_extra, true)) = stack.last() {
                (owner, work, extra) = (owner || top_owner, work || top_work, extra || top_extra);
            }
            let next = lines[i + 1..].iter().find(|next| !pystr::strip(next).is_empty()).copied().unwrap_or_default();
            heading = (owner || work || extra) && line.trim_end_matches([' ', '*', '`']).ends_with(':') && indentation(next) > indent;
            stack.push((indent, owner, work, extra, heading));
        }
        if found("done_box", line) || heading {
            continue;
        }
        if owner || work || (with_extra && extra) {
            out.push((i + 1, (*line).to_owned(), owner));
        }
    }
    out
}

/// `_non_coperto`: `None` if the segment is covered, else why. `open`: the open issues by number, with their status.
fn uncovered(segment: &str, owner: bool, open: &BTreeMap<u64, Option<Status>>, only_open: bool) -> Option<String> {
    let cited = references(segment);
    if !only_open && !cited.is_empty() {
        return None;
    }
    let is_open = |n: &u64| open.contains_key(n);
    if owner {
        if cited.iter().any(|n| open.get(n).copied().flatten().is_some_and(Status::waits_on_owner)) {
            return None;
        }
        if cited.iter().any(is_open) {
            return Some(why("owner_open_not_waiting", &[]));
        }
        return Some(why(if cited.is_empty() { "owner_no_number" } else { "owner_only_closed" }, &[]));
    }
    if cited.iter().any(is_open) {
        return None;
    }
    Some(why(if cited.is_empty() { "no_open_number" } else { "only_closed" }, &[]))
}

/// `_continuazione`: in a line for the owner, a segment with no marker of its own goes on with the one before,
/// which covers itself.
fn continuation(segment: &str, owner: bool) -> bool {
    owner && !found("owner", &lowercase(segment))
}

/// A finding for every marked segment that is not covered.
pub fn state_without_issue(text: &str, open: &BTreeMap<u64, Option<Status>>, project: &str, path: &str, inherit: bool) -> Vec<Finding> {
    let mut out = Vec::new();
    for (n, line, owner) in marked_lines(text, false, inherit) {
        for segment in segments(&line) {
            if continuation(&segment, owner) {
                continue;
            }
            if let Some(reason) = uncovered(&segment, owner, open, true) {
                out.push(Finding::new(CheckId::StateWithoutIssue, project, format!("{path}:{n}"), &segment, reason));
            }
        }
    }
    out
}

/// The lines of `old` that `new` no longer has, normalized, with a marker (the removed lines'
/// too) and a segment not covered whose text `new` does not have either. With `only_open` false a citation of a
/// closed issue covers: the line was tracked and its issue closed.
pub fn state_lines_removed(
    old: &str,
    new: &str,
    open: &BTreeMap<u64, Option<Status>>,
    project: &str,
    path: &str,
    only_open: bool,
    inherit: bool,
) -> Vec<Finding> {
    let new_lines: BTreeSet<String> = clean(new).split('\n').map(normalized_line).collect();
    let new_text = new_lines.iter().cloned().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    for (n, line, owner) in marked_lines(old, true, inherit) {
        if new_lines.contains(&normalized_line(&line)) {
            continue;
        }
        for segment in segments(&line) {
            if continuation(&segment, owner) {
                continue;
            }
            if let Some(reason) = uncovered(&segment, owner, open, only_open)
                && !new_text.contains(&normalized_line(&segment))
            {
                out.push(Finding::new(CheckId::StateLinesRemoved, project, format!("{path}:{n}"), &segment, why("removed", &[("why", &reason)])));
            }
        }
    }
    out
}

/// `domande_aperte`: the open boxes `- [ ]` with no `#N`, in their line or in the indented lines that go on with it.
pub fn open_questions(text: &str, project: &str, path: &str) -> Vec<Finding> {
    let text = clean(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Ok(Some(captures)) = pattern("open_box").captures(line) else { continue };
        let indent = captures.get(1).map_or(0, |m| m.as_str().chars().count());
        let first = captures.get(2).map_or("", |m| m.as_str());
        let mut body = vec![first.to_owned()];
        let mut j = i + 1;
        while j < lines.len()
            && !pystr::strip(lines[j]).is_empty()
            && indentation(lines[j]) > indent
            && !found("open_box", lines[j])
            && !found("bullet", lines[j])
        {
            body.push(pystr::strip(lines[j]).to_owned());
            j += 1;
        }
        if references(&body.join(" ")).is_empty() {
            out.push(Finding::new(CheckId::OpenQuestionsWithoutIssue, project, format!("{path}:{}", i + 1), first, why("open_box", &[])));
        }
    }
    out
}
