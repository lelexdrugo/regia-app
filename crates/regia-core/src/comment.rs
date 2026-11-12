//! The comments of an issue, as `task show` gives them (#423 of the dossier): the role each one's signature
//! carries, the owner's for one with none, the body without the signature, and a worker's report as one
//! line, since the report itself is in the dossier. The signature's words are data of format v1, in
//! `formats/tracker_v1.toml`; this is their one reader (M2 of the conformance study).

use std::sync::OnceLock;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, TimeZone};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::pystr;
use crate::tracker::RawComment;

#[derive(Deserialize)]
struct File {
    signature: SignatureFile,
}

#[derive(Deserialize)]
struct SignatureFile {
    marker: String,
    role_key: String,
    owner: String,
    worker: String,
    visible_role_prefix: String,
    visible_owner: String,
    summary_key: String,
    summary_max: usize,
}

struct Signature {
    marker: Regex,
    visible: Regex,
    summary: Regex,
    owner: String,
    worker: String,
    summary_max: usize,
}

fn signature() -> &'static Signature {
    static SIGNATURE: OnceLock<Signature> = OnceLock::new();
    SIGNATURE.get_or_init(|| {
        let file: File = toml::from_str(crate::v1::VOCABULARY).expect("the embedded v1 vocabulary is valid");
        let words = file.signature;
        let (space, word) = (pystr::SPACE, pystr::WORD);
        let (marker, role) = (regex::escape(&words.marker), regex::escape(&words.role_key));
        let (prefix, owner) = (regex::escape(&words.visible_role_prefix), regex::escape(&words.visible_owner));
        Signature {
            marker: Regex::new(&format!(r"^<!--{space}*{marker}{space}+{role}=([A-Za-z][{word}-]*)(?:{space}[^<>]*)?-->$"))
                .expect("a valid pattern"),
            visible: Regex::new(&format!(r"^— (?:{prefix} [{word}][{word}-]*|{owner}){space}*$")).expect("a valid pattern"),
            summary: Regex::new(&format!(r"(?m)^{}:[ \t]*(.*?)[ \t]*$", regex::escape(&words.summary_key)))
                .expect("a valid pattern"),
            owner: words.owner,
            worker: words.worker,
            summary_max: words.summary_max,
        }
    })
}

/// A comment as `task show --comments` gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Comment {
    pub id: Option<u64>,
    pub created_at: String,
    /// The role of the signature, as the marker writes it: data, as the markers already published are.
    pub role: String,
    pub url: String,
    pub body: String,
    /// A worker's body is its report's one line.
    pub shortened: bool,
}

/// The last line that is not blank, stripped.
fn last_line(body: &str) -> &str {
    pystr::split_lines(body).into_iter().map(pystr::strip).rfind(|line| !line.is_empty()).unwrap_or_default()
}

/// The role of the marker on the last line of `body` that is not blank; `None` without one, or with one
/// that does not read (`ruolo_firma`).
pub fn signature_role(body: &str) -> Option<String> {
    signature().marker.captures(last_line(body)).and_then(|captures| captures.get(1)).map(|role| role.as_str().to_owned())
}

/// `body` without the signature, its visible line and its marker (`_senza_firma`).
pub fn without_signature(body: &str) -> String {
    let mut lines = pystr::split_lines(pystr::rstrip(body));
    if signature_role(body).is_some() {
        // With the end stripped, the last line is the marker.
        lines.pop();
        while lines.last().is_some_and(|line| pystr::strip(line).is_empty()) {
            lines.pop();
        }
        if lines.last().is_some_and(|line| signature().visible.is_match(pystr::strip(line))) {
            lines.pop();
        }
    }
    pystr::strip(&lines.join("\n")).to_owned()
}

