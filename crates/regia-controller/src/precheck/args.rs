//! The command line of the precheck, as the prototype's parser reads it: long options only, each by its
//! name or by a prefix no other option shares, its value after `=` or as the next argument. Anything else,
//! a help included, is an error of the precheck (exit 1), never the parser's own exit.

use super::Fault;
use super::v1::format;

/// What a tick runs with. The series of a stall, its alert and the budget's threshold are read and checked
/// here, for the parts of the precheck that use them (#466).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Only these projects; every active one when empty.
    pub projects: Vec<String>,
    pub stall_n: i64,
    pub stall_s: f64,
    pub realert_s: f64,
    /// The threshold of the 5-hour window, `None` for the policy's.
    pub budget_max: Option<f64>,
    /// The time the whole tick may take, in seconds.
    pub deadline_s: f64,
}

impl Default for Options {
    fn default() -> Options {
        let defaults = &format().options.defaults;
        Options {
            projects: Vec::new(),
            stall_n: defaults.stall_n,
            stall_s: defaults.stall_s,
            realert_s: defaults.realert_s,
            budget_max: None,
            deadline_s: defaults.deadline_s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Name {
    Project,
    StallN,
    StallS,
    RealertS,
    BudgetMax,
    DeadlineS,
}

/// The options by their words.
fn names() -> [(&'static str, Name); 6] {
    let words = &format().options;
    [
        (words.project.as_str(), Name::Project),
        (words.stall_n.as_str(), Name::StallN),
        (words.stall_s.as_str(), Name::StallS),
        (words.realert_s.as_str(), Name::RealertS),
        (words.budget_max.as_str(), Name::BudgetMax),
        (words.deadline_s.as_str(), Name::DeadlineS),
    ]
}

/// `-3`, `-.5`, `-3.5`: what the parser takes as a value, not as an option.
fn is_negative_number(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('-') else { return false };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    match rest.split_once('.') {
        None => digits(rest),
        Some((whole, fraction)) => (whole.is_empty() || digits(whole)) && digits(fraction),
    }
}

/// Whether the parser reads `text` as an option rather than as a value.
fn looks_like_an_option(text: &str) -> bool {
    text.starts_with('-') && text.chars().count() > 1 && !is_negative_number(text) && !text.contains(' ')
}

fn error(detail: String) -> Fault {
    Fault::internal(format!("arguments: {detail}"))
}

/// The option `name` names, exactly or by a prefix only it has.
fn option(name: &str) -> Result<Name, Fault> {
    if let Some((_, option)) = names().into_iter().find(|(word, _)| *word == name) {
        return Ok(option);
    }
    let matches: Vec<(&str, Name)> = names().into_iter().filter(|(word, _)| name.len() > 2 && word.starts_with(name)).collect();
    match matches.as_slice() {
        [(_, option)] => Ok(*option),
        [] => Err(error(format!("unrecognized argument {name}"))),
        _ => Err(error(format!("ambiguous option {name}"))),
    }
}

/// A whole number as Python's `int()` reads one: blanks around it, a sign, `_` between digits.
pub(crate) fn python_int(text: &str) -> Option<i64> {
    let text = regia_core::pystr::strip(text);
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if !well_formed_digits(digits, |c| c.is_ascii_digit()) {
        return None;
    }
    let value: i64 = digits.replace('_', "").parse().ok()?;
    Some(if negative { -value } else { value })
}

/// Digits with `_` only between two of them.
fn well_formed_digits(text: &str, digit: impl Fn(char) -> bool) -> bool {
    !text.is_empty() && !text.starts_with('_') && !text.ends_with('_') && !text.contains("__") && text.chars().all(|c| digit(c) || c == '_')
}

/// A number as Python's `float()` reads one: blanks around it, a sign, a fraction, an exponent, `_`
/// between digits, and the words of infinity and of not a number in any case.
fn python_float(text: &str) -> Option<f64> {
    let text = regia_core::pystr::strip(text);
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let word = unsigned.to_ascii_lowercase();
    if ["inf", "infinity", "nan"].contains(&word.as_str()) {
        return text.parse().ok();
    }
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let (whole, fraction) = mantissa.split_once('.').map_or((mantissa, None), |(whole, fraction)| (whole, Some(fraction)));
    let part = |part: &str| part.is_empty() || well_formed_digits(part, |c| c.is_ascii_digit());
    let mantissa_ok = part(whole) && fraction.is_none_or(part) && !(whole.is_empty() && fraction.is_none_or(str::is_empty));
    let exponent_ok = exponent.is_none_or(|exponent| well_formed_digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent), |c| c.is_ascii_digit()));
    if !(mantissa_ok && exponent_ok) {
        return None;
    }
    text.replace('_', "").parse().ok()
}

/// The precheck's arguments, the prototype's names, or the fault of the first one that does not read.
///
/// # Errors
///
/// An internal fault for an argument the precheck does not know, a value missing or that does not read,
/// or anything that would be a positional argument.
pub fn parse_options(argv: &[String]) -> Result<Options, Fault> {
    let mut options = Options::default();
    let mut rest = argv.iter();
    while let Some(argument) = rest.next() {
        if !looks_like_an_option(argument) || !argument.starts_with("--") || argument == "--" {
            return Err(error(format!("unexpected argument {argument:?}")));
        }
        let (name, given) = match argument.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (argument.as_str(), None),
        };
        let option = option(name)?;
        let value = match given {
            Some(value) => value,
            None => match rest.next() {
                Some(value) if !looks_like_an_option(value) => value.clone(),
                _ => return Err(error(format!("{name}: expected one argument"))),
            },
        };
        let float = |value: &str| python_float(value).ok_or_else(|| error(format!("{name}: invalid float value: {value:?}")));
        match option {
            Name::Project => options.projects.push(value),
            Name::StallN => options.stall_n = python_int(&value).ok_or_else(|| error(format!("{name}: invalid int value: {value:?}")))?,
            Name::StallS => options.stall_s = float(&value)?,
            Name::RealertS => options.realert_s = float(&value)?,
            Name::BudgetMax => options.budget_max = Some(float(&value)?),
            Name::DeadlineS => options.deadline_s = float(&value)?,
        }
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::precheck::Reason;

    fn parse(argv: &[&str]) -> Result<Options, Reason> {
        parse_options(&argv.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>()).map_err(|fault| fault.reason)
    }

    #[test]
    fn the_options_read_by_name_prefix_and_equals() {
        let words = &format().options;
        let options = parse(&[&words.project, "alpha", &format!("{}=beta", words.project), &words.stall_n, " 4 ", &words.deadline_s, "2.5e1"]).unwrap();
        assert_eq!(options.projects, ["alpha", "beta"]);
        assert_eq!((options.stall_n, options.deadline_s), (4, 25.0));
        assert_eq!(options.budget_max, None);
        let prefix = &words.budget_max[..4];
        assert_eq!(parse(&[prefix, "-3"]).unwrap().budget_max, Some(-3.0), "a unique prefix, and a negative number as a value");
        assert_eq!(parse(&[]).unwrap(), Options::default());
        assert_eq!(Options::default().deadline_s, 100.0);
    }

    #[test]
    fn a_wrong_argument_is_an_internal_fault_never_the_parser_s_exit() {
        let words = &format().options;
        let shared = &words.stall_n[..3];
        for argv in [
            vec!["--unknown"],
            vec!["--help"],
            vec!["-h"],
            vec![words.stall_n.as_str(), "x"],
            vec![words.project.as_str()],
            vec!["positional"],
            vec![shared, "3"],
            vec!["--", "x"],
            vec![words.project.as_str(), words.stall_n.as_str()],
            vec![words.stall_s.as_str(), "1__0"],
        ] {
            assert_eq!(parse(&argv).map(|_| ()), Err(Reason::Internal), "{argv:?}");
        }
    }

    #[test]
    fn numbers_read_as_python_reads_them() {
        assert_eq!(python_int("+1_000"), Some(1000));
        assert_eq!(python_int("1.0"), None);
        assert_eq!(python_int("_1"), None);
        for (text, value) in [("1_0.5", 10.5), (".5", 0.5), ("5.", 5.0), ("-1E2", -100.0), (" 7 ", 7.0)] {
            assert_eq!(python_float(text), Some(value), "{text}");
        }
        assert!(python_float("nan").is_some_and(f64::is_nan));
        assert_eq!(python_float("-Infinity"), Some(f64::NEG_INFINITY));
        for bad in ["", ".", "e5", "1e", "1_", "x", "1 0"] {
            assert_eq!(python_float(bad), None, "{bad:?}");
        }
    }
}
