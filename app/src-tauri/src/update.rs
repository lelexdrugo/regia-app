//! Updates from the private releases (threat model §3.9 and §3.10, T07, V21; the dossier's `rilascio.md` §4.4).
//!
//! At start, in a thread of its own, the core reads the newest release with the token `gh` keeps and its
//! `latest.json` (`regia-update`). `tauri-plugin-updater` restores the update from that manifest, which keeps
//! the target for this system and binds the signature to the announced version. The package is the release's
//! asset the manifest's URL names, downloaded through the API, and the plugin checks it again, with the key
//! of `tauri.conf.json`, before it installs.
//!
//! Nothing happens without a native confirmation, which the webview can neither read nor answer (§3.8). The
//! webview takes no part at all: it has no `updater:*` permission and no command of this module.
//!
//! The spec of #495 names `download_and_install`. That call would download with the plugin's own client, which
//! sends the token to wherever the API redirects and leaves its removal to reqwest. `Update::install` on bytes
//! downloaded by the channel runs the same check first, and the token never leaves `api.github.com`.

// A smoke build registers the plugin and never starts a round, so the round is not called there.
#![cfg_attr(feature = "smoke", allow(dead_code))]

use regia_update::{Asset, Channel, PACKAGE_LIMIT, Release, Selector, Token, Transport, asset_for};
use serde_json::Value;
use tauri::{AppHandle, Runtime, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

/// A smoke build has no round at start: it proves the webview alone, and must not ask gh or open a dialog.
#[cfg(not(feature = "smoke"))]
pub use native::start;

/// The native questions of an update (§3.8). The real one is a system dialog; the tests answer by themselves.
pub trait Confirm {
    /// Whether to install `offered` now.
    fn install(&self, offered: &str) -> bool;
    /// An update the user accepted did not install.
    fn failed(&self, reason: &str);
}

/// What one round did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    Declined,
    /// Installed: on Windows the plugin has started the installer and the app has exited already.
    Installed,
}

/// The endpoint the plugin's builder asks for. It is never requested: the update is restored from the
/// manifest the channel read.
fn endpoint() -> Url {
    let url = format!("{}/repos/{}/{}/releases/latest", regia_update::API, regia_update::OWNER, regia_update::REPO);
    Url::parse(&url).expect("a constant URL")
}

/// The update the manifest offers this system, with the release's asset its URL names: `None` when the
/// release is not newer (no downgrade, V21). A URL that is not https is refused (V21).
pub fn offered<R: Runtime>(
    app: &AppHandle<R>,
    pubkey: &str,
    manifest: Value,
    release: &Release,
) -> Result<Option<(Update, Asset)>, String> {
    let updater = app
        .updater_builder()
        .pubkey(pubkey)
        .endpoints(vec![endpoint()])
        .and_then(|builder| builder.build())
        .map_err(|error| error.to_string())?;
    let Some(update) = updater.restore_update(manifest).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let asset = asset_for(release, update.download_url.as_str()).map_err(|error| error.to_string())?.clone();
    Ok(Some((update, asset)))
}

/// One round: the release, its manifest, the offer, the confirmation, the package and the install. Before the
/// confirmation a failure is quiet; after it, the user is told.
pub fn round<R: Runtime, T: Transport>(
    app: &AppHandle<R>,
    channel: &Channel<T>,
    token: &Token,
    selector: &Selector,
    pubkey: &str,
    confirm: &dyn Confirm,
) -> Result<Outcome, String> {
    let release = channel.release(token, selector).map_err(|error| error.to_string())?;
    let manifest = channel.manifest(token, &release).map_err(|error| error.to_string())?;
    let Some((update, asset)) = offered(app, pubkey, manifest, &release)? else {
        return Ok(Outcome::UpToDate);
    };
    if !confirm.install(&update.version) {
        return Ok(Outcome::Declined);
    }
    let installed = channel
        .download(token, &asset, PACKAGE_LIMIT)
        .map_err(|error| error.to_string())
        // `install` verifies first: the key of this build, and the version the signature carries.
        .and_then(|bytes| update.install(bytes).map_err(|error| error.to_string()));
    match installed {
        Ok(()) => Ok(Outcome::Installed),
        Err(reason) => {
            confirm.failed(&reason);
            Err(reason)
        }
    }
}

#[cfg(not(feature = "smoke"))]
mod native {
    use regia_i18n::Catalog;
    use regia_update::{Channel, Selector, Token};
    use tauri::{AppHandle, Manager, Runtime};

    use super::{Confirm, Outcome, round};
    use crate::settings::Store;

    /// At start: one round off the main thread, unless this build has no update key. A newer release is
    /// installed only if the user says yes; then the app starts again (Windows: the installer starts it).
    pub fn start<R: Runtime>(app: AppHandle<R>) {
        let Some(pubkey) = regia_update::pubkey() else {
            return;
        };
        let language = app.state::<Store>().load().speaks();
        std::thread::spawn(move || {
            let confirm = Native { app: app.clone(), catalog: Catalog::new(language) };
            let outcome = Selector::from_env()
                .and_then(|selector| Token::from_gh().map(|token| (selector, token)))
                .map_err(|error| error.to_string())
                .and_then(|(selector, token)| round(&app, &Channel::github(), &token, &selector, pubkey, &confirm));
            match outcome {
                Ok(Outcome::Installed) => app.restart(),
                Ok(_) => {}
                // No window for a failure the user did not ask about: gh missing, offline, no release yet.
                Err(reason) => eprintln!("update: {reason}"),
            }
        });
    }

    /// The system's dialog, from the core, modal on the main window (§3.8): on the main thread, as the dialog
    /// plugin shows it, and answered to this thread.
    struct Native<R: Runtime> {
        app: AppHandle<R>,
        catalog: Catalog,
    }

    impl<R: Runtime> Native<R> {
        fn show(&self, level: rfd::MessageLevel, description: String, buttons: rfd::MessageButtons) -> rfd::MessageDialogResult {
            let (sent, received) = std::sync::mpsc::channel();
            let title = self.catalog.message("update-title");
            let window = self.app.get_webview_window("main");
            let shown = self.app.run_on_main_thread(move || {
                let mut dialog = rfd::AsyncMessageDialog::new()
                    .set_level(level)
                    .set_title(title)
                    .set_description(description)
                    .set_buttons(buttons);
                if let Some(window) = &window {
                    dialog = dialog.set_parent(window);
                }
                let answer = dialog.show();
                std::thread::spawn(move || {
                    let _ = sent.send(tauri::async_runtime::block_on(answer));
                });
            });
            if shown.is_err() {
                return rfd::MessageDialogResult::Cancel;
            }
            received.recv().unwrap_or(rfd::MessageDialogResult::Cancel)
        }
    }

    impl<R: Runtime> Confirm for Native<R> {
        fn install(&self, offered: &str) -> bool {
            let question = self.catalog.message_with("update-offer", "version", offered);
            let answer = self.show(rfd::MessageLevel::Info, question, rfd::MessageButtons::YesNo);
            matches!(answer, rfd::MessageDialogResult::Yes | rfd::MessageDialogResult::Ok)
        }

        fn failed(&self, reason: &str) {
            let message = self.catalog.message_with("update-failed", "reason", reason);
            self.show(rfd::MessageLevel::Error, message, rfd::MessageButtons::Ok);
        }
    }
}

#[cfg(test)]
mod tests;