/// The line that sums up a long comment (`_prima_riga`): a report pasted with its front matter reads as its
/// summary key, or else as its first line that is not the front matter's fence; any other comment as its
/// first line that is not blank. Cut to the format's length, in characters, with an ellipsis.
pub fn summary_line(body: &str) -> String {
    let text = without_signature(body);
    let lines: Vec<&str> = pystr::split_lines(&text).into_iter().filter(|line| !pystr::strip(line).is_empty()).collect();
    let fence = "---";
    let line = if lines.first().is_some_and(|line| pystr::strip(line) == fence) {
        let summary = signature()
            .summary
            .captures(&text)
            .and_then(|captures| captures.get(1))
            .map(|value| pystr::strip(value.as_str()))
            .filter(|value| !value.is_empty())
            .map_or("", |value| value.trim_matches(|c| c == '"' || c == '\''));
        if summary.is_empty() {
            lines[1..].iter().map(|line| pystr::strip(line)).find(|line| *line != fence).unwrap_or_default()
        } else {
            summary
        }
    } else {
        lines.first().map_or("", |line| pystr::strip(line))
    };
    let max = signature().summary_max;
    if line.chars().count() <= max {
        return line.to_owned();
    }
    let cut: String = line.chars().take(max - 1).collect();
    format!("{}…", pystr::rstrip(&cut))
}

/// The comments of an issue, oldest first, from `since` on, that instant included. A worker's is shortened.
pub fn comments(raw: Vec<RawComment>, since: Option<DateTime<FixedOffset>>) -> Result<Vec<Comment>, InstantError> {
    let mut out = Vec::new();
    for comment in raw {
        if let Some(since) = since
            && parse_instant(&comment.created_at)? < since
        {
            continue;
        }
        let role = signature_role(&comment.body).unwrap_or_else(|| signature().owner.clone());
        let shortened = role == signature().worker;
        let body = if shortened { summary_line(&comment.body) } else { without_signature(&comment.body) };
        out.push(Comment { id: comment.id, created_at: comment.created_at, role, url: comment.url, body, shortened });
    }
    Ok(out)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("not an ISO 8601 instant: `{0}`")]
pub struct InstantError(pub String);

