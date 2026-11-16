use std::cell::RefCell;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use minisign::KeyPair;
use serde_json::json;

use super::*;

/// Visible ASCII as a token is, and no token's shape (V18 greps the repo for them).
const CANARY: &str = "canary-bearer-7f3a-not-a-real-token";

/// A key made for one test, as `tauri signer generate` writes its public half.
struct TestKey(KeyPair);

impl TestKey {
    fn new() -> TestKey {
        TestKey(KeyPair::generate_unencrypted_keypair().unwrap())
    }

    fn public(&self) -> String {
        STANDARD.encode(self.0.pk.to_box().unwrap().to_string())
    }

    /// A signature as `tauri signer sign --app-version <version>` makes it: the trusted comment is
    /// `timestamp:…\tfile:…\tversion:…`, without the last field when no version is given.
    fn sign(&self, bytes: &[u8], version: Option<&str>) -> String {
        let mut trusted = "timestamp:1700000000\tfile:app_9.9.9_x64-setup.exe".to_owned();
        if let Some(version) = version {
            trusted.push_str("\tversion:");
            trusted.push_str(version);
        }
        let signature =
            minisign::sign(None, &self.0.sk, Cursor::new(bytes), Some(&trusted), Some("signature from tauri secret key")).unwrap();
        STANDARD.encode(signature.to_string())
    }
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn token() -> Token {
    Token::parse(format!("{CANARY}\n").as_bytes()).unwrap()
}

fn asset(id: u64, name: &str) -> Asset {
    Asset { id, name: name.to_owned(), size: 3, state: "uploaded".to_owned() }
}

fn release() -> Release {
    Release {
        tag_name: "v9.9.9".to_owned(),
        draft: true,
        prerelease: false,
        assets: vec![asset(11, MANIFEST), asset(12, "regia-9.9.9-x86_64-unknown-linux-gnu"), asset(13, "app_9.9.9_x64-setup.exe")],
    }
}

fn manifest(version: &str, url: &str, signature: &str) -> Value {
    json!({ "version": version, "platforms": { "cli-linux-x86_64": { "url": url, "signature": signature } } })
}

const URL: &str = "https://github.com/owner/repo/releases/download/v9.9.9/regia-9.9.9-x86_64-unknown-linux-gnu";

// The token ----------------------------------------------------------------------------------------------

#[test]
fn a_token_is_one_line_of_visible_ascii_and_never_prints() {
    let token = Token::parse(format!("{CANARY}\r\n").as_bytes()).unwrap();
    assert_eq!(token.bearer(), format!("Bearer {CANARY}"));
    assert_eq!(format!("{token:?}"), "Token(<hidden>)");
    for bad in ["", "\n", "two words\n", "tab\there", "line\nbreak\n", "acc\u{e8}nt\n"] {
        assert!(Token::parse(bad.as_bytes()).is_err(), "{bad:?} became a token");
    }
    assert!(Token::parse(&[0xff, 0xfe]).is_err());
}

#[test]
fn a_tag_names_a_version_or_nothing() {
    assert_eq!(Selector::parse(None).unwrap(), Selector::Latest);
    assert_eq!(Selector::parse(Some("  ")).unwrap(), Selector::Latest);
    assert_eq!(Selector::parse(Some("v0.0.2-test.1")).unwrap(), Selector::Tag("v0.0.2-test.1".to_owned()));
    for bad in ["0.0.2", "v", "vx", "v1/../2", "v1 2", "v1?x=2", "-v1", &format!("v1.{}", "0".repeat(70))] {
        assert!(Selector::parse(Some(bad)).is_err(), "{bad:?} was taken as a tag");
    }
    assert_eq!(Selector::variable(), regia_identity::env_var("UPDATE_TAG"));
}

// The key of this build -----------------------------------------------------------------------------------

/// Until the owner generates the key the configuration's key is empty and updates are off; once it is
/// written, it must decode, or no update could ever verify.
#[test]
fn the_build_key_is_off_or_a_minisign_key() {
    match pubkey() {
        None => {}
        Some(key) => assert!(key_decodes(key), "plugins.updater.pubkey of tauri.conf.json is not a minisign key"),
    }
    assert!(key_decodes(&TestKey::new().public()));
    for bad in ["", "not base64!", &STANDARD.encode("untrusted comment: x\nnot a key")] {
        assert!(!key_decodes(bad), "{bad:?} decoded as a key");
    }
}

// V21 on the CLI's check ------------------------------------------------------------------------------------

#[test]
fn v21_a_signed_package_verifies_for_its_version() {
    let key = TestKey::new();
    let bytes = b"the package";
    verify(bytes, &key.sign(bytes, Some("9.9.9")), &key.public(), &version("9.9.9")).unwrap();
    // `v9.9.9` and `9.9.9` are one version, as in the plugin.
    verify(bytes, &key.sign(bytes, Some("v9.9.9")), &key.public(), &version("9.9.9")).unwrap();
}

#[test]
fn v21_an_altered_package_is_refused() {
    let key = TestKey::new();
    let signature = key.sign(b"the package", Some("9.9.9"));
    let error = verify(b"the packagE", &signature, &key.public(), &version("9.9.9")).unwrap_err();
    assert!(matches!(error, UpdateError::BadSignature(_)), "{error}");
    let mut longer = b"the package".to_vec();
    longer.push(0);
    assert!(verify(&longer, &signature, &key.public(), &version("9.9.9")).is_err());
}

#[test]
fn v21_a_signature_of_another_key_is_refused() {
    let (ours, theirs) = (TestKey::new(), TestKey::new());
    let bytes = b"the package";
    let error = verify(bytes, &theirs.sign(bytes, Some("9.9.9")), &ours.public(), &version("9.9.9")).unwrap_err();
    assert!(matches!(error, UpdateError::BadSignature(_)), "{error}");
}

/// The manifest is not signed: a version in it that the signature does not carry is a package of another
/// release, offered under a number it was not made for.
#[test]
fn v21_a_signature_for_another_version_or_for_none_is_refused() {
    let key = TestKey::new();
    let bytes = b"the package";
    let error = verify(bytes, &key.sign(bytes, Some("9.9.8")), &key.public(), &version("9.9.9")).unwrap_err();
    assert!(matches!(error, UpdateError::VersionMismatch { .. }), "{error}");
    let error = verify(bytes, &key.sign(bytes, None), &key.public(), &version("9.9.9")).unwrap_err();
    assert!(matches!(error, UpdateError::UnsignedVersion), "{error}");
    assert!(verify(bytes, "not base64", &key.public(), &version("9.9.9")).is_err());
}

#[test]
fn v21_a_lower_or_equal_version_is_no_update() {
    let key = TestKey::new();
    let signature = key.sign(b"x", Some("9.9.8"));
    let current = version("9.9.9");
    for older in ["9.9.8", "v9.9.9", "9.9.9-rc.1", "0.0.1"] {
        let offered = offer(&manifest(older, URL, &signature), &release(), &current, "cli-linux-x86_64").unwrap();
        assert_eq!(offered, None, "{older} was offered over {current}");
    }
    let newer = offer(&manifest("10.0.0", URL, &signature), &release(), &current, "cli-linux-x86_64").unwrap().unwrap();
    assert_eq!(newer.version, version("10.0.0"));
    assert_eq!(newer.asset.id, 12);
}

#[test]
fn v21_an_http_url_is_refused() {
    let http = URL.replacen("https://", "http://", 1);
    let error = offer(&manifest("10.0.0", &http, "sig"), &release(), &version("9.9.9"), "cli-linux-x86_64").unwrap_err();
    assert!(matches!(error, UpdateError::Insecure(_)), "{error}");
    assert!(matches!(asset_for(&release(), &http), Err(UpdateError::Insecure(_))));
}

#[test]
fn a_manifest_url_names_one_whole_asset_of_the_release() {
    let release = release();
    assert_eq!(asset_for(&release, URL).unwrap().id, 12);
    // Elsewhere on the web, the same file name still means this release's asset: the URL is never asked.
    assert_eq!(asset_for(&release, "https://example.invalid/x/app_9.9.9_x64-setup.exe").unwrap().id, 13);
    for bad in [
        "https://github.com/owner/repo/releases/download/v9.9.9/",
        "https://github.com/owner/repo/releases/download/v9.9.9/other-file",
        "https://github.com/owner/repo/releases/download/v9.9.9/app_9.9.9%5Fx64-setup.exe",
        "not a url",
    ] {
        assert!(asset_for(&release, bad).is_err(), "{bad} named an asset");
    }
    let mut broken = release.clone();
    broken.assets[1].state = "starter".to_owned();
    assert!(matches!(asset_for(&broken, URL), Err(UpdateError::MissingAsset(_))));
    let error = offer(&json!({ "version": "10.0.0", "platforms": {} }), &release, &version("9.9.9"), "cli-linux-x86_64");
    assert!(matches!(error, Err(UpdateError::NoEntry(_))));
}

#[test]
fn the_cli_key_sits_beside_the_plugin_targets() {
    let key = cli_key().expect("a system the releases build for");
    assert!(key.starts_with("cli-") && ["windows", "darwin", "linux"].iter().any(|os| key.contains(os)), "{key}");
}

// The channel, on recorded answers ------------------------------------------------------------------------

/// Answers by URL, and keeps every request: its URL, whether it carried the token, and its `Accept`.
#[derive(Default)]
struct Recorded {
    answers: Vec<(String, Answer)>,
    seen: RefCell<Vec<(String, bool, &'static str)>>,
}

impl Recorded {
    fn answer(mut self, url: &str, status: u16, location: Option<&str>, body: &[u8]) -> Recorded {
        let answer = Answer { status, location: location.map(str::to_owned), body: body.to_vec() };
        self.answers.push((url.to_owned(), answer));
        self
    }
}

impl Transport for Recorded {
    fn get(&self, url: &str, bearer: Option<&Token>, accept: &'static str, limit: u64) -> Result<Answer> {
        self.seen.borrow_mut().push((url.to_owned(), bearer.is_some(), accept));
        let (_, answer) = self.answers.iter().find(|(known, _)| known == url).ok_or(UpdateError::Unreachable)?;
        if answer.body.len() as u64 > limit {
            return Err(UpdateError::BadAnswer("too long".to_owned()));
        }
        Ok(Answer { status: answer.status, location: answer.location.clone(), body: answer.body.clone() })
    }
}

const BASE: &str = "https://api.github.test";

fn repo(path: &str) -> String {
    format!("{BASE}/repos/{OWNER}/{REPO}{path}")
}

fn release_json() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "tag_name": "v9.9.9", "draft": false, "prerelease": false,
        "assets": [
            { "id": 11, "name": MANIFEST, "size": 30, "state": "uploaded", "url": "https://evil.invalid/steal" },
            { "id": 12, "name": "regia-9.9.9-x86_64-unknown-linux-gnu", "size": 3, "state": "uploaded" }
        ]
    }))
    .unwrap()
}

