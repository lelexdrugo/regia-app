//! The registry of a project against the prototype's `registro` and `registro_con` (R1–R3 of the dossier's
//! conformance study, §9.3): vectors the prototype gave, and properties on generated blocks. The prototype
//! reads the block with Python's rules, so the generated text mixes Python's whitespace and line breaks,
//! U+001C–U+001F among them, where Rust's own rules differ.

use proptest::prelude::*;
use regia_core::{Registry, registry_with};

/// What `registro({"body": body}).items()` gave for each body, on the prototype at `9cda78f`.
const VECTORS: [(&str, &[(&str, &str)]); 11] = [
    ("<!--regia:progetto\nid: a\n-->", &[("id", "a")]),
    ("<!--  regia:progetto  id: inline -->", &[("id", "inline")]),
    ("<!-- regia:progetto\r\nid: a\r\ncartella: b\r\n-->", &[("id", "a"), ("cartella", "b")]),
    ("<!-- regia:progetto\u{1c}id: x\u{1d}wip: 2\u{1f} -->", &[("id", "x"), ("wip", "2")]),
    ("<!-- regia:progetto\nid: a\u{2028}wip: 3\u{85}prodotti: x/y\n-->", &[("id", "a"), ("wip", "3"), ("prodotti", "x/y")]),
    ("<!-- regia:progettoX\nid: a\n-->", &[("id", "a")]),
    ("<!-- regia:progetto\n\u{a0}id\u{a0}:\u{a0}v\u{a0}\n: empty key\nno colon\n-->", &[("id", "v"), ("", "empty key")]),
    ("<!-- other -->\n<!-- regia:progetto\nid: z\n-->\n<!-- regia:progetto\nid: second\n-->", &[("id", "z")]),
    ("<!-- regia:progetto\nid: a\nid: b\ncartella: c\nid: d\n-->", &[("id", "d"), ("cartella", "c")]),
    ("<!-- regia:progetto\nid: never closed\n", &[]),
    ("<!-- regia:progetto\nnota: a --> b\n-->", &[("nota", "a")]),
];

/// What `registro_con(body, {key: value})` gave, on the prototype at `9cda78f`.
const WRITES: [(&str, (&str, &str), &str); 4] = [
    ("x\n<!-- regia:progetto\nid: a\nwip: 2\n\n-->\ny", ("wip", "3"), "x\n<!-- regia:progetto\nid: a\nwip: 3\n\n-->\ny"),
    ("x\n<!-- regia:progetto\nid: a\nwip: 2\n\n-->\ny", ("nuova", "v"), "x\n<!-- regia:progetto\nid: a\nwip: 2\nnuova: v\n\n-->\ny"),
    ("<!-- regia:progetto-->", ("k", "v"), "<!-- regia:progettok: v\n-->"),
    ("<!-- regia:progetto\nwip: 1\nwip: 2\n-->", ("wip", "9"), "<!-- regia:progetto\nwip: 9\nwip: 2\n-->"),
];

fn entries(registry: &Registry) -> Vec<(String, String)> {
    registry.entries().to_vec()
}

fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

#[test]
fn the_registry_reads_as_the_prototype_s() {
    for (body, expected) in VECTORS {
        assert_eq!(entries(&Registry::parse(body)), owned(expected), "{body:?}");
    }
    for (body, (key, value), expected) in WRITES {
        assert_eq!(registry_with(body, &[(key, value)]).as_deref(), Some(expected), "{body:?}");
    }
    assert_eq!(registry_with("no block here", &[("k", "v")]), None);
}

/// R3: with a key written twice, the write lands on the first and the read gives the last, so the value
/// written does not read back. `project set` refuses that case for this reason.
#[test]
fn a_key_written_twice_does_not_read_back_what_was_written() {
    let (body, (key, value), _) = WRITES[3];
    let written = registry_with(body, &[(key, value)]).unwrap();
    assert_eq!(Registry::parse(&written).get(key), Some("2"));
}

