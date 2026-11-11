use std::path::Path;

use tauri_build::{AppManifest, Attributes};

/// The commands of a release build. Declaring them turns on Tauri's ACL for them: a command no capability
/// allows cannot be called from the webview. `tests/security.rs` (V02) keeps this list, the handlers in
/// `main.rs` and `capabilities/main.json` equal.
const COMMANDS: &[&str] = &["get_app_info", "get_brief", "get_settings", "set_settings"];

/// Test only: compiled in with the `smoke` feature, never in a release build (see `main.rs`).
const SMOKE_COMMANDS: &[&str] = &["smoke_report", "smoke_corpus"];

/// The frontend is built by Vite into `app/ui/dist`, and `tauri::generate_context!` embeds it.
const FRONTEND: &str = "../ui/dist";

fn main() {
    // The proc macro embeds the files without telling Cargo: a rebuilt frontend must rebuild the app.
    println!("cargo:rerun-if-changed={FRONTEND}");
    // With a path pattern, tauri-build leaves the watch to us: without it, an edited capability would
    // not reach the compiled ACL until something else rebuilt the app.
    println!("cargo:rerun-if-changed=capabilities");
    if !Path::new(FRONTEND).join("index.html").is_file() {
        panic!(
            "the frontend is not built: run `npm ci --ignore-scripts` and then `npm run build` in app/ui \
             (see app/ui/README.md)"
        );
    }

    let smoke = std::env::var_os("CARGO_FEATURE_SMOKE").is_some();
    let commands: &'static [&'static str] =
        if smoke { Vec::leak([COMMANDS, SMOKE_COMMANDS].concat()) } else { COMMANDS };
    let capabilities = if smoke { "./capabilities/**/*.json" } else { "./capabilities/*.json" };
    let attributes = Attributes::new()
        .app_manifest(AppManifest::new().commands(commands))
        .capabilities_path_pattern(capabilities);
    tauri_build::try_build(attributes).expect("tauri build");
}
