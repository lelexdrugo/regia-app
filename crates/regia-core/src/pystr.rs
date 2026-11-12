//! Python's rules for text, where the v1 formats depend on them: the prototype reads the tracker with
//! `str.strip()`, `str.splitlines()` and the regex classes `\s` and `\w`. Its whitespace is Unicode's plus
//! the four separators U+001C–U+001F, and its line breaks are more than `\n` and `\r`.

/// `str.isspace()`.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.strip()`.
pub fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `str.rstrip()`.
pub fn rstrip(text: &str) -> &str {
    text.trim_end_matches(is_space)
}

/// A boundary of `str.splitlines()`: U+001F is whitespace but not a line break.
pub fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// `str.splitlines()`: `\r\n` is one break, and a break at the end makes no empty last line.
pub fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if !is_line_break(c) {
            continue;
        }
        lines.push(&text[start..at]);
        start = at + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
            chars.next();
            start += 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// The class of `\s` in a pattern of the prototype, for the `regex` crate.
pub const SPACE: &str = r"[\s\x1C-\x1F]";

/// The members of `\w` in a pattern of the prototype, to put inside a class: letters, numbers and `_`.
/// The `regex` crate's own `\w` also takes combining marks and connector punctuation.
pub const WORD: &str = r"\p{L}\p{N}_";

#[cfg(test)]
mod tests {
    use super::*;

    /// The values `python -c "print(repr(s.splitlines()))"` gives for each.
    #[test]
    fn lines_split_as_in_python() {
        let cases: [(&str, &[&str]); 9] = [
            ("", &[]),
            ("\n", &[""]),
            ("a\n", &["a"]),
            ("a\n\n", &["a", ""]),
            ("a\r\nb", &["a", "b"]),
            ("a\r\rb", &["a", "", "b"]),
            ("a\u{85}b\u{2028}c\u{2029}d", &["a", "b", "c", "d"]),
            ("a\u{1c}b\u{1d}c\u{1e}d\u{1f}e", &["a", "b", "c", "d\u{1f}e"]),
            ("a\u{0b}b\u{0c}c", &["a", "b", "c"]),
        ];
        for (text, lines) in cases {
            assert_eq!(split_lines(text), lines, "{text:?}");
        }
    }

    #[test]
    fn whitespace_is_python_s() {
        assert_eq!(strip("\u{1f}\u{a0} a b\u{3000}\u{1c}"), "a b");
        assert_eq!(rstrip(" a \u{2029}"), " a");
        assert!(!is_space('\u{200b}'), "a zero-width space is not whitespace in Python");
    }
}
