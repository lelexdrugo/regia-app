//! The comments of an issue, as `task show` gives them (#423 of the dossier): the role each one's signature
//! carries, the owner's for one with none, the body without the signature, and a worker's report as one
//! line, since the report itself is in the dossier. The signature's words are data of format v1, in
//! `formats/tracker_v1.toml`; this is their one reader (M2 of the conformance study), and the one writer
//! of the signature `comment` puts on what it posts (M1, M3, M5).

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
    session_key: String,
    no_session: String,
    owner: String,
    orchestrator: String,
    worker: String,
    chat: String,
    visible_role_prefix: String,
    visible_owner: String,
    summary_key: String,
    summary_max: usize,
}

struct Signature {
    marker: Regex,
    visible: Regex,
    summary: Regex,
    /// A character a session's id cannot keep in the marker: `[^\w.:-]`.
    outside_session: Regex,
    /// The end of what `gh issue comment` prints: the URL of the comment, with its id.
    comment_url_id: Regex,
    words: SignatureFile,
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
            outside_session: Regex::new(&format!(r"[^{word}.:-]")).expect("a valid pattern"),
            comment_url_id: Regex::new(&format!(r"#issuecomment-([0-9]+){space}*$")).expect("a valid pattern"),
            words,
        }
    })
}

/// Who a comment of the CLI is signed by (M3 of the conformance study). For the precheck only the owner's
/// is an event: every other role is the regia's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureRole {
    /// A decision of the owner that a chat relays.
    Owner,
    /// The session whose step of the issue's project is open.
    Orchestrator,
    Worker,
    Chat,
}

impl SignatureRole {
    pub const ALL: [SignatureRole; 4] = [SignatureRole::Owner, SignatureRole::Orchestrator, SignatureRole::Worker, SignatureRole::Chat];
    /// The roles a caller names by hand; the other two are decided (`RUOLI_FIRMA`).
    pub const BY_HAND: [SignatureRole; 2] = [SignatureRole::Worker, SignatureRole::Chat];

    /// The word the marker writes: data of format v1.
    #[must_use]
    pub fn word(self) -> &'static str {
        let words = &signature().words;
        match self {
            SignatureRole::Owner => &words.owner,
            SignatureRole::Orchestrator => &words.orchestrator,
            SignatureRole::Worker => &words.worker,
            SignatureRole::Chat => &words.chat,
        }
    }

    /// The role a word names.
    #[must_use]
    pub fn from_word(word: &str) -> Option<SignatureRole> {
        SignatureRole::ALL.into_iter().find(|role| role.word() == word)
    }

    /// The role of a comment (`_ruolo_commento`), in this order: the owner's when a chat relays the owner's
    /// decision; the orchestrator's when the step open on the issue's project is this session's; the role
    /// given; a chat's.
    #[must_use]
    pub fn of_comment(on_behalf_of_owner: bool, step_of_this_session: bool, given: Option<SignatureRole>) -> SignatureRole {
        if on_behalf_of_owner {
            SignatureRole::Owner
        } else if step_of_this_session {
            SignatureRole::Orchestrator
        } else {
            given.unwrap_or(SignatureRole::Chat)
        }
    }
}

/// `body` signed by `role` for `session` (M1 of the conformance study, `_firma_commento`): the body without
/// its trailing blanks, a blank line, the visible line, and last the marker with the session's id, each
/// character outside `[\w.:-]` as `_`, or the format's word for no session. A body whose last line that is
/// not blank is already a marker that reads is returned as it is, whoever signed it: signing twice is
/// signing once, and the precheck reads one role.
#[must_use]
pub fn sign(body: &str, role: SignatureRole, session: Option<&str>) -> String {
    if signature_role(body).is_some() {
        return body.to_owned();
    }
    let signature = signature();
    let words = &signature.words;
    let visible = match role {
        SignatureRole::Owner => words.visible_owner.clone(),
        other => format!("{} {}", words.visible_role_prefix, other.word()),
    };
    let session = session.filter(|session| !session.is_empty()).unwrap_or(&words.no_session);
    let session = signature.outside_session.replace_all(session, "_");
    format!(
        "{}\n\n— {visible}\n<!-- {} {}={} {}={session} -->",
        pystr::rstrip(body),
        words.marker,
        words.role_key,
        role.word(),
        words.session_key
    )
}

