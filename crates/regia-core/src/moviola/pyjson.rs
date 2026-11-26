//! JSON as the prototype's `json` module reads and writes it, for the moviola's files that both implementations
//! write and read back: the raw store's records, the aggregates, and the store's record of what was consolidated.
//! An object keeps its keys in the order they were first read or set, as a Python `dict` does, and a key set again
//! keeps its place. `serde_json`'s own map sorts its keys: a file written back through it would change its bytes,
//! and the store's fingerprints with them. Equality, truth and `str()` are Python's, where the prototype's code
//! depends on them.
//!
//! Reading goes through `serde_json`'s parser into these values. Its limits, on values no writer of the regia
//! produces: `NaN`, `Infinity`, a number past the floats' range (`1e400`), a lone surrogate (`"\ud800"`) and nesting
//! deeper than 128 do not read, where Python reads them; `-0` reads as the float `-0.0` and an integer past 64 bits
//! as a float, where Python keeps an integer.

use std::collections::HashMap;
use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::checks::decimal_digit;
use crate::pystr;

/// A value as `json.loads` gives it.
#[derive(Debug, Clone)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    List(Vec<Json>),
    Object(Object),
}

static NULL: Json = Json::Null;

/// A JSON object as a Python `dict`: its keys in the order they were first set.
#[derive(Debug, Clone, Default)]
pub struct Object(Vec<(String, Json)>);

