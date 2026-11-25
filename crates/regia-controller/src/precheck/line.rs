//! The outcome line (P1, P2 of the dossier's conformance study): the closed vocabulary of outcomes and
//! reasons, the exit code each outcome means, and the one JSON line the precheck prints last. This is the
//! only place an exit code comes from.

use super::v1::format;

/// What a tick found, as the controller reads it from the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// There is work for a step: exit 0.
    NewActivity,
    /// A step that only reports: exit 0.
    TerminalState,
    /// Nothing to do now: exit 3, which is not an error.
    Idle,
    /// The precheck could not decide, and fails closed: exit 1.
    Error,
}

impl Outcome {
    pub const ALL: [Outcome; 4] = [Outcome::NewActivity, Outcome::TerminalState, Outcome::Idle, Outcome::Error];

    #[must_use]
    pub fn word(self) -> &'static str {
        let outcomes = &format().outcomes;
        match self {
            Outcome::NewActivity => &outcomes.new_activity.word,
            Outcome::TerminalState => &outcomes.terminal_state.word,
            Outcome::Idle => &outcomes.idle.word,
            Outcome::Error => &outcomes.error.word,
        }
    }

    /// The exit code of the outcome, from the format's table.
    #[must_use]
    pub fn exit(self) -> u8 {
        let outcomes = &format().outcomes;
        match self {
            Outcome::NewActivity => outcomes.new_activity.exit,
            Outcome::TerminalState => outcomes.terminal_state.exit,
            Outcome::Idle => outcomes.idle.exit,
            Outcome::Error => outcomes.error.exit,
        }
    }
}

/// Why a tick or a project has its outcome. Each reason belongs to one outcome, so a line cannot pair a
/// reason with another outcome's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// Approved tasks, ready, with every dependency closed, and a free place in the project's wip.
    Ready,
    /// Reports written or rewritten since the step's instant.
    Reports,
    /// Comments of the owner on an issue that waits for the owner.
    Answers,
    /// Inbox items created since the step's instant.
    Inbox,
    /// Expired leases of tasks in progress.
    Leases,
    /// Items to formulate that are not events (#466).
    Backlog,
    /// A worker in progress that has been silent too long (#466).
    SilentWorkers,
    NoEvent,
    /// Someone may hold the project's lock.
    Busy,
    /// Ready tasks, and no free place in the wip.
    WipLimit,
    /// The budget does not allow automatic work (#466).
    Budget,
    /// Only Inbox items, with the open Proposals at their cap (#466).
    ProposalsExhausted,
    /// Issues wait for the owner, and nothing else happened.
    WaitingOnOwner,
    /// Tasks in progress, and no event.
    WorkInProgress,
    /// In the vocabulary, produced by no rule.
    ObjectiveDone,
    /// The same work seen tick after tick (#466).
    Stall,
    /// The tracker could not be read.
    Tracker,
    /// The state could not be read.
    Runtime,
    /// A value of the state, the registry or the reports does not read.
    Unreadable,
    /// Anything else: a wrong argument, a project nobody knows, a fault of the precheck itself.
    Internal,
}

impl Reason {
    pub const ALL: [Reason; 20] = [
        Reason::Ready,
        Reason::Reports,
        Reason::Answers,
        Reason::Inbox,
        Reason::Leases,
        Reason::Backlog,
        Reason::SilentWorkers,
        Reason::NoEvent,
        Reason::Busy,
        Reason::WipLimit,
        Reason::Budget,
        Reason::ProposalsExhausted,
        Reason::WaitingOnOwner,
        Reason::WorkInProgress,
        Reason::ObjectiveDone,
        Reason::Stall,
        Reason::Tracker,
        Reason::Runtime,
        Reason::Unreadable,
        Reason::Internal,
    ];

    /// The events of a project, first the one that names its new activity.
    pub const EVENTS_BY_PRIORITY: [Reason; 6] =
        [Reason::Answers, Reason::Reports, Reason::Leases, Reason::SilentWorkers, Reason::Ready, Reason::Inbox];

    /// The reasons to stay idle, first the one that names an idle tick.
    pub const IDLE_BY_PRIORITY: [Reason; 7] = [
        Reason::Budget,
        Reason::Busy,
        Reason::WipLimit,
        Reason::ProposalsExhausted,
        Reason::WorkInProgress,
        Reason::WaitingOnOwner,
        Reason::NoEvent,
    ];

