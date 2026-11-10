//! The threat model's checks on the app's own files (dossier, `docs/app/architettura/modello-minacce.md`, §6):
//! V01 the capabilities, V02 the three lists of commands, V05 `tauri.conf.json`, V27 no devtools in a release
//! build, V28 no listener on the webview's events. Each check returns the problems it finds, so that every
//! test can also show its check failing on an altered copy.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The exact policy, with ADR 0001 §9's six directives first and the threat model's other seven after
/// (§3.5): `base-uri` and `form-action` do not fall back to `default-src`, and the rest say it again.
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; \
                   connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; \
                   frame-src 'none'; worker-src 'none'; media-src 'none'; manifest-src 'none'";

/// The commands of the threat model's §3.4: no other command may reach the webview without a new row there.
const COMMANDS_OF_THE_MODEL: [&str; 24] = [
    "get_app_info",
    "get_setup_status",
    "get_brief",
    "list_projects",
    "get_project",
    "get_task",
    "get_document",
    "get_timeline",
    "list_sessions",
    "get_session_transcript",
    "get_daily",
    "get_console_feed",
    "get_settings",
    "get_controller_status",
    "run_setup_step",
    "decide_gate",
    "set_settings",
    "open_session",
    "open_external_link",
    "copy_context_pack",
    "set_controller_state",
    "check_for_update",
    "install_update",
    "export_diagnostics",
];

/// The only core permissions the model admits (§3.3), and only once a view listens to the core's events.
const CORE_PERMISSIONS: [&str; 2] = ["core:event:allow-listen", "core:event:allow-unlisten"];

/// Plugin prefixes no capability may name (V01).
const FORBIDDEN_PREFIXES: [&str; 18] = [
    "shell:",
    "fs:",
    "http:",
    "opener:",
    "process:",
    "dialog:",
    "clipboard-manager:",
    "store:",
    "sql:",
    "stronghold:",
    "upload:",
    "websocket:",
    "os:",
    "notification:",
    "global-shortcut:",
    "autostart:",
    "updater:",
    "deep-link:",
];

fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&read(path)).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// `get_brief` → `allow-get-brief`: the identifier `AppManifest::commands` generates (closes the threat
/// model's first [NV] of V29: the build fails on an identifier that does not exist).
fn permission_of(command: &str) -> String {
    format!("allow-{}", command.replace('_', "-"))
}

