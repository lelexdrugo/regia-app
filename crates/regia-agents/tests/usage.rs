//! The usage probes on recorded answers, with credentials written at run time around canary tokens:
//! no test reads a real credential or asks a real endpoint. The prototype's cases are those of
//! `tools/tests/test_sveglia.py` (`TestFinestraNonAperta`, `TestSondaClaude`), plus the consent of
//! #396 and the canaries of V19.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use regia_agents::{
    Bearer, CLAUDE_URL, CODEX_URL, Dry, Env, MACOS_CLAUDE, PROBES_OFF, Platform, ProbeConsent, Settings, Transport, TransportError, UsageProbes,
    WakeOutcome, WakeUp,
};
use regia_store::{ManualClock, TokenStatus, UsageStatus, WallTime, Window};
use serde_json::{Value, json};

/// 2026-11-11T02:00:00Z.
const NOW: i64 = 1_794_362_400;
const CLAUDE_CANARY: &str = "canary-claude-access-4b1e-never-printed";
const CODEX_CANARY: &str = "canary-codex-claim-9d2c-never-printed";

/// A call the fake endpoints saw.
#[derive(Debug, Clone)]
struct Call {
    url: String,
    bearer: String,
    headers: Vec<(&'static str, String)>,
}

/// The endpoints, from recorded answers or a failure, with every call kept.
#[derive(Clone)]
struct Recorded {
    claude: Result<Vec<u8>, TransportError>,
    codex: Result<Vec<u8>, TransportError>,
    calls: Arc<Mutex<Vec<Call>>>,
}

impl Recorded {
    fn new() -> Recorded {
        Recorded { claude: Ok(recorded("claude-usage.json")), codex: Ok(recorded("codex-usage.json")), calls: Arc::default() }
    }

    fn claude(mut self, answer: Result<Value, TransportError>) -> Recorded {
        self.claude = answer.map(|value| value.to_string().into_bytes());
        self
    }

    fn codex(mut self, answer: Result<Value, TransportError>) -> Recorded {
        self.codex = answer.map(|value| value.to_string().into_bytes());
        self
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl Transport for Recorded {
    fn get(&self, url: &str, bearer: &Bearer, headers: &[(&'static str, String)]) -> Result<Vec<u8>, TransportError> {
        self.calls.lock().unwrap().push(Call { url: url.to_owned(), bearer: bearer.expose().to_owned(), headers: headers.to_vec() });
        if url == CLAUDE_URL { self.claude.clone() } else { self.codex.clone() }
    }
}

fn recorded(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("recorded").join(name)).unwrap()
}

/// A fresh folder under `target/` with the two credentials files, as Claude Code and Codex write them.
struct Home {
    dir: PathBuf,
}

impl Home {
    fn new(label: &str) -> Home {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("usage-probes").join(label);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let home = Home { dir };
        home.claude(json!({"claudeAiOauth": {"accessToken": CLAUDE_CANARY, "refreshToken": CLAUDE_CANARY, "expiresAt": (NOW + 8 * 3600) * 1000, "subscriptionType": "max"}}));
        home.codex(NOW + 3 * 24 * 3600, Some("account-0001"));
        home
    }

    fn claude_file(&self) -> PathBuf {
        self.dir.join(".credentials.json")
    }

    fn codex_file(&self) -> PathBuf {
        self.dir.join("auth.json")
    }

    fn claude(&self, doc: Value) {
        std::fs::write(self.claude_file(), doc.to_string()).unwrap();
    }

    /// Codex's file with a JWT whose payload carries `exp` and a canary claim.
    fn codex(&self, exp: i64, account: Option<&str>) {
        let jwt = codex_jwt(exp);
        let mut tokens = json!({"access_token": jwt, "id_token": jwt, "refresh_token": CODEX_CANARY});
        if let Some(account) = account {
            tokens["account_id"] = account.into();
        }
        std::fs::write(self.codex_file(), json!({"tokens": tokens}).to_string()).unwrap();
    }

    fn env(&self) -> Env {
        Env::from_pairs([
            ("REGIA_CLAUDE_CREDENTIALS", self.claude_file()),
            ("REGIA_CODEX_AUTH", self.codex_file()),
        ])
    }
}

fn codex_jwt(exp: i64) -> String {
    let header = base64url(br#"{"alg":"none"}"#);
    let payload = base64url(json!({"exp": exp, "canary": CODEX_CANARY}).to_string().as_bytes());
    format!("{header}.{payload}.c2lnbmF0dXJl")
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let bits = chunk.iter().fold(0_u32, |acc, byte| (acc << 8) | u32::from(*byte)) << (8 * (3 - chunk.len()));
        for index in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(bits >> (18 - 6 * index) & 63) as usize]));
        }
    }
    out
}

