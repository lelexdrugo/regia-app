//! The usage probes (ADR 0001 §6, "probe usage"; F065 and F122 of the dossier's inventory): what is
//! left of Claude's and Codex's windows, read from two endpoints that are not documented, with the
//! sign-in tokens the two CLIs keep in their files. A token is read into this process and sent only
//! as the bearer of its own provider's request; it never reaches a reading, a reason, a log or a file
//! (threat model §3.12, V19). Nothing is renewed: a token that expired stays so until its CLI runs.
//!
//! **Only with the user's consent** (the owner's decision on #396). With the probes refused, nothing
//! here opens a credentials file or asks an endpoint, and every provider reads
//! [`UsageStatus::ProbeOff`], which is not [`UsageStatus::Unknown`]: a refused probe is a state, not
//! a failure. The only value that holds the credentials' paths and the transport is built from
//! [`ProbeConsent::Granted`].
//!
//! Every failure is a reading with its reason, never an error: the endpoints can change without
//! notice, and an answer the probe does not expect reads unknown.

use std::path::PathBuf;
use std::sync::Arc;

use regia_identity::prototype_env_var;
use regia_store::{Clock, Precision, ProviderUsage, Reading, SystemClock, TokenState, TokenStatus, WallTime, Window};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{AgentService, Env, LaunchSurface, ServiceId, WakeOutcome};

/// The user's answer, at setup, to the usage probes (ADR 0001 §6). Without an answer there is no
/// consent: see [`crate::Settings::probe_consent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeConsent {
    Granted,
    Refused,
}

/// Claude's endpoint, not documented: the one Claude Code reads its own usage from.
pub const CLAUDE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Codex's endpoint, not documented.
pub const CODEX_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// Below this, Codex's token is about to expire and the reading says so: one day, the prototype's
/// `SOGLIA_TOKEN_S` (#314 of the dossier).
pub const TOKEN_WARNING_S: i64 = 24 * 3600;

/// The windows, by name and length in seconds.
const WINDOWS: [(&str, f64); 2] = [("5h", 5.0 * 3600.0), ("7d", 7.0 * 24.0 * 3600.0)];

/// The reason of every provider when the user refused the probes.
pub const PROBES_OFF: &str = "the usage probes are off: the user did not consent to them";
/// Where macOS keeps Claude Code's sign-in is not proven, so the probe does not look there.
pub const MACOS_CLAUDE: &str = "where Claude Code keeps its credentials on macOS is not proven: the probe does not read them";

/// Why a GET gave no body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// The endpoint answered with this status, outside 2xx. A redirect is one too: none is followed.
    Status(u16),
    Timeout,
    /// No connection: the name, the network or TLS.
    Unreachable,
    /// The answer could not be read whole.
    Invalid,
}

/// The port the probes ask the endpoints through. The product's is [`crate::Https`]; a test
/// answers from recorded responses and never reaches a real endpoint.
pub trait Transport: Send + Sync {
    /// One GET of `url` with `bearer` as its token and `headers` besides: the body of a 2xx answer.
    /// The bearer must not appear in any error.
    fn get(&self, url: &str, bearer: &Bearer, headers: &[(&'static str, String)]) -> Result<Vec<u8>, TransportError>;
}

/// A sign-in token on its way to its endpoint. It does not print: its `Debug` hides it, and it has
/// no `Display`.
pub struct Bearer(pub(crate) String);

impl Bearer {
    /// The token, for the `Authorization` header and nothing else.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Bearer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bearer(<hidden>)")
    }
}

/// The system the probes run on: the place of Claude's credentials depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Other,
}

impl Platform {
    #[must_use]
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") { Platform::MacOs } else { Platform::Other }
    }

    /// Whether the probes read Claude's windows and token here, with the consent. On macOS they do not:
    /// where Claude Code keeps its credentials is not proven there ([`MACOS_CLAUDE`]). This is the one
    /// place the rule lives: the probes ask it, and so does whoever must know what they never read, such
    /// as the budget gate of the controller, which then does not trust what the cache holds of Claude.
    #[must_use]
    pub fn probes_read_claude(self) -> bool {
        self != Platform::MacOs
    }
}

/// The usage probes, with the user's consent or without it.
pub struct UsageProbes {
    clock: Arc<dyn Clock>,
    granted: Option<Granted>,
}

