// Generated from app/src-tauri/src/ipc.rs by its tests: do not edit by hand.
// After changing a type: cargo test -p regia-app -- --ignored write_bindings

export type AppInfo = { product_name: string, version: string, 
/**
 * A language tag of the catalog, such as `en` or `it`.
 */
language: string, 
/**
 * The operating system, as Rust names it: `windows`, `macos`, `linux`.
 */
os: string, 
/**
 * True only in a test build that reports what it rendered (`smoke` feature).
 */
smoke: boolean, };

export type BriefView = { waiting_on_owner: Array<TaskView>, ready: Array<TaskView>, in_progress: Array<TaskView>, graph: GraphView | null, };

export type GraphView = { cycles: Array<Array<number>>, truncated: Array<number>, };

export type TaskView = { number: number, title: string, };