fn clock() -> ManualClock {
    ManualClock::new(WallTime::from_unix_micros(NOW * 1_000_000).unwrap(), 0.0, "rust/manual")
}

fn probes(consent: ProbeConsent, home: &Home, endpoints: &Recorded) -> UsageProbes {
    UsageProbes::new(consent, home.env(), endpoints.clone()).with_clock(clock()).on_platform(Platform::Other)
}

/// Every place a token could leak to: the JSON, the reasons, the debug forms.
fn assert_no_canary(text: &str) {
    for canary in [CLAUDE_CANARY, CODEX_CANARY, "Y2FuYXJ5"] {
        assert!(!text.contains(canary), "a token leaked: {text}");
    }
    assert!(!text.contains(&codex_jwt(NOW + 3 * 24 * 3600)));
}

/// A file the operating system will not let anyone open while this lives: an exclusive handle on
/// Windows, no permission at all on Unix. A probe that tried would find it unreadable.
struct Locked {
    #[cfg(windows)]
    _handle: std::fs::File,
    #[cfg(unix)]
    path: PathBuf,
}

impl Locked {
    fn new(path: &Path) -> Locked {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let handle = std::fs::OpenOptions::new().read(true).share_mode(0).open(path).unwrap();
            assert!(std::fs::read(path).is_err(), "the exclusive handle must keep every other open out");
            Locked { _handle: handle }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
            assert!(std::fs::read(path).is_err(), "running as root: no permission bit keeps the canary unread, so this test proves nothing");
            Locked { path: path.to_path_buf() }
        }
    }
}

#[cfg(unix)]
impl Drop for Locked {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
    }
}

/// Acceptance 2 of #463: with the probes refused the credentials stay closed, nothing is asked, and
/// both providers read `probe_off`. The control: with consent the same locked files read as
/// unreadable, so a probe that tried would show it.
#[test]
fn with_the_probes_refused_the_canary_stays_unopened_and_the_reading_says_probe_off() {
    let home = Home::new("refused");
    let endpoints = Recorded::new();
    let (_claude, _codex) = (Locked::new(&home.claude_file()), Locked::new(&home.codex_file()));
    let refused = probes(ProbeConsent::Refused, &home, &endpoints);
    let reading = refused.read();
    for (_, usage) in &reading.providers {
        assert_eq!((usage.status, usage.reason.as_deref()), (UsageStatus::ProbeOff, Some(PROBES_OFF)));
        assert!(usage.windows.is_empty() && usage.plan.is_none());
    }
    let json = serde_json::to_value(&reading).unwrap();
    assert_eq!((json["provider"]["claude"]["status"].as_str(), json["provider"]["codex"]["status"].as_str()), (Some("probe_off"), Some("probe_off")));
    assert_eq!(refused.claude_token_expiry(), None);
    assert_eq!(refused.codex_token(), None);
    assert!(endpoints.calls().is_empty(), "no endpoint is asked");
    let granted = probes(ProbeConsent::Granted, &home, &endpoints).read();
    assert_eq!(granted.provider("claude").unwrap().reason.as_deref(), Some("Claude's credentials cannot be read"));
    assert_eq!(granted.provider("codex").unwrap().reason.as_deref(), Some("Codex's credentials cannot be read"));
}

