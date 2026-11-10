//! Embeds every file of `table/`, so a slice adds the file of its group without touching the code.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let folder = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("the manifest folder")).join("table");
    println!("cargo:rerun-if-changed={}", folder.display());
    let mut names: Vec<String> = fs::read_dir(&folder)
        .expect("the table folder")
        .map(|entry| entry.expect("a table file").file_name().into_string().expect("a UTF-8 file name"))
        .filter(|name| name.ends_with(".toml"))
        .collect();
    names.sort();
    let mut out = String::from("&[\n");
    for name in &names {
        let path = folder.join(name).display().to_string();
        out.push_str(&format!("    ({name:?}, include_str!({path:?})),\n"));
    }
    out.push_str("]\n");
    let target = Path::new(&env::var("OUT_DIR").expect("the build output folder")).join("tables.rs");
    fs::write(target, out).expect("the list of table files");
}
