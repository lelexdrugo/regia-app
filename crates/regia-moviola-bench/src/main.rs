//! Dev only: the window the moviola's bench runs in (README.md beside this crate). It opens the one page that
//! `npm run bench` serves on the loopback, in the product's own Tauri, wry and webview, and writes nothing: the
//! page sends its numbers back to that server. Nothing is embedded, so the workspace builds it without a page,
//! and it has no command and no capability.

use tauri::{Url, WebviewUrl, WebviewWindowBuilder};

/// The page to open: an `http` URL on the loopback, set by scripts/run-bench.mjs.
const URL_VAR: &str = "MOVIOLA_BENCH_URL";

fn page() -> Result<Url, String> {
    let raw = std::env::var(URL_VAR).map_err(|_| format!("{URL_VAR} is not set: start the bench with `npm run bench`"))?;
    let url = Url::parse(&raw).map_err(|error| format!("{URL_VAR}: {error}"))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost"));
    if url.scheme() != "http" || !loopback {
        return Err(format!("{URL_VAR} must be an http URL on the loopback, not {url}"));
    }
    Ok(url)
}

fn main() {
    let mut url = match page() {
        Ok(url) => url,
        Err(message) => {
            eprintln!("moviola-bench: {message}");
            std::process::exit(2);
        }
    };
    // The page cannot read these: the user agent names neither WebKitGTK's version nor WKWebView's.
    url.query_pairs_mut()
        .append_pair("webview", &tauri::webview_version().unwrap_or_default())
        .append_pair("tauri", tauri::VERSION);
    let origin = url.origin();
    tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("moviola bench")
                .inner_size(1280.0, 800.0)
                .resizable(false)
                .center()
                .focused(true)
                // A covered window gets no animation frames on some engines, and the run would stall.
                .always_on_top(true)
                // The page stays on its server; `/exit?code=N` is how it ends the run, with that exit code.
                .on_navigation(move |to| {
                    if to.origin() != origin {
                        return false;
                    }
                    if to.path() != "/exit" {
                        return true;
                    }
                    let code = to.query_pairs().find(|(key, _)| key == "code").and_then(|(_, value)| value.parse().ok());
                    handle.exit(code.unwrap_or(1));
                    false
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the bench window runs");
}