/// An ISO 8601 instant as the prototype reads one (`_istante`): a date, or a date and a time after one
/// character, with or without an offset; `Z` is `+00:00`, and without an offset it is UTC. The prototype's
/// reader takes some forms this does not: the basic ones with no separators, and week dates.
pub fn parse_instant(text: &str) -> Result<DateTime<FixedOffset>, InstantError> {
    let error = || InstantError(text.to_owned());
    let written = pystr::strip(text).replace('Z', "+00:00");
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let number = |part: &str| if digits(part) { part.parse::<u32>().ok() } else { None };
    let date = written.get(..10).filter(|date| date.is_ascii()).ok_or_else(error)?;
    let [year, month, day] = [&date[..4], &date[5..7], &date[8..]];
    if date.as_bytes()[4] != b'-' || date.as_bytes()[7] != b'-' || [year, month, day].iter().any(|part| !digits(part)) {
        return Err(error());
    }
    let date = NaiveDate::from_ymd_opt(year.parse().map_err(|_| error())?, number(month).ok_or_else(error)?, number(day).ok_or_else(error)?)
        .ok_or_else(error)?;
    let mut rest = written[10..].chars();
    if rest.next().is_none() {
        return Ok(FixedOffset::east_opt(0).expect("UTC").from_utc_datetime(&date.and_time(NaiveTime::MIN)));
    }
    let rest = rest.as_str();
    let (time, offset) = match rest.find(['+', '-']) {
        Some(at) => (&rest[..at], Some(&rest[at..])),
        None => (rest, None),
    };
    let (clock, fraction) = time.split_once(['.', ',']).map_or((time, None), |(clock, fraction)| (clock, Some(fraction)));
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|part| part.len() != 2) || (fraction.is_some() && parts.len() < 3) {
        return Err(error());
    }
    let field = |i: usize| parts.get(i).map_or(Some(0), |part| number(part));
    let nanos = match fraction {
        None => 0,
        Some(fraction) if digits(fraction) => {
            let padded: String = fraction.chars().chain(std::iter::repeat('0')).take(9).collect();
            padded.parse::<u32>().map_err(|_| error())?
        }
        Some(_) => return Err(error()),
    };
    let time = NaiveTime::from_hms_nano_opt(field(0).ok_or_else(error)?, field(1).ok_or_else(error)?, field(2).ok_or_else(error)?, nanos)
        .ok_or_else(error)?;
    let seconds = match offset {
        None => 0,
        Some(offset) => {
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let body = &offset[1..];
            let parts: Vec<&str> = if body.contains(':') { body.split(':').collect() } else { body.as_bytes().chunks(2).map(|chunk| std::str::from_utf8(chunk).unwrap_or_default()).collect() };
            if parts.is_empty() || parts.len() > 3 || parts.iter().any(|part| part.len() != 2) {
                return Err(error());
            }
            let values: Option<Vec<i32>> = parts.iter().map(|part| number(part).and_then(|value| i32::try_from(value).ok())).collect();
            let values = values.ok_or_else(error)?;
            sign * (values[0] * 3600 + values.get(1).copied().unwrap_or(0) * 60 + values.get(2).copied().unwrap_or(0))
        }
    };
    let zone = FixedOffset::east_opt(seconds).ok_or_else(error)?;
    zone.from_local_datetime(&date.and_time(time)).single().ok_or_else(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> SignatureFile {
        let file: File = toml::from_str(crate::v1::VOCABULARY).unwrap();
        file.signature
    }

    /// A body signed as the CLI signs it (`_firma_commento`).
    fn signed(body: &str, role: &str) -> String {
        let words = words();
        let visible = if role == words.owner { words.visible_owner } else { format!("{} {role}", words.visible_role_prefix) };
        format!("{body}\n\n— {visible}\n<!-- {} {}={role} id=s-1 -->", words.marker, words.role_key)
    }

    #[test]
    fn the_role_is_the_marker_s_and_the_signature_comes_off() {
        let worker = words().worker;
        let body = signed("A question.\n", &worker);
        assert_eq!(signature_role(&body).as_deref(), Some(worker.as_str()));
        assert_eq!(signature_role(&format!("{body}\n\n  \n")).as_deref(), Some(worker.as_str()), "blank lines after it do not count");
        assert_eq!(without_signature(&body), "A question.");
        assert_eq!(signature_role("Written on the site."), None);
        assert_eq!(without_signature("  Written on the site.\n"), "Written on the site.");
        let owner = words().owner;
        assert_eq!(without_signature(&signed("A.", &owner)), "A.");
    }

    #[test]
    fn a_worker_s_report_reads_as_its_summary() {
        let key = words().summary_key;
        assert_eq!(summary_line(&format!("---\ntype: done\n{key}: \"One line.\"\n---\n\nBody.")), "One line.");
        // As the prototype gives it: with an empty summary, the first line after the fence, the key's own.
        assert_eq!(summary_line(&format!("---\n{key}:\n---\nFirst.")), format!("{key}:"));
        assert_eq!(summary_line("---\n---\nFirst."), "First.");
        assert_eq!(summary_line("\n\n  Plain first line  \nsecond"), "Plain first line");
        let long = "x".repeat(300);
        let cut = summary_line(&long);
        assert_eq!((cut.chars().count(), cut.ends_with('…')), (240, true));
    }

    #[test]
    fn instants_read_as_the_prototype_reads_them() {
        let utc = |text: &str| parse_instant(text).map(|instant| instant.to_rfc3339());
        assert_eq!(utc("2026-11-03T01:04:00Z").unwrap(), "2026-11-03T01:04:00+00:00");
        assert_eq!(utc(" 2026-11-03 ").unwrap(), "2026-11-03T00:00:00+00:00");
        assert_eq!(utc("2026-11-03T01:04").unwrap(), "2026-11-03T01:04:00+00:00");
        assert_eq!(utc("2026-11-03 01:04:05.5+01:30").unwrap(), "2026-11-03T01:04:05.500+01:30");
        for wrong in ["", "2026-11", "03/11/2026", "2026-11-03T1:04", "2026-13-01", "2026-11-03Tnoon"] {
            assert!(parse_instant(wrong).is_err(), "{wrong}");
        }
    }
}
