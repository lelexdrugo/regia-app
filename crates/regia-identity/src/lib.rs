//! The provisional product name lives here and nowhere else (ADR 0001, "Product name").
//!
//! Cargo manifests and `tauri.conf.json` cannot read a Rust constant, so they repeat the value and
//! `crates/regia-conformance/tests/single_name.rs` fails if any copy drifts from this constant.

use std::env;
use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};

/// The one constant.
pub const PRODUCT_NAME: &str = "regia";

/// The CLI binary. Must match `[[bin]] name` in `crates/regia-cli/Cargo.toml`.
pub const BIN_NAME: &str = PRODUCT_NAME;

/// Publisher prefix of the app identifier. The identifier is `<prefix>.<product name>`.
pub const IDENTIFIER_PREFIX: &str = "com.lelexdrugo";

pub fn app_identifier() -> String {
    format!("{IDENTIFIER_PREFIX}.{PRODUCT_NAME}")
}

/// The name the Python prototype uses for its state folder and environment variables. It is pinned on
/// purpose: both implementations must find the same `regia.db` while they coexist, so these names do
/// not follow a rename of the product. It goes away with the prototype.
pub const PROTOTYPE_NAME: &str = "regia";

/// `env_var("LANG")` is `REGIA_LANG`: the product's own variables follow the name too.
pub fn env_var(suffix: &str) -> String {
    format!("{}_{suffix}", PRODUCT_NAME.to_uppercase())
}

/// A variable that is also the prototype's contract: `REGIA_RUNTIME`, `REGIA_HOME`.
pub fn prototype_env_var(suffix: &str) -> String {
    format!("{}_{suffix}", PROTOTYPE_NAME.to_uppercase())
}

/// Per-user configuration: agent services, the primary service, user policy overrides.
pub fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", PRODUCT_NAME).map(|dirs| dirs.config_dir().to_path_buf())
}

/// Default parent folder of the dossiers created by the setup.
pub fn dossiers_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().join(format!("{PRODUCT_NAME}-dossiers")))
}

/// Machine-local state, shared with the prototype, by its rule: the environment variable, then
/// `%LOCALAPPDATA%\<name>` on Windows, then `$XDG_STATE_HOME/<name>` or `~/.local/state/<name>`.
pub fn state_dir() -> Option<PathBuf> {
    state_dir_by(cfg!(windows), non_empty_var, || BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()))
}

