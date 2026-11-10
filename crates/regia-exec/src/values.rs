//! The values that may reach a program's argv (threat model §3.2, T04). Each constructor refuses
//! what a tool could read as an option, or what could change meaning on the way: a leading dash,
//! control characters, `..`, `@`, spaces where the tool has no use for them, excessive length.

use std::path::PathBuf;

/// Why a value was refused. It never repeats the value, which may be hostile and would end up on a
/// terminal.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind}: {reason}")]
pub struct InvalidValue {
    pub kind: &'static str,
    pub reason: &'static str,
}

fn refuse<T>(kind: &'static str, reason: &'static str) -> Result<T, InvalidValue> {
    Err(InvalidValue { kind, reason })
}

/// Present, not too long, no leading dash, no control character.
fn common(kind: &'static str, text: &str, max: usize) -> Result<(), InvalidValue> {
    if text.is_empty() {
        return refuse(kind, "empty");
    }
    if text.len() > max {
        return refuse(kind, "too long");
    }
    if text.starts_with('-') {
        return refuse(kind, "starts with a dash");
    }
    if text.chars().any(char::is_control) {
        return refuse(kind, "has a control character");
    }
    Ok(())
}

fn only(kind: &'static str, text: &str, allowed: impl Fn(char) -> bool) -> Result<(), InvalidValue> {
    if text.chars().all(allowed) { Ok(()) } else { refuse(kind, "has a character outside its alphabet") }
}

mod sealed {
    pub trait Sealed {}
}

/// A value that can be an argument. Only the types of this module implement it.
pub trait ArgValue: sealed::Sealed {
    fn as_arg(&self) -> &str;
}

macro_rules! arg_value {
    ($($kind:ty),*) => {$(
        impl sealed::Sealed for $kind {}
        impl ArgValue for $kind {
            fn as_arg(&self) -> &str {
                self.as_str()
            }
        }
    )*};
}

arg_value!(IssueNumber, RepoSlug, GitRef, ModelId, Effort, DossierPath, Text);

/// A GitHub issue number, from 1 to 2³¹ − 1, the range of GraphQL's `Int`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueNumber {
    value: u64,
    text: String,
}

impl IssueNumber {
    const KIND: &str = "issue number";

    pub fn new(value: u64) -> Result<Self, InvalidValue> {
        if value == 0 || value > i32::MAX as u64 {
            return refuse(Self::KIND, "out of range");
        }
        Ok(Self { value, text: value.to_string() })
    }

    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        common(Self::KIND, text, 10)?;
        only(Self::KIND, text, |c| c.is_ascii_digit())?;
        if text.starts_with('0') {
            return refuse(Self::KIND, "has a leading zero");
        }
        Self::new(text.parse().map_err(|_| InvalidValue { kind: Self::KIND, reason: "out of range" })?)
    }

    pub fn get(&self) -> u64 {
        self.value
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// `owner/name` as GitHub allows it: an owner of letters, digits and single inner hyphens, up to
/// 39; a name of letters, digits, `.`, `_` and `-`, up to 100, with no `..`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoSlug {
    text: String,
    slash: usize,
}

