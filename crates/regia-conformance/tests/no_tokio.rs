//! Threads, not tokio (ADR 0001 §7): the CLI, and the controller's engine it hosts, compile no async runtime. The
//! graph is the lockfile's, as `cargo metadata` resolves it for every platform: the normal and build edges from a
//! package, never the dev ones. The control: the same walk from the Tauri app, which does run on tokio, finds it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use regia_conformance::workspace_root;
use serde_json::Value;

fn metadata() -> Value {
    let cargo = std::env::var_os("CARGO").map_or_else(|| PathBuf::from(env!("CARGO")), PathBuf::from);
    let args = ["metadata", "--format-version=1", "--locked"].map(str::to_owned);
    let output = regia_exec::harness::launch(&cargo, &args, &workspace_root()).run().expect("cargo metadata runs");
    assert!(output.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("the metadata is JSON")
}

/// The names of the packages `root` depends on through normal and build edges, itself included.
fn reached(metadata: &Value, root: &str) -> BTreeSet<String> {
    let names: BTreeMap<&str, &str> = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|package| (package["id"].as_str().expect("an id"), package["name"].as_str().expect("a name")))
        .collect();
    let nodes: BTreeMap<&str, &Value> =
        metadata["resolve"]["nodes"].as_array().expect("the resolved graph").iter().map(|node| (node["id"].as_str().expect("an id"), node)).collect();
    let start = names.iter().find(|(_, name)| **name == root).map(|(id, _)| *id).unwrap_or_else(|| panic!("no package {root}"));
    let (mut seen, mut queue) = (BTreeSet::new(), vec![start]);
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        for dep in nodes[id]["deps"].as_array().into_iter().flatten() {
            let kinds = dep["dep_kinds"].as_array().into_iter().flatten();
            if kinds.into_iter().any(|kind| kind["kind"].is_null() || kind["kind"] == "build") {
                queue.push(dep["pkg"].as_str().expect("a package id"));
            }
        }
    }
    seen.iter().map(|id| names[id].to_owned()).collect()
}

#[test]
fn neither_the_cli_nor_the_controller_compiles_tokio() {
    let metadata = metadata();
    for root in ["regia-cli", "regia-controller"] {
        let graph = reached(&metadata, root);
        assert!(graph.contains("regia-core"), "{root}: the walk reaches the core");
        assert!(!graph.contains("tokio"), "{root} depends on tokio");
    }
    assert!(reached(&metadata, "regia-app").contains("tokio"), "the control: the walk does not see the app's tokio");
}