/// Whitespace of Python that is not a line break, inside a line.
const BLANKS: [char; 6] = [' ', '\t', '\u{1f}', '\u{a0}', '\u{2000}', '\u{3000}'];
/// Line breaks of Python's `splitlines`, `\r\n` as one.
const BREAKS: [&str; 9] = ["\n", "\r\n", "\r", "\u{0b}", "\u{0c}", "\u{1c}", "\u{1e}", "\u{85}", "\u{2028}"];

fn blank() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(BLANKS.to_vec()), 0..3).prop_map(|chars| chars.into_iter().collect())
}

/// A key: no colon, no blank at its ends, never a line break.
fn key() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_é-]{0,6}"
}

/// A value: colons and inner spaces allowed, never a line break nor the end of the block.
fn value() -> impl Strategy<Value = String> {
    "[a-z0-9/:.é-]([a-z0-9/:. é-]{0,10}[a-z0-9/:.é-])?"
}

#[derive(Debug, Clone)]
enum Line {
    Entry { key: String, value: String, before: String, around: String, after: String },
    Noise(String),
}

fn line() -> impl Strategy<Value = Line> {
    prop_oneof![
        4 => (key(), value(), blank(), blank(), blank())
            .prop_map(|(key, value, before, around, after)| Line::Entry { key, value, before, around, after }),
        1 => "[a-z ]{0,8}".prop_map(Line::Noise),
    ]
}

/// A body with a block of `lines`, joined by `breaks`, between text that is not a registry.
fn body(lines: &[Line], breaks: &[&str]) -> String {
    let mut block = String::from("<!-- regia:progetto\n");
    for (i, line) in lines.iter().enumerate() {
        match line {
            Line::Entry { key, value, before, around, after } => block.push_str(&format!("{before}{key}{around}:{around}{value}{after}")),
            Line::Noise(text) => block.push_str(text),
        }
        block.push_str(breaks[i % breaks.len()]);
    }
    format!("intro: not a registry\n{block}-->\n\n## after\nkey: not in the block\n")
}

/// The registry the lines mean: each key where it is first written, with the value written last.
fn meaning(lines: &[Line]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in lines {
        if let Line::Entry { key, value, .. } = line {
            match out.iter_mut().find(|(known, _)| known == key) {
                Some(entry) => entry.1.clone_from(value),
                None => out.push((key.clone(), value.clone())),
            }
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// R1: the first block, each line split at its first colon, both halves stripped of Python's
    /// whitespace, a line with no colon ignored, the last value of a key in the place of the first.
    #[test]
    fn a_block_reads_as_its_lines_mean(lines in prop::collection::vec(line(), 0..8), breaks in prop::collection::vec(prop::sample::select(BREAKS.to_vec()), 1..4)) {
        prop_assert_eq!(entries(&Registry::parse(&body(&lines, &breaks))), meaning(&lines));
    }

    /// R3: `registro(registro_con(body, {k: v}))[k] == v` for a body with no key written twice, and the rest
    /// of the registry and of the body stays as it was.
    #[test]
    fn a_key_written_reads_back(lines in prop::collection::vec(line(), 0..6), target in key(), written in value()) {
        let mut seen = std::collections::BTreeSet::new();
        let unique: Vec<Line> = lines.into_iter().filter(|line| match line {
            Line::Entry { key, .. } => seen.insert(key.clone()),
            Line::Noise(_) => true,
        }).collect();
        let before = body(&unique, &["\n"]);
        let after = registry_with(&before, &[(&target, &written)]).expect("the body has a block");
        let registry = Registry::parse(&after);
        prop_assert_eq!(registry.get(&target), Some(written.as_str()));
        let mut expected = meaning(&unique);
        match expected.iter_mut().find(|(key, _)| *key == target) {
            Some(entry) => entry.1.clone_from(&written),
            None => expected.push((target.clone(), written.clone())),
        }
        prop_assert_eq!(entries(&registry), expected);
        let (head, tail) = (before.split("<!--").next().unwrap(), before.rsplit("-->").next().unwrap());
        prop_assert!(after.starts_with(head) && after.ends_with(tail), "the text around the block changed");
    }
}
