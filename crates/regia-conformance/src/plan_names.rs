//! The one normalization of `plan`'s comparisons with the prototype (#444), declared: the native plan quotes its own
//! command line, in the first line of the diagram and in the line before it, and names its own option of a root in
//! the hint of a project's cut (`docs/contract/commands.md`, the plan's command line); the prototype quotes its own. Here the
//! product's names become the prototype's, by the facade table, as whole words. `tests/plan.rs` and `live-check`
//! include this file by its path, so neither touches the harness's library.

use regia_facade::Table;

/// The product's names of `plan` and of its options, each with the prototype's, as the facade table translates them.
pub(crate) fn names() -> Vec<(String, String)> {
    let table = Table::embedded();
    let mut out = Vec::new();
    for argv in [&["--root", "1", "--mermaid", "--max-nodes", "3"][..], &["--project", "p"]] {
        let english: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let python = table.python_argv("plan", &english).expect("the table knows plan");
        out.push(("plan".to_owned(), python[0].clone()));
        out.extend(english.iter().zip(&python[1..]).filter(|(arg, _)| arg.starts_with("--")).map(|(arg, theirs)| (arg.clone(), theirs.clone())));
    }
    out.sort();
    out.dedup();
    out
}

/// `text` with each of `from` as a whole word, between blanks, backticks or the ends, as `to`.
pub(crate) fn replace_word(text: &str, from: &str, to: &str) -> String {
    let edge = |c: Option<char>| c.is_none_or(|c| c.is_whitespace() || c == '`');
    let mut out = String::new();
    let mut rest = text;
    // The character of the text before `rest`.
    let mut previous: Option<char> = None;
    while let Some(at) = rest.find(from) {
        let before = if at == 0 { previous } else { rest[..at].chars().last() };
        let after = rest[at + from.len()..].chars().next();
        out.push_str(&rest[..at]);
        out.push_str(if edge(before) && edge(after) { to } else { from });
        previous = from.chars().last();
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

/// `text`, a string the native plan wrote, with the product's names of the command line as the prototype's.
pub(crate) fn in_the_prototype_s_names(text: &str) -> String {
    let names = names();
    let command = names.iter().find(|(ours, _)| ours == "plan").map(|(_, theirs)| theirs.clone()).expect("the command's name");
    let bin = regia_identity::BIN_NAME;
    let mut out = replace_word(text, &format!("{bin} plan"), &format!("{bin} {command}"));
    for (ours, theirs) in names.iter().filter(|(ours, _)| ours.starts_with("--")) {
        out = replace_word(&out, ours, theirs);
    }
    out
}
