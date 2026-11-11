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

export type Document = { blocks: Array<Block>, 
/**
 * The text was longer than the cap (`text::DOCUMENT_LIMIT`), and its end is missing.
 */
truncated: boolean, };

export type Block = { "kind": "paragraph", content: Array<Inline>, } | { "kind": "heading", level: number, content: Array<Inline>, } | { "kind": "quote", blocks: Array<Block>, } | { "kind": "code", text: string, } | { "kind": "list", ordered: boolean, items: Array<Array<Block>>, } | { "kind": "table", head: Array<Array<Inline>>, rows: Array<Array<Array<Inline>>>, } | { "kind": "rule" };

export type Inline = { "kind": "text", text: string, } | { "kind": "emphasis", content: Array<Inline>, } | { "kind": "strong", content: Array<Inline>, } | { "kind": "code", text: string, } | { "kind": "break" } | { "kind": "link", content: Array<Inline>, target: LinkTarget, } | { "kind": "image", alt: string, host: string | null, };

export type LinkTarget = { "kind": "external", url: string, host: string, confirm: boolean, } | { "kind": "refused", reason: LinkRefusal, };

export type LinkRefusal = "invalid" | "scheme" | "credentials" | "too_long";
