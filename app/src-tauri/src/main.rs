#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(feature = "smoke", not(debug_assertions)))]
compile_error!("the smoke feature lets the webview write a file and quit the app: never in a release build");

mod ipc;
mod navigation;
#[cfg(feature = "smoke")]
mod smoke;

use regia_core::BriefOptions;
use regia_i18n::Language;
use regia_runtime::load_brief;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Runtime, WebviewWindowBuilder};

use crate::ipc::{AppInfo, BriefView};
use crate::navigation::AppOrigin;

/// Product, version, language and system. The webview renders the catalog itself, with Fluent: this
/// tells it which language.
#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        product_name: regia_identity::PRODUCT_NAME.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        language: Language::detect().tag().to_owned(),
        os: std::env::consts::OS.to_owned(),
        smoke: cfg!(feature = "smoke"),
    }
}

/// Runs off the main thread: reading the tracker takes seconds.
#[tauri::command(async)]
fn get_brief() -> Result<BriefView, String> {
    load_brief(&BriefOptions::default()).map(|brief| BriefView::from(&brief)).map_err(|error| error.to_string())
}

/// The main window keeps its size and title in `tauri.conf.json`, where `create: false` leaves it to this
/// function: the two handlers that keep the webview inside the app exist only on the builder (V09).
fn open_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let config = app.config().app.windows.iter().find(|window| window.label == "main").cloned();
    let config = config.expect("tauri.conf.json declares the main window");
    let origin = AppOrigin::current();
    WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(move |url| origin.allows(url))
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .build()?;
    Ok(())
}

fn builder<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    #[cfg(feature = "smoke")]
    let builder = builder.invoke_handler(tauri::generate_handler![get_app_info, get_brief, smoke::smoke_report]);
    #[cfg(not(feature = "smoke"))]
    let builder = builder.invoke_handler(tauri::generate_handler![get_app_info, get_brief]);
    builder
}

fn main() {
    #[cfg(feature = "smoke")]
    smoke::start_watchdog();
    builder(tauri::Builder::default())
        .setup(|app| {
            #[cfg(feature = "smoke")]
            smoke::start_pings(app.handle().clone());
            Ok(open_main_window(app.handle())?)
        })
        .run(tauri::generate_context!())
        .expect("the app runs");
}

/// V10, on Tauri's mock runtime with this build's real ACL: what the webview may not call, it cannot.
/// `app-smoke` asks the same of the real webview on each system.
#[cfg(test)]
mod acl {
    use serde_json::{Value, json};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
    use tauri::webview::InvokeRequest;
    use tauri::{WebviewUrl, WebviewWindow};

    use super::*;

    fn window() -> WebviewWindow<MockRuntime> {
        let app = builder(mock_builder()).build(tauri::generate_context!()).expect("the mock app");
        WebviewWindowBuilder::new(&app, "main", WebviewUrl::default()).build().expect("the main window")
    }

    fn origin() -> &'static str {
        match AppOrigin::current() {
            AppOrigin::CustomScheme => "tauri://localhost",
            AppOrigin::Localhost => "http://tauri.localhost",
        }
    }

    fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value, from: &str) -> Result<Value, Value> {
        let request = InvokeRequest {
            cmd: command.to_owned(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: from.parse().unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_owned(),
        };
        get_ipc_response(window, request).map(|body| body.deserialize::<Value>().unwrap())
    }

    #[test]
    fn the_webview_reaches_its_own_commands() {
        let window = window();
        let info = invoke(&window, "get_app_info", json!({}), origin()).expect("get_app_info is allowed");
        assert_eq!(info["product_name"], regia_identity::PRODUCT_NAME);
        // A command without arguments ignores extra fields, and none of this build's commands panics.
        invoke(&window, "get_app_info", json!({ "extra": [1, 2, 3] }), origin()).expect("extra fields are ignored");
    }

    #[test]
    fn everything_else_is_refused() {
        let window = window();
        let mut refused = vec![
            ("plugin:shell|execute", json!({ "program": "probe", "args": ["--version"] })),
            ("plugin:shell|spawn", json!({ "program": "probe", "args": [] })),
            ("plugin:fs|read_file", json!({ "path": "/etc/passwd" })),
            ("plugin:opener|open_url", json!({ "url": "https://example.invalid" })),
            ("plugin:window|set_title", json!({ "label": "main", "value": "x" })),
            ("plugin:webview|create_webview_window", json!({ "options": { "label": "x", "url": "https://example.invalid" } })),
            ("plugin:webview|internal_toggle_devtools", json!({})),
            ("plugin:event|emit", json!({ "event": "x", "payload": 1 })),
            ("plugin:event|listen", json!({ "event": "x", "target": { "kind": "Any" }, "handler": 3 })),
            ("plugin:app|version", json!({})),
            ("plugin:path|resolve_directory", json!({ "directory": 1 })),
            ("bootstrap", json!({})),
            ("read_file", json!({ "path": "/etc/passwd" })),
        ];
        if !cfg!(feature = "smoke") {
            refused.push(("smoke_report", json!({ "report": {} })));
        }
        for (command, args) in refused {
            assert!(invoke(&window, command, args, origin()).is_err(), "{command} was allowed");
        }
    }

    #[test]
    fn a_remote_page_reaches_nothing() {
        let window = window();
        for from in ["https://example.invalid", "http://tauri.localhost.attacker.com", "tauri://localhost.attacker.com"] {
            assert!(invoke(&window, "get_app_info", json!({}), from).is_err(), "get_app_info from {from}");
        }
    }
}
