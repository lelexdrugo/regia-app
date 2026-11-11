//! Links written by others, checked before anyone can follow one (threat model §3.7 point 6). A view gets
//! no `href`: it shows a button with the link's text and its host, and opening it is the core's job
//! (`open_external_link`, threat model §3.4), which checks the URL again with this same function.

use serde::Serialize;
use url::Url;

/// The longest URL the core opens.
pub const URL_LIMIT: usize = 2048;

/// The one host opened without a native confirmation (threat model §3.7 point 6, §3.8).
pub const TRUSTED_HOST: &str = "github.com";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkTarget {
    /// An https URL the core may open. `host` is in ASCII, punycode for an international name, so a
    /// homograph shows as what it is. `confirm` is true for every host but github.com on its own port.
    External { url: String, host: String, confirm: bool },
    /// Shown as text and never opened.
    Refused { reason: LinkRefusal },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LinkRefusal {
    /// Not an absolute URL with a host.
    Invalid,
    /// Any scheme but https: `javascript:`, `data:`, `file:`, `http:`, or one the system maps to a program.
    Scheme,
    /// A `user:password@` part.
    Credentials,
    /// Longer than [`URL_LIMIT`], as written or once parsed.
    TooLong,
}

pub fn check_link(written: &str) -> LinkTarget {
    let refused = |reason| LinkTarget::Refused { reason };
    if written.len() > URL_LIMIT {
        return refused(LinkRefusal::TooLong);
    }
    let Ok(url) = Url::parse(written) else {
        return refused(LinkRefusal::Invalid);
    };
    if url.scheme() != "https" {
        return refused(LinkRefusal::Scheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return refused(LinkRefusal::Credentials);
    }
    let Some(host) = url.host_str().filter(|host| !host.is_empty()) else {
        return refused(LinkRefusal::Invalid);
    };
    if url.as_str().len() > URL_LIMIT {
        return refused(LinkRefusal::TooLong);
    }
    let confirm = host != TRUSTED_HOST || url.port().is_some();
    LinkTarget::External { url: url.as_str().to_owned(), host: host.to_owned(), confirm }
}

/// The host an image would have loaded from, for its placeholder; none for `data:` and the like.
pub fn image_host(written: &str) -> Option<String> {
    let url = Url::parse(written).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.host_str().map(str::to_owned)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(written: &str) -> Option<LinkRefusal> {
        match check_link(written) {
            LinkTarget::Refused { reason } => Some(reason),
            LinkTarget::External { .. } => None,
        }
    }

    #[test]
    fn only_https_without_credentials_and_with_a_host_opens() {
        let cases = [
            ("javascript:window.__pwned=1", LinkRefusal::Scheme),
            ("JaVaScRiPt:alert(1)", LinkRefusal::Scheme),
            ("data:text/html;base64,PHNjcmlwdD4=", LinkRefusal::Scheme),
            ("file:///etc/passwd", LinkRefusal::Scheme),
            ("http://example.com/", LinkRefusal::Scheme),
            ("ms-settings:", LinkRefusal::Scheme),
            ("mailto:someone@example.invalid", LinkRefusal::Scheme),
            ("vbscript:msgbox(1)", LinkRefusal::Scheme),
            ("https://user:secret@github.com/", LinkRefusal::Credentials),
            ("https://token@github.com/", LinkRefusal::Credentials),
            ("#123", LinkRefusal::Invalid),
            ("/relative/path", LinkRefusal::Invalid),
            ("", LinkRefusal::Invalid),
        ];
        for (written, reason) in cases {
            assert_eq!(refusal(written), Some(reason), "{written}");
        }
        let long = format!("https://github.com/{}", "a".repeat(URL_LIMIT));
        assert_eq!(refusal(&long), Some(LinkRefusal::TooLong));
    }

    #[test]
    fn the_host_is_ascii_and_only_github_skips_the_confirmation() {
        assert_eq!(
            check_link("https://github.com/owner/repo/issues/1"),
            LinkTarget::External { url: "https://github.com/owner/repo/issues/1".into(), host: "github.com".into(), confirm: false }
        );
        assert_eq!(check_link("https://GitHub.com:443/x"), check_link("https://github.com/x"), "default port and case");
        for (written, host) in [
            ("https://github.com:8443/", "github.com"),
            ("https://gist.github.com/", "gist.github.com"),
            ("https://github.com.example.invalid/", "github.com.example.invalid"),
            // A Cyrillic і in place of the Latin i.
            ("https://g\u{456}thub.com/", "xn--gthub-n2e.com"),
            ("https://[::1]/", "[::1]"),
        ] {
            match check_link(written) {
                LinkTarget::External { host: got, confirm, .. } => {
                    assert_eq!(got, host, "{written}");
                    assert!(confirm, "{written} must ask first");
                }
                other => panic!("{written}: {other:?}"),
            }
        }
    }

    #[test]
    fn an_image_has_a_host_only_over_http() {
        assert_eq!(image_host("https://attacker.invalid/?d=1").as_deref(), Some("attacker.invalid"));
        assert_eq!(image_host("data:image/png;base64,AAAA"), None);
        assert_eq!(image_host("x.png"), None);
    }
}
