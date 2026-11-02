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
    if let Some(dir) = non_empty_var(&prototype_env_var("RUNTIME")) {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        return non_empty_var("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join(PROTOTYPE_NAME));
    }
    if let Some(dir) = non_empty_var("XDG_STATE_HOME") {
        return Some(PathBuf::from(dir).join(PROTOTYPE_NAME));
    }
    BaseDirs::new().map(|dirs| dirs.home_dir().join(".local/state").join(PROTOTYPE_NAME))
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
}