/// #462/V4: the wake-up runs on the real consent, from the settings, and reads the token's expiry
/// with the probes' reader. Refused: a hundred rounds that would each wake, and not one wake-up nor one
/// open of the locked credentials. Granted: every round wakes, and reads the expiry around it.
#[test]
fn the_wake_up_takes_the_consent_of_the_settings_and_the_probes_reader() {
    let home = Home::new("wake-up");
    let endpoints = Recorded::new();
    let refused = Settings::parse("principal = \"claude\"\n").unwrap();
    assert_eq!(refused.probes, None, "no answer yet");
    let explicit = Settings::parse("probes = \"refused\"\n").unwrap();
    for settings in [refused, explicit] {
        let lock = Locked::new(&home.claude_file());
        let reader = probes(settings.probe_consent(), &home, &endpoints);
        let mut wake_up = WakeUp::new(settings.probe_consent());
        let mut wakes = 0;
        for round in 0..100 {
            let now = chrono::DateTime::from_timestamp(NOW + round * 3700, 0).unwrap();
            if wake_up.round(true, now, || reader.wake(&Dry::new())).is_some() {
                wakes += 1;
            }
        }
        assert_eq!(wakes, 0);
        drop(lock);
    }
    let granted = Settings::parse("probes = \"granted\"\n").unwrap();
    assert_eq!(granted.probe_consent(), ProbeConsent::Granted);
    let reader = probes(granted.probe_consent(), &home, &endpoints);
    let mut wake_up = WakeUp::new(granted.probe_consent());
    let attempt = wake_up.round(true, chrono::DateTime::from_timestamp(NOW, 0).unwrap(), || reader.wake(&Dry::new())).cloned().expect("a wake-up");
    assert!(attempt.outcome.fake, "Dry wakes nothing");
    assert_eq!(attempt.outcome.token_expiry_before.as_deref(), Some("2026-11-11T10:00:00+00:00"));
    assert!(!attempt.outcome.token_renewed);
    assert!(endpoints.calls().is_empty(), "the wake-up asks no endpoint");
    assert!(Settings::parse("probes = \"maybe\"\n").is_err());
}

/// A reading with consent, on the recorded answers: the token goes only to its own endpoint, as
/// its bearer, and the reading says nothing of it.
#[test]
fn with_consent_each_token_goes_to_its_endpoint_only_and_the_reading_is_the_answers() {
    let home = Home::new("granted");
    let endpoints = Recorded::new();
    let reading = probes(ProbeConsent::Granted, &home, &endpoints).read();
    let calls = endpoints.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!((calls[0].url.as_str(), calls[0].bearer.as_str()), (CLAUDE_URL, CLAUDE_CANARY));
    assert_eq!(calls[0].headers, [("anthropic-beta", "oauth-2025-04-20".to_owned())]);
    assert_eq!((calls[1].url.as_str(), calls[1].bearer.as_str()), (CODEX_URL, codex_jwt(NOW + 3 * 24 * 3600).as_str()));
    assert_eq!(calls[1].headers, [("ChatGPT-Account-Id", "account-0001".to_owned())]);
    let json = serde_json::to_value(&reading).unwrap();
    assert_eq!(
        json,
        json!({
            "generated": "2026-11-11T02:00:00+00:00",
            "provider": {
                "claude": {"provider": "claude", "status": "ok", "plan": "max", "windows": {
                    "5h": {"status": "ok", "used_pct": 14.0, "remaining_pct": 86.0, "reset": "2026-11-11T06:40+00:00", "reset_in_s": 16800},
                    "7d": {"status": "ok", "used_pct": 58.0, "remaining_pct": 42.0, "reset": "2026-11-11T22:00+00:00", "reset_in_s": 72000}}},
                "codex": {"provider": "codex", "status": "ok", "plan": "plus", "windows": {
                    "5h": {"status": "ok", "used_pct": 0.0, "remaining_pct": 100.0, "reset": "2026-11-11T07:00+00:00", "reset_in_s": 18000},
                    "7d": {"status": "ok", "used_pct": 20.0, "remaining_pct": 80.0, "reset": "2026-11-15T17:06+00:00", "reset_in_s": 400000}},
                    "token": {"status": "valid", "expires_at": "2026-11-14T02:00:00+00:00", "expires_in_s": 259200}}
            }
        })
    );
    assert_no_canary(&json.to_string());
    assert_no_canary(&format!("{reading:?}"));
}

