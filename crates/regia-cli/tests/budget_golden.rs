//! Acceptance 1 of #463: the JSON of `budget`, with the probes on, equals the prototype's translated by
//! the facade's table. The prototype's output is recorded in `golden/budget-python.json` by
//! `oracle/budget_oracle.py`, on the recorded answers of `regia-agents` and at the same instant; here
//! the native probes read the same answers through a fake transport. No endpoint is asked, and the
//! credentials are written at run time with tokens that are not tokens.

use std::path::{Path, PathBuf};

use regia_agents::{Bearer, CLAUDE_URL, Env, Platform, ProbeConsent, Transport, TransportError, UsageProbes};
use regia_facade::Table;
use regia_store::{ManualClock, WallTime};
use serde_json::{Value, json};

/// 2026-11-11T02:00:00Z, the oracle's instant.
const NOW: i64 = 1_794_362_400;

struct Recorded;

impl Transport for Recorded {
    fn get(&self, url: &str, _bearer: &Bearer, _headers: &[(&'static str, String)]) -> Result<Vec<u8>, TransportError> {
        let name = if url == CLAUDE_URL { "claude-usage.json" } else { "codex-usage.json" };
        Ok(std::fs::read(manifest().join("..").join("regia-agents").join("tests").join("recorded").join(name)).unwrap())
    }
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A JWT whose payload has the oracle's `exp`.
fn codex_jwt(exp: i64) -> String {
    let payload = json!({"exp": exp, "canary": "not-a-token"}).to_string();
    format!("eyJhbGciOiJub25lIn0.{}.c2lnbmF0dXJl", base64url(payload.as_bytes()))
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

fn credentials(dir: &Path) -> Env {
    std::fs::create_dir_all(dir).unwrap();
    let (claude, codex) = (dir.join(".credentials.json"), dir.join("auth.json"));
    let oauth = json!({"claudeAiOauth": {"accessToken": "not-a-token", "expiresAt": (NOW + 8 * 3600) * 1000, "subscriptionType": "max"}});
    std::fs::write(&claude, oauth.to_string()).unwrap();
    let tokens = json!({"tokens": {"access_token": codex_jwt(NOW + 3 * 24 * 3600), "account_id": "account-0001"}});
    std::fs::write(&codex, tokens.to_string()).unwrap();
    Env::from_pairs([("REGIA_CLAUDE_CREDENTIALS", claude), ("REGIA_CODEX_AUTH", codex)])
}

#[test]
fn the_native_budget_equals_the_prototype_translated() {
    let golden: Value = serde_json::from_str(&std::fs::read_to_string(manifest().join("tests").join("golden").join("budget-python.json")).unwrap()).unwrap();
    let translated = Table::embedded().translate_output("budget", &golden).expect("the table translates the prototype's output");
    let env = credentials(&PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("budget-golden"));
    let clock = ManualClock::new(WallTime::from_unix_micros(NOW * 1_000_000).unwrap(), 0.0, "rust/manual");
    let native = UsageProbes::new(ProbeConsent::Granted, env, Recorded).with_clock(clock).on_platform(Platform::Other).read();
    assert_eq!(serde_json::to_value(&native).unwrap(), translated);
}
