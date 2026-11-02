#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(feature = "smoke", not(debug_assertions)))]
compile_error!("the smoke feature lets the webview write a file and quit the app: never in a release build");

use std::collections::BTreeMap;

use regia_core::{Brief, BriefOptions};
use regia_i18n::{Catalog, Language};
use regia_runtime::load_brief;
use serde::Serialize;

#[derive(Serialize)]
struct Bootstrap {
    language: &'static str,
    strings: BTreeMap<String, String>,
    smoke: bool,
}

/// The language and the catalog: the webview has no Fluent runtime of its own.
#[tauri::command]
fn bootstrap() -> Bootstrap {
    let language = Language::detect();
    Bootstrap {
        language: language.tag(),
        strings: Catalog::new(language).all(),
        smoke: cfg!(feature = "smoke"),
    }
}

/// Runs off the main thread: reading the tracker takes seconds.
#[tauri::command(async)]
fn get_brief() -> Result<Brief, String> {
    load_brief(&BriefOptions::default()).map_err(|error| error.to_string())
}

/// Test only (see `regia-conformance`, `app-smoke`): stores what the webview rendered and exits.
#[cfg(feature = "smoke")]
#[tauri::command]
fn smoke_report(app: tauri::AppHandle, report: serde_json::Value) -> Result<(), String> {
    let path = std::env::var(regia_identity::env_var("SMOKE_OUT")).map_err(|error| error.to_string())?;
    std::fs::write(path, report.to_string()).map_err(|error| error.to_string())?;
    app.exit(0);
    Ok(())
}

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "smoke")]
    let builder = builder.invoke_handler(tauri::generate_handler![bootstrap, get_brief, smoke_report]);
    #[cfg(not(feature = "smoke"))]
    let builder = builder.invoke_handler(tauri::generate_handler![bootstrap, get_brief]);
    builder.run(tauri::generate_context!()).expect("the app runs");
}
