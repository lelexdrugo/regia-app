//! `self-update`: this command line, replaced by the one of a newer release (the dossier's `rilascio.md` §4.4
//! point 6). The same channel as the app's (`regia-update`): the release read with the token gh keeps, its
//! `latest.json`, the entry `cli-<os>-<arch>` and its asset, then minisign with the key of this build and the
//! version the signature carries. A copy that came with the app is left to the app's updater: replacing it
//! alone would break the bundle's seal on macOS and split the versions of app and CLI elsewhere.
//!
//! The name and its arguments are new names of the contract: a proposal for the gate of the names (✋C).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use regia_i18n::Catalog;
use regia_identity::BIN_NAME;
use regia_update::{Channel, PACKAGE_LIMIT, Selector, Token, UpdateError};
use semver::Version;
use serde_json::json;

#[derive(Args)]
pub struct SelfUpdateArgs {
    /// Only say whether a newer version is there: nothing is downloaded.
    #[arg(long)]
    check: bool,
    /// Machine-readable output.
    #[arg(long)]
    json: bool,
}

/// What the command found and did.
#[derive(Debug, PartialEq, Eq)]
struct Report {
    current: Version,
    available: Option<Version>,
    installed: bool,
}

pub fn run(args: &SelfUpdateArgs, catalog: &Catalog) -> Result<ExitCode, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let report = update(args.check, &exe, catalog)?;
    if args.json {
        let answer = json!({
            "current": report.current.to_string(),
            "available": report.available.as_ref().map(Version::to_string),
            "installed": report.installed,
        });
        println!("{answer}");
    } else {
        let line = match (&report.available, report.installed) {
            (None, _) => catalog.message_with("update-current", "version", &report.current.to_string()),
            (Some(version), true) => catalog.message_with("update-installed", "version", &version.to_string()),
            (Some(version), false) => catalog.message_with("update-available", "version", &version.to_string()),
        };
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

fn update(check: bool, exe: &Path, catalog: &Catalog) -> Result<Report, String> {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|error| error.to_string())?;
    remove_the_old(exe);
    let fail = |error: UpdateError| error.to_string();
    let pubkey = regia_update::pubkey().ok_or(UpdateError::NoKey).map_err(fail)?;
    let key = regia_update::cli_key().ok_or("the releases have no command line for this system")?;
    let selector = Selector::from_env().map_err(fail)?;
    let token = Token::from_gh().map_err(fail)?;
    let channel = Channel::github();
    let release = channel.release(&token, &selector).map_err(fail)?;
    let manifest = channel.manifest(&token, &release).map_err(fail)?;
    let Some(offer) = regia_update::offer(&manifest, &release, &current, &key).map_err(fail)? else {
        return Ok(Report { current, available: None, installed: false });
    };
    if check {
        return Ok(Report { current, available: Some(offer.version), installed: false });
    }
    if came_with_the_app(exe) {
        return Err(catalog.message("update-with-the-app"));
    }
    let bytes = channel.download(&token, &offer.asset, PACKAGE_LIMIT).map_err(fail)?;
    regia_update::verify(&bytes, &offer.signature, pubkey, &offer.version).map_err(fail)?;
    replace(exe, &bytes).map_err(|error| format!("{}: {error}", exe.display()))?;
    Ok(Report { current, available: Some(offer.version), installed: true })
}

/// A copy the app's installer put there: beside the app's binary (Windows, a deb or an rpm), or inside an
/// app bundle (macOS).
fn came_with_the_app(exe: &Path) -> bool {
    let app = format!("{BIN_NAME}-app{}", std::env::consts::EXE_SUFFIX);
    let in_a_bundle = exe.ancestors().any(|folder| folder.extension().is_some_and(|extension| extension == "app"));
    in_a_bundle || exe.with_file_name(app).is_file()
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    exe.with_file_name(name)
}

/// The binary a previous update set aside on Windows, where a running program can be renamed but not removed.
fn remove_the_old(exe: &Path) {
    let _ = fs::remove_file(sibling(exe, ".old"));
}

/// `bytes` in place of `exe`, through a file beside it: on Unix one rename over the old binary, with its
/// mode; on Windows the running binary is renamed aside first, and put back if the second rename fails.
fn replace(exe: &Path, bytes: &[u8]) -> io::Result<()> {
    let fresh = sibling(exe, ".new");
    if let Err(error) = fs::write(&fresh, bytes) {
        let _ = fs::remove_file(&fresh);
        return Err(error);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(exe).map(|meta| meta.permissions().mode()).unwrap_or(0o755) | 0o111;
        fs::set_permissions(&fresh, fs::Permissions::from_mode(mode))?;
        fs::rename(&fresh, exe)
    }
    #[cfg(not(unix))]
    {
        let old = sibling(exe, ".old");
        let _ = fs::remove_file(&old);
        fs::rename(exe, &old)?;
        fs::rename(&fresh, exe).inspect_err(|_| {
            let _ = fs::rename(&old, exe);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(label: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-self-update").join(label);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_binary_is_replaced_whole_and_nothing_is_left_beside_it() {
        let dir = folder("replace");
        let exe = dir.join(format!("{BIN_NAME}{}", std::env::consts::EXE_SUFFIX));
        fs::write(&exe, b"old binary").unwrap();
        replace(&exe, b"new binary").unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new binary");
        assert!(!sibling(&exe, ".new").exists());
        remove_the_old(&exe);
        let left: Vec<_> = fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left.len(), 1, "{left:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(fs::metadata(&exe).unwrap().permissions().mode() & 0o111, 0, "not executable");
        }
    }

    #[test]
    fn a_copy_that_came_with_the_app_is_the_apps_to_update() {
        let dir = folder("with-the-app");
        let exe = dir.join(format!("{BIN_NAME}{}", std::env::consts::EXE_SUFFIX));
        fs::write(&exe, b"cli").unwrap();
        assert!(!came_with_the_app(&exe), "a copy on its own");
        fs::write(dir.join(format!("{BIN_NAME}-app{}", std::env::consts::EXE_SUFFIX)), b"app").unwrap();
        assert!(came_with_the_app(&exe), "beside the app");
        let bundled = dir.join("Product.app").join("Contents").join("MacOS").join(BIN_NAME);
        assert!(came_with_the_app(&bundled), "inside a bundle");
    }
}