/// What only a consent builds: the environment the credentials are found by, and the transport.
struct Granted {
    env: Env,
    transport: Box<dyn Transport>,
    platform: Platform,
}

impl UsageProbes {
    /// The probes as `consent` allows them. With [`ProbeConsent::Refused`] the environment and the
    /// transport are dropped here, unused.
    pub fn new(consent: ProbeConsent, env: Env, transport: impl Transport + 'static) -> UsageProbes {
        let granted = (consent == ProbeConsent::Granted).then(|| Granted { env, transport: Box::new(transport), platform: Platform::current() });
        UsageProbes { clock: Arc::new(SystemClock), granted }
    }

    /// Reads time, and the local offset the readings are written in, from `clock`.
    #[must_use]
    pub fn with_clock(mut self, clock: impl Clock + 'static) -> UsageProbes {
        self.clock = Arc::new(clock);
        self
    }

    /// As if on `platform`, for the tests of another system's rule.
    #[must_use]
    pub fn on_platform(mut self, platform: Platform) -> UsageProbes {
        if let Some(granted) = &mut self.granted {
            granted.platform = platform;
        }
        self
    }

    #[must_use]
    pub fn consent(&self) -> ProbeConsent {
        if self.granted.is_some() { ProbeConsent::Granted } else { ProbeConsent::Refused }
    }

    /// Both providers, now: the prototype's `sonda()`.
    #[must_use]
    pub fn read(&self) -> Reading {
        let now = self.clock.now();
        Reading {
            generated: Some(self.local(now, Precision::Seconds)),
            providers: vec![("claude".to_owned(), self.claude()), ("codex".to_owned(), self.codex())],
            cache: None,
        }
    }

    /// Claude's windows, from `claudeAiOauth` in Claude Code's credentials.
    #[must_use]
    pub fn claude(&self) -> ProviderUsage {
        let provider = ServiceId::Claude.id();
        let Some(granted) = &self.granted else { return ProviderUsage::probe_off(provider, PROBES_OFF) };
        if !granted.platform.probes_read_claude() {
            return ProviderUsage::unknown(provider, None, MACOS_CLAUDE, None);
        }
        let doc = match read_json(credentials(&granted.env, ServiceId::Claude), "Claude") {
            Ok(doc) => doc,
            Err(reason) => return ProviderUsage::unknown(provider, None, reason, None),
        };
        let oauth = doc.get("claudeAiOauth").and_then(Value::as_object);
        let Some((oauth, bearer)) = oauth.and_then(|oauth| Some((oauth, Bearer(text(oauth, "accessToken")?)))) else {
            return ProviderUsage::unknown(provider, None, "Claude's OAuth token is absent", None);
        };
        let plan = text(oauth, "subscriptionType");
        let now = self.clock.now();
        if oauth.get("expiresAt").and_then(number).is_some_and(|ms| ms / 1000.0 <= now.unix_secs_f64()) {
            return ProviderUsage::unknown(provider, plan, "token expired: Claude Code renews it, the probe does not", None);
        }
        let headers = [("anthropic-beta", "oauth-2025-04-20".to_owned())];
        let answer = match granted.transport.get(CLAUDE_URL, &bearer, &headers) {
            Ok(body) => body,
            Err(error) => return ProviderUsage::unknown(provider, plan, transport_reason(error), None),
        };
        drop(bearer);
        let Some(data) = json_object(&answer) else {
            return ProviderUsage::unknown(provider, plan, answer_reason(&answer), None);
        };
        let windows = vec![
            ("5h".to_owned(), self.claude_window(data.get("five_hour"), now)),
            ("7d".to_owned(), self.claude_window(data.get("seven_day"), now)),
        ];
        ProviderUsage::new(provider, plan, windows, None, None)
    }