/// V19 on every way a reading fails: no reason and no debug form carries a token.
#[test]
fn no_failure_carries_a_token() {
    let home = Home::new("failures");
    for failure in [
        Err(TransportError::Status(401)),
        Err(TransportError::Status(403)),
        Err(TransportError::Status(429)),
        Err(TransportError::Status(500)),
        Err(TransportError::Status(302)),
        Err(TransportError::Timeout),
        Err(TransportError::Unreachable),
        Err(TransportError::Invalid),
        Ok(json!([CLAUDE_CANARY])),
        Ok(json!({"five_hour": CLAUDE_CANARY, "seven_day": {"utilization": CLAUDE_CANARY, "resets_at": CLAUDE_CANARY}})),
    ] {
        let endpoints = Recorded::new().claude(failure.clone()).codex(failure.clone());
        let reading = probes(ProbeConsent::Granted, &home, &endpoints).read();
        let text = format!("{} {reading:?}", serde_json::to_string(&reading).unwrap());
        assert_no_canary(&text);
        assert_eq!(reading.provider("claude").unwrap().status, UsageStatus::Unknown, "{failure:?}");
    }
    let refused = Recorded::new().codex(Err(TransportError::Status(401)));
    let codex = probes(ProbeConsent::Granted, &home, &refused).codex();
    let token = codex.token.as_ref().unwrap();
    assert_eq!((token.status, token.http_status), (TokenStatus::Refused, Some(401)));
    assert!(codex.reason.as_deref().unwrap().contains("although its expiry has not passed (it expires on 14/11 02:00)"), "{codex:?}");
}

/// `TestFinestraNonAperta`: after its reset, a window with a use and no reset is not open; a null use
/// stays unknown with its own reason; an open window stays as before.
#[test]
fn a_window_after_its_reset_is_ok_and_not_open() {
    let home = Home::new("not-open");
    let answer = json!({"five_hour": {"utilization": 0.0, "resets_at": null}, "seven_day": {"utilization": 13.0, "resets_at": "2099-01-01T00:00:00+00:00"}});
    let claude = probes(ProbeConsent::Granted, &home, &Recorded::new().claude(Ok(answer))).claude();
    assert_eq!(claude.window("5h"), Some(&Window::NotOpen { used_pct: 0.0, remaining_pct: 100.0 }));
    let json = serde_json::to_value(claude.window("5h").unwrap()).unwrap();
    assert_eq!(json, json!({"status": "ok", "used_pct": 0.0, "remaining_pct": 100.0, "open": false, "reset": null, "reset_in_s": null}));
    let open = serde_json::to_value(claude.window("7d").unwrap()).unwrap();
    assert!(open.get("open").is_none() && open["reset"].as_str().unwrap().starts_with("2099-01-01"));
    assert!(open["reset_in_s"].as_i64().unwrap() > 0);
    let null = json!({"five_hour": {"utilization": null, "resets_at": null}});
    let claude = probes(ProbeConsent::Granted, &home, &Recorded::new().claude(Ok(null))).claude();
    assert_eq!(claude.window("5h"), Some(&Window::unknown("utilization is null in the answer")));
    assert_eq!(claude.window("7d"), Some(&Window::unknown("field missing in the answer")));
    assert_eq!(claude.reason.as_deref(), Some("utilization is null in the answer"));
}

