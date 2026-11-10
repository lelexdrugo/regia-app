//! Rules of ADR 0001 and ADR 0017 that are cheap to check on every build.

use std::fs;
use std::path::{Path, PathBuf};

use regia_conformance::workspace_root;
use regia_identity::{BIN_NAME, PRODUCT_NAME, app_identifier};
use serde_json::Value;

fn files_with_extension(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            // Built or installed, not written here: the frontend's dependencies and its bundle too.
            let generated = ["target", "gen", "node_modules", "dist"];
            if path.file_name().is_some_and(|name| !generated.iter().any(|skip| name == *skip)) {
                files_with_extension(&path, extensions, out);
            }
        } else if path.extension().is_some_and(|ext| extensions.contains(&ext.to_string_lossy().as_ref())) {
            out.push(path);
        }
    }
}

/// Manifests cannot read the constant, so they repeat it and this test keeps them honest.
#[test]
fn the_product_name_has_one_source() {
    let root = workspace_root();
    let cli: toml::Table = toml::from_str(&fs::read_to_string(root.join("crates/regia-cli/Cargo.toml")).unwrap()).unwrap();
    assert_eq!(cli["bin"].as_array().unwrap()[0]["name"].as_str().unwrap(), BIN_NAME);

    let conf: Value = serde_json::from_str(&fs::read_to_string(root.join("app/src-tauri/tauri.conf.json")).unwrap()).unwrap();
    assert_eq!(conf["productName"], PRODUCT_NAME);
    assert_eq!(conf["identifier"], app_identifier());
    assert_eq!(conf["app"]["windows"][0]["title"], PRODUCT_NAME);
}

/// No string in the sources spells the name outside the identity crate, except crate paths.
#[test]
fn only_the_identity_crate_spells_the_name_in_a_string() {
    let root = workspace_root();
    let mut sources = Vec::new();
    files_with_extension(&root.join("crates"), &["rs"], &mut sources);
    files_with_extension(&root.join("app"), &["rs", "js", "ts", "svelte", "html", "css"], &mut sources);
    let needle = format!("\"{PRODUCT_NAME}\"");
    let this_file = root.join("crates/regia-conformance/tests/guards.rs");
    for path in sources {
        if path.starts_with(root.join("crates/regia-identity")) || path == this_file {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&needle), "{} spells the name", path.display());
    }
}

/// #147: no Italian identifier in the code, outside catalogs and the facade table. Tracker strings
/// of format v1 and the oracle's fixtures are data, and live in their own non-source files.
#[test]
fn no_italian_identifiers_in_the_sources() {
    const WORDS: [&str; 33] = [
        "aspetta", "pronti", "pronto", "progetto", "progetti", "stato", "stati", "maturita", "obiettivo", "bloccato",
        "catena", "sblocca", "ciclo", "sessione", "abbozzo", "cartella", "titolare", "giornalieri", "decisione", "proposta",
        "approvato", "scartato", "fatto", "grafo", "cicli", "troncati", "corso", "totale", "motivo", "esito", "voci",
        "passo", "lancio",
    ];
    let root = workspace_root();
    let mut sources = Vec::new();
    files_with_extension(&root.join("crates"), &["rs"], &mut sources);
    files_with_extension(&root.join("app"), &["rs", "js", "ts", "svelte", "html", "css"], &mut sources);
    let this_file = root.join("crates/regia-conformance/tests/guards.rs");
    let tests_dir = root.join("crates/regia-conformance/tests");
    let facade = root.join("crates/regia-facade/src/lib.rs");
    for path in sources {
        // The conformance tests name the prototype's keys on purpose: they feed the table.
        if path == this_file || path.starts_with(&tests_dir) {
            continue;
        }
        // The facade's unit tests do too. Only that file's test module is exempt.
        let text = fs::read_to_string(&path).unwrap();
        let code = if path == facade { text.split("#[cfg(test)]").next().unwrap() } else { text.as_str() };
        for token in code.split(|c: char| !c.is_alphanumeric()) {
            // Splits camelCase, so `statoTask` is caught too.
            let mut part = String::new();
            let mut parts = Vec::new();
            for c in token.chars() {
                if c.is_uppercase() && !part.is_empty() {
                    parts.push(std::mem::take(&mut part));
                }
                part.extend(c.to_lowercase());
            }
            parts.push(part);
            for word in &parts {
                assert!(!WORDS.contains(&word.as_str()), "{} contains the Italian word `{word}`", path.display());
            }
        }
    }
}

// The app's capability, command lists and CSP are checked next to them, in app/src-tauri/tests/security.rs
// (threat model V01, V02, V05).
