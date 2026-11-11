#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(feature = "smoke", not(debug_assertions)))]
compile_error!("the smoke feature lets the webview write a file and quit the app: never in a release build");

mod ipc;
mod navigation;
mod settings;
#[cfg(feature = "smoke")]
mod smoke;

use std::path::PathBuf;

use regia_core::BriefOptions;
use regia_runtime::load_brief;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow, WebviewWindowBuilder};

use crate::ipc::{AppInfo, BriefView};
use crate::navigation::AppOrigin;
use crate::settings::{Settings, SettingsChange, Store};

/// Product, version, language and system. The webview renders the catalog itself, with Fluent: this
/// tells it which language.
#[tauri::command]
fn get_app_info(store: State<'_, Store>) -> AppInfo {
    AppInfo {
        product_name: regia_identity::PRODUCT_NAME.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        language: store.load().speaks().tag().to_owned(),
        os: std::env::consts::OS.to_owned(),
        smoke: cfg!(feature = "smoke"),
    }
}

/// Runs off the main thread: reading the tracker takes seconds.
#[tauri::command(async)]
fn get_brief() -> Result<BriefView, String> {
    load_brief(&BriefOptions::default()).map(|brief| BriefView::from(&brief)).map_err(|error| error.to_string())
}

/// The direction, the density and the language chosen in the app (`settings.rs`).
#[tauri::command]
fn get_settings(store: State<'_, Store>) -> Settings {
    store.load()
}

/// Saves the fields of `settings`, none of them sensitive, so no native confirmation (threat model §3.4), and
/// answers what is saved. The window's own colour and title bar follow the direction.
#[tauri::command]
fn set_settings<R: Runtime>(
    window: WebviewWindow<R>,
    store: State<'_, Store>,
    settings: SettingsChange,
) -> Result<Settings, String> {
    let saved = store.change(settings)?;
    // Cosmetic: the page has its colours already, and a system that cannot change these keeps the old ones.
    let _ = window.set_background_color(Some(saved.direction.ground()));
    let _ = window.set_theme(Some(saved.direction.theme()));
    Ok(saved)
}

/// The folder of the settings file: the user's configuration folder, or in a smoke run the folder of its
/// report, which `app-smoke` makes for each run, so a test never reads or writes the user's settings.
fn settings_dir() -> Option<PathBuf> {
    #[cfg(feature = "smoke")]
    let dir = std::env::var_os(regia_identity::env_var("SMOKE_OUT"))
        .and_then(|out| PathBuf::from(out).parent().map(PathBuf::from));
    #[cfg(not(feature = "smoke"))]
    let dir = regia_identity::config_dir();
    dir
}

/// The main window keeps its size and title in `tauri.conf.json`, where `create: false` leaves it to this
/// function: the two handlers that keep the webview inside the app exist only on the builder (V09). Its
/// colour is the saved direction's ground from the start, so the frame before the page is not white.
fn open_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let config = app.config().app.windows.iter().find(|window| window.label == "main").cloned();
    let config = config.expect("tauri.conf.json declares the main window");
    let direction = app.state::<Store>().load().direction;
    let origin = AppOrigin::current();
    WebviewWindowBuilder::from_config(app, &config)?
        .background_color(direction.ground())
        .theme(Some(direction.theme()))
        .on_navigation(move |url| origin.allows(url))
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .build()?;
    Ok(())
}

fn builder<R: Runtime>(builder: tauri::Builder<R>, store: Store) -> tauri::Builder<R> {
    let builder = builder.manage(store);
    #[cfg(feature = "smoke")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        get_app_info,
        get_brief,
        get_settings,
        set_settings,
        smoke::smoke_report,
        smoke::smoke_corpus
    ]);
    #[cfg(not(feature = "smoke"))]
    let builder = builder.invoke_handler(tauri::generate_handler![get_app_info, get_brief, get_settings, set_settings]);
    builder
}

fn main() {
    #[cfg(feature = "smoke")]
    smoke::start_watchdog();
    let store = Store::new(settings_dir());
    let mut context = tauri::generate_context!();
    settings::draw_first_paint(&mut context, store.clone());
    builder(tauri::Builder::default(), store)
        .setup(|app| {
            #[cfg(feature = "smoke")]
            smoke::start_pings(app.handle().clone());
            Ok(open_main_window(app.handle())?)
        })
        .run(context)
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

    /// The settings stay in a folder of the test's own, never the user's configuration.
    fn window_with(dir: Option<PathBuf>) -> WebviewWindow<MockRuntime> {
        let app = builder(mock_builder(), Store::new(dir)).build(tauri::generate_context!()).expect("the mock app");
        WebviewWindowBuilder::new(&app, "main", WebviewUrl::default()).build().expect("the main window")
    }

    fn window() -> WebviewWindow<MockRuntime> {
        window_with(None)
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

    /// Refused by the ACL itself, not failed in the command: a call the ACL let through could fail too.
    fn assert_refused(outcome: Result<Value, Value>, call: &str) {
        match outcome {
            Ok(value) => panic!("{call} was allowed: {value}"),
            Err(error) => assert!(error.to_string().contains("not allowed"), "{call} failed after the ACL: {error}"),
        }
    }

    #[test]
    fn the_webview_reaches_its_own_commands() {
        let window = window();
        let info = invoke(&window, "get_app_info", json!({}), origin()).expect("get_app_info is allowed");
        assert_eq!(info["product_name"], regia_identity::PRODUCT_NAME);
        // A command without arguments ignores extra fields, and none of this build's commands panics.
        invoke(&window, "get_app_info", json!({ "extra": [1, 2, 3] }), origin()).expect("extra fields are ignored");
        let settings = invoke(&window, "get_settings", json!({}), origin()).expect("get_settings is allowed");
        assert_eq!(settings, json!({ "direction": "montaggio", "density": "compact", "language": null }));
    }

    #[test]
    fn set_settings_saves_known_fields_and_refuses_the_rest() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-settings/acl");
        let _ = std::fs::remove_dir_all(&dir);
        let window = window_with(Some(dir.clone()));
        let saved = invoke(&window, "set_settings", json!({ "settings": { "direction": "nero" } }), origin());
        assert_eq!(saved.expect("a known field is saved")["direction"], "nero");
        assert_eq!(invoke(&window, "get_settings", json!({}), origin()).unwrap()["direction"], "nero");
        assert!(std::fs::read_to_string(dir.join(settings::FILE)).unwrap().contains("direction = \"nero\""));
        for refused in [json!({ "theme": "nero" }), json!({ "direction": "purple" }), json!({ "path": "/etc" })] {
            let outcome = invoke(&window, "set_settings", json!({ "settings": refused }), origin());
            assert!(outcome.is_err(), "{refused} was accepted");
        }
        assert_eq!(invoke(&window, "get_settings", json!({}), origin()).unwrap()["direction"], "nero");
        // Without a configuration folder, nothing is written, and the webview hears why.
        assert!(invoke(&self::window(), "set_settings", json!({ "settings": { "density": "comfortable" } }), origin()).is_err());
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
            assert_refused(invoke(&window, command, args, origin()), command);
        }
    }

    #[test]
    fn a_remote_page_reaches_nothing() {
        let window = window();
        for from in ["https://example.invalid", "http://tauri.localhost.attacker.com", "tauri://localhost.attacker.com"] {
            assert_refused(invoke(&window, "get_app_info", json!({}), from), &format!("get_app_info from {from}"));
        }
    }
}
