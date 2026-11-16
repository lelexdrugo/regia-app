//! V21 on the plugin itself (threat model §6): the real `tauri-plugin-updater`, registered as the app registers
//! it, on Tauri's mock runtime, with keys made in the test. No install ever succeeds here: on Windows a
//! successful one starts an installer and ends the process.

use std::cell::{Cell, RefCell};
use std::io::Cursor;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign::KeyPair;
use regia_update::{Answer, MANIFEST, OWNER, REPO, UpdateError};
use serde_json::json;
use tauri::App;
use tauri::test::{MockRuntime, mock_builder};

use super::*;
use crate::moviola::Moviola;
use crate::settings::Store;

/// A key made for one test, as `tauri signer generate` writes its public half.
struct TestKey(KeyPair);

impl TestKey {
    fn new() -> TestKey {
        TestKey(KeyPair::generate_unencrypted_keypair().unwrap())
    }

    fn public(&self) -> String {
        STANDARD.encode(self.0.pk.to_box().unwrap().to_string())
    }

    /// As `tauri signer sign --app-version <version>` signs, or without the version when none is given.
    fn sign(&self, bytes: &[u8], version: Option<&str>) -> String {
        let mut trusted = format!("timestamp:1700000000\tfile:{PACKAGE}");
        if let Some(version) = version {
            trusted.push_str("\tversion:");
            trusted.push_str(version);
        }
        let signature =
            minisign::sign(None, &self.0.sk, Cursor::new(bytes), Some(&trusted), Some("signature from tauri secret key")).unwrap();
        STANDARD.encode(signature.to_string())
    }
}

const PACKAGE: &str = "app_9.9.9_x64-setup.exe";
const BASE: &str = "https://api.github.test";

fn app() -> App<MockRuntime> {
    crate::builder(mock_builder(), Store::new(None), Moviola::default()).build(tauri::generate_context!()).expect("the mock app")
}

fn url() -> String {
    format!("https://github.com/owner/repo/releases/download/v9.9.9/{PACKAGE}")
}

/// A manifest as `release.yml` writes it, with this system's key of the plugin (`<os>-<arch>`).
fn manifest(version: &str, url: &str, signature: &str) -> Value {
    let target = tauri_plugin_updater::target().expect("a system the plugin updates");
    json!({ "version": version, "notes": "n", "platforms": { target: { "url": url, "signature": signature } } })
}

fn asset(id: u64, name: &str) -> Asset {
    Asset { id, name: name.to_owned(), size: 9, state: "uploaded".to_owned() }
}

fn release() -> Release {
    Release { tag_name: "v9.9.9".to_owned(), draft: true, prerelease: false, assets: vec![asset(11, MANIFEST), asset(12, PACKAGE)] }
}

#[test]
fn v21_the_plugin_takes_a_package_signed_for_the_version_it_announces() {
    let app = app();
    let key = TestKey::new();
    let bytes = b"a package".to_vec();
    let signed = manifest("9.9.9", &url(), &key.sign(&bytes, Some("9.9.9")));
    let (update, asset) = offered(app.handle(), &key.public(), signed, &release()).unwrap().expect("a newer release");
    assert_eq!((update.version.as_str(), asset.id), ("9.9.9", 12));
    assert_eq!(update.current_version, env!("CARGO_PKG_VERSION"));
    update.verify(&bytes).expect("the package verifies");
}

#[test]
fn v21_the_plugin_refuses_an_altered_package() {
    let app = app();
    let key = TestKey::new();
    let bytes = b"a package".to_vec();
    let signed = manifest("9.9.9", &url(), &key.sign(&bytes, Some("9.9.9")));
    let (update, _) = offered(app.handle(), &key.public(), signed, &release()).unwrap().unwrap();
    let mut altered = bytes.clone();
    altered[0] ^= 1;
    assert!(update.verify(&altered).is_err(), "an altered byte verified");
    assert!(update.install(&altered).is_err(), "install did not check first");
}

#[test]
fn v21_the_plugin_refuses_a_signature_of_another_key() {
    let app = app();
    let (ours, theirs) = (TestKey::new(), TestKey::new());
    let bytes = b"a package".to_vec();
    let signed = manifest("9.9.9", &url(), &theirs.sign(&bytes, Some("9.9.9")));
    let (update, _) = offered(app.handle(), &ours.public(), signed, &release()).unwrap().unwrap();
    assert!(update.verify(&bytes).is_err(), "another key's signature verified");
}

/// The manifest is not signed: restored from it, the update demands the version inside the signature, so a
/// valid package of another release cannot be offered under a newer number.
#[test]
fn v21_the_plugin_refuses_a_signature_for_another_version_or_for_none() {
    let app = app();
    let key = TestKey::new();
    let bytes = b"a package".to_vec();
    for signed_for in [Some("9.9.8"), None] {
        let lying = manifest("9.9.9", &url(), &key.sign(&bytes, signed_for));
        let (update, _) = offered(app.handle(), &key.public(), lying, &release()).unwrap().unwrap();
        assert!(update.verify(&bytes).is_err(), "a package signed for {signed_for:?} passed as 9.9.9");
    }
}

