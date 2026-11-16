//! The update channel the app and the CLI share (dossier: `docs/app/architettura/rilascio.md` §4.4; threat
//! model §3.10, T07, V21). The releases are private and no package holds a token, so:
//!
//! - **The token** is the one `gh` keeps for github.com, read when an update is looked for and held in memory
//!   only ([`Token::from_gh`]). It goes to `api.github.com` and nowhere else.
//! - **Discovery** reads the release (the latest published one, or one named by its tag, drafts included, for
//!   the owner's test), then downloads its `latest.json` through the API with `Accept: application/octet-stream`.
//!   The API answers with a redirect to a signed URL of GitHub's storage, which is followed here, over https,
//!   without the token ([`Channel::download`]).
//! - **The package** a manifest names is fetched by the file name its URL ends with, as the release's asset of
//!   that name ([`asset_for`]): no URL written in the manifest is ever requested, and one that is not https is
//!   refused (V21).
//! - **The check** is minisign with the key compiled into this build ([`pubkey`]), and the signature must carry
//!   the version the manifest announces, as `tauri-plugin-updater` does with `requireSignedVersion`. The app
//!   hands the bytes to the plugin, which checks them again before it installs; the CLI checks them here
//!   ([`verify`]).

use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use regia_exec::{Pin, Program};
use regia_identity::PRODUCT_NAME;
use semver::Version;
use serde::Deserialize;
use serde_json::Value;
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// The repository whose releases are the update channel. A release here is private: it needs a token.
pub const OWNER: &str = "lelexdrugo";
pub const REPO: &str = "regia-app";
/// The manifest's asset on a release, as `release.yml` uploads it.
pub const MANIFEST: &str = "latest.json";
/// GitHub's REST API: the only host that ever receives the token.
pub const API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const JSON: &str = "application/vnd.github+json";
const OCTET: &str = "application/octet-stream";

/// The largest manifest or release listing read.
pub const MANIFEST_LIMIT: u64 = 1 << 20;
/// The largest package read: the AppImage is the biggest, about 80 MiB.
pub const PACKAGE_LIMIT: u64 = 512 << 20;

/// The app's `tauri.conf.json`: the one place the update key is written, which the plugin reads as its
/// configuration and this crate reads for the CLI.
const TAURI_CONF: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../app/src-tauri/tauri.conf.json"));

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("this build has no update key: updates are off")]
    NoKey,
    #[error("gh is needed to read the releases: {0}")]
    NoGh(String),
    #[error("gh has no token for github.com: run `gh auth login`")]
    NotSignedIn,
    #[error("{0} is not a tag of a release")]
    BadTag(String),
    #[error("GitHub answered HTTP {0}")]
    Status(u16),
    #[error("GitHub could not be reached")]
    Unreachable,
    #[error("GitHub did not answer in time")]
    Timeout,
    #[error("an answer that does not read: {0}")]
    BadAnswer(String),
    #[error("no published release yet")]
    NoRelease,
    #[error("no release has the tag {0}: a draft is visible only to an account that can write to the repository")]
    NoSuchTag(String),
    #[error("the release has no asset {0}")]
    MissingAsset(String),
    #[error("{0}: an update travels over https only")]
    Insecure(String),
    #[error("the manifest has no entry for {0}")]
    NoEntry(String),
    #[error("the signature does not verify: {0}")]
    BadSignature(String),
    #[error("the package is signed for version {signed}, and offered as {offered}")]
    VersionMismatch { signed: String, offered: String },
    #[error("the signature does not name the version it was made for")]
    UnsignedVersion,
}

pub type Result<T> = std::result::Result<T, UpdateError>;

/// The update key of this build: `plugins.updater.pubkey` of the app's `tauri.conf.json`, base64 of minisign's
/// text form, as `tauri signer generate` writes it. `None` while it is empty: then updates are off, in the app
/// and in the CLI.
pub fn pubkey() -> Option<&'static str> {
    static KEY: OnceLock<Option<String>> = OnceLock::new();
    KEY.get_or_init(|| {
        let conf: Value = serde_json::from_str(TAURI_CONF).ok()?;
        conf["plugins"]["updater"]["pubkey"].as_str().filter(|key| !key.is_empty()).map(str::to_owned)
    })
    .as_deref()
}

/// Whether `key` is a minisign public key in the plugin's form (V21's check of the configuration).
pub fn key_decodes(key: &str) -> bool {
    text_of(key).is_ok_and(|text| minisign_verify::PublicKey::decode(&text).is_ok())
}