impl RepoSlug {
    const KIND: &str = "repository";

    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        common(Self::KIND, text, 140)?;
        let Some((owner, name)) = text.split_once('/') else {
            return refuse(Self::KIND, "is not owner/name");
        };
        if owner.is_empty() || owner.len() > 39 || owner.ends_with('-') || owner.contains("--") {
            return refuse(Self::KIND, "has an invalid owner");
        }
        only(Self::KIND, owner, |c| c.is_ascii_alphanumeric() || c == '-')?;
        if name.is_empty() || name.len() > 100 || name.starts_with('-') || name == "." || name.contains("..") {
            return refuse(Self::KIND, "has an invalid name");
        }
        only(Self::KIND, name, |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))?;
        Ok(Self { text: text.to_owned(), slash: owner.len() })
    }

    pub fn owner(&self) -> &str {
        &self.text[..self.slash]
    }

    pub fn name(&self) -> &str {
        &self.text[self.slash + 1..]
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// A branch, tag or revision, in a narrower alphabet than git's: letters, digits, `.`, `_`, `-`,
/// and `/` between components. No component starts with `.` or `-` or ends with `.lock`; no `..`,
/// no `//`, no trailing `/` or `.`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitRef(String);

impl GitRef {
    const KIND: &str = "git ref";

    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        common(Self::KIND, text, 200)?;
        only(Self::KIND, text, |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))?;
        if text.contains("..") || text.ends_with('.') {
            return refuse(Self::KIND, "has a misplaced dot");
        }
        for component in text.split('/') {
            if component.is_empty() {
                return refuse(Self::KIND, "has an empty component");
            }
            if component.starts_with(['.', '-']) || component.ends_with(".lock") {
                return refuse(Self::KIND, "has a component git refuses");
            }
        }
        Ok(Self(text.to_owned()))
    }

    pub fn head() -> Self {
        Self("HEAD".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A model name as the agents' CLIs take it (`opus`, `claude-sonnet-4-5`, `gpt-5.4-codex`): letters,
/// digits, `.`, `_`, `:` and `-`, starting with a letter or a digit, up to 64.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelId(String);

impl ModelId {
    const KIND: &str = "model";

    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        common(Self::KIND, text, 64)?;
        only(Self::KIND, text, |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))?;
        if !text.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return refuse(Self::KIND, "does not start with a letter or a digit");
        }
        if text.contains("..") {
            return refuse(Self::KIND, "has a misplaced dot");
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The effort levels the agents' CLIs know. A closed list: a policy file cannot add one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effort {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 6] = [Effort::Minimal, Effort::Low, Effort::Medium, Effort::High, Effort::Xhigh, Effort::Max];

    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        Self::ALL
            .into_iter()
            .find(|effort| effort.as_str() == text)
            .ok_or(InvalidValue { kind: "effort", reason: "is not a known level" })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Effort::Minimal => "minimal",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Xhigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

/// A path inside the dossier, relative, with `/`. Components are letters, digits, `.`, `_` and
/// `-`; none is `.`, starts with `-`, ends with `.`, or is a name Windows reserves, and no `..`
/// appears anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DossierPath(String);

impl DossierPath {
    const KIND: &str = "dossier path";

    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        common(Self::KIND, text, 240)?;
        only(Self::KIND, text, |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))?;
        if text.contains("..") {
            return refuse(Self::KIND, "has a misplaced dot");
        }
        for component in text.split('/') {
            if component.is_empty() {
                return refuse(Self::KIND, "has an empty component");
            }
            if component == "." || component == ".." || component.ends_with('.') {
                return refuse(Self::KIND, "has a misplaced dot");
            }
            if component.starts_with('-') {
                return refuse(Self::KIND, "has a component that starts with a dash");
            }
            if reserved_on_windows(component) {
                return refuse(Self::KIND, "has a name Windows reserves");
            }
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_path(&self) -> PathBuf {
        self.0.split('/').collect()
    }
}

/// `CON`, `NUL`, `COM1`… with or without an extension.
fn reserved_on_windows(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or("").to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit()
        }
    }
}

/// Free text a tool reads as data, such as a value the facade passes to the prototype: anything but
/// a leading dash, a NUL or another control character except tab and line breaks, up to 64 KiB. A
/// `.cmd` takes much less ([`Launch::run`](crate::Launch::run)).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Text(String);

impl Text {
    const KIND: &str = "text";

    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        if text.is_empty() {
            return refuse(Self::KIND, "empty");
        }
        if text.len() > 64 * 1024 {
            return refuse(Self::KIND, "too long");
        }
        if text.starts_with('-') {
            return refuse(Self::KIND, "starts with a dash");
        }
        if text.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')) {
            return refuse(Self::KIND, "has a control character");
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The hostile values of V11 (threat model §6).
    fn hostile() -> Vec<String> {
        let mut out: Vec<String> = [
            "-x",
            "--upload-pack=x",
            "--output=x",
            "-c core.sshCommand=x",
            ";",
            "&",
            "|",
            "%PATH%",
            "^",
            "\"",
            "a\nb",
            "",
            "ünïcödé",
            "@file",
            "--",
            "..",
            "a/../b",
        ]
        .map(str::to_owned)
        .to_vec();
        out.push("x".repeat(10 * 1024));
        out
    }

    #[test]
    fn typed_values_refuse_every_hostile_value() {
        for value in hostile() {
            assert!(IssueNumber::parse(&value).is_err(), "issue number took {value:?}");
            assert!(RepoSlug::parse(&value).is_err(), "repository took {value:?}");
            assert!(GitRef::parse(&value).is_err(), "git ref took {value:?}");
            assert!(ModelId::parse(&value).is_err(), "model took {value:?}");
            assert!(Effort::parse(&value).is_err(), "effort took {value:?}");
            assert!(DossierPath::parse(&value).is_err(), "dossier path took {value:?}");
        }
    }

