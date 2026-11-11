//! The webview never leaves the app (threat model T14, V09). The main window may navigate only within
//! the app's own origin, and every request for a new window is refused.

use tauri::Url;

/// How this system serves the app's own files to the webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppOrigin {
    /// `tauri://localhost`, on macOS and Linux.
    CustomScheme,
    /// `http://tauri.localhost`, on Windows: WebView2 serves custom protocols on that host.
    Localhost,
}

impl AppOrigin {
    pub const fn current() -> Self {
        if cfg!(any(windows, target_os = "android")) { AppOrigin::Localhost } else { AppOrigin::CustomScheme }
    }

    /// The app's own origin and nothing else: another scheme, a port, a user, or a host that only begins
    /// like ours is refused. `useHttpsScheme` stays off (V05), so Windows never serves `https:`.
    pub fn allows(self, url: &Url) -> bool {
        let (scheme, host) = match self {
            AppOrigin::CustomScheme => ("tauri", "localhost"),
            AppOrigin::Localhost => ("http", "tauri.localhost"),
        };
        url.scheme() == scheme
            && url.host_str() == Some(host)
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::AppOrigin::{CustomScheme, Localhost};
    use super::*;

    fn allows(origin: AppOrigin, url: &str) -> bool {
        origin.allows(&url.parse().unwrap_or_else(|error| panic!("{url}: {error}")))
    }

    #[test]
    fn each_system_allows_its_own_origin() {
        for url in ["tauri://localhost", "tauri://localhost/", "tauri://localhost/index.html", "tauri://localhost/#/brief"] {
            assert!(allows(CustomScheme, url), "{url}");
            assert!(!allows(Localhost, url), "{url} on Windows");
        }
        for url in ["http://tauri.localhost", "http://tauri.localhost/index.html", "http://TAURI.localhost/?q=1"] {
            assert!(allows(Localhost, url), "{url}");
            assert!(!allows(CustomScheme, url), "{url} on macOS and Linux");
        }
    }

    /// V09's list, with the two origins that only begin like ours.
    #[test]
    fn nothing_else_passes_on_either_system() {
        let refused = [
            "http://tauri.localhost.attacker.com",
            "http://tauri.localhost.attacker.com/index.html",
            "tauri://localhost.attacker.com",
            "tauri://localhost.attacker.com/",
            "https://example.com",
            "https://tauri.localhost",
            "http://localhost",
            "http://tauri.localhost:8080",
            "tauri://localhost:1430",
            "http://user@tauri.localhost",
            "http://user:secret@tauri.localhost",
            "tauri://user@localhost",
            "http://attacker.com/tauri.localhost",
            "file:///",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "data:text/html,<p>x</p>",
            "about:blank",
            "ipc://localhost/get_brief",
            "isolation://localhost",
            "blob:tauri://localhost/0",
        ];
        for url in refused {
            assert!(!allows(CustomScheme, url), "{url} on macOS and Linux");
            assert!(!allows(Localhost, url), "{url} on Windows");
        }
    }

    #[test]
    fn this_build_picks_the_origin_of_its_system() {
        let expected = if cfg!(windows) { Localhost } else { CustomScheme };
        assert_eq!(AppOrigin::current(), expected);
    }
}