    /// Codex's windows, from `tokens` in its `auth.json`. Its token's expiry comes from the token
    /// itself, decoded and never verified or sent elsewhere: past it, the endpoint is not asked.
    #[must_use]
    pub fn codex(&self) -> ProviderUsage {
        let provider = ServiceId::Codex.id();
        let Some(granted) = &self.granted else { return ProviderUsage::probe_off(provider, PROBES_OFF) };
        let doc = match read_json(credentials(&granted.env, ServiceId::Codex), "Codex") {
            Ok(doc) => doc,
            Err(reason) => return ProviderUsage::unknown(provider, None, reason, None),
        };
        let tokens = doc.get("tokens").and_then(Value::as_object);
        let Some((tokens, bearer)) = tokens.and_then(|tokens| Some((tokens, Bearer(text(tokens, "access_token")?)))) else {
            return ProviderUsage::unknown(provider, None, "Codex's token is absent", None);
        };
        let now = self.clock.now();
        let token = self.token_state(jwt_expiry(bearer.expose()), now);
        if token.status == TokenStatus::Expired {
            let reason = format!(
                "token expired on {}: Codex renews it at its next request (run Codex), the probe does not",
                self.when(token.expires_at.as_deref())
            );
            return ProviderUsage::unknown(provider, None, reason, Some(token));
        }
        let mut headers = Vec::new();
        if let Some(account) = tokens.get("account_id").filter(|account| truthy(account)) {
            headers.push(("ChatGPT-Account-Id", python_str(account)));
        }
        let answer = match granted.transport.get(CODEX_URL, &bearer, &headers) {
            Ok(body) => body,
            Err(TransportError::Status(code @ (401 | 403))) => {
                let reason = if token.expires_at.is_some() {
                    format!(
                        "token refused (HTTP {code}) although its expiry has not passed (it expires on {}): revoked or invalid, Codex may need a new sign-in; the probe does not renew it",
                        self.when(token.expires_at.as_deref())
                    )
                } else {
                    refused_reason(code)
                };
                let refused = TokenState { status: TokenStatus::Refused, http_status: Some(code), ..token };
                return ProviderUsage::unknown(provider, None, reason, Some(refused));
            }
            Err(error) => return ProviderUsage::unknown(provider, None, transport_reason(error), Some(token)),
        };
        drop(bearer);
        let Some(data) = json_object(&answer) else {
            return ProviderUsage::unknown(provider, None, answer_reason(&answer), Some(token));
        };
        let plan = text(&data, "plan_type");
        ProviderUsage::new(provider, plan, self.codex_windows(data.get("rate_limit")), Some(token), None)
    }

    /// When Claude's sign-in token expires, local time to the second: `expiresAt` alone, never the
    /// token. The token wake-up compares it before and after (#190 of the dossier). `None` without
    /// consent, on macOS, or when it cannot be read.
    #[must_use]
    pub fn claude_token_expiry(&self) -> Option<String> {
        let granted = self.granted.as_ref().filter(|granted| granted.platform.probes_read_claude())?;
        let doc = read_json(credentials(&granted.env, ServiceId::Claude), "Claude").ok()?;
        let ms = doc.get("claudeAiOauth")?.as_object()?.get("expiresAt").and_then(number)?;
        WallTime::from_unix_secs_f64(ms / 1000.0).map(|at| self.local(at, Precision::Seconds))
    }

    /// Codex's token from the file alone, no network: its state and expiry, for the brief (#550 of the
    /// dossier). `None` without consent or when the expiry cannot be read.
    #[must_use]
    pub fn codex_token(&self) -> Option<TokenState> {
        let granted = self.granted.as_ref()?;
        let doc = read_json(credentials(&granted.env, ServiceId::Codex), "Codex").ok()?;
        let token = doc.get("tokens")?.as_object()?.get("access_token")?.as_str()?;
        Some(self.token_state(jwt_expiry(token), self.clock.now())).filter(|state| state.expires_at.is_some())
    }

    /// The token wake-up of `surface`, with Claude's expiry read before and after it by
    /// [`UsageProbes::claude_token_expiry`]: the real reader the wake-up composes (#462).
    pub fn wake(&self, surface: &dyn LaunchSurface) -> WakeOutcome {
        WakeOutcome::with_expiry(|| self.claude_token_expiry(), || surface.wake())
    }

    fn claude_window(&self, block: Option<&Value>, now: WallTime) -> Window {
        let Some(block) = block.and_then(Value::as_object) else { return Window::unknown("field missing in the answer") };
        let Some(utilization) = block.get("utilization").filter(|value| !value.is_null()) else {
            return Window::unknown("utilization is null in the answer");
        };
        let unexpected = || Window::unknown("unexpected format of the field");
        let Some(used) = python_float(utilization) else { return unexpected() };
        match block.get("resets_at").filter(|value| truthy(value)) {
            None => Window::read(used, None),
            Some(reset) => match WallTime::parse_iso(&python_str(reset)) {
                Some(at) => Window::read(used, Some((self.local(at, Precision::Minutes), whole_seconds(now, at)))),
                None => unexpected(),
            },
        }
    }