    /// Free text keeps what is data (`;`, `&`, quotes, line breaks, Unicode) and refuses what a
    /// parser would take for an option.
    #[test]
    fn text_refuses_only_what_reads_as_an_option_or_is_empty() {
        for value in hostile() {
            let refused = value.is_empty() || value.starts_with('-');
            assert_eq!(Text::parse(&value).is_err(), refused, "{value:?}");
        }
        assert!(Text::parse("\u{1b}[31m").is_err(), "an escape sequence is a control character");
        assert!(Text::parse(&"x".repeat(64 * 1024 + 1)).is_err());
    }

    #[test]
    fn ordinary_values_pass() {
        assert_eq!(IssueNumber::parse("438").unwrap().get(), 438);
        assert!(IssueNumber::parse("0").is_err() && IssueNumber::parse("2147483648").is_err());
        let slug = RepoSlug::parse("octo-org/my.repo_2").unwrap();
        assert_eq!((slug.owner(), slug.name()), ("octo-org", "my.repo_2"));
        for refused in ["octo--org/x", "octo-/x", "octo/", "/x", "octo/.", "octo/-x", "a/b/c"] {
            assert!(RepoSlug::parse(refused).is_err(), "{refused}");
        }
        for branch in ["HEAD", "main", "lelexdrugo/regia-438-exec", "v1.2.3"] {
            assert!(GitRef::parse(branch).is_ok(), "{branch}");
        }
        for refused in ["a//b", "a/", ".hidden", "a/.b", "x.lock", "a/x.lock", "a.", "a/-b"] {
            assert!(GitRef::parse(refused).is_err(), "{refused}");
        }
        for model in ["opus", "claude-sonnet-4-5", "gpt-5.4-codex", "us.anthropic.claude:0"] {
            assert!(ModelId::parse(model).is_ok(), "{model}");
        }
        assert!(ModelId::parse(".x").is_err() && ModelId::parse("a b").is_err() && ModelId::parse("opus[1m]").is_err());
        assert_eq!(Effort::parse("xhigh").unwrap(), Effort::Xhigh);
        for path in ["tools/regia.py", ".regia/report/2026-11-10-438-x.md", "a_b-c.d"] {
            assert!(DossierPath::parse(path).is_ok(), "{path}");
        }
        for refused in ["/abs", "a//b", "a/", "./a", "a/./b", "a.", "con", "docs/NUL.txt", "COM1", "tools/-x", "C:/x"] {
            assert!(DossierPath::parse(refused).is_err(), "{refused}");
        }
        assert_eq!(DossierPath::parse("tools/regia.py").unwrap().to_path(), PathBuf::from("tools").join("regia.py"));
    }

    /// Whatever a constructor accepts keeps the properties the arguments rely on.
    fn safe_token(text: &str) -> bool {
        !text.is_empty()
            && !text.starts_with('-')
            && !text.contains("..")
            && !text.contains('@')
            && text.chars().all(|c| c.is_ascii_graphic())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn accepted_values_are_safe_tokens(text in "[-a-zA-Z0-9._:/@ ;&|%^\"\\\\\n\t\u{1b}é]{0,24}") {
            if let Ok(value) = IssueNumber::parse(&text) { prop_assert!(safe_token(value.as_str())); }
            if let Ok(value) = RepoSlug::parse(&text) { prop_assert!(safe_token(value.as_str())); }
            if let Ok(value) = GitRef::parse(&text) { prop_assert!(safe_token(value.as_str()) && !value.as_str().contains("//")); }
            if let Ok(value) = ModelId::parse(&text) { prop_assert!(safe_token(value.as_str()) && !value.as_str().contains('/')); }
            if let Ok(value) = DossierPath::parse(&text) {
                prop_assert!(safe_token(value.as_str()) && !value.as_str().starts_with('/') && !value.as_str().contains(':'));
            }
            if let Ok(value) = Text::parse(&text) {
                prop_assert!(!value.as_str().is_empty() && !value.as_str().starts_with('-') && !value.as_str().contains('\x1b'));
            }
        }

        #[test]
        fn any_string_is_judged_without_panicking(text in any::<String>()) {
            let _ = (IssueNumber::parse(&text), RepoSlug::parse(&text), GitRef::parse(&text));
            let _ = (ModelId::parse(&text), Effort::parse(&text), DossierPath::parse(&text), Text::parse(&text));
        }
    }
}