fn text_of(base64: &str) -> Result<String> {
    let bytes = STANDARD.decode(base64.trim()).map_err(|error| UpdateError::BadSignature(error.to_string()))?;
    String::from_utf8(bytes).map_err(|_| UpdateError::BadSignature("not text".to_owned()))
}

/// A token of GitHub, on its way to `api.github.com`. It does not print: `Debug` hides it, and it has no
/// `Display`, no `Serialize` and no `Clone`.
pub struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<hidden>)")
    }
}

impl Token {
    /// What `gh auth token` printed: one token, then a line end. Anything else is not used as a header.
    pub fn parse(stdout: &[u8]) -> Result<Token> {
        let text = std::str::from_utf8(stdout).map_err(|_| UpdateError::NotSignedIn)?;
        let token = text.strip_suffix('\n').unwrap_or(text);
        let token = token.strip_suffix('\r').unwrap_or(token);
        let shaped = !token.is_empty() && token.len() <= 1024 && token.bytes().all(|byte| byte.is_ascii_graphic());
        if shaped { Ok(Token(token.to_owned())) } else { Err(UpdateError::NotSignedIn) }
    }

    /// `gh auth token --hostname=github.com`, through the pinned `gh` and its allow-listed environment.
    pub fn from_gh() -> Result<Token> {
        let gh = Pin::resolve(Program::Gh).map_err(|error| UpdateError::NoGh(error.to_string()))?;
        let output = gh
            .launch(regia_exec::gh::auth_token(), &regia_exec::neutral_folder())
            .run()
            .map_err(|error| UpdateError::NoGh(error.to_string()))?;
        if !output.status.success() {
            return Err(UpdateError::NotSignedIn);
        }
        Token::parse(&output.stdout)
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

/// Which release the channel reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// The latest published release: GitHub leaves drafts and prereleases out.
    Latest,
    /// The release with this tag, a draft too: the owner's test of a release before it is published.
    Tag(String),
}

impl Selector {
    /// The variable that names a release by its tag (`REGIA_UPDATE_TAG`): without it, the latest one.
    pub fn variable() -> String {
        regia_identity::env_var("UPDATE_TAG")
    }

    /// From the variable's value: empty or absent is the latest release.
    pub fn parse(value: Option<&str>) -> Result<Selector> {
        match value.map(str::trim) {
            None | Some("") => Ok(Selector::Latest),
            Some(tag) => {
                let shaped = tag.len() <= 64
                    && tag.starts_with('v')
                    && tag[1..].starts_with(|c: char| c.is_ascii_digit())
                    && tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
                if shaped { Ok(Selector::Tag(tag.to_owned())) } else { Err(UpdateError::BadTag(tag.to_owned())) }
            }
        }
    }