#[test]
fn the_token_goes_to_the_api_and_the_storage_gets_none() {
    let storage = "https://storage.github.test/signed?sig=abc";
    let transport = Recorded::default()
        .answer(&repo("/releases/latest"), 200, None, &release_json())
        .answer(&repo("/releases/assets/11"), 302, Some(storage), b"")
        .answer(storage, 200, None, br#"{ "version": "9.9.9", "platforms": {} }"#);
    let channel = Channel::at(BASE, transport);
    let release = channel.release(&token(), &Selector::Latest).unwrap();
    assert_eq!(release.tag_name, "v9.9.9");
    let manifest = channel.manifest(&token(), &release).unwrap();
    assert_eq!(manifest_version(&manifest).unwrap(), version("9.9.9"));
    let seen = channel.transport.seen.borrow();
    assert_eq!(
        *seen,
        [
            (repo("/releases/latest"), true, JSON),
            // The asset's `url` in the answer is never used: the API URL is built from the id.
            (repo("/releases/assets/11"), true, OCTET),
            (storage.to_owned(), false, OCTET),
        ]
    );
}

#[test]
fn a_redirect_to_plain_http_is_refused() {
    let transport = Recorded::default()
        .answer(&repo("/releases/latest"), 200, None, &release_json())
        .answer(&repo("/releases/assets/11"), 302, Some("http://storage.github.test/x"), b"");
    let channel = Channel::at(BASE, transport);
    let release = channel.release(&token(), &Selector::Latest).unwrap();
    assert!(matches!(channel.manifest(&token(), &release), Err(UpdateError::Insecure(_))));
    assert_eq!(channel.transport.seen.borrow().len(), 2, "the http target was not asked");
}

#[test]
fn a_tag_finds_a_draft_in_the_listing_and_its_absence_says_why() {
    let listing = serde_json::to_vec(&json!([
        { "tag_name": "v9.9.8", "draft": false, "assets": [] },
        { "tag_name": "v9.9.9", "draft": true, "assets": [] }
    ]))
    .unwrap();
    let channel = Channel::at(BASE, Recorded::default().answer(&repo("/releases?per_page=100"), 200, None, &listing));
    let draft = channel.release(&token(), &Selector::Tag("v9.9.9".to_owned())).unwrap();
    assert!(draft.draft);
    let missing = channel.release(&token(), &Selector::Tag("v1.0.0".to_owned()));
    assert!(matches!(missing, Err(UpdateError::NoSuchTag(_))));
    let none = Channel::at(BASE, Recorded::default().answer(&repo("/releases/latest"), 404, None, b""));
    assert!(matches!(none.release(&token(), &Selector::Latest), Err(UpdateError::NoRelease)));
    let refused = Channel::at(BASE, Recorded::default().answer(&repo("/releases/latest"), 401, None, b""));
    assert!(matches!(refused.release(&token(), &Selector::Latest), Err(UpdateError::Status(401))));
}

#[test]
fn a_release_without_its_manifest_is_named_so() {
    let release = Release { assets: vec![], ..self::release() };
    let channel = Channel::at(BASE, Recorded::default());
    assert!(matches!(channel.manifest(&token(), &release), Err(UpdateError::MissingAsset(_))));
}

// The real transport, on a local server -------------------------------------------------------------------

/// A local server over plain HTTP that answers each connection with one of `answers` and sends back the
/// request it received. GitHub is never asked.
fn server(answers: Vec<String>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (sent, received) = mpsc::channel();
    thread::spawn(move || {
        for answer in answers {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            sent.send(request).unwrap();
            let mut stream = stream;
            stream.write_all(answer.as_bytes()).unwrap();
        }
    });
    (address, received)
}

#[test]
fn over_the_wire_the_storage_never_sees_the_token() {
    let body = r#"{ "version": "9.9.9", "platforms": {} }"#;
    let (storage, storage_requests) =
        server(vec![format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())]);
    let target = format!("{storage}/release-asset?sig=abc");
    let (api, api_requests) = server(vec![format!(
        "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )]);
    let channel = Channel::at(&api, Https::plain_for_tests());
    let bytes = channel.download(&token(), &asset(11, MANIFEST), MANIFEST_LIMIT).unwrap();
    assert_eq!(bytes, body.as_bytes());
    let first = api_requests.recv().unwrap().to_ascii_lowercase();
    assert!(first.starts_with(&format!("get /repos/{OWNER}/{REPO}/releases/assets/11 http/1.1")), "{first}");
    assert!(first.contains(&format!("authorization: bearer {}", CANARY.to_ascii_lowercase())), "{first}");
    assert!(first.contains("accept: application/octet-stream"), "{first}");
    let second = storage_requests.recv().unwrap().to_ascii_lowercase();
    assert!(second.starts_with("get /release-asset?sig=abc http/1.1"), "{second}");
    assert!(!second.contains("authorization") && !second.contains(&CANARY.to_ascii_lowercase()), "{second}");
}

/// By hand, on a folder that `finalize` of `release.yml` signed with a throwaway key: `UPDATE_PROBE=<folder>`
/// holding `latest.json`, the files it names and `key.pub`. What `tauri signer sign --app-version` writes
/// verifies here, every entry of the manifest maps to its file, and an altered byte fails.
#[test]
#[ignore = "needs a folder signed by the Tauri CLI"]
fn what_the_tauri_cli_signs_verifies_here() {
    let dir = std::path::PathBuf::from(std::env::var("UPDATE_PROBE").expect("UPDATE_PROBE names the folder"));
    let manifest: Value = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST)).unwrap()).unwrap();
    let pubkey = std::fs::read_to_string(dir.join("key.pub")).unwrap();
    let names: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    let assets = names.iter().zip(1..).map(|(name, id)| asset(id, name)).collect();
    let release = Release { tag_name: "v9.9.9".to_owned(), draft: true, prerelease: false, assets };
    let keys: Vec<String> = manifest["platforms"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys.len(), 14, "{keys:?}");
    for key in keys {
        let offered = offer(&manifest, &release, &version("0.0.1"), &key).unwrap().expect("a newer version");
        let mut bytes = std::fs::read(dir.join(&offered.asset.name)).unwrap();
        verify(&bytes, &offered.signature, pubkey.trim(), &offered.version).unwrap_or_else(|error| panic!("{key}: {error}"));
        bytes.push(0);
        assert!(verify(&bytes, &offered.signature, pubkey.trim(), &offered.version).is_err(), "{key}: altered");
        eprintln!("{key}: {} verifies", offered.asset.name);
    }
}

/// By hand, with the network and gh signed in: the channel against GitHub itself, read only. Until the first
/// release the latest is none, and a tag that does not exist is named so.
#[test]
#[ignore = "needs the network and gh"]
fn github_answers_the_channel() {
    let token = Token::from_gh().expect("gh auth token");
    let channel = Channel::github();
    let latest = channel.release(&token, &Selector::Latest);
    eprintln!("latest: {:?}", latest.as_ref().map(|release| &release.tag_name));
    assert!(matches!(latest, Ok(_) | Err(UpdateError::NoRelease)), "{latest:?}");
    let missing = channel.release(&token, &Selector::Tag("v0.0.0-none".to_owned()));
    assert!(matches!(missing, Err(UpdateError::NoSuchTag(_))), "{missing:?}");
}

#[test]
fn the_product_transport_refuses_plain_http() {
    let (address, _requests) = server(vec![]);
    let channel = Channel::at(&address, Https::new());
    let error = channel.release(&token(), &Selector::Latest).unwrap_err();
    assert!(!format!("{error:?}").contains(CANARY));
    assert!(matches!(error, UpdateError::Insecure(_) | UpdateError::Unreachable), "{error}");
}