/// `TestSondaClaude`: the night of 22/10, ok with the window not open; an expired token asks
/// nothing; the expiry reads without the token; an expiry absent or unreadable is none.
#[test]
fn claude_after_the_reset_with_an_expired_token_and_its_expiry() {
    let home = Home::new("claude");
    let night = Recorded::new().claude(Ok(json!({"five_hour": {"utilization": 0.0, "resets_at": null}, "seven_day": {"utilization": 30.0, "resets_at": "2026-11-12T22:00:00Z"}})));
    let claude = probes(ProbeConsent::Granted, &home, &night).claude();
    assert_eq!((claude.status, claude.window("5h").and_then(Window::used_pct)), (UsageStatus::Ok, Some(0.0)));
    assert!(matches!(claude.window("5h"), Some(Window::NotOpen { .. })));

    home.claude(json!({"claudeAiOauth": {"accessToken": CLAUDE_CANARY, "expiresAt": (NOW - 60) * 1000, "subscriptionType": "max"}}));
    let endpoints = Recorded::new();
    let expired = probes(ProbeConsent::Granted, &home, &endpoints).claude();
    assert!(endpoints.calls().is_empty(), "an expired token asks nothing");
    assert_eq!((expired.status, expired.plan.as_deref()), (UsageStatus::Unknown, Some("max")));
    assert!(expired.reason.as_deref().unwrap().contains("token expired"));

    home.claude(json!({"claudeAiOauth": {"accessToken": CLAUDE_CANARY, "expiresAt": (NOW + 3600) * 1000}}));
    let expiry = probes(ProbeConsent::Granted, &home, &endpoints).claude_token_expiry().expect("the expiry");
    assert_eq!(WallTime::parse_iso(&expiry).map(WallTime::unix_micros), Some((NOW + 3600) * 1_000_000));
    assert_no_canary(&expiry);

    std::fs::remove_file(home.claude_file()).unwrap();
    assert_eq!(probes(ProbeConsent::Granted, &home, &endpoints).claude_token_expiry(), None, "a missing file");
    std::fs::write(home.claude_file(), "{broken").unwrap();
    assert_eq!(probes(ProbeConsent::Granted, &home, &endpoints).claude_token_expiry(), None, "a broken file");
    home.claude(json!({"claudeAiOauth": {"accessToken": CLAUDE_CANARY, "expiresAt": "tomorrow"}}));
    assert_eq!(probes(ProbeConsent::Granted, &home, &endpoints).claude_token_expiry(), None, "an expiry that is not a number");
}

/// Codex's token from its expiry alone: about to expire, then expired, when nothing is asked.
#[test]
fn codex_reads_its_token_from_the_expiry_alone() {
    let home = Home::new("codex");
    home.codex(NOW + 13 * 3600, None);
    let endpoints = Recorded::new();
    let probes_now = probes(ProbeConsent::Granted, &home, &endpoints);
    let codex = probes_now.codex();
    assert_eq!(codex.token.as_ref().map(|token| token.status), Some(TokenStatus::Expiring));
    assert!(endpoints.calls()[0].headers.is_empty(), "no account, no header");
    assert_eq!(probes_now.codex_token().map(|token| token.status), Some(TokenStatus::Expiring));
    home.codex(NOW - 1, None);
    let endpoints = Recorded::new();
    let codex = probes(ProbeConsent::Granted, &home, &endpoints).codex();
    assert!(endpoints.calls().is_empty(), "an expired token asks nothing");
    assert_eq!(codex.token.as_ref().map(|token| (token.status, token.expires_in_s)), Some((TokenStatus::Expired, Some(0))));
    assert!(codex.reason.as_deref().unwrap().starts_with("token expired on 11/11 01:59"), "{codex:?}");
    // The windows by their length, whatever their order.
    home.codex(NOW + 3 * 24 * 3600, None);
    let swapped = json!({"rate_limit": {"primary_window": {"used_percent": 7, "limit_window_seconds": 604800, "reset_at": NOW + 100},
                                        "secondary_window": {"used_percent": 3, "limit_window_seconds": 18000, "reset_at": NOW + 50}}});
    let codex = probes(ProbeConsent::Granted, &home, &Recorded::new().codex(Ok(swapped))).codex();
    assert_eq!((codex.window("5h").and_then(Window::used_pct), codex.window("7d").and_then(Window::used_pct)), (Some(3.0), Some(7.0)));
}

