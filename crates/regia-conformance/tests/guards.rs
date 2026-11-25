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

/// Words of the facade table that are English too, or the name of a tool: the table spells them as the
/// prototype's output does, and they may be in the sources. Every other word of the table that is four letters
/// or more is Italian, and guarded (#559, #435/V9). Each is a word of the table today: a word no longer
/// there is a stale exception, and a test says so.
const ALSO_ENGLISH: [&str; 7] = ["closedat", "come", "createdat", "orca", "statereason", "tracker", "updatedat"];

/// Where a word of the brief's table is also the prototype's field of another command, read as data: the
/// budget's, the policy's, the moviola's and the agents' files. They are outside the brief, and outside the
/// owner of this guard (#559): each is listed, with the file, so a new use anywhere else is a failure.
const DATA_OF_OTHER_COMMANDS: [(&str, &[&str]); 8] = [
    ("crates/regia-agents/src/cli.rs", &["ultimo"]),
    ("crates/regia-agents/tests/cli.rs", &["ultimo"]),
    ("crates/regia-cli/src/budget_text.rs", &["testo"]),
    ("crates/regia-cli/src/policy.rs", &["comando", "testo"]),
    ("crates/regia-core/src/policy.rs", &["comando", "numero", "scaduto"]),
    ("crates/regia-store/tests/budget.rs", &["entro", "valore"]),
    ("app/ui/prove/moviola.js", &["perche"]),
    ("app/ui/src/lib/moviola/search.ts", &["perche"]),
];

/// The words the brief's output is written in, as the facade table knows them: the keys it renames, split
/// into words. The values of its enums are not here: they are the prototype's messages of other commands. They come from the table, which the build
/// compiles in and `translate_python` follows (a key it does not know is an error), so a word of the output
/// that is not here is one the product does not read. Four letters or more: `da`, `di`, `in` are words of
/// other languages too.
fn words_of_the_table() -> std::collections::BTreeSet<String> {
    let mut words = std::collections::BTreeSet::new();
    let mut take = |name: &str| {
        for word in name.split(|c: char| !c.is_alphabetic()).filter(|word| word.chars().count() >= 4) {
            words.insert(word.to_lowercase());
        }
    };
    let dir = workspace_root().join("crates/regia-facade/table");
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        // The words of the brief's output, not of every command's: the sources hold the prototype's other
        // strings as data (the budget's, the policy's, the moviola's), as the rule below allows.
        if path.file_name().is_none_or(|name| name != "brief.toml" && name != "common.toml") {
            continue;
        }
        let table: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for shape in table.get("shapes").and_then(toml::Value::as_table).into_iter().flat_map(toml::Table::values) {
            let fields = shape.get("fields").and_then(toml::Value::as_table);
            for (key, field) in fields.into_iter().flatten() {
                if field.get("to").and_then(toml::Value::as_str) != Some(key) {
                    take(key);
                }
            }
        }
    }
    words
}

/// The words no identifier, string or comment of the sources may hold: the hand list, and every word of the
/// table that is not also English.
fn guarded_words(by_hand: &[&str]) -> std::collections::BTreeSet<String> {
    let mut words = words_of_the_table();
    words.retain(|word| !ALSO_ENGLISH.contains(&word.as_str()));
    words.extend(by_hand.iter().map(|word| (*word).to_owned()));
    words
}

/// The words the table adds are guarded, and the exceptions are the table's own words: a word of the
/// brief's output that the old list missed (entro, scaduto, ignorati, numero, valore, campo, and the keys of
/// the controller's warnings) is a word of the table now, so it is guarded without anyone adding it by hand.
#[test]
fn the_words_of_the_table_are_guarded_unless_they_are_english_too() {
    let table = words_of_the_table();
    for word in ["entro", "scaduto", "ignorati", "numero", "valore", "campo", "aspetta", "pronti", "sblocca"] {
        assert!(table.contains(word), "{word} is not a word of the table: the guard would not see it");
        assert!(guarded_words(&[]).contains(word), "{word} is not guarded by the table alone");
    }
    for english in ALSO_ENGLISH {
        assert!(table.contains(english), "{english} is no longer a word of the table: drop it from ALSO_ENGLISH");
        assert!(!guarded_words(&[]).contains(english));
    }
}

/// #147: no Italian identifier in the code, outside catalogs and the facade table. Tracker strings
/// of format v1 and the oracle's fixtures are data, and live in their own non-source files.
#[test]
fn no_italian_identifiers_in_the_sources() {
    const WORDS: [&str; 34] = [
        "aspetta", "pronti", "pronto", "progetto", "progetti", "stato", "stati", "maturita", "obiettivo", "bloccato",
        "catena", "sblocca", "ciclo", "sessione", "abbozzo", "cartella", "titolare", "giornalieri", "decisione", "proposta",
        "approvato", "scartato", "fatto", "grafo", "cicli", "troncati", "corso", "totale", "motivo", "esito", "voci",
        "passo", "lancio", "oggi",
    ];
    let guarded = guarded_words(&WORDS);
    let mut found = std::collections::BTreeSet::new();
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
                let relative = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
                let data = DATA_OF_OTHER_COMMANDS.iter().any(|(file, words)| relative.ends_with(file) && words.contains(&word.as_str()));
                if guarded.contains(word.as_str()) && !data {
                    found.insert(format!("{} `{word}`", path.strip_prefix(&root).unwrap_or(&path).display()));
                }
            }
        }
    }
    assert!(found.is_empty(), "Italian words in the sources:
{}", found.iter().cloned().collect::<Vec<_>>().join("
"));
}

// The app's capability, command lists and CSP are checked next to them, in app/src-tauri/tests/security.rs
// (threat model V01, V02, V05).

/// #439/D5, closed in 470cf2e: `live-check` runs against the real tracker and prints only paths and counts, so
/// its code never reads what a run said on stderr (an error of the product can carry a value of the tracker).
/// The second half is the control: the reading of the first version, which printed it, is what this sees.
#[test]
fn live_check_never_reads_what_a_run_said_on_stderr() {
    fn reads_stderr(code: &str) -> bool {
        code.lines().filter(|line| !line.trim_start().starts_with("//")).any(|line| line.contains("stderr"))
    }
    let source = fs::read_to_string(workspace_root().join("crates/regia-conformance/src/bin/live_check.rs")).unwrap();
    assert!(!reads_stderr(&source), "live-check reads stderr");
    let first_version = r#"assert!(output.status.success(), "regia failed: {}", String::from_utf8_lossy(&output.stderr));"#;
    assert!(reads_stderr(first_version), "the check does not see the first version's reading");
}
