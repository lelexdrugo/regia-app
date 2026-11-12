//! The events cursor of a project, read as the prototype's precheck reads it (L10, P4 of the dossier's
//! conformance study, #406): the instant of the last step, the comments that step wrote itself, and the
//! reports it processed with the date each had. Its failures keep the prototype's classes: a value that
//! does not read is `unreadable`; an own part that is an object of no kind at all is `internal`, as the
//! prototype's attribute error is.

use std::collections::{BTreeMap, BTreeSet};

use regia_store::WallTime;
use serde_json::Value;

use super::Fault;
use super::v1::format;

/// What the precheck needs of the events cursor before it looks at the lock.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Since {
    /// The instant from which reports, comments and Inbox items are new; `None` makes everything new.
    pub(crate) t: Option<WallTime>,
    /// The ids of the comments the steps wrote themselves, which are never an answer.
    pub(crate) own_comments: BTreeSet<i128>,
}

/// An instant of the cursor, or of the tracker, as `datetime.fromisoformat` reads it after the `Z`; text
/// only, since no other JSON value spells an instant.
pub(crate) fn instant(value: &Value) -> Result<WallTime, Fault> {
    value
        .as_str()
        .and_then(WallTime::parse_iso)
        .ok_or_else(|| Fault::unreadable(format!("not a valid instant in the cursor: {value}")))
}

/// Python's truth of a JSON value: `null`, `false`, zero and empty are false.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// `value.get(own) or {}`: the cursor's own parts, empty when they are false; a true value that is not an
/// object fails as the prototype's attribute error does.
fn own_parts(fields: &serde_json::Map<String, Value>) -> Result<Option<&serde_json::Map<String, Value>>, Fault> {
    match fields.get(&format().events.own) {
        Some(Value::Object(own)) => Ok(Some(own)),
        Some(other) if truthy(other) => Err(Fault::internal(format!("the own parts of the events cursor are not an object: {other}"))),
        _ => Ok(None),
    }
}

/// The instant and the own comments of the cursor's value: none without a cursor, the instant alone in the
/// old form that is only text.
pub(crate) fn since(value: &Value) -> Result<Since, Fault> {
    let fields = match value {
        Value::Null => return Ok(Since::default()),
        Value::String(_) => return Ok(Since { t: Some(instant(value)?), own_comments: BTreeSet::new() }),
        Value::Object(fields) => fields,
        other => return Err(Fault::unreadable(format!("events cursor of an unexpected kind: {other}"))),
    };
    let words = &format().events;
    let own = own_parts(fields)?;
    let comments = own.and_then(|own| own.get(&words.comments)).filter(|comments| truthy(comments));
    let mut own_comments = BTreeSet::new();
    if let Some(comments) = comments {
        let ids = comments.as_array().and_then(|ids| ids.iter().map(whole_number).collect::<Option<Vec<i128>>>());
        own_comments = ids.ok_or_else(|| Fault::unreadable("the own comments are not a list of whole numbers".to_owned()))?.into_iter().collect();
    }
    let t = match fields.get(&words.t) {
        None | Some(Value::Null) => None,
        Some(t) => Some(instant(t)?),
    };
    Ok(Since { t, own_comments })
}

/// A JSON number written as a whole number, as Python's `json` makes an `int` of it: `1.0` is not one.
fn whole_number(value: &Value) -> Option<i128> {
    match value {
        Value::Number(number) => number.as_i64().map(i128::from).or_else(|| number.as_u64().map(i128::from)),
        _ => None,
    }
}

/// The reports the steps processed, by file name, with the date each had then (#406): none for a cursor
/// that is not an object or has no such part.
pub(crate) fn processed_reports(value: &Value) -> Result<BTreeMap<String, WallTime>, Fault> {
    let Value::Object(fields) = value else { return Ok(BTreeMap::new()) };
    let reports = own_parts(fields)?.and_then(|own| own.get(&format().events.reports)).filter(|reports| truthy(reports));
    let Some(reports) = reports else { return Ok(BTreeMap::new()) };
    let Value::Object(reports) = reports else {
        return Err(Fault::unreadable("the processed reports are not a map of names to dates".to_owned()));
    };
    reports.iter().map(|(name, date)| Ok((name.clone(), instant(date)?))).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::precheck::Reason;

    fn words() -> (&'static str, &'static str, &'static str, &'static str) {
        let words = &format().events;
        (&words.t, &words.own, &words.comments, &words.reports)
    }

    #[test]
    fn no_cursor_makes_everything_new_and_the_old_form_is_the_instant() {
        assert_eq!(since(&Value::Null), Ok(Since::default()));
        let old = since(&json!("2026-01-10T08:00:00Z")).unwrap();
        assert_eq!(old.t.map(WallTime::to_iso_seconds).as_deref(), Some("2026-01-10T08:00:00Z"));
        assert!(old.own_comments.is_empty());
        assert_eq!(processed_reports(&json!("2026-01-10T08:00:00Z")), Ok(BTreeMap::new()), "the old form has no reports");
    }

    #[test]
    fn the_parts_read_with_the_prototype_s_rules() {
        let (t, own, comments, reports) = words();
        let value = json!({t: "2026-01-10T08:00:00+01:00", own: {comments: [6276660826_u64, -1], reports: {"a.md": "2026-01-10T07:00:00.123456+00:00"}}});
        let read = since(&value).unwrap();
        assert_eq!(read.t.map(WallTime::to_iso_seconds).as_deref(), Some("2026-01-10T07:00:00Z"));
        assert_eq!(read.own_comments, BTreeSet::from([6_276_660_826, -1]));
        let processed = processed_reports(&value).unwrap();
        assert_eq!(processed["a.md"].to_python_iso(), "2026-01-10T07:00:00.123456+00:00");
        // Parts that are false read as absent: no instant makes everything new.
        let empty = json!({own: {comments: [], reports: {}}});
        assert_eq!(since(&empty), Ok(Since::default()));
        assert_eq!(processed_reports(&empty), Ok(BTreeMap::new()));
        assert_eq!(since(&json!({t: null, own: null})), Ok(Since::default()));
    }

    #[test]
    fn a_cursor_that_does_not_read_fails_in_the_prototype_s_class() {
        let (t, own, comments, reports) = words();
        for bad in [json!(42), json!(["x"]), json!(true), json!("not an instant"), json!({t: "nope"}), json!({t: 5}), json!({t: "2026-01-10T08:00:00Z", own: {comments: ["a"]}}), json!({own: {comments: [1.0]}}), json!({own: {comments: [true]}}), json!({own: {comments: {"a": 1}}})] {
            assert_eq!(since(&bad).map_err(|fault| fault.reason), Err(Reason::Unreadable), "{bad}");
        }
        for bad in [json!({own: [1]}), json!({own: "x"}), json!({own: 1})] {
            assert_eq!(since(&bad).map_err(|fault| fault.reason), Err(Reason::Internal), "{bad}");
        }
        for bad in [json!({own: {reports: ["a.md"]}}), json!({own: {reports: {"a.md": "yesterday"}}}), json!({own: {reports: {"a.md": 5}}})] {
            assert_eq!(processed_reports(&bad).map_err(|fault| fault.reason), Err(Reason::Unreadable), "{bad}");
        }
    }
}
