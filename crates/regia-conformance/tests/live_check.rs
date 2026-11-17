//! What `live-check` starts, and what it may print (#559, #435/V6 and #439/D5). One test only in this
//! binary: it sets a variable of its own process, which no other thread may be reading meanwhile.

use std::path::Path;

use regia_conformance::{live_rust_launch, workspace_root};
use regia_identity::env_var;
use serde_json::Value;

const PROBE: &str = env!("CARGO_BIN_EXE_probe");

/// The Rust side of `live-check` counts the due dates from the real clock, as the prototype does, and
/// not from the day a `REGIA_TODAY` in the shell names. The probe stands in for the binary and says what
/// it received.
#[test]
fn live_check_starts_the_rust_side_without_the_fixed_day_and_on_the_native_engine() {
    let today = env_var("TODAY");
    // SAFETY: the only test of this binary: no other thread reads the environment meanwhile.
    unsafe { std::env::set_var(&today, "2026-01-12") };
    let now = env_var("NOW");
    // SAFETY: as above.
    unsafe { std::env::set_var(&now, "2026-01-12T10:00:00Z") };
    let dossier = workspace_root();
    let output = live_rust_launch(Path::new(PROBE), &["brief".to_owned()], &dossier).run().expect("the probe runs");
    let report: Value = serde_json::from_slice(&output.stdout).expect("the probe prints JSON");
    let env = report["env"].as_object().expect("the probe lists its environment");
    // Never print the environment: it holds the session's own tokens.
    assert!(!env.contains_key(&today), "{today} reached the Rust side of live-check");
    assert!(!env.contains_key(&now), "{now} reached the Rust side of live-check");
    assert_eq!(env.get(&env_var("ENGINE")).and_then(Value::as_str), Some("rust"));
    assert_eq!(report["argv"], serde_json::json!(["brief"]));
}
