//! Compiles in every catalog file of every language. An area is a file: a new one in a language's folder
//! is picked up here without a change to the code, as the webview's `import.meta.glob` picks it up
//! (`app/ui/src/lib/i18n.ts`).

use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

fn sorted(dir: &Path, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> =
        fs::read_dir(dir).expect("locales").map(|entry| entry.expect("an entry").path()).filter(|path| keep(path)).collect();
    paths.sort();
    paths
}

fn main() {
    // A directory: Cargo reruns this when any file in it changes, is added or goes.
    println!("cargo:rerun-if-changed=locales");
    let locales = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let mut out = String::from(
        "/// The `.ftl` files of a language, by file name in name order.\n\
         fn sources(tag: &str) -> &'static [(&'static str, &'static str)] {\n    match tag {\n",
    );
    for language in sorted(&locales, |path| path.is_dir()) {
        let tag = language.file_name().and_then(|name| name.to_str()).expect("a language tag");
        writeln!(out, "        {tag:?} => &[").unwrap();
        for file in sorted(&language, |path| path.extension().is_some_and(|ext| ext == "ftl")) {
            let name = file.file_name().and_then(|name| name.to_str()).expect("a file name");
            writeln!(out, "            ({name:?}, include_str!({:?})),", file.display().to_string()).unwrap();
        }
        out.push_str("        ],\n");
    }
    out.push_str("        _ => &[],\n    }\n}\n");
    let target = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("sources.rs");
    fs::write(target, out).expect("sources.rs");
}
