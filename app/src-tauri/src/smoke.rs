//! Test only, with the `smoke` feature: the webview reports what it rendered and what its probes found, and
//! the app exits. `regia-conformance`'s `app-smoke` compares the report with the CLI's brief; the probes are
//! judged here, so a failed probe fails `app-smoke` through the exit code.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Runtime};

/// Long enough for a cold webview on a CI runner, much shorter than `app-smoke`'s two minutes: a frontend
/// that never reports costs this, not the whole timeout, on each language.
const WATCHDOG: Duration = Duration::from_secs(40);

/// Ends the app if the webview never reports: a script that failed to load, or a navigation that left it.
pub fn start_watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(WATCHDOG);
        eprintln!("smoke: the webview sent no report in {} s", WATCHDOG.as_secs());
        std::process::exit(2);
    });
}

#[derive(Debug, Deserialize)]
struct Check {
    name: String,
    ok: bool,
    #[serde(default)]
    detail: String,
}

/// Stores the report where `app-smoke` reads it, then exits: 0 if the webview ran its checks and every one
/// passed, 1 otherwise, with the failures on stderr.
#[tauri::command]
pub fn smoke_report<R: Runtime>(app: AppHandle<R>, report: Value) -> Result<(), String> {
    let path = std::env::var(regia_identity::env_var("SMOKE_OUT")).map_err(|error| error.to_string())?;
    std::fs::write(path, report.to_string()).map_err(|error| error.to_string())?;
    let failures = match serde_json::from_value::<Vec<Check>>(report["checks"].clone()) {
        Ok(checks) if checks.is_empty() => vec!["the webview ran no check".to_owned()],
        Ok(checks) => checks.iter().filter(|check| !check.ok).map(|check| format!("{}: {}", check.name, check.detail)).collect(),
        Err(error) => vec![format!("the report has no list of checks: {error}")],
    };
    for failure in &failures {
        eprintln!("smoke check failed: {failure}");
    }
    app.exit(if failures.is_empty() { 0 } else { 1 });
    Ok(())
}
