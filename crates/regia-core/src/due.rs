//! Due dates. A task's spec may declare the date after which it loses its value (#244 of the dossier, contract
//! of the graph §4.2). In each section of the brief the tasks due soon go first, by date; an overdue task
//! keeps its place and says so; a date that cannot be read is ignored and listed, so it does not go unseen.

use chrono::{Days, NaiveDate};
use regex::Regex;
use serde::Serialize;

use crate::brief::BriefTask;
use crate::model::DueDate;

/// A task is due soon from today to this many days later, both included. A date already past is overdue,
/// not soon: it would stay on top forever.
pub const DUE_SOON_DAYS: u64 = 3;

/// A day written `YYYY-MM-DD` with ASCII digits, as the prototype accepts it: a real date from year 1.
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, byte)| if i == 4 || i == 7 { *byte == b'-' } else { byte.is_ascii_digit() });
    if !shaped {
        return None;
    }
    let year: i32 = text[..4].parse().ok()?;
    let month: u32 = text[5..7].parse().ok()?;
    let day: u32 = text[8..].parse().ok()?;
    (year >= 1).then(|| NaiveDate::from_ymd_opt(year, month, day)).flatten()
}

/// Python's `str.strip()`: Unicode whitespace, and the four separators U+001C–U+001F, which Python counts as
/// whitespace and Unicode does not.
fn strip_python(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// How a body declares its due date: the first YAML block is the spec's, and only a line of its own, in the
/// first column, counts. The key and the placeholder are data of the tracker format.
#[derive(Debug, Clone)]
pub struct DueRule {
    block: Regex,
    line: Regex,
    placeholder: String,
}

impl DueRule {
    pub fn new(key: &str, placeholder: &str) -> Self {
        Self {
            block: Regex::new(r"(?s)```ya?ml[ \t]*\r?\n(.*?)```").expect("a valid pattern"),
            line: Regex::new(&format!(r"(?m)^{}[ \t]*:[ \t]*(.*?)[ \t]*$", regex::escape(key))).expect("a valid pattern"),
            placeholder: placeholder.to_owned(),
        }
    }

    /// `None` without the line, with an empty value or with the template's placeholder. A trailing comment and
    /// quotes around the value do not count.
    pub fn read(&self, body: &str) -> Option<DueDate> {
        let block = self.block.captures(body)?.get(1)?.as_str();
        let written = self.line.captures(block)?.get(1)?.as_str();
        let value = written.split('#').next().unwrap_or_default();
        let value = strip_python(value).trim_matches(|c| c == '"' || c == '\'');
        if value.is_empty() || value == self.placeholder {
            return None;
        }
        Some(parse_date(value).map_or_else(|| DueDate::Malformed(value.to_owned()), DueDate::Date))
    }
}

/// What the brief says of an entry's due date. Each key is there only when it applies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DueMark {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub due_soon: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub overdue: bool,
}

/// A due date the brief could not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IgnoredDue {
    pub number: u64,
    pub value: String,
}

