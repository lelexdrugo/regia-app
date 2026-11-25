//! #452, Acceptance 2 and the properties of R3–R5 (`conformita.md` §1.3 of the dossier), on generated inputs.
//!
//! - **R3**: on every valid body, a key set with `registry_with` reads back with `Registry::parse` as the value set,
//!   the body outside the block is the same byte for byte, and every other key reads as it did. A valid body has a
//!   registry and the key at most once; its lines end in LF or CRLF, as the site writes them. The control: with the
//!   key twice the property fails, as it does in the prototype, and that is why `project set` refuses a lever
//!   written twice.
//! - **R4**: a lever is taken if and only if it is ASCII digits, blanks around them allowed, within its limits.
//! - **R5**: a repo is taken if and only if its `<owner>/<name>` has the prototype's pattern and does not end in
//!   `.git`, a bare name being the owner's; a list of products is each item a repo, the words for none giving none.
//!
//! Each expectation is written here again, independently of the product's code, from the prototype's rules.

use proptest::prelude::*;
use regia_core::{ProjectLever, Registry, products_list, pystr, registry_key, registry_key_count, registry_marker, registry_with, repo_argument};

/// The keys a generated registry uses: those `project new` writes, and one written by hand.
fn keys() -> Vec<&'static str> {
    vec![
        registry_key("id"),
        registry_key("folder"),
        registry_key("playbook"),
        registry_key("priority"),
        registry_key("wip"),
        registry_key("repo"),
        registry_key("products"),
        "colour_by_hand",
    ]
}

/// A value as `project set` writes one: no line break, no blank at its ends, no end of the block.
fn value() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9/_.,:-][a-zA-Z0-9/_.,: -]{0,18}[a-zA-Z0-9/_.,:-]|[a-zA-Z0-9]?".prop_filter("stripped", |value| pystr::strip(value) == value)
}

/// A line of the block that is not the key's: another key with its value, spaced in one of the ways a hand writes
/// it, a blank line, or a line with no colon.
fn other_line(key: &'static str) -> impl Strategy<Value = String> {
    let others: Vec<&'static str> = keys().into_iter().filter(|other| *other != key).collect();
    prop_oneof![
        (prop::sample::select(others), value(), 0..3usize).prop_map(|(other, value, form)| match form {
            0 => format!("{other}: {value}"),
            1 => format!(" {other} :{value}"),
            _ => format!("{other}:   {value}  "),
        }),
        "[ ]{0,3}",
        "[a-z ]{1,8}",
    ]
}

/// A valid body: text before, the block with its lines and the key at most once, text after; the line end of the
/// block, LF or CRLF; and where the key's line is, if it is there.
#[derive(Debug, Clone)]
struct Body {
    before: String,
    head: String,
    lines: Vec<String>,
    end: &'static str,
    after: String,
}

impl Body {
    fn text(&self) -> String {
        let mut block: String = self.lines.iter().map(|line| format!("{line}{}", self.end)).collect();
        block.push_str("-->");
        format!("{}{}{block}{}", self.before, self.head, self.after)
    }
}

fn body(key: &'static str) -> impl Strategy<Value = Body> {
    (
        "[a-zA-Z0-9 #.,\n]{0,30}",
        "[ \t]{0,2}",
        prop::collection::vec(other_line(key), 0..8),
        prop::option::of((value(), any::<prop::sample::Index>())),
        prop::bool::ANY,
        "[a-zA-Z0-9 #.,<>!:\n-]{0,30}",
    )
        .prop_map(move |(before, space, mut lines, own, crlf, after)| {
            let end = if crlf { "\r\n" } else { "\n" };
            if let Some((own, at)) = own {
                let at = at.index(lines.len() + 1);
                lines.insert(at, format!("{key}: {own}"));
            }
            Body { before, head: format!("<!--{space}{}{end}", registry_marker()), lines, end, after }
        })
}