impl Object {
    pub fn new() -> Object {
        Object::default()
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    /// `dict.get(key)`: an absent key reads as `None`.
    pub fn value(&self, key: &str) -> &Json {
        self.get(key).unwrap_or(&NULL)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `dict[key] = value`: a key already there keeps its place.
    pub fn insert(&mut self, key: impl Into<String>, value: Json) {
        let key = key.into();
        match self.0.iter_mut().find(|(name, _)| *name == key) {
            Some((_, slot)) => *slot = value,
            None => self.0.push((key, value)),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Json)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Python's `==` of two `dict`s: the same keys with equal values, whatever their order.
    pub fn py_eq(&self, other: &Object) -> bool {
        self.len() == other.len() && self.iter().all(|(key, value)| other.get(key).is_some_and(|theirs| value.py_eq(theirs)))
    }
}

impl FromIterator<(String, Json)> for Object {
    fn from_iter<I: IntoIterator<Item = (String, Json)>>(items: I) -> Object {
        let mut object = Object::new();
        for (key, value) in items {
            object.insert(key, value);
        }
        object
    }
}

/// An ordered map with string keys, as a Python `dict`: insertion order, a key set again keeps its place, and a
/// key removed leaves the order of the others as it was. Indexed, for the store's thousands of records.
#[derive(Debug, Clone)]
pub struct Dict<V> {
    entries: Vec<Option<(String, V)>>,
    index: HashMap<String, usize>,
}

impl<V> Default for Dict<V> {
    fn default() -> Dict<V> {
        Dict { entries: Vec::new(), index: HashMap::new() }
    }
}

impl<V> Dict<V> {
    pub fn new() -> Dict<V> {
        Dict::default()
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).and_then(|at| self.entries[*at].as_ref()).map(|(_, value)| value)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    pub fn insert(&mut self, key: String, value: V) {
        match self.index.get(&key) {
            Some(at) => self.entries[*at] = Some((key, value)),
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push(Some((key, value)));
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<V> {
        let at = self.index.remove(key)?;
        self.entries[at].take().map(|(_, value)| value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().flatten().map(|(key, value)| (key.as_str(), value))
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

impl Json {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Json::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Json]> {
        match self {
            Json::List(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// Python's truth: `None`, `False`, zero, and an empty string, list or object are false.
    pub fn truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(value) => *value,
            Json::Int(value) => *value != 0,
            Json::Float(value) => *value != 0.0,
            Json::Str(text) => !text.is_empty(),
            Json::List(items) => !items.is_empty(),
            Json::Object(object) => !object.is_empty(),
        }
    }

    /// Python's `str()`: `None`, `True`, `12`, `1.5`, a string as it is, a list or an object as its `repr`. The `repr`
    /// of a string escapes what Python escapes among the ASCII characters and the controls, and keeps the rest:
    /// Python also escapes the other unprintable characters, which no field of the regia holds.
    pub fn py_str(&self) -> String {
        match self {
            Json::Str(text) => text.clone(),
            other => repr(other),
        }
    }

    /// The value of a number, `True` as 1, for Python's numeric equality.
    fn number(&self) -> Option<Number> {
        match self {
            Json::Bool(value) => Some(Number::Int(i128::from(*value))),
            Json::Int(value) => Some(Number::Int(*value)),
            Json::Float(value) => Some(Number::Float(*value)),
            _ => None,
        }
    }

    /// Python's `==`: `1 == 1.0 == True`, an object equal to another whatever the order of its keys, `nan`
    /// different from itself.
    pub fn py_eq(&self, other: &Json) -> bool {
        if let (Some(a), Some(b)) = (self.number(), other.number()) {
            return match (a, b) {
                (Number::Int(a), Number::Int(b)) => a == b,
                (Number::Float(a), Number::Float(b)) => a == b,
                (Number::Int(a), Number::Float(b)) | (Number::Float(b), Number::Int(a)) => int_eq_float(a, b),
            };
        }
        match (self, other) {
            (Json::Null, Json::Null) => true,
            (Json::Str(a), Json::Str(b)) => a == b,
            (Json::List(a), Json::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.py_eq(b)),
            (Json::Object(a), Json::Object(b)) => a.py_eq(b),
            _ => false,
        }
    }
}

#[derive(Clone, Copy)]
enum Number {
    Int(i128),
    Float(f64),
}

/// An integer and a float are equal when the float is that whole number exactly.
fn int_eq_float(int: i128, float: f64) -> bool {
    // Every f64 inside this range converts to i128 exactly; past it no i128 is equal.
    float.is_finite() && float.fract() == 0.0 && float.abs() < 1.7e38 && float as i128 == int
}

/// Python's `repr()` of a value.
fn repr(value: &Json) -> String {
    match value {
        Json::Null => "None".to_owned(),
        Json::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
        Json::Int(value) => value.to_string(),
        Json::Float(value) if value.is_nan() => "nan".to_owned(),
        Json::Float(value) if value.is_infinite() => if *value > 0.0 { "inf" } else { "-inf" }.to_owned(),
        Json::Float(value) => float_repr(*value),
        Json::Str(text) => repr_str(text),
        Json::List(items) => format!("[{}]", items.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Json::Object(object) => {
            format!("{{{}}}", object.iter().map(|(key, item)| format!("{}: {}", repr_str(key), repr(item))).collect::<Vec<_>>().join(", "))
        }
    }
}

/// A string's `repr`: single quotes, or double ones when it has a single quote and no double one.
fn repr_str(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() && u32::from(c) < 0x100 => out.push_str(&format!("\\x{:02x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

// ---------- reading ----------

/// Why a text is not JSON.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct JsonError(String);

/// `json.loads(text)`, through `serde_json`'s parser (see the module's documentation for where the two differ). A
/// key set twice keeps its first place and its last value.
pub fn loads(text: &str) -> Result<Json, JsonError> {
    serde_json::from_str(text).map_err(|error| JsonError(error.to_string()))
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Json, E> {
        Ok(Json::Int(i128::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Json, E> {
        Ok(Json::Int(i128::from(value)))
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

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_none<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Json, D::Error> {
        Json::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut object = Object::new();
        while let Some((key, value)) = map.next_entry::<String, Json>()? {
            object.insert(key, value);
        }
        Ok(Json::Object(object))
    }
}

// ---------- writing ----------

/// `json.dumps(value, ensure_ascii=False, separators=(",", ":"))`: compact, every character as it is but the
/// quote, the backslash and the controls.
pub fn dumps(value: &Json) -> String {
    let mut out = String::new();
    write_compact(value, &mut out);
    out
}

/// [`dumps`] of an object.
pub fn dumps_object(object: &Object) -> String {
    let mut out = String::new();
    write_object(object, &mut out);
    out
}

fn write_object(object: &Object, out: &mut String) {
    out.push('{');
    for (n, (key, item)) in object.iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        write_str(key, false, out);
        out.push(':');
        write_compact(item, out);
    }
    out.push('}');
}

fn write_compact(value: &Json, out: &mut String) {
    match value {
        Json::List(items) => {
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        Json::Object(object) => write_object(object, out),
        scalar => write_scalar(scalar, false, out),
    }
}

/// `json.dumps(value, indent=1, sort_keys=True)`: a key and its value on a line, one space deeper at each level,
/// the keys in order, every character outside printable ASCII escaped.
pub fn dumps_sorted(value: &Json) -> String {
    let mut out = String::new();
    write_sorted(value, 0, &mut out);
    out
}

fn write_sorted(value: &Json, level: usize, out: &mut String) {
    let indent = |level: usize, out: &mut String| {
        out.push('\n');
        out.push_str(&" ".repeat(level));
    };
    match value {
        Json::List(items) if items.is_empty() => out.push_str("[]"),
        Json::Object(object) if object.is_empty() => out.push_str("{}"),
        Json::List(items) => {
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                indent(level + 1, out);
                write_sorted(item, level + 1, out);
            }
            indent(level, out);
            out.push(']');
        }
        Json::Object(object) => {
            let mut entries: Vec<(&str, &Json)> = object.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (n, (key, item)) in entries.into_iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                indent(level + 1, out);
                write_str(key, true, out);
                out.push_str(": ");
                write_sorted(item, level + 1, out);
            }
            indent(level, out);
            out.push('}');
        }
        scalar => write_scalar(scalar, true, out),
    }
}

fn write_scalar(value: &Json, ascii: bool, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Json::Int(value) => out.push_str(&value.to_string()),
        Json::Float(value) if value.is_nan() => out.push_str("NaN"),
        Json::Float(value) if value.is_infinite() => out.push_str(if *value > 0.0 { "Infinity" } else { "-Infinity" }),
        Json::Float(value) => out.push_str(&float_repr(*value)),
        Json::Str(text) => write_str(text, ascii, out),
        Json::List(_) | Json::Object(_) => write_compact(value, out),
    }
}

/// A string as Python's `json` writes it: the five short escapes, `\u00XX` for the other controls, and with
/// `ascii` every character outside space to tilde as `\uXXXX`, a pair of surrogates past U+FFFF. Hex in lower case.
fn write_str(text: &str, ascii: bool, out: &mut String) {
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
            c if u32::from(c) < 0x20 || ascii => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The shortest digits that read back, and where the decimal point goes after the first of them.
fn shortest_digits(value: f64) -> (String, i32) {
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0".to_owned() } else { digits.to_owned() };
    (digits, exponent.parse::<i32>().unwrap_or(0) + 1)
}

/// A finite float as Python's `repr` writes it: fixed from 1e-4 up to 1e16 with at least one decimal, else in
/// exponent form with a sign and at least two digits of exponent.
fn float_repr(value: f64) -> String {
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0.0");
    }
    let (digits, point) = shortest_digits(value);
    let count = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let zeros = |n: i32| "0".repeat(usize::try_from(n).unwrap_or(0));
    if -4 < point && point <= 16 {
        let body = if point <= 0 {
            format!("0.{}{digits}", zeros(-point))
        } else if point >= count {
            format!("{digits}{}.0", zeros(point - count))
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

// ---------- Python's int() ----------

/// What Python's `int(value)` gives: a number, nothing (`TypeError` or `ValueError`), or the `OverflowError` of an
/// infinite float.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntOf {
    Value(i128),
    Not,
    Overflow,
}

/// `int(value)`: an integer as it is, a boolean as 0 or 1, a float cut toward zero, a string as [`int_of_text`]
/// reads it. A value past `i128` is a limit of this reader: nothing.
pub fn int_of(value: &Json) -> IntOf {
    match value {
        Json::Int(value) => IntOf::Value(*value),
        Json::Bool(value) => IntOf::Value(i128::from(*value)),
        Json::Float(value) if value.is_infinite() => IntOf::Overflow,
        Json::Float(value) if value.is_nan() => IntOf::Not,
        Json::Float(value) => {
            let cut = value.trunc();
            if cut.abs() < 1.7e38 { IntOf::Value(cut as i128) } else { IntOf::Not }
        }
        Json::Str(text) => int_of_text(text).map_or(IntOf::Not, IntOf::Value),
        Json::Null | Json::List(_) | Json::Object(_) => IntOf::Not,
    }
}

/// `int(text)`: Python's whitespace around it, a sign, Unicode's decimal digits with single underscores between
/// them, nothing else.
pub fn int_of_text(text: &str) -> Option<i128> {
    let text = pystr::strip(text);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return None;
    }
    let mut value: i128 = 0;
    for c in digits.chars().filter(|c| *c != '_') {
        let digit = i128::from(decimal_digit(c)?);
        value = value.checked_mul(10)?.checked_add(digit)?;
    }
    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> String {
        dumps(&loads(text).unwrap())
    }

    /// The bytes Python writes back for what it read: order kept, spaces dropped, escapes undone where it can.
    #[test]
    fn a_line_reads_and_writes_back_as_python_does() {
        assert_eq!(round_trip(r#"{"b": 1, "a": [true, null, 2.50, 0, 1e3]}"#), r#"{"b":1,"a":[true,null,2.5,0,1000.0]}"#);
        assert_eq!(round_trip(r#"{"a":1,"b":2,"a":3}"#), r#"{"a":3,"b":2}"#, "a key set twice keeps its first place");
        assert_eq!(round_trip(r#""\u00e8\ud83d\ude00\/\u0001\u007f\"\\\n\t\b\f\r""#), "\"è😀/\\u0001\u{7f}\\\"\\\\\\n\\t\\b\\f\\r\"");
        assert_eq!(round_trip("[1E300, -1e-400]"), "[1e+300,-0.0]");
        assert_eq!(round_trip("18446744073709551615"), "18446744073709551615");
        assert_eq!(round_trip("-9223372036854775808"), "-9223372036854775808");
        assert_eq!(round_trip(" \t[]\r\n"), "[]");
        assert_eq!(round_trip("\"\u{2028}\""), "\"\u{2028}\"", "ensure_ascii=False writes it as it is");
    }

    #[test]
    fn what_python_refuses_is_refused() {
        for text in ["", "{", "[1,]", "{\"a\":1,}", "01", "1.", "-", "\"a\u{1}\"", "\u{feff}{}", "{} x", "{'a':1}", "nul", "\"\\x\"", "[1] \u{a0}", "1e"] {
            assert!(loads(text).is_err(), "{text:?}");
        }
        // The limits of the module's documentation, which Python reads.
        for text in ["NaN", "[Infinity]", "1e400", "\"\\ud800\""] {
            assert!(loads(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn floats_are_written_as_python_s_repr() {
        let cases = [
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1.5e-7, "1.5e-07"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-2.5, "-2.5"),
            (123456789.125, "123456789.125"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
        ];
        for (value, text) in cases {
            assert_eq!(float_repr(value), text);
        }
        assert_eq!(dumps(&Json::Float(-0.0)), "-0.0");
        assert_eq!(dumps(&Json::Float(f64::NAN)), "NaN");
    }

    #[test]
    fn the_sorted_form_indents_by_one_and_escapes_outside_ascii() {
        let value = loads(r#"{"z": {"b": "è😀\u007f", "a": []}, "a": {}}"#).unwrap();
        assert_eq!(dumps_sorted(&value), "{\n \"a\": {},\n \"z\": {\n  \"a\": [],\n  \"b\": \"\\u00e8\\ud83d\\ude00\\u007f\"\n }\n}");
        assert_eq!(dumps_sorted(&loads("[1, [2]]").unwrap()), "[\n 1,\n [\n  2\n ]\n]");
    }

    #[test]
    fn equality_truth_and_str_are_python_s() {
        let a = loads(r#"{"x": 1, "y": [true]}"#).unwrap();
        let b = loads(r#"{"y": [1.0], "x": true}"#).unwrap();
        assert!(a.py_eq(&b));
        assert!(!Json::Float(f64::NAN).py_eq(&Json::Float(f64::NAN)));
        assert!(!Json::Str("1".to_owned()).py_eq(&Json::Int(1)));
        assert!(!Json::Str(String::new()).truthy() && Json::Float(f64::NAN).truthy() && !Json::Int(0).truthy());
        assert_eq!(Json::Null.py_str(), "None");
        assert_eq!(Json::Bool(true).py_str(), "True");
        assert_eq!(Json::Float(f64::INFINITY).py_str(), "inf");
        assert_eq!(Json::Float(3.0).py_str(), "3.0");
        assert_eq!(loads(r#"["a", "it's", 1, null, {"k": "\"q\""}]"#).unwrap().py_str(), r#"['a', "it's", 1, None, {'k': '"q"'}]"#);
    }

    #[test]
    fn int_reads_as_python_s() {
        let cases = [
            (" 7 ", Some(7)),
            ("+5", Some(5)),
            ("-0", Some(0)),
            ("1_000", Some(1000)),
            ("\u{663}\u{661}", Some(31)),
            ("1__0", None),
            ("_1", None),
            ("1_", None),
            ("1.0", None),
            ("- 1", None),
            ("", None),
            ("0x10", None),
        ];
        for (text, value) in cases {
            assert_eq!(int_of_text(text), value, "{text:?}");
        }
        assert_eq!(int_of(&Json::Float(-2.9)), IntOf::Value(-2));
        assert_eq!(int_of(&Json::Float(f64::INFINITY)), IntOf::Overflow);
        assert_eq!(int_of(&Json::Bool(true)), IntOf::Value(1));
        assert_eq!(int_of(&Json::Null), IntOf::Not);
    }
}