/// `entries`, already in their section's order, with the tasks due soon on top, by date and then in that
/// order; the rest does not move. Returns the entries and the due dates that could not be read.
pub fn with_due_dates(entries: Vec<BriefTask>, today: NaiveDate) -> (Vec<BriefTask>, Vec<IgnoredDue>) {
    let last = today.checked_add_days(Days::new(DUE_SOON_DAYS)).unwrap_or(NaiveDate::MAX);
    let mut ignored = Vec::new();
    let mut soon = Vec::new();
    let mut rest = Vec::new();
    for mut entry in entries {
        match entry.task.due.clone() {
            None => rest.push(entry),
            Some(DueDate::Malformed(value)) => {
                ignored.push(IgnoredDue { number: entry.task.number, value });
                rest.push(entry);
            }
            Some(DueDate::Date(date)) => {
                entry.due.due = Some(date);
                if today <= date && date <= last {
                    entry.due.due_soon = true;
                    soon.push((date, entry));
                } else {
                    entry.due.overdue = date < today;
                    rest.push(entry);
                }
            }
        }
    }
    // Stable: on the same date the section's order stays.
    soon.sort_by_key(|(date, _)| *date);
    (soon.into_iter().map(|(_, entry)| entry).chain(rest).collect(), ignored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{IssueState, Task};

    fn rule() -> DueRule {
        DueRule::new("due", "YYYY-MM-DD")
    }

    fn spec(line: &str) -> String {
        format!("A spec.\n\n```yaml\nproject: p\nsize: S\n{line}\nmaturity: ready\n```\n")
    }

    fn day(text: &str) -> NaiveDate {
        parse_date(text).unwrap()
    }

    fn entry(number: u64, due: Option<DueDate>) -> BriefTask {
        let task = Task {
            number,
            title: String::new(),
            state: IssueState::Open,
            state_reason: None,
            status: None,
            project: None,
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
            due,
        };
        BriefTask { task, metrics: None, due: DueMark::default() }
    }

    #[test]
    fn the_line_is_read_in_the_first_yaml_block_only() {
        let date = Some(DueDate::Date(day("2026-11-05")));
        assert_eq!(rule().read(&spec("due: 2026-11-05")), date);
        assert_eq!(rule().read(&spec("due: 2026-11-05   # the K0")), date);
        assert_eq!(rule().read(&spec("due: \"2026-11-05\"")), date);
        assert_eq!(rule().read(&spec("due: '2026-11-05'")), date);
        assert_eq!(rule().read("```yaml\r\ndue: 2026-11-05\r\n```\r\n"), date, "CRLF lines");
        assert_eq!(rule().read("```yml\ndue: 2026-11-05\n```"), date);
        assert_eq!(rule().read(&spec("size: M")), None);
        assert_eq!(rule().read("due: 2026-11-05\n(in prose, not in the YAML block)"), None);
        assert_eq!(rule().read(""), None);
        let later = "```yaml\nproject: p\n```\n\n## A quoted example\n```yaml\ndue: 2026-11-03\n```\n";
        assert_eq!(rule().read(later), None, "only the first block is the spec");
        let two = "```yaml\ndue: 2026-11-02\n```\n\n```yaml\ndue: 2026-12-01\n```\n";
        assert_eq!(rule().read(two), Some(DueDate::Date(day("2026-11-02"))));
        assert_eq!(rule().read("```yaml\nother:\n  due: 2026-11-03\n```\n"), None, "a nested key");
    }

    #[test]
    fn a_malformed_date_is_kept_as_written() {
        for value in ["tomorrow", "5/11", "2026-13-40", "2026-11-5", "2026-02-30", "0000-01-01", "２０２６-１１-０５"] {
            assert_eq!(rule().read(&spec(&format!("due: {value}"))), Some(DueDate::Malformed(value.to_owned())), "{value}");
        }
    }

    #[test]
    fn the_placeholder_and_an_empty_value_mean_no_date() {
        for line in ["due: YYYY-MM-DD        # optional: the date after which…", "due:", "due:   # to decide"] {
            assert_eq!(rule().read(&spec(line)), None, "{line}");
        }
    }

    #[test]
    fn whitespace_is_stripped_as_python_strips_it() {
        let date = Some(DueDate::Date(day("2026-11-05")));
        assert_eq!(rule().read(&spec("due:\u{1c}2026-11-05\u{1f}")), date, "separators Python counts as space");
        assert_eq!(rule().read(&spec("due: \u{a0}2026-11-05\u{3000}")), date);
    }

    #[test]
    fn leap_days_and_the_first_year() {
        assert_eq!(parse_date("2028-02-29"), NaiveDate::from_ymd_opt(2028, 2, 29));
        assert_eq!(parse_date("2026-02-29"), None);
        assert_eq!(parse_date("0001-01-01"), NaiveDate::from_ymd_opt(1, 1, 1));
        assert_eq!(parse_date("+2026-01-01"), None);
        assert_eq!(parse_date("2026-01-01 "), None);
    }

    #[test]
    fn the_window_runs_from_today_to_three_days_later_and_past_dates_stay() {
        let today = day("2026-11-02");
        let dated = |number, text| entry(number, Some(DueDate::Date(day(text))));
        let entries = vec![
            dated(1, "2026-11-06"),
            dated(2, "2026-11-05"),
            dated(3, "2026-11-02"),
            dated(4, "2026-10-28"),
            dated(5, "2026-11-04"),
            entry(6, None),
        ];
        let (out, ignored) = with_due_dates(entries, today);
        assert!(ignored.is_empty());
        let numbers: Vec<u64> = out.iter().map(|e| e.task.number).collect();
        assert_eq!(numbers, [3, 5, 2, 1, 4, 6]);
        let soon: Vec<bool> = out.iter().map(|e| e.due.due_soon).collect();
        assert_eq!(soon, [true, true, true, false, false, false]);
        assert!(out[4].due.overdue && out[4].due.due == Some(day("2026-10-28")));
        assert!(!out[0].due.overdue, "today is soon, not overdue");
        assert_eq!(out[3].due, DueMark { due: Some(day("2026-11-06")), due_soon: false, overdue: false });
        assert_eq!(out[5].due, DueMark::default());
    }

    #[test]
    fn on_the_same_date_the_order_stays_and_a_malformed_date_is_listed() {
        let today = day("2026-11-02");
        let entries = vec![
            entry(30, Some(DueDate::Date(day("2026-11-03")))),
            entry(10, Some(DueDate::Malformed("soon".into()))),
            entry(20, Some(DueDate::Date(day("2026-11-03")))),
        ];
        let (out, ignored) = with_due_dates(entries, today);
        let numbers: Vec<u64> = out.iter().map(|e| e.task.number).collect();
        assert_eq!(numbers, [30, 20, 10]);
        assert_eq!(ignored, [IgnoredDue { number: 10, value: "soon".into() }]);
    }

    #[test]
    fn only_the_keys_that_apply_are_written() {
        let mark = |due: Option<&str>, due_soon, overdue| {
            serde_json::to_value(DueMark { due: due.map(day), due_soon, overdue }).unwrap()
        };
        assert_eq!(mark(None, false, false), serde_json::json!({}));
        assert_eq!(mark(Some("2026-11-03"), true, false), serde_json::json!({"due": "2026-11-03", "due_soon": true}));
        assert_eq!(mark(Some("2026-10-03"), false, true), serde_json::json!({"due": "2026-10-03", "overdue": true}));
    }
}