fn key() -> impl Strategy<Value = &'static str> {
    prop::sample::select(keys())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// R3, Acceptance 2: `registro(registro_con(body, {k: v}))[k] == v` on every valid body, the rest of the body
    /// the same, and the other keys as they were.
    #[test]
    fn a_key_set_reads_back_as_its_value((key, body) in key().prop_flat_map(|key| (Just(key), body(key))), value in value()) {
        let text = body.text();
        prop_assert!(registry_key_count(&text, key) <= 1);
        let written = registry_with(&text, &[(key, value.as_str())]).expect("a valid body has a registry");
        let (before, after) = (Registry::parse(&text), Registry::parse(&written));
        prop_assert_eq!(after.get(key), Some(value.as_str()));
        let start = format!("{}{}", body.before, body.head);
        prop_assert!(written.starts_with(&start), "the text before the block");
        let end = format!("-->{}", body.after);
        prop_assert!(written.ends_with(&end), "the text after the block");
        for other in keys().into_iter().filter(|other| *other != key) {
            prop_assert_eq!(after.get(other), before.get(other), "{}", other);
        }
    }

    /// R4: a lever is taken if and only if its blanks off it is ASCII digits within its limits.
    #[test]
    fn a_lever_is_taken_only_within_its_limits(text in "[ 0-9+._e²٣-]{0,6}|[0-9]{1,3}") {
        for lever in ProjectLever::ALL {
            let stripped = pystr::strip(&text);
            let digits = !stripped.is_empty() && stripped.bytes().all(|byte| byte.is_ascii_digit());
            let (minimum, maximum) = lever.limits();
            let expected = digits.then(|| stripped.parse::<u64>().unwrap()).filter(|n| *n >= minimum && maximum.is_none_or(|maximum| *n <= maximum));
            prop_assert_eq!(lever.parse(&text).ok(), expected, "{:?} {:?}", lever, text);
        }
    }

    /// R5: a repo is taken if and only if its full name has the prototype's pattern and does not end in `.git`.
    #[test]
    fn a_repo_is_taken_only_with_the_pattern(text in "[ a-zA-Z0-9._/-]{0,12}|[a-z]{1,6}(/[a-z.]{1,6})?(\\.git)?") {
        let stripped = pystr::strip(&text);
        let full = if stripped.contains('/') { stripped.to_owned() } else { format!("owner/{stripped}") };
        let expected = (repo_pattern(&full) && !full.ends_with(".git")).then_some(full);
        prop_assert_eq!(repo_argument(&text, "owner").ok(), expected);
    }

    /// R5: each item of a list of products is a repo, or the list is refused; a word for none is no product.
    #[test]
    fn a_list_of_products_is_each_a_repo(items in prop::collection::vec("[ ]{0,2}([a-z]{1,5}(/[a-z]{1,5})?|a b|x\\.git)[ ]{0,2}", 0..4)) {
        let text = items.join(",");
        let wanted: Vec<&str> = text.split(',').map(pystr::strip).filter(|item| !item.is_empty()).collect();
        let full = |item: &str| if item.contains('/') { item.to_owned() } else { format!("owner/{item}") };
        let expected = if wanted.iter().all(|item| repo_pattern(&full(item)) && !full(item).ends_with(".git")) {
            Some(wanted.iter().map(|item| full(item)).collect::<Vec<_>>())
        } else {
            None
        };
        prop_assert_eq!(products_list(&text, "owner").ok(), expected);
    }
}

/// `[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9][A-Za-z0-9._-]*`, the prototype's `REPO_RE`, written again.
fn repo_pattern(full: &str) -> bool {
    let part = |part: &str| part.starts_with(|c: char| c.is_ascii_alphanumeric()) && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    full.split_once('/').is_some_and(|(owner, name)| part(owner) && part(name))
}

/// The words for none are no product, in any case and with blanks around them.
#[test]
fn the_words_for_none_are_no_product() {
    for none in ["", " ", "-", "—", "nessuno", "Nessuno", " NESSUNA "] {
        assert_eq!(products_list(none, "owner"), Ok(Vec::new()), "{none:?}");
    }
}

/// The control of R3: with the key twice, `registry_with` rewrites the first line and `Registry::parse` reads the
/// last, so the value set does not read back; the count that makes `project set` refuse it is two.
#[test]
fn with_the_key_twice_the_value_set_does_not_read_back() {
    let wip = registry_key("wip");
    let body = format!("<!-- {}\nid: a\n{wip}: 2\n{wip}: 5\n-->\n", registry_marker());
    assert_eq!(registry_key_count(&body, wip), 2);
    let written = registry_with(&body, &[(wip, "3")]).unwrap();
    assert_eq!(Registry::parse(&written).get(wip), Some("5"), "the property fails on a body that is not valid");
}