    /// The prototype's `_finestre_codex`: each window by its length, the order of the answer only as
    /// a fallback for a block that does not say its length.
    fn codex_windows(&self, rate_limit: Option<&Value>) -> Vec<(String, Window)> {
        let Some(rate_limit) = rate_limit.and_then(Value::as_object) else {
            return WINDOWS.iter().map(|(name, _)| ((*name).to_owned(), Window::unknown("rate_limit missing in the answer"))).collect();
        };
        let blocks = [rate_limit.get("primary_window"), rate_limit.get("secondary_window")];
        let mut found: Vec<(String, Window)> = Vec::new();
        for block in blocks {
            let length = block.and_then(Value::as_object).and_then(|block| block.get("limit_window_seconds")).and_then(Value::as_f64);
            for (name, seconds) in WINDOWS {
                if length == Some(seconds) && !found.iter().any(|(other, _)| other == name) {
                    found.push((name.to_owned(), self.codex_window(block)));
                }
            }
        }
        for ((name, _), block) in WINDOWS.iter().zip(blocks) {
            if !found.iter().any(|(other, _)| other == name) {
                let without_length = block.and_then(Value::as_object).is_some_and(|block| block.get("limit_window_seconds").is_none_or(Value::is_null));
                let window = if without_length { self.codex_window(block) } else { Window::unknown("window missing from the answer") };
                found.push(((*name).to_owned(), window));
            }
        }
        found
    }

    fn codex_window(&self, block: Option<&Value>) -> Window {
        let block = block.and_then(Value::as_object);
        let field = |name: &str| block.and_then(|block| block.get(name)).filter(|value| !value.is_null());
        let (Some(used), Some(reset)) = (field("used_percent"), field("reset_at")) else {
            return Window::unknown("field missing in the answer");
        };
        match (python_float(used), python_float(reset).and_then(WallTime::from_unix_secs_f64)) {
            (Some(used), Some(at)) => Window::read(used, Some((self.local(at, Precision::Minutes), whole_seconds(self.clock.now(), at)))),
            _ => Window::unknown("unexpected format of the field"),
        }
    }

    /// The prototype's `_token_codex`: the state from the expiry alone, `expires_at` absolute.
    fn token_state(&self, expiry: Option<WallTime>, now: WallTime) -> TokenState {
        let Some(at) = expiry else {
            return TokenState {
                status: TokenStatus::Unknown,
                expires_at: None,
                expires_in_s: None,
                reason: Some("the token's expiry cannot be read".to_owned()),
                http_status: None,
            };
        };
        let left = now.seconds_until(at).trunc() as i64;
        let status = if left <= 0 {
            TokenStatus::Expired
        } else if left < TOKEN_WARNING_S {
            TokenStatus::Expiring
        } else {
            TokenStatus::Valid
        };
        TokenState { status, expires_at: Some(self.local(at, Precision::Seconds)), expires_in_s: Some(left.max(0)), reason: None, http_status: None }
    }

    fn local(&self, at: WallTime, precision: Precision) -> String {
        at.to_local_iso(self.clock.local_offset_s(at), precision)
    }

    /// The day and time of an expiry in a reason.
    fn when(&self, expires_at: Option<&str>) -> String {
        expires_at.and_then(WallTime::parse_iso).map_or_else(|| "?".to_owned(), |at| at.to_local_day_time(self.clock.local_offset_s(at)))
    }
}

/// Where a service's credentials are: the prototype's variable, else the file in the service's folder,
/// which follows its own variable (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) and then the home.
fn credentials(env: &Env, service: ServiceId) -> Option<PathBuf> {
    let variable = match service {
        ServiceId::Claude => prototype_env_var("CLAUDE_CREDENTIALS"),
        ServiceId::Codex => prototype_env_var("CODEX_AUTH"),
    };
    env.get(&variable).map(PathBuf::from).or_else(|| AgentService::known(service).credentials_file(env))
}