    pub fn from_env() -> Result<Selector> {
        Selector::parse(std::env::var(Selector::variable()).ok().as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub size: u64,
    /// `uploaded` once the asset is whole; an upload that broke leaves `starter`.
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    /// The whole asset with this exact name.
    pub fn asset(&self, name: &str) -> Result<&Asset> {
        self.assets
            .iter()
            .find(|asset| asset.name == name && asset.state == "uploaded")
            .ok_or_else(|| UpdateError::MissingAsset(name.to_owned()))
    }
}

/// One GET: the status, the redirect's target and, for a 2xx, the body.
#[derive(Debug, Default)]
pub struct Answer {
    pub status: u16,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

/// How the channel reaches GitHub. The real one is [`Https`]; a test can answer by itself.
pub trait Transport {
    /// GET `url` with `Accept: accept`, and `bearer` in `Authorization` when there is one. No redirect is
    /// followed, and a 2xx body is read to at most `limit` bytes.
    fn get(&self, url: &str, bearer: Option<&Token>, accept: &'static str, limit: u64) -> Result<Answer>;
}

/// GETs over the system's TLS and its roots, as the usage probes make them (threat model §3.12): no redirect
/// followed, a stall of 30 s ends the call, and a whole download ends within 15 minutes.
pub struct Https {
    agent: Agent,
}

impl Default for Https {
    fn default() -> Https {
        Https::new()
    }
}

impl Https {
    #[must_use]
    pub fn new() -> Https {
        Https::configured(true)
    }

    /// Plain http, for a test's local server only. The product's channel is always [`Https::new`].
    #[doc(hidden)]
    #[must_use]
    pub fn plain_for_tests() -> Https {
        Https::configured(false)
    }

    fn configured(https_only: bool) -> Https {
        let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
        let config = Agent::config_builder()
            .https_only(https_only)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .timeout_global(Some(Duration::from_secs(15 * 60)))
            .user_agent(format!("{PRODUCT_NAME}-update/{}", env!("CARGO_PKG_VERSION")))
            .tls_config(tls)
            .build();
        Https { agent: config.into() }
    }
}

impl Transport for Https {
    fn get(&self, url: &str, bearer: Option<&Token>, accept: &'static str, limit: u64) -> Result<Answer> {
        let mut request = self.agent.get(url).header("Accept", accept).header("X-GitHub-Api-Version", API_VERSION);
        if let Some(token) = bearer {
            request = request.header("Authorization", token.bearer());
        }
        let mut answer = request.call().map_err(failure)?;
        let status = answer.status().as_u16();
        let location = answer.headers().get("location").and_then(|value| value.to_str().ok()).map(str::to_owned);
        let body = if (200..300).contains(&status) {
            answer.body_mut().with_config().limit(limit).read_to_vec().map_err(failure)?
        } else {
            Vec::new()
        };
        Ok(Answer { status, location, body })
    }
}

/// The kind of a failure, and nothing of the request: no URL, no header.
fn failure(error: ureq::Error) -> UpdateError {
    match error {
        ureq::Error::StatusCode(code) => UpdateError::Status(code),
        ureq::Error::Timeout(_) => UpdateError::Timeout,
        ureq::Error::BodyExceedsLimit(limit) => UpdateError::BadAnswer(format!("more than {limit} bytes")),
        ureq::Error::RequireHttpsOnly(_) => UpdateError::Insecure("a URL".to_owned()),
        _ => UpdateError::Unreachable,
    }
}

/// The releases of the channel's repository, through `transport`.
pub struct Channel<T: Transport = Https> {
    base: String,
    transport: T,
}

impl Channel<Https> {
    /// The product's channel: [`API`] over [`Https`].
    #[must_use]
    pub fn github() -> Channel<Https> {
        Channel { base: API.to_owned(), transport: Https::new() }
    }
}

impl<T: Transport> Channel<T> {
    /// Another API root and transport, for a test. The token still goes only to `base`.
    pub fn at(base: &str, transport: T) -> Channel<T> {
        Channel { base: base.trim_end_matches('/').to_owned(), transport }
    }

    /// The transport, for a test that looks at what was asked.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    fn repo(&self, path: &str) -> String {
        format!("{}/repos/{OWNER}/{REPO}{path}", self.base)
    }

    /// The API URL of an asset: built from its id, never read from an answer.
    #[must_use]
    pub fn asset_url(&self, asset: &Asset) -> String {
        self.repo(&format!("/releases/assets/{}", asset.id))
    }

    fn api_json(&self, token: &Token, url: &str) -> Result<Option<Vec<u8>>> {
        let answer = self.transport.get(url, Some(token), JSON, MANIFEST_LIMIT)?;
        match answer.status {
            200 => Ok(Some(answer.body)),
            404 => Ok(None),
            status => Err(UpdateError::Status(status)),
        }
    }

    /// The release `selector` names.
    pub fn release(&self, token: &Token, selector: &Selector) -> Result<Release> {
        match selector {
            Selector::Latest => {
                let body = self.api_json(token, &self.repo("/releases/latest"))?.ok_or(UpdateError::NoRelease)?;
                serde_json::from_slice(&body).map_err(|error| UpdateError::BadAnswer(error.to_string()))
            }
            Selector::Tag(tag) => {
                // The listing has the drafts the token's account can see; a lookup by tag would not.
                let body = self.api_json(token, &self.repo("/releases?per_page=100"))?.unwrap_or_default();
                let releases: Vec<Release> =
                    serde_json::from_slice(&body).map_err(|error| UpdateError::BadAnswer(error.to_string()))?;
                releases.into_iter().find(|release| &release.tag_name == tag).ok_or_else(|| UpdateError::NoSuchTag(tag.clone()))
            }
        }
    }

    /// An asset's bytes. The API answers 200 with them, or a redirect to a signed URL of the storage, which is
    /// followed once, over https and without the token.
    pub fn download(&self, token: &Token, asset: &Asset, limit: u64) -> Result<Vec<u8>> {
        let first = self.transport.get(&self.asset_url(asset), Some(token), OCTET, limit)?;
        match first.status {
            200 => Ok(first.body),
            301 | 302 | 303 | 307 | 308 => {
                let location = first.location.ok_or_else(|| UpdateError::BadAnswer("a redirect with no target".to_owned()))?;
                let target = url::Url::parse(&location).map_err(|error| UpdateError::BadAnswer(error.to_string()))?;
                if target.scheme() != "https" && self.base.starts_with("https:") {
                    return Err(UpdateError::Insecure(format!("the storage of {}", asset.name)));
                }
                let second = self.transport.get(target.as_str(), None, OCTET, limit)?;
                match second.status {
                    200 => Ok(second.body),
                    status => Err(UpdateError::Status(status)),
                }
            }
            status => Err(UpdateError::Status(status)),
        }
    }

    /// The release's `latest.json`, as JSON: what the app hands to the plugin and the CLI reads.
    pub fn manifest(&self, token: &Token, release: &Release) -> Result<Value> {
        let bytes = self.download(token, release.asset(MANIFEST)?, MANIFEST_LIMIT)?;
        serde_json::from_slice(&bytes).map_err(|error| UpdateError::BadAnswer(format!("{MANIFEST}: {error}")))
    }
}

/// The release's asset a manifest's `url` names, by the file name the URL ends with. The URL itself is never
/// requested; one that is not https is refused all the same (V21).
pub fn asset_for<'a>(release: &'a Release, url: &str) -> Result<&'a Asset> {
    let parsed = url::Url::parse(url).map_err(|_| UpdateError::BadAnswer(format!("{url} is not a URL")))?;
    if parsed.scheme() != "https" {
        return Err(UpdateError::Insecure(url.to_owned()));
    }
    // The names `release.yml` gives need no escaping, so a `%` is refused rather than decoded.
    let name = parsed.path_segments().and_then(|mut segments| segments.next_back()).unwrap_or_default();
    let shaped = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    if !shaped {
        return Err(UpdateError::BadAnswer(format!("{url} names no file")));
    }
    release.asset(name)
}

/// The version a manifest announces, as the plugin reads it: `version` (or `name`), with an optional `v`.
pub fn manifest_version(manifest: &Value) -> Result<Version> {
    let text = manifest["version"].as_str().or_else(|| manifest["name"].as_str()).unwrap_or_default();
    Version::parse(text.trim_start_matches('v')).map_err(|error| UpdateError::BadAnswer(format!("version {text:?}: {error}")))
}

/// The manifest's key for the CLI of this system: `cli-<os>-<arch>`, beside the app's `<os>-<arch>` keys,
/// with the plugin's names for both.
#[must_use]
pub fn cli_key() -> Option<String> {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return None;
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return None;
    };
    Some(format!("cli-{os}-{arch}"))
}

/// What a manifest offers to the CLI: a version newer than `current`, the asset and its signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: Version,
    pub asset: Asset,
    pub signature: String,
}

