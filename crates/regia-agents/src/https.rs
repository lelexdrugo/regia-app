//! The product's transport for the usage probes: one GET over the system's TLS and its roots
//! (schannel, Security.framework, OpenSSL), and nothing the probes do not need (threat model §3.12).
//!
//! - https only, and no redirect followed: the bearer cannot follow a 3xx to another host, as it
//!   would with the prototype's `urllib`, which keeps the header across a redirect;
//! - 15 s for the whole call, the prototype's `TIMEOUT`;
//! - an answer read to at most 1 MiB, and only its body: no header of the answer is kept;
//! - the token goes only in `Authorization`, and no error carries the request.

use std::io::ErrorKind;
use std::time::Duration;

use regia_identity::PRODUCT_NAME;
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::usage::{Bearer, Transport, TransportError};

/// The longest a probe's call lasts, end to end.
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// The largest answer read: the real ones are a few hundred bytes.
const BODY_LIMIT: u64 = 1 << 20;

/// GETs over the system's TLS.
pub struct Https {
    agent: Agent,
}

impl Default for Https {
    fn default() -> Https {
        Https::new()
    }
}

impl Https {
    #[must_use]
    pub fn new() -> Https {
        Https::configured(true, TIMEOUT)
    }

    fn configured(https_only: bool, timeout: Duration) -> Https {
        let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
        let config = Agent::config_builder()
            .https_only(https_only)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .user_agent(format!("{PRODUCT_NAME}-usage/{}", env!("CARGO_PKG_VERSION")))
            .tls_config(tls)
            .build();
        Https { agent: config.into() }
    }
}

impl Transport for Https {
    fn get(&self, url: &str, bearer: &Bearer, headers: &[(&'static str, String)]) -> Result<Vec<u8>, TransportError> {
        let mut request = self
            .agent
            .get(url)
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {}", bearer.expose()));
        for (name, value) in headers {
            request = request.header(*name, value.as_str());
        }
        let mut answer = request.call().map_err(failure)?;
        let status = answer.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(TransportError::Status(status));
        }
        answer.body_mut().with_config().limit(BODY_LIMIT).read_to_vec().map_err(|error| match failure(error) {
            TransportError::Timeout => TransportError::Timeout,
            _ => TransportError::Invalid,
        })
    }
}

/// The kind of a failure, and nothing of the request.
fn failure(error: ureq::Error) -> TransportError {
    match error {
        ureq::Error::StatusCode(code) => TransportError::Status(code),
        ureq::Error::Timeout(_) => TransportError::Timeout,
        ureq::Error::Io(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => TransportError::Timeout,
        ureq::Error::BodyExceedsLimit(_) => TransportError::Invalid,
        _ => TransportError::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    const CANARY: &str = "canary-bearer-7f3a-not-a-real-token";

    /// A local server over plain HTTP that answers each connection with one of `answers` and sends
    /// back what it received. The probes' real endpoints are never asked.
    fn server(answers: Vec<&'static str>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let (sent, received) = mpsc::channel();
        thread::spawn(move || {
            // A silent answer keeps its connection open until every answer is given.
            let mut held = Vec::new();
            for answer in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                }
                sent.send(request).unwrap();
                let mut stream = stream;
                if answer.is_empty() {
                    held.push(stream);
                } else {
                    stream.write_all(answer.as_bytes()).unwrap();
                }
            }
        });
        (address, received)
    }

    fn plain(timeout: Duration) -> Https {
        Https::configured(false, timeout)
    }

    #[test]
    fn the_token_goes_only_in_its_header_and_the_status_maps_to_a_kind() {
        let ok = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
        let refused = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let limited = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let moved = "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (address, requests) = server(vec![ok, refused, limited, moved]);
        let https = plain(Duration::from_secs(5));
        let bearer = Bearer(CANARY.to_owned());
        let url = format!("{address}/api/oauth/usage");
        assert_eq!(https.get(&url, &bearer, &[("anthropic-beta", "oauth-2025-04-20".to_owned())]).unwrap(), b"{}");
        let first = requests.recv().unwrap().to_ascii_lowercase();
        assert!(first.starts_with("get /api/oauth/usage http/1.1"), "{first}");
        assert!(first.contains(&format!("authorization: bearer {CANARY}")), "{first}");
        assert!(first.contains("anthropic-beta: oauth-2025-04-20") && first.contains("accept: application/json"), "{first}");
        assert!(first.contains(&format!("user-agent: {PRODUCT_NAME}-usage/")), "{first}");
        for expected in [TransportError::Status(401), TransportError::Status(429), TransportError::Status(302)] {
            let error = https.get(&url, &bearer, &[]).unwrap_err();
            assert_eq!(error, expected);
            assert!(!format!("{error:?}").contains(CANARY));
            requests.recv().unwrap();
        }
        assert!(requests.try_recv().is_err(), "the redirect was not followed");
    }

    #[test]
    fn a_silent_server_is_a_timeout_and_a_bad_body_is_invalid() {
        let (address, _requests) = server(vec!["", "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n{"]);
        let https = plain(Duration::from_millis(300));
        let bearer = Bearer(CANARY.to_owned());
        assert_eq!(https.get(&address, &bearer, &[]), Err(TransportError::Timeout));
        assert!(matches!(https.get(&address, &bearer, &[]), Err(TransportError::Invalid | TransportError::Timeout)));
        // Windows retries a refused connection for about two seconds before it fails.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        assert_eq!(plain(Duration::from_secs(10)).get(&format!("http://{closed}"), &bearer, &[]), Err(TransportError::Unreachable));
    }

    #[test]
    fn plain_http_is_refused_by_the_product() {
        let (address, _requests) = server(vec![]);
        assert_eq!(Https::new().get(&address, &Bearer(CANARY.to_owned()), &[]), Err(TransportError::Unreachable));
    }

    /// A real handshake on the system's TLS and roots: one GET to GitHub, with a bearer that is no
    /// token. Any status proves both; neither probe's endpoint is asked. By hand, with the network:
    /// `cargo test -p regia-agents --lib https -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn the_system_tls_shakes_hands_with_github() {
        let answer = Https::new().get("https://api.github.com/", &Bearer(CANARY.to_owned()), &[]).map(|body| body.len());
        eprintln!("GitHub answered {answer:?}");
        assert!(matches!(answer, Ok(_) | Err(TransportError::Status(_))), "{answer:?}");
    }
}