/// A named change to a copy of a file, which its check must notice.
type Alteration = (&'static str, fn(&mut Value));

fn strings(value: &Value) -> Vec<&str> {
    value.as_array().map(|items| items.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

// V01 ----------------------------------------------------------------------------------------------------

fn capability_problems(capability: &Value, allowed_commands: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    if capability.get("remote").is_some() {
        problems.push("a remote origin may reach the IPC".to_owned());
    }
    if strings(&capability["windows"]) != ["main"] {
        problems.push(format!("windows is {}, not exactly [\"main\"]", capability["windows"]));
    }
    if capability.get("webviews").is_some() {
        problems.push("the capability names webviews".to_owned());
    }
    let Some(permissions) = capability["permissions"].as_array() else {
        problems.push("permissions is not a list of identifiers".to_owned());
        return problems;
    };
    for permission in permissions {
        let Some(permission) = permission.as_str() else {
            problems.push(format!("a scoped permission: {permission}"));
            continue;
        };
        if let Some(prefix) = FORBIDDEN_PREFIXES.iter().find(|prefix| permission.starts_with(*prefix)) {
            problems.push(format!("{permission}: the {prefix} plugin is forbidden"));
        }
        if permission == "core:default" || (permission.starts_with("core:") && permission.ends_with(":default")) {
            problems.push(format!("{permission}: a default set grants devtools and emit"));
        }
        if permission.starts_with("core:event:allow-emit")
            || permission.starts_with("core:webview:allow-create")
            || permission.contains("internal-toggle-devtools")
        {
            problems.push(format!("{permission} is forbidden"));
        }
        let own = allowed_commands.iter().any(|command| permission_of(command) == permission);
        if !own && !CORE_PERMISSIONS.contains(&permission) {
            problems.push(format!("{permission} is not in the versioned list"));
        }
    }
    problems
}

fn admitted_commands() -> Vec<&'static str> {
    COMMANDS_OF_THE_MODEL.to_vec()
}

/// Only `main.json` is compiled into a release build: `build.rs` reads `capabilities/*.json` without
/// recursion, and the smoke capability lives one folder down.
#[test]
fn v01_one_capability_covers_the_window_and_it_stays_minimal() {
    let directory = app_dir().join("capabilities");
    let mut files: Vec<String> = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(files, ["main.json"], "a release build compiles every file here");

    let main = read_json(&directory.join("main.json"));
    assert_eq!(main["identifier"], "main");
    assert_eq!(capability_problems(&main, &admitted_commands()), Vec::<String>::new(), "capabilities/main.json");

    let smoke = read_json(&directory.join("smoke/smoke.json"));
    assert_eq!(capability_problems(&smoke, &["smoke_report"]), Vec::<String>::new(), "capabilities/smoke/smoke.json");
    assert_eq!(strings(&smoke["permissions"]), ["allow-smoke-report"]);
}

#[test]
fn v01_fails_on_each_alteration_of_the_capability() {
    let main = read_json(&app_dir().join("capabilities/main.json"));
    let alterations: [Alteration; 10] = [
        ("core:default", |c| c["permissions"].as_array_mut().unwrap().push(json!("core:default"))),
        ("core:window:default", |c| c["permissions"].as_array_mut().unwrap().push(json!("core:window:default"))),
        ("shell", |c| c["permissions"].as_array_mut().unwrap().push(json!("shell:allow-execute"))),
        ("fs", |c| c["permissions"].as_array_mut().unwrap().push(json!("fs:allow-read-file"))),
        ("emit", |c| c["permissions"].as_array_mut().unwrap().push(json!("core:event:allow-emit"))),
        ("devtools", |c| {
            c["permissions"].as_array_mut().unwrap().push(json!("core:webview:allow-internal-toggle-devtools"))
        }),
        ("an unknown command", |c| c["permissions"].as_array_mut().unwrap().push(json!("allow-run-anything"))),
        ("remote", |c| c["remote"] = json!({ "urls": ["https://example.com"] })),
        ("every window", |c| c["windows"] = json!(["*"])),
        ("a second window", |c| c["windows"] = json!(["main", "other"])),
    ];
    for (name, alter) in alterations {
        let mut altered = main.clone();
        alter(&mut altered);
        assert!(!capability_problems(&altered, &admitted_commands()).is_empty(), "V01 misses {name}");
    }
}

// V02 ----------------------------------------------------------------------------------------------------

/// The quoted names of a `const <name>: &[&str] = &[...];` in `build.rs`.
fn build_list(source: &str, name: &str) -> Vec<String> {
    let declaration = format!("const {name}: &[&str] = &[");
    let start = source.find(&declaration).unwrap_or_else(|| panic!("build.rs has no {name}")) + declaration.len();
    let body = &source[start..];
    let body = &body[..body.find("];").unwrap()];
    body.split(',').map(|item| item.trim().trim_matches('"').to_owned()).filter(|item| !item.is_empty()).collect()
}

/// The command names in each `generate_handler![...]` of `main.rs`, without their module path.
fn handler_lists(source: &str) -> Vec<Vec<String>> {
    source
        .match_indices("generate_handler![")
        .map(|(start, needle)| {
            let body = &source[start + needle.len()..];
            let body = &body[..body.find(']').unwrap()];
            body.split(',')
                .map(|item| item.trim().rsplit("::").next().unwrap_or_default().to_owned())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .collect()
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

fn command_list_problems(build: &str, main: &str, capability: &Value, smoke_capability: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let release = sorted(build_list(build, "COMMANDS"));
    let smoke_only = sorted(build_list(build, "SMOKE_COMMANDS"));
    let mut smoke = release.clone();
    smoke.extend(smoke_only.iter().cloned());
    let smoke = sorted(smoke);

    let handlers: Vec<Vec<String>> = handler_lists(main).into_iter().map(sorted).collect();
    if !handlers.contains(&release) {
        problems.push(format!("no generate_handler! registers exactly {release:?}: {handlers:?}"));
    }
    if !handlers.contains(&smoke) {
        problems.push(format!("no generate_handler! registers exactly {smoke:?}: {handlers:?}"));
    }
    if handlers.len() != 2 {
        problems.push(format!("{} generate_handler! lists, not one per build", handlers.len()));
    }
    let granted = sorted(strings(&capability["permissions"]).into_iter().map(str::to_owned).collect());
    let expected = sorted(release.iter().map(|command| permission_of(command)).collect());
    if granted.iter().filter(|permission| !CORE_PERMISSIONS.contains(&permission.as_str())).cloned().collect::<Vec<_>>()
        != expected
    {
        problems.push(format!("main.json grants {granted:?}, build.rs declares {expected:?}"));
    }
    let smoke_granted = sorted(strings(&smoke_capability["permissions"]).into_iter().map(str::to_owned).collect());
    if smoke_granted != sorted(smoke_only.iter().map(|command| permission_of(command)).collect()) {
        problems.push(format!("smoke.json grants {smoke_granted:?}, build.rs declares {smoke_only:?}"));
    }
    for command in &release {
        if !admitted_commands().contains(&command.as_str()) {
            problems.push(format!("{command} is not a command of the threat model's §3.4"));
        }
    }
    problems
}

struct Sources {
    build: String,
    main: String,
    capability: Value,
    smoke: Value,
}

fn sources() -> Sources {
    let dir = app_dir();
    Sources {
        build: read(&dir.join("build.rs")),
        main: read(&dir.join("src/main.rs")),
        capability: read_json(&dir.join("capabilities/main.json")),
        smoke: read_json(&dir.join("capabilities/smoke/smoke.json")),
    }
}

#[test]
fn v02_build_rs_the_handlers_and_the_capability_list_the_same_commands() {
    let s = sources();
    assert_eq!(command_list_problems(&s.build, &s.main, &s.capability, &s.smoke), Vec::<String>::new());
}

#[test]
fn v02_fails_when_one_list_drifts() {
    let s = sources();
    let main_with_extra = s.main.replacen("generate_handler![", "generate_handler![read_file, ", 1);
    let build_without_brief = s.build.replacen(", \"get_brief\"", "", 1);
    assert_ne!(build_without_brief, s.build, "the alteration must change build.rs");
    let mut capability_with_extra = s.capability.clone();
    capability_with_extra["permissions"].as_array_mut().unwrap().push(json!("allow-get-settings"));
    let mut smoke_with_extra = s.smoke.clone();
    smoke_with_extra["permissions"].as_array_mut().unwrap().push(json!("allow-get-brief"));
    let cases = [
        ("a handler no capability allows", command_list_problems(&s.build, &main_with_extra, &s.capability, &s.smoke)),
        ("a command build.rs forgot", command_list_problems(&build_without_brief, &s.main, &s.capability, &s.smoke)),
        ("a permission with no handler", command_list_problems(&s.build, &s.main, &capability_with_extra, &s.smoke)),
        ("a smoke grant of a release command", command_list_problems(&s.build, &s.main, &s.capability, &smoke_with_extra)),
    ];
    for (name, problems) in cases {
        assert!(!problems.is_empty(), "V02 misses {name}");
    }
}

// V05 ----------------------------------------------------------------------------------------------------

fn csp_directive<'a>(csp: &'a str, name: &str) -> Vec<&'a str> {
    csp.split(';')
        .map(str::trim)
        .find_map(|directive| {
            let mut tokens = directive.split_whitespace();
            (tokens.next() == Some(name)).then(|| tokens.collect())
        })
        .unwrap_or_default()
}

fn config_problems(conf: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let app = &conf["app"];
    let security = &app["security"];
    match security["csp"].as_str() {
        None => problems.push(format!("csp is {}, not the policy string", security["csp"])),
        Some(csp) => {
            if csp != CSP {
                problems.push(format!("csp is not the exact policy: {csp}"));
            }
            for directive in ["script-src", "connect-src", "img-src", "font-src", "style-src", "default-src"] {
                for token in csp_directive(csp, directive) {
                    if token.starts_with("'unsafe-") || ["*", "http:", "https:", "data:", "blob:"].contains(&token) {
                        problems.push(format!("{directive} allows {token}"));
                    }
                }
            }
        }
    }
    if app["withGlobalTauri"] != json!(false) {
        problems.push(format!("withGlobalTauri is {}, not false", app["withGlobalTauri"]));
    }
    if security["freezePrototype"] != json!(true) {
        problems.push("freezePrototype is not true".to_owned());
    }
    if security["assetProtocol"] != json!({ "enable": false }) {
        problems.push(format!("assetProtocol is {}, not {{\"enable\": false}}", security["assetProtocol"]));
    }
    for forbidden in ["devCsp", "capabilities", "dangerousDisableAssetCspModification"] {
        if security.get(forbidden).is_some() {
            problems.push(format!("security sets {forbidden}"));
        }
    }
    if security["headers"].get("Content-Security-Policy").is_some() {
        problems.push("the CSP is set as a header too".to_owned());
    }
    if conf["build"].get("devUrl").is_some() {
        problems.push("a devUrl serves the assets without the CSP".to_owned());
    }
    let windows = app["windows"].as_array().cloned().unwrap_or_default();
    if windows.len() != 1 || windows[0]["label"] != "main" || windows[0]["create"] != json!(false) {
        problems.push("the one window, main, is not left to Rust with create: false".to_owned());
    }
    for window in &windows {
        if window.get("useHttpsScheme").is_some_and(|value| value != &json!(false)) {
            problems.push("useHttpsScheme moves the app's origin on Windows".to_owned());
        }
        if window.get("url").is_some() {
            problems.push("the window names a url".to_owned());
        }
    }
    if let Some(updater) = conf["plugins"].get("updater") {
        if updater.get("dangerousInsecureTransportProtocol").is_some() {
            problems.push("the updater allows insecure transport".to_owned());
        }
        if strings(&updater["endpoints"]).iter().any(|endpoint| !endpoint.starts_with("https://")) {
            problems.push("an updater endpoint is not https".to_owned());
        }
    }
    problems
}

#[test]
fn v05_tauri_conf_has_the_exact_csp_and_the_safe_switches() {
    let conf = read_json(&app_dir().join("tauri.conf.json"));
    assert_eq!(config_problems(&conf), Vec::<String>::new());
}

#[test]
fn v05_fails_on_each_alteration_of_the_configuration() {
    let conf = read_json(&app_dir().join("tauri.conf.json"));
    let alterations: [Alteration; 13] = [
        ("unsafe-inline", |c| {
            c["app"]["security"]["csp"] = json!(CSP.replace("script-src 'self'", "script-src 'self' 'unsafe-inline'"))
        }),
        ("unsafe-eval", |c| {
            c["app"]["security"]["csp"] = json!(CSP.replace("script-src 'self'", "script-src 'self' 'unsafe-eval'"))
        }),
        ("https images", |c| c["app"]["security"]["csp"] = json!(CSP.replace("img-src 'self'", "img-src 'self' https:"))),
        ("no policy", |c| c["app"]["security"]["csp"] = Value::Null),
        ("a global Tauri", |c| c["app"]["withGlobalTauri"] = json!(true)),
        ("the asset protocol", |c| c["app"]["security"]["assetProtocol"] = json!({ "enable": true })),
        ("no frozen prototype", |c| c["app"]["security"]["freezePrototype"] = json!(false)),
        ("CSP modification off", |c| c["app"]["security"]["dangerousDisableAssetCspModification"] = json!(true)),
        ("a dev policy", |c| c["app"]["security"]["devCsp"] = json!("default-src *")),
        ("a window Tauri creates", |c| c["app"]["windows"][0]["create"] = json!(true)),
        ("https on Windows", |c| c["app"]["windows"][0]["useHttpsScheme"] = json!(true)),
        ("a dev server", |c| c["build"]["devUrl"] = json!("http://localhost:1420")),
        ("an http updater", |c| c["plugins"]["updater"] = json!({ "endpoints": ["http://example.com/latest.json"] })),
    ];
    for (name, alter) in alterations {
        let mut altered = conf.clone();
        alter(&mut altered);
        assert!(!config_problems(&altered).is_empty(), "V05 misses {name}");
    }
}

// V27 ----------------------------------------------------------------------------------------------------

/// Tauri compiles devtools into a release build only with its `devtools` feature. Only manifests of this
/// workspace can turn it on: the app has no Tauri plugin that could.
fn devtools_problems(workspace: &toml::Table, app: &toml::Table) -> Vec<String> {
    let mut problems = Vec::new();
    let features = |dependency: &toml::Value| -> Vec<String> {
        dependency
            .get("features")
            .and_then(toml::Value::as_array)
            .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    let workspace_tauri = &workspace["workspace"]["dependencies"]["tauri"];
    if features(workspace_tauri).iter().any(|feature| feature == "devtools") {
        problems.push("the workspace turns on tauri/devtools".to_owned());
    }
    let app_tauri = app.get("dependencies").and_then(|deps| deps.get("tauri"));
    if app_tauri.is_some_and(|tauri| features(tauri).iter().any(|feature| feature == "devtools")) {
        problems.push("regia-app turns on tauri/devtools".to_owned());
    }
    if let Some(own) = app.get("features").and_then(toml::Value::as_table) {
        for (name, enables) in own {
            if enables.as_array().is_some_and(|items| items.iter().any(|item| item.as_str() == Some("tauri/devtools"))) {
                problems.push(format!("the feature {name} turns on tauri/devtools"));
            }
        }
    }
    problems
}

fn manifests() -> (toml::Table, toml::Table) {
    let dir = app_dir();
    let workspace = toml::from_str(&read(&dir.join("../../Cargo.toml"))).unwrap();
    let app = toml::from_str(&read(&dir.join("Cargo.toml"))).unwrap();
    (workspace, app)
}

#[test]
fn v27_no_devtools_in_a_release_build() {
    let (workspace, app) = manifests();
    assert_eq!(devtools_problems(&workspace, &app), Vec::<String>::new());
    // V01 refuses `internal-toggle-devtools` in a capability; this is the build side.
    let (mut workspace, mut with_feature) = (workspace, app);
    workspace["workspace"]["dependencies"]["tauri"]["features"].as_array_mut().unwrap().push("devtools".into());
    assert!(!devtools_problems(&workspace, &with_feature).is_empty(), "V27 misses a devtools feature on the workspace");
    let (workspace, _) = manifests();
    with_feature["features"].as_table_mut().unwrap().insert("inspect".into(), vec![toml::Value::from("tauri/devtools")].into());
    assert!(!devtools_problems(&workspace, &with_feature).is_empty(), "V27 misses a feature that turns devtools on");
}

// V03, the npm side --------------------------------------------------------------------------------------

/// The Rust side is `deny.toml`'s bans, which read the lockfile. Here: no `@tauri-apps/plugin-*` package
/// in the frontend's manifest or in its lockfile, where a transitive one would show.
fn npm_plugin_problems(manifest: &Value, lock: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    for section in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
        for name in manifest[section].as_object().into_iter().flat_map(|deps| deps.keys()) {
            if name.starts_with("@tauri-apps/plugin-") {
                problems.push(format!("package.json {section} has {name}"));
            }
        }
    }
    for path in lock["packages"].as_object().into_iter().flat_map(|packages| packages.keys()) {
        if path.contains("node_modules/@tauri-apps/plugin-") {
            problems.push(format!("package-lock.json installs {path}"));
        }
    }
    problems
}

/// The plugins of V03, in the Rust graph: `deny.toml` bans them in CI, and this keeps the same rule in
/// every `cargo test`, as the conformance guards did before it.
const FORBIDDEN_PLUGINS: [&str; 11] = [
    "tauri-plugin-shell",
    "tauri-plugin-fs",
    "tauri-plugin-http",
    "tauri-plugin-opener",
    "tauri-plugin-process",
    "tauri-plugin-clipboard-manager",
    "tauri-plugin-store",
    "tauri-plugin-sql",
    "tauri-plugin-stronghold",
    "tauri-plugin-deep-link",
    "tauri-plugin-autostart",
];

#[test]
fn v03_the_lockfile_has_no_forbidden_plugin() {
    let lock: toml::Table = toml::from_str(&read(&app_dir().join("../../Cargo.lock"))).unwrap();
    let names: Vec<&str> =
        lock["package"].as_array().unwrap().iter().filter_map(|package| package["name"].as_str()).collect();
    assert!(names.contains(&"tauri"), "the lockfile is read");
    for name in names {
        assert!(!FORBIDDEN_PLUGINS.contains(&name), "{name} is not allowed (threat model §3.9)");
    }
}

#[test]
fn v03_the_frontend_installs_no_tauri_plugin() {
    let ui = app_dir().join("../ui");
    let manifest = read_json(&ui.join("package.json"));
    let lock = read_json(&ui.join("package-lock.json"));
    assert_eq!(npm_plugin_problems(&manifest, &lock), Vec::<String>::new());
    assert!(lock["packages"].as_object().is_some_and(|packages| packages.contains_key("node_modules/@tauri-apps/api")));

    let mut with_shell = manifest.clone();
    with_shell["dependencies"]["@tauri-apps/plugin-shell"] = json!("2.0.0");
    assert!(!npm_plugin_problems(&with_shell, &lock).is_empty(), "V03 misses a plugin in package.json");
    let mut with_transitive = lock.clone();
    with_transitive["packages"]["node_modules/x/node_modules/@tauri-apps/plugin-fs"] = json!({ "version": "2.0.0" });
    assert!(!npm_plugin_problems(&manifest, &with_transitive).is_empty(), "V03 misses a plugin in the lockfile");
}

// V28 ----------------------------------------------------------------------------------------------------

/// The core listens to no event of the webview: events go from the core to the webview only (T16).
fn listener_problems(file: &str, source: &str) -> Vec<String> {
    [".listen(", ".listen_any(", ".once(", ".once_any(", "listen_global("]
        .iter()
        .filter(|call| source.contains(*call))
        .map(|call| format!("{file} calls {call}"))
        .collect()
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn v28_the_core_listens_to_no_webview_event() {
    let mut files = vec![app_dir().join("build.rs")];
    rust_sources(&app_dir().join("src"), &mut files);
    let problems: Vec<String> =
        files.iter().flat_map(|path| listener_problems(&path.display().to_string(), &read(path))).collect();
    assert_eq!(problems, Vec::<String>::new());
    assert!(!listener_problems("altered.rs", "app.listen(\"tauri://drag\", |_| {});").is_empty(), "V28 misses listen");
}