/// Acceptance 3 of #463, the rule: on macOS Claude reads unknown and its file is not opened, even
/// with consent; Codex reads its file as anywhere else.
#[test]
fn on_macos_claude_is_unknown_without_reading_and_codex_reads_its_file() {
    let home = Home::new("macos");
    let endpoints = Recorded::new();
    let _lock = Locked::new(&home.claude_file());
    let on_macos = probes(ProbeConsent::Granted, &home, &endpoints).on_platform(Platform::MacOs);
    let reading = on_macos.read();
    let claude = reading.provider("claude").unwrap();
    assert_eq!((claude.status, claude.reason.as_deref()), (UsageStatus::Unknown, Some(MACOS_CLAUDE)));
    assert_eq!(on_macos.claude_token_expiry(), None);
    assert_eq!(reading.provider("codex").unwrap().status, UsageStatus::Ok);
    assert_eq!(endpoints.calls().iter().map(|call| call.url.as_str()).collect::<Vec<_>>(), [CODEX_URL]);
}

/// The credentials follow the prototype's variable, else the service's folder and its variable.
#[test]
fn the_credentials_follow_the_variable_then_the_service_folder() {
    let home = Home::new("folders");
    let endpoints = Recorded::new();
    let folder_env = Env::from_pairs([("CLAUDE_CONFIG_DIR", home.dir.as_os_str()), ("CODEX_HOME", home.dir.as_os_str())]);
    let probes_by_folder = UsageProbes::new(ProbeConsent::Granted, folder_env, endpoints.clone()).with_clock(clock()).on_platform(Platform::Other);
    assert_eq!(probes_by_folder.read().provider("claude").unwrap().status, UsageStatus::Ok);
    let nowhere = Env::from_pairs([("REGIA_CLAUDE_CREDENTIALS", home.dir.join("missing.json"))]);
    let missing = UsageProbes::new(ProbeConsent::Granted, nowhere, endpoints).with_clock(clock()).on_platform(Platform::Other);
    assert_eq!(missing.claude().reason.as_deref(), Some("Claude's credentials were not found"));
    let wake = WakeOutcome::with_expiry(|| missing.claude_token_expiry(), WakeOutcome::fake);
    assert_eq!(wake.token_expiry_before, None);
}

/// Acceptance 4 of #463: after a reading with consent through the cache, no file of the app's
/// configuration or state holds a canary, the database's bytes included.
#[test]
fn after_a_reading_no_file_of_the_app_holds_a_canary() {
    let home = Home::new("no-canary-in-files");
    let config = home.dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("agents.toml"), "probes = \"granted\"\n").unwrap();
    let settings = Settings::load(&config).unwrap();
    let state = home.dir.join("state");
    let store = regia_store::Store::at(&state).unwrap().with_clock(clock());
    let endpoints = Recorded::new().codex(Err(TransportError::Status(401)));
    let reader = probes(settings.probe_consent(), &home, &endpoints);
    let reading = store.budget(0, false, || Ok::<_, std::convert::Infallible>(reader.read())).unwrap();
    assert_eq!(reading.provider("claude").unwrap().status, UsageStatus::Ok);
    let mut files = Vec::new();
    for dir in [&config, &state] {
        files.extend(std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path()));
    }
    assert!(files.iter().any(|file| file.ends_with(regia_store::database_name())), "{files:?}");
    for file in files {
        let bytes = std::fs::read(&file).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_no_canary(&text);
    }
}