#[test]
fn v21_a_lower_or_equal_version_is_no_update() {
    let app = app();
    let key = TestKey::new();
    for older in ["0.0.0", env!("CARGO_PKG_VERSION")] {
        let signed = manifest(older, &url(), &key.sign(b"x", Some(older)));
        assert!(offered(app.handle(), &key.public(), signed, &release()).unwrap().is_none(), "{older} was offered");
    }
}

#[test]
fn v21_an_http_url_is_refused() {
    let app = app();
    let key = TestKey::new();
    let http = url().replacen("https://", "http://", 1);
    let signed = manifest("9.9.9", &http, &key.sign(b"x", Some("9.9.9")));
    let refused = offered(app.handle(), &key.public(), signed, &release());
    assert!(refused.is_err_and(|reason| reason.contains("https")), "an http URL was taken");
}

// A whole round, on recorded answers ---------------------------------------------------------------------

/// GitHub as the channel reads it: the release, its manifest and its package, by URL. It keeps what was asked.
struct Served {
    answers: Vec<(String, Vec<u8>)>,
    asked: RefCell<Vec<String>>,
}

impl Transport for Served {
    fn get(&self, url: &str, _: Option<&Token>, _: &'static str, _: u64) -> Result<Answer, UpdateError> {
        self.asked.borrow_mut().push(url.to_owned());
        let (_, body) = self.answers.iter().find(|(known, _)| known == url).ok_or(UpdateError::Unreachable)?;
        Ok(Answer { status: 200, location: None, body: body.clone() })
    }
}

fn served(manifest: &Value, package: &[u8]) -> Channel<Served> {
    let repo = format!("{BASE}/repos/{OWNER}/{REPO}");
    let release = json!({
        "tag_name": "v9.9.9", "draft": false,
        "assets": [
            { "id": 11, "name": MANIFEST, "state": "uploaded" },
            { "id": 12, "name": PACKAGE, "state": "uploaded" }
        ]
    });
    let answers = vec![
        (format!("{repo}/releases/latest"), serde_json::to_vec(&release).unwrap()),
        (format!("{repo}/releases/assets/11"), serde_json::to_vec(manifest).unwrap()),
        (format!("{repo}/releases/assets/12"), package.to_vec()),
    ];
    Channel::at(BASE, Served { answers, asked: RefCell::new(Vec::new()) })
}

/// The test's own answers to the native questions (§3.8): compiled in tests only, never in a build (V26).
struct Answering {
    yes: bool,
    asked: Cell<u32>,
    told: RefCell<Vec<String>>,
}

impl Answering {
    fn new(yes: bool) -> Answering {
        Answering { yes, asked: Cell::new(0), told: RefCell::new(Vec::new()) }
    }
}

impl Confirm for Answering {
    fn install(&self, offered: &str) -> bool {
        assert_eq!(offered, "9.9.9");
        self.asked.set(self.asked.get() + 1);
        self.yes
    }

    fn failed(&self, reason: &str) {
        self.told.borrow_mut().push(reason.to_owned());
    }
}

fn token() -> Token {
    Token::parse(b"canary-bearer-7f3a-not-a-real-token\n").unwrap()
}

#[test]
fn nothing_is_downloaded_until_the_user_says_yes() {
    let app = app();
    let key = TestKey::new();
    let bytes = b"a package".to_vec();
    let channel = served(&manifest("9.9.9", &url(), &key.sign(&bytes, Some("9.9.9"))), &bytes);
    let no = Answering::new(false);
    let outcome = round(app.handle(), &channel, &token(), &Selector::Latest, &key.public(), &no);
    assert_eq!(outcome, Ok(Outcome::Declined));
    assert_eq!(no.asked.get(), 1);
    assert!(!channel_asked(&channel).iter().any(|url| url.ends_with("/assets/12")), "the package was downloaded");
}

#[test]
fn an_accepted_package_that_does_not_verify_is_not_installed_and_the_user_is_told() {
    let app = app();
    let (ours, theirs) = (TestKey::new(), TestKey::new());
    let bytes = b"a package".to_vec();
    let channel = served(&manifest("9.9.9", &url(), &theirs.sign(&bytes, Some("9.9.9"))), &bytes);
    let yes = Answering::new(true);
    let outcome = round(app.handle(), &channel, &token(), &Selector::Latest, &ours.public(), &yes);
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(channel_asked(&channel).iter().any(|url| url.ends_with("/assets/12")), "the package was read");
    assert_eq!(yes.told.borrow().len(), 1, "the user was not told");
}

#[test]
fn an_older_release_asks_nothing() {
    let app = app();
    let key = TestKey::new();
    let channel = served(&manifest("0.0.0", &url(), &key.sign(b"x", Some("0.0.0"))), b"x");
    let silent = Answering::new(true);
    assert_eq!(round(app.handle(), &channel, &token(), &Selector::Latest, &key.public(), &silent), Ok(Outcome::UpToDate));
    assert_eq!(silent.asked.get(), 0);
}

fn channel_asked(channel: &Channel<Served>) -> Vec<String> {
    channel.transport().asked.borrow().clone()
}
