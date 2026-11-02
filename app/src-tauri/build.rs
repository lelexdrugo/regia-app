use tauri_build::{AppManifest, Attributes};

// Declaring the commands here turns on Tauri's ACL for them: a command no capability allows cannot
// be called from the webview.
fn main() {
    let smoke = std::env::var_os("CARGO_FEATURE_SMOKE").is_some();
    let commands: &[&str] =
        if smoke { &["bootstrap", "get_brief", "smoke_report"] } else { &["bootstrap", "get_brief"] };
    let capabilities = if smoke { "./capabilities/**/*.json" } else { "./capabilities/*.json" };
    let attributes = Attributes::new()
        .app_manifest(AppManifest::new().commands(commands))
        .capabilities_path_pattern(capabilities);
    tauri_build::try_build(attributes).expect("tauri build");
}
