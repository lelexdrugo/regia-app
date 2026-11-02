//! Starts the real app on a fixture, once per language, and checks that the webview rendered the same
//! brief the CLI prints: the same tasks in the same order, the catalog's headings, and hostile titles
//! as plain text. The app must be built with its `smoke` feature, which reports the DOM and exits.
//!
//!     cargo run -p regia-conformance --bin app-smoke

use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use regia_conformance::{Case, apply_fixture_env, bin_dir, prepare_state_dir, read_json, run_rust};
use regia_i18n::{Catalog, Language};
use regia_identity::env_var;
use serde::Deserialize;
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
struct Report {
    language: String,
    sections: std::collections::BTreeMap<String, Vec<u64>>,
    headings: Vec<String>,
    images: u64,
    scripts_inline: u64,
    text: String,
}

fn numbers(section: &Value) -> Vec<u64> {
    section
        .as_array()
        .expect("a section is a list")
        .iter()
        .map(|task| task["number"].as_u64().expect("a number"))
        .collect()
}

fn check(language: Language, case: &Case, expected: &Value) -> Result<(), String> {
    let state = prepare_state_dir(&format!("smoke-{language}"));
    let out = state.join("smoke.json");
    let app = bin_dir().join(format!("{}-app{}", regia_identity::PRODUCT_NAME, std::env::consts::EXE_SUFFIX));
    let mut command = Command::new(&app);
    apply_fixture_env(&mut command, &case.responses_path(), &state);
    command.env(env_var("LANG"), language.tag()).env(env_var("SMOKE_OUT"), &out);
    let mut child = command.spawn().map_err(|error| format!("{}: {error}", app.display()))?;

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            return Err(format!("{language}: the app did not report within {TIMEOUT:?}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    if !status.success() {
        return Err(format!("{language}: the app exited with {status}"));
    }
    let report: Report = read_json(&out);
    println!("{language}: reported after {:.1}s: {:?}", start.elapsed().as_secs_f32(), report.sections);

    let mut problems = Vec::new();
    if report.language != language.tag() {
        problems.push(format!("language is {}", report.language));
    }
    for key in ["waiting_on_owner", "ready", "in_progress"] {
        let wanted = numbers(&expected[key]);
        if report.sections.get(key) != Some(&wanted) {
            problems.push(format!("{key}: rendered {:?}, CLI says {wanted:?}", report.sections.get(key)));
        }
    }
    let catalog = Catalog::new(language);
    for id in ["brief-waiting", "brief-ready", "brief-in-progress"] {
        let heading = catalog.message(id);
        if !report.headings.contains(&heading) {
            problems.push(format!("heading missing: {heading}"));
        }
    }
    if report.images != 0 || report.scripts_inline != 0 {
        problems.push(format!(
            "a hostile title became markup: {} images, {} inline scripts",
            report.images, report.scripts_inline
        ));
    }
    if !report.text.contains("<img src=x onerror=") {
        problems.push("the hostile title is not shown as plain text".to_owned());
    }
    if problems.is_empty() { Ok(()) } else { Err(format!("{language}: {}", problems.join("; "))) }
}

fn main() -> ExitCode {
    let case = Case::named("core");
    let expected = run_rust("smoke-cli", &case.spec.args, &case.responses_path()).expect("the CLI brief");
    let mut failed = false;
    for language in Language::ALL {
        if let Err(message) = check(language, &case, &expected) {
            eprintln!("FAILED {message}");
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        println!("the app shows the CLI's brief in {} languages", Language::ALL.len());
        ExitCode::SUCCESS
    }
}
