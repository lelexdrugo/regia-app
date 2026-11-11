//! Text written by others, made fit to show (threat model T19, §3.7 point 4). Titles, bodies, comments,
//! reports and transcripts come from people and from agents that read the web: before any view gets them,
//! nothing in them may move the cursor, colour the screen, reverse the reading order or hide a character.
//!
//! - C0 and C1 controls go, except line feed and tab, and so do whole ANSI escape sequences.
//! - Bidirectional controls and invisible format characters become a visible glyph, `‹U+202E›`, in code too.
//!   Zero-width joiner and non-joiner stay: emoji sequences and Persian or Indic text need them, and they
//!   neither reorder nor hide anything.
//! - A document is capped at [`DOCUMENT_LIMIT`] bytes; a title is one line of at most [`TITLE_LIMIT`]
//!   characters. Either says when it was cut.

/// The threat model's proposal for one document (§3.7 point 4).
pub const DOCUMENT_LIMIT: usize = 256 * 1024;

/// The threat model's proposal for a title, in characters.
pub const TITLE_LIMIT: usize = 200;

/// Text after normalization, and whether a cap cut it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub text: String,
    pub truncated: bool,
}

/// Controls, escape sequences and invisible characters dealt with; line breaks kept as `\n`.
pub fn normalize(text: &str) -> String {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\r' => {
                out.push('\n');
                if chars.get(i + 1) == Some(&'\n') {
                    i += 1;
                }
            }
            '\u{85}' | '\u{2028}' | '\u{2029}' => out.push('\n'),
            '\u{1B}' => {
                i = escape_end(&chars, i + 1);
                continue;
            }
            // The 8-bit forms of CSI, OSC and the other string introducers.
            '\u{9B}' => {
                i = csi_end(&chars, i + 1);
                continue;
            }
            '\u{90}' | '\u{98}' | '\u{9D}' | '\u{9E}' | '\u{9F}' => {
                i = string_end(&chars, i + 1);
                continue;
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c if is_hidden(c) => out.push_str(&glyph(c)),
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// [`normalize`], then at most [`DOCUMENT_LIMIT`] bytes, cut on a character boundary.
pub fn normalize_document(text: &str) -> Normalized {
    let mut text = normalize(text);
    let truncated = text.len() > DOCUMENT_LIMIT;
    if truncated {
        let mut end = DOCUMENT_LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Normalized { text, truncated }
}

/// [`normalize`] on one line, without the spaces at its ends, at most [`TITLE_LIMIT`] characters; a cut
/// title ends with `…`.
pub fn normalize_title(text: &str) -> Normalized {
    let line: String = normalize(text).chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).collect();
    let line = line.trim();
    if line.chars().count() <= TITLE_LIMIT {
        return Normalized { text: line.to_owned(), truncated: false };
    }
    let mut text: String = line.chars().take(TITLE_LIMIT - 1).collect();
    text.push('…');
    Normalized { text, truncated: true }
}

/// Characters that change how text reads without showing themselves: bidirectional controls, marks and
/// isolates, and the invisible format characters a reader cannot see (Unicode's Cf, and the Hangul
/// fillers that pass for blanks). ZWJ and ZWNJ are left out on purpose: see the module's comment.
fn is_hidden(c: char) -> bool {
    matches!(
        c,
        '\u{AD}'
            | '\u{61C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{180E}'
            | '\u{200B}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// `‹U+202E›`: the threat model's visible form of a hidden character.
fn glyph(c: char) -> String {
    format!("‹U+{:04X}›", u32::from(c))
}

/// Past an escape sequence whose ESC is just before `i`: CSI, the string sequences (OSC, DCS, SOS, PM,
/// APC), or a two- or three-character sequence. A sequence that breaks off loses only what was read.
fn escape_end(chars: &[char], i: usize) -> usize {
    match chars.get(i) {
        Some('[') => csi_end(chars, i + 1),
        Some(']' | 'P' | 'X' | '^' | '_') => string_end(chars, i + 1),
        Some(c) if ('\u{20}'..='\u{2F}').contains(c) => {
            let mut j = i;
            while chars.get(j).is_some_and(|c| ('\u{20}'..='\u{2F}').contains(c)) {
                j += 1;
            }
            if chars.get(j).is_some_and(|c| ('\u{30}'..='\u{7E}').contains(c)) { j + 1 } else { j }
        }
        Some(c) if ('\u{30}'..='\u{7E}').contains(c) => i + 1,
        _ => i,
    }
}

/// Past the parameters, intermediates and final byte of a CSI sequence.
fn csi_end(chars: &[char], mut i: usize) -> usize {
    while chars.get(i).is_some_and(|c| ('\u{30}'..='\u{3F}').contains(c)) {
        i += 1;
    }
    while chars.get(i).is_some_and(|c| ('\u{20}'..='\u{2F}').contains(c)) {
        i += 1;
    }
    if chars.get(i).is_some_and(|c| ('\u{40}'..='\u{7E}').contains(c)) { i + 1 } else { i }
}

/// Past a string sequence and its terminator: BEL, ST (ESC `\`) or the 8-bit ST. One without a terminator
/// before the end of its line loses only its introducer, so it cannot swallow the rest of a document.
fn string_end(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        match c {
            '\u{7}' | '\u{9C}' => return i + 1,
            '\u{1B}' if chars.get(i + 1) == Some(&'\\') => return i + 2,
            '\n' | '\r' => return start,
            _ => i += 1,
        }
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_and_escape_sequences_go_and_line_breaks_stay() {
        let cases = [
            ("a\u{0}b\u{7}c\u{8}d", "abcd"),
            ("one\r\ntwo\rthree\u{2028}four", "one\ntwo\nthree\nfour"),
            ("tab\tand\nline", "tab\tand\nline"),
            ("\u{1B}[31mred\u{1B}[0m plain", "red plain"),
            ("\u{1B}[1;38;5;196mbold\u{1B}[m", "bold"),
            ("\u{1B}]8;;https://example.invalid\u{7}link\u{1B}]8;;\u{7}", "link"),
            ("\u{1B}]0;window title\u{1B}\\after", "after"),
            ("\u{9B}31mred", "red"),
            ("\u{1B}(Bascii\u{1B}7saved\u{1B}8", "asciisaved"),
            ("\u{7F}\u{80}\u{9F}x", "x"),
            ("\u{FEFF}start", "start"),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize(input), expected, "{input:?}");
        }
    }

    #[test]
    fn an_unterminated_string_sequence_cannot_hide_the_rest() {
        assert_eq!(normalize("\u{1B}]8;;https://example.invalid\nvisible"), "8;;https://example.invalid\nvisible");
        assert_eq!(normalize("before\u{1B}]never ends"), "beforenever ends");
    }

    #[test]
    fn bidi_and_invisible_characters_become_visible() {
        assert_eq!(normalize("Approve \u{202E}evil\u{202C} done"), "Approve ‹U+202E›evil‹U+202C› done");
        assert_eq!(normalize("a\u{2066}b\u{2069}c\u{200F}d"), "a‹U+2066›b‹U+2069›c‹U+200F›d");
        assert_eq!(normalize("zero\u{200B}width"), "zero‹U+200B›width");
        assert_eq!(normalize("tag\u{E0041}\u{E0042}"), "tag‹U+E0041›‹U+E0042›");
        assert_eq!(normalize("mid\u{FEFF}dle"), "mid‹U+FEFF›dle");
        // Kept: a family emoji and a Persian word need them.
        assert_eq!(normalize("👩\u{200D}👧"), "👩\u{200D}👧");
        assert_eq!(normalize("می\u{200C}خواهم"), "می\u{200C}خواهم");
    }

    #[test]
    fn a_title_is_one_line_with_a_cap() {
        assert_eq!(normalize_title("  first\nsecond\tthird  ").text, "first second third");
        let long = "x".repeat(TITLE_LIMIT + 5);
        let cut = normalize_title(&long);
        assert!(cut.truncated);
        assert_eq!(cut.text.chars().count(), TITLE_LIMIT);
        assert!(cut.text.ends_with('…'));
        let exact = normalize_title(&"y".repeat(TITLE_LIMIT));
        assert!(!exact.truncated);
        assert_eq!(exact.text.chars().count(), TITLE_LIMIT);
    }

    #[test]
    fn a_document_has_a_cap_on_a_character_boundary() {
        let short = normalize_document("small");
        assert_eq!(short, Normalized { text: "small".to_owned(), truncated: false });
        let big = normalize_document(&"è".repeat(DOCUMENT_LIMIT));
        assert!(big.truncated);
        assert!(big.text.len() <= DOCUMENT_LIMIT && big.text.len() > DOCUMENT_LIMIT - 2);
        assert!(big.text.chars().all(|c| c == 'è'));
    }
}
