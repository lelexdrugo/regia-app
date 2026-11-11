//! The types the webview receives, and only those. They are the app's own view of the core, not the CLI's
//! JSON: every key is always present, with `null` for "none", so the TypeScript that `ts-rs` generates
//! from them says exactly what arrives. Their names repeat the CLI contract's where one exists.

use regia_core::{Brief, BriefTask};
use serde::Serialize;

/// What the webview needs before it draws anything: the threat model's `get_app_info` (§3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AppInfo {
    pub product_name: String,
    pub version: String,
    /// A language tag of the catalog, such as `en` or `it`.
    pub language: String,
    /// The operating system, as Rust names it: `windows`, `macos`, `linux`.
    pub os: String,
    /// True only in a test build that reports what it rendered (`smoke` feature).
    pub smoke: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskView {
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GraphView {
    pub cycles: Vec<Vec<u64>>,
    pub truncated: Vec<u64>,
}

/// The brief's three sections in the core's order, and the dependency warnings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BriefView {
    pub waiting_on_owner: Vec<TaskView>,
    pub ready: Vec<TaskView>,
    pub in_progress: Vec<TaskView>,
    pub graph: Option<GraphView>,
}

fn tasks(section: &[BriefTask]) -> Vec<TaskView> {
    section.iter().map(|entry| TaskView { number: entry.task.number, title: entry.task.title.clone() }).collect()
}

impl From<&Brief> for BriefView {
    fn from(brief: &Brief) -> Self {
        BriefView {
            waiting_on_owner: tasks(&brief.waiting_on_owner),
            ready: tasks(&brief.ready),
            in_progress: tasks(&brief.in_progress),
            graph: brief
                .graph
                .as_ref()
                .map(|graph| GraphView { cycles: graph.cycles.clone(), truncated: graph.truncated.clone() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ts_rs::{Config, TS};

    use super::*;

    const HEADER: &str = "// Generated from app/src-tauri/src/ipc.rs by its tests: do not edit by hand.\n\
                          // After changing a type: cargo test -p regia-app -- --ignored write_bindings\n";

    fn bindings_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui/src/lib/bindings.ts")
    }

    /// One file, one declaration per type. Issue numbers fit a JavaScript number, and `serde_json` sends
    /// them as numbers, so `u64` is `number` and not ts-rs's default `bigint`.
    fn bindings() -> String {
        let config = Config::new().with_large_int("number");
        let declarations =
            [AppInfo::decl(&config), BriefView::decl(&config), GraphView::decl(&config), TaskView::decl(&config)];
        let mut out = HEADER.to_owned();
        for declaration in declarations {
            out.push_str("\nexport ");
            out.push_str(&declaration);
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_frontend_has_the_types_of_this_build() {
        let committed = std::fs::read_to_string(bindings_path()).expect("app/ui/src/lib/bindings.ts");
        assert_eq!(committed.replace("\r\n", "\n"), bindings(), "stale bindings: run the ignored test write_bindings");
    }

    #[test]
    #[ignore = "writes app/ui/src/lib/bindings.ts"]
    fn write_bindings() {
        std::fs::write(bindings_path(), bindings()).unwrap();
    }

    #[test]
    fn numbers_and_absent_values_have_the_types_the_webview_reads() {
        let out = bindings();
        assert!(!out.contains("bigint"), "{out}");
        assert!(out.contains("graph: GraphView | null"), "{out}");
    }
}