/// The id of the comment whose URL `gh issue comment` printed (M5 of the conformance study): the digits
/// after `#issuecomment-` at the end of the last line of what it printed. `None` when the line does not
/// end so: the comment exists all the same, only its id is unknown.
#[must_use]
pub fn comment_id(printed: &str) -> Option<u64> {
    let last = pystr::split_lines(pystr::strip(printed)).pop()?;
    signature().comment_url_id.captures(last)?.get(1)?.as_str().parse().ok()
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
    marker_role(last_line(body))
}

/// The role of the author's marker that `line` is, whole; `None` if it is not one. The one reader of the marker's
/// pattern (M2): the checks find the line their own way, on `\n` only (`ruolo_del_commento`).
pub fn marker_role(line: &str) -> Option<String> {
    signature().marker.captures(line).and_then(|captures| captures.get(1)).map(|role| role.as_str().to_owned())
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
    let max = signature().words.summary_max;
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
        let role = signature_role(&comment.body).unwrap_or_else(|| signature().words.owner.clone());
        let shortened = role == signature().words.worker;
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

    /// SplitMix64: the workspace has no property-testing crate, and a fixed seed replays a failure.
    struct Draw(u64);

    impl Draw {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }

        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % n as u64).unwrap()
        }

        fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[self.below(items.len())]
        }
    }

    /// Pieces of a body: Python's blanks and line breaks, the signature's own characters, and markers that
    /// read and that do not, in the format's words.
    fn pieces() -> Vec<String> {
        let w = words();
        let marker = |role: &str, rest: &str| format!("<!-- {} {}={role}{rest} -->", w.marker, w.role_key);
        let mut out: Vec<String> = [
            "a", "Bé", " ", "\t", "\n", "\r", "\r\n", "\u{2028}", "\u{2029}", "\u{85}", "\u{0b}", "\u{0c}", "\u{1c}", "\u{1d}", "\u{1e}",
            "\u{1f}", "\u{a0}", "\u{3000}", "\u{200b}", "—", "·", "<", ">", "-->", "<!--", "=", "_", "x y", "日本", "`",
        ]
        .map(str::to_owned)
        .to_vec();
        out.extend([
            marker(&w.worker, &format!(" {}=a", w.session_key)),
            marker(&w.chat, ""),
            marker("", ""),
            format!("<!--{}  {}={}-->", w.marker, w.role_key, w.owner),
            format!("<!-- {} {}={}", w.marker, w.role_key, w.worker),
            format!("— {} {}", w.visible_role_prefix, w.chat),
            format!("— {}", w.visible_owner),
        ]);
        out
    }

    fn body(draw: &mut Draw, pieces: &[String]) -> String {
        (0..draw.below(9)).map(|_| draw.pick(pieces).clone()).collect()
    }

    const SESSIONS: [Option<&str>; 10] =
        [None, Some(""), Some("s-1"), Some("a b"), Some("../x:y"), Some("ü日_9"), Some("-->"), Some("a\u{2028}b"), Some("<x>"), Some("x.y:z-w")];

    /// P of the spec: signing twice is signing once, whatever the second role and session, and a body
    /// without a marker that reads reads back the role it was signed with, and comes back without the
    /// signature as `task show` gives it. A body with one is left as it is. Blank lines after the marker do
    /// not count.
    #[test]
    fn signing_is_idempotent_and_reads_back_its_role() {
        let (pieces, mut draw) = (pieces(), Draw(445));
        let (mut unsigned, mut signed_already) = (0, 0);
        for _ in 0..4000 {
            let body = body(&mut draw, &pieces);
            let (role, other) = (*draw.pick(&SignatureRole::ALL), *draw.pick(&SignatureRole::ALL));
            let (session, other_session) = (*draw.pick(&SESSIONS), *draw.pick(&SESSIONS));
            let once = sign(&body, role, session);
            assert_eq!(sign(&once, other, other_session), once, "{body:?}");
            if signature_role(&body).is_some() {
                signed_already += 1;
                assert_eq!(once, body, "a body with a marker is not signed again: {body:?}");
                continue;
            }
            unsigned += 1;
            assert_eq!(signature_role(&once).as_deref(), Some(role.word()), "{body:?}");
            let lines = pystr::split_lines(pystr::rstrip(&body)).join("\n");
            assert_eq!(without_signature(&once), pystr::strip(&lines), "{body:?}");
            for blanks in ["\n", "\n \t\n", "\u{2028}\u{1f}", "\r\n\r\n"] {
                assert_eq!(signature_role(&format!("{once}{blanks}")).as_deref(), Some(role.word()), "{blanks:?}");
            }
        }
        assert!(unsigned > 1000 && signed_already > 100, "the generator covers both: {unsigned}, {signed_already}");
    }

    /// The marker keeps `[\w.:-]` of the session's id and writes every other character as `_`; no session,
    /// or an empty one, is the format's word.
    #[test]
    fn the_session_s_id_is_written_safe() {
        let w = words();
        let marker = |session: Option<&str>| sign("x", SignatureRole::Chat, session).lines().last().unwrap().to_owned();
        let expected = |id: &str| format!("<!-- {} {}={} {}={id} -->", w.marker, w.role_key, w.chat, w.session_key);
        assert_eq!(marker(Some("a b/c>d")), expected("a_b_c_d"));
        assert_eq!(marker(Some("ü日_9.x:y-z")), expected("ü日_9.x:y-z"));
        assert_eq!(marker(Some("a\u{2028}b")), expected("a_b"));
        assert_eq!(marker(None), expected(&w.no_session));
        assert_eq!(marker(Some("")), expected(&w.no_session));
    }

    /// The visible line: the owner's own words, the others with the prefix and the role.
    #[test]
    fn the_visible_line_names_the_role() {
        let w = words();
        let visible = |role| sign("x  \n\n", role, Some("s")).lines().nth(2).unwrap().to_owned();
        assert_eq!(visible(SignatureRole::Owner), format!("— {}", w.visible_owner));
        assert_eq!(visible(SignatureRole::Worker), format!("— {} {}", w.visible_role_prefix, w.worker));
        assert!(sign("x  \n\n", SignatureRole::Chat, None).starts_with("x\n\n— "), "trailing blanks go");
        for role in SignatureRole::ALL {
            assert_eq!(SignatureRole::from_word(role.word()), Some(role));
        }
        assert_eq!(SignatureRole::from_word("nobody"), None);
    }

    /// M3: the owner's flag wins over a step of this session, which wins over the role given.
    #[test]
    fn the_role_of_a_comment_follows_the_prototype_s_order() {
        use SignatureRole::{Chat, Orchestrator, Owner, Worker};
        assert_eq!(SignatureRole::of_comment(true, true, Some(Worker)), Owner);
        assert_eq!(SignatureRole::of_comment(false, true, Some(Worker)), Orchestrator);
        assert_eq!(SignatureRole::of_comment(false, false, Some(Worker)), Worker);
        assert_eq!(SignatureRole::of_comment(false, false, None), Chat);
        assert_eq!(SignatureRole::BY_HAND, [Worker, Chat]);
    }

    /// M5: the id ends the last line `gh` printed; anything else is no id, and not an error.
    #[test]
    fn the_comment_s_id_is_the_end_of_its_url() {
        assert_eq!(comment_id("https://tracker.invalid/issues/5#issuecomment-9202\n"), Some(9202));
        assert_eq!(comment_id("a notice\nhttps://x/issues/5#issuecomment-7  \n\n"), Some(7));
        for none in ["", "\n", "https://x/issues/5", "https://x/issues/5#issuecomment-", "#issuecomment-7 later", "#issuecomment-7\nhttps://x/5"] {
            assert_eq!(comment_id(none), None, "{none:?}");
        }
        assert_eq!(comment_id("#issuecomment-99999999999999999999999"), None, "out of range");
    }
}
