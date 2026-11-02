use base64::Engine;
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf, thread, time::Duration};

fn out_dir() -> Option<PathBuf> {
    env::var_os("MOVIOLA_BENCH_OUT").map(PathBuf::from)
}

fn var(name: &str) -> Value {
    env::var(name).map(Value::String).unwrap_or(Value::Null)
}

/// Everything the page needs to know to run: what to measure (from the environment) and what the host is.
#[tauri::command]
fn bench_config() -> Value {
    let flags = [
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "LIBGL_ALWAYS_SOFTWARE",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "GALLIUM_DRIVER",
        "GDK_BACKEND",
        "MOVIOLA_BENCH_LABEL",
    ];
    let env_flags: serde_json::Map<String, Value> = flags.iter().map(|k| (k.to_string(), var(k))).collect();
    json!({
        "out": out_dir().map(|p| p.to_string_lossy().into_owned()),
        "renderers": var("MOVIOLA_BENCH_RENDERERS"),
        "scales": var("MOVIOLA_BENCH_SCALES"),
        "reps": var("MOVIOLA_BENCH_REPS"),
        "phase_ms": var("MOVIOLA_BENCH_PHASE_MS"),
        "warmup_ms": var("MOVIOLA_BENCH_WARMUP_MS"),
        "repeat": var("MOVIOLA_BENCH_REPEAT"),
        "sanity": env::var("MOVIOLA_BENCH_SANITY").map(|v| v == "1").unwrap_or(false),
        "shots": env::var("MOVIOLA_BENCH_SHOTS").map(|v| v != "0").unwrap_or(true),
        "host": {
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "tauri": tauri::VERSION,
            "webview": tauri::webview_version().ok(),
            "env": env_flags,
        }
    })
}

/// Writes one file under MOVIOLA_BENCH_OUT. The name is a relative path of safe components; nothing else can be written.
#[tauri::command]
fn bench_save(name: String, data: String, base64: bool) -> Result<(), String> {
    let root = out_dir().ok_or("MOVIOLA_BENCH_OUT is not set")?;
    let ok = !name.is_empty()
        && name.split('/').all(|c| {
            !c.is_empty() && c != "." && c != ".." && c.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '+'))
        });
    if !ok {
        return Err(format!("refused file name {name:?}"));
    }
    let path = root.join(&name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = if base64 {
        base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|e| e.to_string())?
    } else {
        data.into_bytes()
    };
    fs::write(path, bytes).map_err(|e| e.to_string())
}

#[tauri::command]
fn bench_exit(app: tauri::AppHandle, code: i32) {
    app.exit(code);
}

pub fn run() {
    // a hung run must not hold a CI runner: hard stop after MOVIOLA_BENCH_TIMEOUT_S (default 40 minutes)
    let limit: u64 = env::var("MOVIOLA_BENCH_TIMEOUT_S").ok().and_then(|v| v.parse().ok()).unwrap_or(2400);
    if out_dir().is_some() {
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(limit));
            eprintln!("moviola-bench: timeout after {limit} s");
            std::process::exit(3);
        });
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![bench_config, bench_save, bench_exit])
        .run(tauri::generate_context!())
        .expect("error while running the tauri application");
}
