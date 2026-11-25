//! `absolute-path` (#283 of the dossier, contratti.md «Riferimenti»): an absolute path of the machine written in
//! the state file, in a report, or in an entry of the decisions, from the format's date. The exceptions are by form, not
//! by place: the folders of a report's front matter, fenced blocks and quoted lines. And `decisions-over-cap` (#36,
//! ADR 0024 of the dossier): `decisioni.md` past its cap, in characters.

use std::collections::BTreeMap;
use std::path::Path;

use super::{CheckId, Finding, absolute_from, clean, files, found, pattern, why};
use crate::pystr;
use crate::reference::{ReferenceKind, resolve};

/// The kind of a line of a checked file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A fence or a line inside one.
    Block,
    Quote,
    Text,
}

/// `_assoluti_in_riga`: the absolute paths of a line. The pattern finds where one may start; whether it is
/// absolute is the resolver's to say, the one definition; a full stop, colon or `!` at its end is the sentence's.
fn absolute_in_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for found_path in pattern("absolute").find_iter(line).flatten() {
        let candidate = found_path.as_str().trim_end_matches(['.', ':', '!']);
        if !candidate.is_empty() && !found("bare_root", candidate) && resolve(candidate, Path::new(".")).kind() == ReferenceKind::Absolute {
            out.push(candidate.to_owned());
        }
    }
    out
}

/// `_righe_classificate`: every line `(number from 1, line, kind)`, and the line of a fence never closed. A fence
/// closes one of the same sign at least as long; one never closed runs to the end of the file, as in CommonMark.
fn classified_lines(text: &str) -> (Vec<(usize, String, Kind)>, Option<usize>) {
    let mut open: Option<String> = None;
    let mut open_line = None;
    let mut out = Vec::new();
    for (i, line) in clean(text).split('\n').enumerate() {
        let n = i + 1;
        let fence = pattern("fence").captures(line).ok().flatten().and_then(|captures| captures.get(1)).map(|m| m.as_str().to_owned());
        if let Some(opening) = &open {
            out.push((n, line.to_owned(), Kind::Block));
            let sign = opening.chars().next().unwrap_or('`');
            if fence.as_ref().is_some_and(|fence| fence.starts_with(sign) && fence.len() >= opening.len())
                && pystr::strip(line).trim_matches(sign).is_empty()
            {
                open = None;
                open_line = None;
            }
            continue;
        }
        if let Some(fence) = fence {
            open = Some(fence);
            open_line = Some(n);
            out.push((n, line.to_owned(), Kind::Block));
            continue;
        }
        let kind = if found("quote", line) { Kind::Quote } else { Kind::Text };
        out.push((n, line.to_owned(), kind));
    }
    (out, open_line)
}

/// `_data_per_riga`: the date of the entry of the decisions each line belongs to. An entry begins with `- <date> ·`
/// and goes on until another entry (a bullet in the first column), a title, or a blank line followed by a line that
/// is not indented; the lines of a fenced block do not change the entry.
fn date_per_line(lines: &[(usize, String, Kind)]) -> BTreeMap<usize, String> {
    let mut date: Option<String> = None;
    let mut after_blank = false;
    let mut out = BTreeMap::new();
    for (n, line, kind) in lines {
        if *kind == Kind::Block {
            if let Some(date) = &date {
                out.insert(*n, date.clone());
            }
            continue;
        }
        if let Ok(Some(entry)) = pattern("decision_entry").captures(line) {
            date = entry.get(1).map(|m| m.as_str().to_owned());
            after_blank = false;
        } else if line.starts_with('#') || found("other_entry", line) {
            date = None;
            after_blank = false;
        } else if pystr::strip(line).is_empty() {
            after_blank = true;
        } else {
            if after_blank && !line.chars().next().is_some_and(pystr::is_space) {
                date = None;
            }
            after_blank = false;
        }
        if let Some(date) = &date {
            out.insert(*n, date.clone());
        }
    }
    out
}

/// `_front_matter_fine`: the index from 0 of the `---` that closes the front matter, if the file opens with one.
fn front_matter_end(lines: &[(usize, String, Kind)]) -> Option<usize> {
    let first = lines.first()?;
    if pystr::strip(&first.1) != "---" {
        return None;
    }
    lines.iter().enumerate().skip(1).find(|(_, (_, line, _))| pystr::strip(line) == "---").map(|(i, _)| i)
}

fn search(text: &str, path: &str, per_entry: bool, front_matter: bool, from: &str, project: &str, out: &mut Vec<Finding>) {
    let (lines, open) = classified_lines(text);
    let end = if front_matter { front_matter_end(&lines) } else { None };
    let dates = per_entry.then(|| date_per_line(&lines));
    for (n, line, kind) in &lines {
        if *kind != Kind::Text {
            continue;
        }
        if let Some(dates) = &dates
            && dates.get(n).map_or("", String::as_str) < from
        {
            continue;
        }
        if end.is_some_and(|end| n - 1 <= end) && found("front_matter_exempt", line) {
            continue;
        }
        for absolute in absolute_in_line(line) {
            out.push(Finding::new(CheckId::AbsolutePath, project, format!("{path}:{n}"), &absolute, why("absolute_path", &[])));
        }
    }
    if let Some(open) = open {
        out.push(Finding::new(CheckId::AbsolutePath, project, format!("{path}:{open}"), &why("open_fence_text", &[]), why("open_fence", &[])));
    }
}

/// A finding for every absolute path in `state` (always: it is rewritten at every step), in the
/// reports `(name, text)` whose name is dated from `from` (the format's date by default), and in the entries of the
/// decisions' files `(path, text)` dated from it. A fence never closed is a finding of its own.
pub fn absolute_paths(
    state: Option<&str>,
    reports: &[(String, String)],
    decisions: Option<&[(String, Option<String>)]>,
    project: &str,
    from: Option<&str>,
) -> Vec<Finding> {
    let from = from.unwrap_or_else(|| absolute_from());
    let mut out = Vec::new();
    if let Some(state) = state {
        search(state, &files().state, false, false, from, project, &mut out);
    }
    let mut sorted: Vec<&(String, String)> = reports.iter().collect();
    sorted.sort();
    for (name, text) in sorted {
        if name.chars().take(10).collect::<String>().as_str() >= from {
            search(text, &format!("{}/{name}", files().reports), false, true, from, project, &mut out);
        }
    }
    for (path, text) in decisions.unwrap_or_default() {
        if let Some(text) = text {
            search(text, path, true, false, from, project, &mut out);
        }
    }
    out
}

/// `decisioni_oltre_tetto`: a finding if the decisions' file has more than `cap` characters (not bytes), counted on
/// the text as it is on disk, its `\r` included.
pub fn decisions_over_cap(text: Option<&str>, project: &str, cap: u64, path: &str) -> Vec<Finding> {
    let count = text.map_or(0, |text| text.chars().count());
    if u64::try_from(count).unwrap_or(u64::MAX) <= cap {
        return Vec::new();
    }
    let text = why("over_cap_text", &[("path", path), ("count", &count.to_string())]);
    vec![Finding::new(CheckId::DecisionsOverCap, project, path.to_owned(), &text, why("over_cap", &[("cap", &cap.to_string()), ("project", project)]))]
}