/// The prototype's `_leggi_json`: read only, and the reason when it fails. No reason names the path.
fn read_json(path: Option<PathBuf>, name: &str) -> Result<Map<String, Value>, String> {
    let not_found = || format!("{name}'s credentials were not found");
    let path = path.ok_or_else(not_found)?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(not_found()),
        Err(_) => return Err(format!("{name}'s credentials cannot be read")),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(doc)) => Ok(doc),
        Ok(_) => Err(format!("{name}'s credentials are in an unexpected format")),
        Err(_) => Err(format!("{name}'s credentials cannot be read")),
    }
}

fn json_object(body: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(doc)) => Some(doc),
        _ => None,
    }
}

/// An answer that is not an object: valid JSON of another shape, or no JSON at all.
fn answer_reason(body: &[u8]) -> &'static str {
    if serde_json::from_slice::<Value>(body).is_ok() { "answer in an unexpected format" } else { "invalid answer" }
}

fn refused_reason(code: u16) -> String {
    format!("token refused (HTTP {code}): expired or invalid, the probe does not renew it")
}

/// The prototype's `_get_json`, on the reasons.
fn transport_reason(error: TransportError) -> String {
    match error {
        TransportError::Status(code @ (401 | 403)) => refused_reason(code),
        TransportError::Status(429) => "too many requests (HTTP 429): the endpoint limits the rate, try again later".to_owned(),
        TransportError::Status(code) => format!("HTTP error {code}"),
        TransportError::Timeout => "timeout".to_owned(),
        TransportError::Unreachable => "network unreachable".to_owned(),
        TransportError::Invalid => "invalid answer".to_owned(),
    }
}

/// A field that is a non-empty string.
fn text(doc: &Map<String, Value>, key: &str) -> Option<String> {
    doc.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned)
}

/// A number, not a boolean: Python's `isinstance(x, (int, float))` on JSON.
fn number(value: &Value) -> Option<f64> {
    value.as_f64()
}

/// Python's `float(x)` on a JSON value: a number, a numeric string, or a boolean.
fn python_float(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().replace('_', "").parse().ok(),
        Value::Bool(flag) => Some(f64::from(u8::from(*flag))),
        _ => None,
    }
}

/// Python's `str(x)` on a JSON value, for the few the probes turn into text.
fn python_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        other => other.to_string(),
    }
}

/// Python's truth of a JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// `max(0, int((at - now).total_seconds()))`.
fn whole_seconds(now: WallTime, at: WallTime) -> i64 {
    (now.seconds_until(at).trunc() as i64).max(0)
}

/// The `exp` of a JWT's payload, which is decoded and never verified: the prototype's `_exp_jwt`.
fn jwt_expiry(jwt: &str) -> Option<WallTime> {
    let payload = jwt.split('.').nth(1)?;
    let claims: Value = serde_json::from_slice(&base64_lenient(payload)).ok()?;
    let exp = claims.as_object()?.get("exp").and_then(number)?;
    WallTime::from_unix_secs_f64(exp)
}

/// Base64 in either alphabet, as Python's `urlsafe_b64decode` without validation reads it: what is
/// outside the alphabet is skipped, and so is a missing padding.
fn base64_lenient(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut count) = (0_u32, 0_u32);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => continue,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jwt_gives_its_expiry_and_nothing_else() {
        // {"exp":1794407822,"sub":"x"} in base64url, without padding.
        let jwt = "eyJhbGciOiJub25lIn0.eyJleHAiOjE3OTQ0MDc4MjIsInN1YiI6IngifQ.signature";
        assert_eq!(jwt_expiry(jwt).map(WallTime::unix_micros), Some(1_794_407_822_000_000));
        assert_eq!(jwt_expiry("no-dots"), None);
        assert_eq!(jwt_expiry("a.eyJleHAiOnRydWV9.c"), None, "a boolean exp is no expiry");
        assert_eq!(base64_lenient("aGk"), b"hi");
        assert_eq!(base64_lenient("a G\nk="), b"hi");
    }

    #[test]
    fn python_values_read_as_python_reads_them() {
        assert_eq!(python_float(&Value::from(" 14.5 ")), Some(14.5));
        assert_eq!(python_float(&Value::from(true)), Some(1.0));
        assert_eq!(python_float(&serde_json::json!([1])), None);
        assert_eq!(python_str(&Value::from(42)), "42");
        assert!(!truthy(&Value::from("")) && truthy(&Value::from(1)));
        assert_eq!(format!("{:?}", Bearer("secret".to_owned())), "Bearer(<hidden>)");
    }
}