/// The entry `key` of the manifest, if the release is newer than `current`: an equal or lower version is no
/// update, so a downgrade is never offered (V21).
pub fn offer(manifest: &Value, release: &Release, current: &Version, key: &str) -> Result<Option<Offer>> {
    let version = manifest_version(manifest)?;
    if version <= *current {
        return Ok(None);
    }
    let entry = &manifest["platforms"][key];
    let (Some(url), Some(signature)) = (entry["url"].as_str(), entry["signature"].as_str()) else {
        return Err(UpdateError::NoEntry(key.to_owned()));
    };
    let asset = asset_for(release, url)?.clone();
    Ok(Some(Offer { version, asset, signature: signature.to_owned() }))
}

/// minisign's check of `bytes` with `pubkey`, both in the plugin's base64 form, and of the version the
/// signature's trusted comment carries (`version:<v>`, which `tauri signer sign --app-version` writes): it must
/// be `version`, as the plugin's `requireSignedVersion` demands. The trusted comment is read only after the
/// global signature, which covers it, has verified.
pub fn verify(bytes: &[u8], signature: &str, pubkey: &str, version: &Version) -> Result<()> {
    let key = minisign_verify::PublicKey::decode(&text_of(pubkey)?)
        .map_err(|error| UpdateError::BadSignature(format!("the key: {error}")))?;
    let signature = minisign_verify::Signature::decode(&text_of(signature)?)
        .map_err(|error| UpdateError::BadSignature(format!("the signature: {error}")))?;
    key.verify(bytes, &signature, true).map_err(|error| UpdateError::BadSignature(error.to_string()))?;
    let signed = signature.trusted_comment().split('\t').find_map(|field| field.strip_prefix("version:"));
    let Some(signed) = signed else {
        return Err(UpdateError::UnsignedVersion);
    };
    match Version::parse(signed.trim_start_matches('v')) {
        Ok(signed) if signed == *version => Ok(()),
        _ => Err(UpdateError::VersionMismatch { signed: signed.to_owned(), offered: version.to_string() }),
    }
}

#[cfg(test)]
mod tests;