    #[must_use]
    pub fn outcome(self) -> Outcome {
        match self {
            Reason::Ready
            | Reason::Reports
            | Reason::Answers
            | Reason::Inbox
            | Reason::Leases
            | Reason::Backlog
            | Reason::SilentWorkers => Outcome::NewActivity,
            Reason::NoEvent
            | Reason::Busy
            | Reason::WipLimit
            | Reason::Budget
            | Reason::ProposalsExhausted
            | Reason::WaitingOnOwner
            | Reason::WorkInProgress => Outcome::Idle,
            Reason::ObjectiveDone | Reason::Stall => Outcome::TerminalState,
            Reason::Tracker | Reason::Runtime | Reason::Unreadable | Reason::Internal => Outcome::Error,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        let outcomes = &format().outcomes;
        let (new, idle, terminal, error) =
            (&outcomes.new_activity.reasons, &outcomes.idle.reasons, &outcomes.terminal_state.reasons, &outcomes.error.reasons);
        match self {
            Reason::Ready => &new.ready,
            Reason::Reports => &new.reports,
            Reason::Answers => &new.answers,
            Reason::Inbox => &new.inbox,
            Reason::Leases => &new.leases,
            Reason::Backlog => &new.backlog,
            Reason::SilentWorkers => &new.silent_workers,
            Reason::NoEvent => &idle.no_event,
            Reason::Busy => &idle.busy,
            Reason::WipLimit => &idle.wip_limit,
            Reason::Budget => &idle.budget,
            Reason::ProposalsExhausted => &idle.proposals_exhausted,
            Reason::WaitingOnOwner => &idle.waiting_on_owner,
            Reason::WorkInProgress => &idle.work_in_progress,
            Reason::ObjectiveDone => &terminal.objective_done,
            Reason::Stall => &terminal.stall,
            Reason::Tracker => &error.tracker,
            Reason::Runtime => &error.runtime,
            Reason::Unreadable => &error.unreadable,
            Reason::Internal => &error.internal,
        }
    }
}

/// A JSON value whose objects keep the order their keys were written in, as Python's `json.dumps` keeps a
/// dict's: the line is the prototype's byte for byte, not only as a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    /// A float, written as Python's `repr` writes it: `10.0`, `74.9`, `1e-05`.
    Float(f64),
    Str(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Compact, with every character outside printable ASCII escaped, as `json.dumps(…,
    /// ensure_ascii=True, separators=(",", ":"))` writes it.
    #[must_use]
    pub fn to_ascii(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Int(value) => out.push_str(&value.to_string()),
            Json::Float(value) => out.push_str(&python_repr(*value)),
            Json::Str(text) => write_str(text, out),
            Json::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(fields) => {
                out.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

impl Json {
    /// The value of `key` in an object, the first when it repeats.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    /// A whole number, as Python's `int` reads one of a JSON line: a float is not one.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_list(&self) -> Option<&[Json]> {
        match self {
            Json::List(items) => Some(items),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_object(&self) -> bool {
        matches!(self, Json::Object(_))
    }

    /// The same value as a `serde_json::Value`: the order of the keys goes, a float stays a float.
    #[must_use]
    pub fn to_value(&self) -> serde_json::Value {
        use serde_json::Value;
        match self {
            Json::Null => Value::Null,
            Json::Bool(value) => Value::Bool(*value),
            Json::Int(value) => Value::from(*value),
            Json::Float(value) => serde_json::Number::from_f64(*value).map_or(Value::Null, Value::Number),
            Json::Str(text) => Value::String(text.clone()),
            Json::List(items) => Value::Array(items.iter().map(Json::to_value).collect()),
            Json::Object(fields) => Value::Object(fields.iter().map(|(key, value)| (key.clone(), value.to_value())).collect()),
        }
    }
}

/// A JSON text read with its objects in the order it wrote their keys, as Python's `json.loads` keeps a
/// dict's: what the controller attaches to a prompt is the precheck's payload byte for byte.
impl<'de> serde::Deserialize<'de> for Json {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(OrderedVisitor)
    }
}

struct OrderedVisitor;

impl<'de> serde::de::Visitor<'de> for OrderedVisitor {
    type Value = Json;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_none<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Json, E> {
        Ok(Json::Int(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Json, E> {
        Ok(i64::try_from(value).map_or(Json::Float(value as f64), Json::Int))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Json, E> {
        Ok(Json::Float(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Json, E> {
        Ok(Json::Str(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Json, E> {
        Ok(Json::Str(value))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<Json>()? {
            items.push(item);
        }
        Ok(Json::List(items))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut fields = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Json>()? {
            fields.push((key, value));
        }
        Ok(Json::Object(fields))
    }
}

/// The shortest digits that read back as `value`, and the place of the decimal point after the first of
/// them: `74.9` is `("749", 2)`, `0.001` is `("1", -2)`.
fn shortest_digits(value: f64) -> (String, i32) {
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0".to_owned() } else { digits.to_owned() };
    (digits, exponent.parse::<i32>().unwrap_or(0) + 1)
}

/// A float as Python's `repr` writes it, which is what its `json` writes: the shortest digits that read
/// back, fixed from 1e-4 up to 1e16 with at least one decimal, else in exponent form with a sign and at
/// least two digits of exponent. Python's `json` writes `NaN` and `Infinity` for the values outside.
#[must_use]
pub fn python_repr(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity".to_owned() } else { "-Infinity".to_owned() };
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0.0");
    }
    let (digits, point) = shortest_digits(value);
    let count = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    if -4 < point && point <= 16 {
        let body = if point <= 0 {
            format!("0.{}{digits}", "0".repeat(usize::try_from(-point).unwrap_or(0)))
        } else if point >= count {
            format!("{digits}{}.0", "0".repeat(usize::try_from(point - count).unwrap_or(0)))
        } else {
            let (whole, fraction) = digits.split_at(usize::try_from(point).unwrap_or(0));
            format!("{whole}.{fraction}")
        };
        return format!("{sign}{body}");
    }
    let (first, rest) = digits.split_at(1);
    let mantissa = if rest.is_empty() { first.to_owned() } else { format!("{first}.{rest}") };
    let exponent = point - 1;
    format!("{sign}{mantissa}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
}

/// A string as Python's `json` writes it in ASCII: the five short escapes, and `\uXXXX` in lower case for
/// every other character outside the space-to-tilde range, a pair of surrogates past U+FFFF.
fn write_str(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// The line of a tick's verdict and its exit code. The line names the version, the outcome its reason
/// belongs to, the exit code, the reason, the projects' cursors, the payload and `t`, in that order. When it
/// would be longer than the format allows, its payload keeps only the number of projects, and the cursors
/// go: the controller reads the outcome from the end of a bounded output.
#[must_use]
pub fn outcome_line(reason: Reason, cursor: &Json, payload: &Json, t: &str) -> (u8, String) {
    let words = &format().line;
    let outcome = reason.outcome();
    let build = |cursor: &Json, payload: &Json| {
        Json::Object(vec![
            (words.keys.version.clone(), Json::Int(i64::from(words.version))),
            (words.keys.outcome.clone(), Json::Str(outcome.word().to_owned())),
            (words.keys.exit.clone(), Json::Int(i64::from(outcome.exit()))),
            (words.keys.reason.clone(), Json::Str(reason.word().to_owned())),
            (words.keys.cursor.clone(), cursor.clone()),
            (words.keys.payload.clone(), payload.clone()),
            (words.keys.t.clone(), Json::Str(t.to_owned())),
        ])
        .to_ascii()
    };
    let line = build(cursor, payload);
    if line.len() <= words.max_length {
        return (outcome.exit(), line);
    }
    let payload_words = &format().payload;
    let projects = match payload {
        Json::Object(fields) => fields.iter().find(|(key, _)| *key == payload_words.projects).map_or(0, |(_, value)| match value {
            Json::List(items) => items.len(),
            _ => 0,
        }),
        _ => 0,
    };
    let reduced = Json::Object(vec![
        (payload_words.truncated.clone(), Json::Bool(true)),
        (payload_words.projects.clone(), Json::Int(i64::try_from(projects).unwrap_or(i64::MAX))),
    ]);
    (outcome.exit(), build(&Json::Null, &reduced))
}

/// The payload of an error: its detail, cut to the format's length in characters.
#[must_use]
pub fn error_payload(detail: &str) -> Json {
    let cut: String = detail.chars().take(format().line.detail_length).collect();
    Json::Object(vec![(format().payload.detail.clone(), Json::Str(cut))])
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "2026-01-10T09:00:00Z";

    fn parsed(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("the line is JSON")
    }

    #[test]
    fn every_reason_gives_its_outcome_s_exit_and_words() {
        let mut seen = std::collections::BTreeSet::new();
        for reason in Reason::ALL {
            let (exit, line) = outcome_line(reason, &Json::List(vec![]), &Json::Object(vec![]), T);
            let value = parsed(&line);
            let keys = &format().line.keys;
            assert_eq!(exit, reason.outcome().exit(), "{reason:?}");
            assert_eq!(value[&keys.outcome], reason.outcome().word(), "{reason:?}");
            assert_eq!(value[&keys.reason], reason.word(), "{reason:?}");
            assert_eq!(value[&keys.exit], i64::from(exit), "{reason:?}");
            assert_eq!(value[&keys.version], 1);
            assert_eq!(value[&keys.t], T);
            seen.insert(reason.outcome().word());
        }
        assert_eq!(seen.len(), Outcome::ALL.len(), "every outcome has a reason");
        let exits: Vec<u8> = Outcome::ALL.iter().map(|outcome| outcome.exit()).collect();
        assert_eq!(exits, [0, 0, 3, 1], "new activity, terminal state, idle, error");
    }

    #[test]
    fn the_words_are_distinct_within_an_outcome() {
        for outcome in Outcome::ALL {
            let words: Vec<&str> = Reason::ALL.iter().filter(|reason| reason.outcome() == outcome).map(|reason| reason.word()).collect();
            let unique: std::collections::BTreeSet<&&str> = words.iter().collect();
            assert_eq!(unique.len(), words.len(), "{outcome:?}");
        }
    }

    #[test]
    fn the_line_is_compact_ascii_in_the_prototype_s_key_order() {
        let payload = error_payload("perché è così èè 中 \u{1F600} \u{7f}\"\\\n");
        let (exit, line) = outcome_line(Reason::Tracker, &Json::Null, &payload, T);
        assert_eq!(exit, 1);
        assert!(line.is_ascii() && !line.contains('\n') && !line.contains(", ") && !line.contains(": "), "{line}");
        let keys = &format().line.keys;
        let order: Vec<&str> = [&keys.version, &keys.outcome, &keys.exit, &keys.reason, &keys.cursor, &keys.payload, &keys.t]
            .iter()
            .map(|key| key.as_str())
            .collect();
        let positions: Vec<usize> = order.iter().map(|key| line.find(&format!("\"{key}\":")).expect("every key")).collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{line}");
        // Lower-case hex, a pair of surrogates past U+FFFF, DEL escaped too, then the short escapes.
        let escaped: String = ["00e9", "00e8", "00ec", "00e8", "00e8", "4e2d", "d83d", "de00", "007f"].iter().map(|unit| format!("\\u{unit}")).collect();
        let found: String = line.match_indices("\\u").map(|(at, _)| &line[at..at + 6]).collect();
        assert_eq!(found, escaped, "{line}");
        assert!(line.ends_with(&format!("{}\"{}{}{}n\"}},\"{}\":\"{T}\"}}", '\\', '\\', '\\', '\\', keys.t)), "{line}");
        assert_eq!(parsed(&line)[&keys.payload][&format().payload.detail], "perché è così èè 中 \u{1F600} \u{7f}\"\\\n");
    }

    #[test]
    fn a_line_too_long_keeps_only_the_count_of_projects() {
        let projects: Vec<Json> = (0..200).map(|i| Json::Object(vec![("id".to_owned(), Json::Str(format!("p{i}{}", "x".repeat(80))))])).collect();
        let payload = Json::Object(vec![(format().payload.projects.clone(), Json::List(projects))]);
        let cursor = Json::List(vec![Json::Null]);
        let (exit, line) = outcome_line(Reason::Ready, &cursor, &payload, T);
        assert_eq!(exit, 0);
        assert!(line.len() <= format().line.max_length);
        let value = parsed(&line);
        let keys = &format().line.keys;
        assert_eq!(value[&keys.cursor], serde_json::Value::Null, "the cursors go with the projects");
        assert_eq!(value[&keys.payload], serde_json::json!({format().payload.truncated.as_str(): true, format().payload.projects.as_str(): 200}));
        assert_eq!(value[&keys.outcome], Outcome::NewActivity.word());
    }

    /// `repr` of Python 3.12 on the floats a line carries: shares, and the forms at the edges.
    #[test]
    fn a_float_is_written_as_python_writes_it() {
        for (value, python) in [
            (10.0, "10.0"),
            (74.9, "74.9"),
            (0.1, "0.1"),
            (100.0, "100.0"),
            (-2.5, "-2.5"),
            (0.0, "0.0"),
            (0.0001, "0.0001"),
            (0.000_01, "1e-05"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (123_456_789.125, "123456789.125"),
            (1.5e-7, "1.5e-07"),
            (f64::NAN, "NaN"),
        ] {
            assert_eq!(python_repr(value), python, "{value}");
        }
    }

    #[test]
    fn the_detail_of_an_error_is_cut_in_characters() {
        let payload = error_payload(&"é".repeat(400));
        let Json::Object(fields) = payload else { panic!("an object") };
        let Json::Str(detail) = &fields[0].1 else { panic!("a string") };
        assert_eq!(detail.chars().count(), format().line.detail_length);
    }
}