/// The rule of [`state_dir`] with the platform, the environment and the home folder given, so that a test
/// tries each rule of each platform without touching the process's environment, as the prototype's tests
/// stub the platform. `var` gives a variable that is set and not empty.
fn state_dir_by(
    windows: bool,
    var: impl Fn(&str) -> Option<String>,
    home: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(dir) = var(&prototype_env_var("RUNTIME")) {
        return Some(PathBuf::from(dir));
    }
    if windows {
        return var("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join(PROTOTYPE_NAME));
    }
    if let Some(dir) = var("XDG_STATE_HOME") {
        return Some(PathBuf::from(dir).join(PROTOTYPE_NAME));
    }
    home().map(|home| home.join(".local/state").join(PROTOTYPE_NAME))
}

fn non_empty_var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_derives_from_the_constant() {
        assert_eq!(BIN_NAME, PRODUCT_NAME);
        assert!(app_identifier().ends_with(&format!(".{PRODUCT_NAME}")));
        assert!(env_var("X").starts_with(&PRODUCT_NAME.to_uppercase()));
        assert_eq!(prototype_env_var("RUNTIME"), "REGIA_RUNTIME", "the prototype reads this very name");
        if let Some(dir) = config_dir() {
            assert!(dir.to_string_lossy().contains(PRODUCT_NAME));
        }
        if let Some(dir) = dossiers_dir() {
            assert!(dir.to_string_lossy().contains(PRODUCT_NAME));
        }
    }

    // The state folder's rule against the prototype's tests of the same rule (see PROTOTYPE_CASES.md in this
    // crate, which names each by its Python name and says which test here answers it). `with` takes the
    // platform and the variables that are set; an empty value is the prototype's "not set".

    use std::collections::HashMap;

    const HOME: &str = "/home/someone";

    fn with(windows: bool, vars: &[(&str, &str)]) -> Option<PathBuf> {
        let vars: HashMap<String, String> = vars.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
        state_dir_by(
            windows,
            |name| vars.get(name).filter(|value| !value.is_empty()).cloned(),
            || Some(PathBuf::from(HOME)),
        )
    }

    fn at(base: &str) -> Option<PathBuf> {
        Some(PathBuf::from(base).join(PROTOTYPE_NAME))
    }

    /// P1: the folder named by `REGIA_RUNTIME` wins, on either platform.
    #[test]
    fn the_runtime_variable_wins_over_everything() {
        let all = [("REGIA_RUNTIME", "/r"), ("LOCALAPPDATA", "/l"), ("XDG_STATE_HOME", "/x")];
        for windows in [true, false] {
            assert_eq!(with(windows, &all), Some(PathBuf::from("/r")), "windows: {windows}");
        }
    }

    /// P2: an empty `REGIA_RUNTIME` is not set, and the platform's rule applies.
    #[test]
    fn an_empty_runtime_variable_counts_as_unset() {
        let vars = [("REGIA_RUNTIME", ""), ("LOCALAPPDATA", "/l"), ("XDG_STATE_HOME", "/x")];
        assert_eq!(with(true, &vars), at("/l"));
        assert_eq!(with(false, &vars), at("/x"));
    }

    /// P3: on Windows the folder is `regia` under `LOCALAPPDATA`, and `XDG_STATE_HOME` does not count.
    #[test]
    fn on_windows_the_folder_is_under_localappdata_and_xdg_does_not_count() {
        assert_eq!(with(true, &[("LOCALAPPDATA", "/l"), ("XDG_STATE_HOME", "/x")]), at("/l"));
        assert_eq!(with(true, &[("LOCALAPPDATA", "/l")]), at("/l"));
    }

    /// P4, a DIFFERENCE pinned as it is: on Windows without `LOCALAPPDATA` (unset or empty) the prototype's
    /// runtime-folder function falls back to `<home>/AppData/Local/regia`; this gives no folder, as the
    /// prototype's `radice()` does (it raises) and `regia-store` documents (`NoFolder`). Only the tracker
    /// file's lookup differs, and only on a Windows that has no `LOCALAPPDATA`.
    #[test]
    fn on_windows_without_localappdata_there_is_no_folder() {
        assert_eq!(with(true, &[("XDG_STATE_HOME", "/x")]), None);
        assert_eq!(with(true, &[("LOCALAPPDATA", ""), ("XDG_STATE_HOME", "/x")]), None);
    }

    /// P5: elsewhere the folder is `regia` under `XDG_STATE_HOME`, and `LOCALAPPDATA` does not count.
    #[test]
    fn off_windows_the_folder_is_under_xdg_state_home_and_localappdata_does_not_count() {
        assert_eq!(with(false, &[("XDG_STATE_HOME", "/x"), ("LOCALAPPDATA", "/l")]), at("/x"));
        assert_eq!(with(false, &[("XDG_STATE_HOME", "/x")]), at("/x"));
    }

    /// P6: without `XDG_STATE_HOME` (unset or empty) the folder is under the home's `.local/state`, and
    /// `LOCALAPPDATA` still does not count.
    #[test]
    fn off_windows_without_xdg_state_home_the_folder_is_under_the_home() {
        let expected = Some(PathBuf::from(HOME).join(".local/state").join(PROTOTYPE_NAME));
        assert_eq!(with(false, &[]), expected);
        assert_eq!(with(false, &[("XDG_STATE_HOME", "")]), expected);
        assert_eq!(with(false, &[("LOCALAPPDATA", "/l")]), expected);
    }

    /// P7: the rule names the folder and creates nothing.
    #[test]
    fn the_rule_does_not_create_the_folder() {
        let fresh = std::env::temp_dir().join(format!("regia-identity-never-created-{}", std::process::id()));
        let named = with(false, &[("REGIA_RUNTIME", fresh.to_str().unwrap())]);
        assert_eq!(named, Some(fresh.clone()));
        assert!(!fresh.exists());
    }
}
