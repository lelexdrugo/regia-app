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

export type BriefView = { waiting_on_owner: Array<TaskView>, 
/**
 * The tasks of `waiting_on_owner` that need a working session with the owner, in the same order.
 */
session: Array<TaskView>, ready: Array<TaskView>, in_progress: Array<TaskView>, graph: GraphView | null, due_ignored: Array<IgnoredDueView>, controller: ControllerView | null, dossier: Array<DossierView>, 
/**
 * Keyed by project.
 */
outside_tracker: { [key in string]: OutsideTrackerView }, 
/**
 * The sections of the contract this build does not produce: the view says so, instead of an all-clear.
 */
not_ported: Array<string>, };

export type TaskView = { number: number, title: string, status: StatusView | null, 
/**
 * The project's id, from its label: `null` without one, or with an empty one.
 */
project: string | null, 
/**
 * The graph's measures, on the sections it orders; `null` in a cycle and in `in_progress`.
 */
unblocks: number | null, unblocks_total: number | null, chain: number | null, cycle: boolean, 
/**
 * The due date the spec declares, `YYYY-MM-DD`, when the core could read one.
 */
due: string | null, due_soon: boolean, overdue: boolean, };

export type StatusView = "inbox" | "proposal" | "approved" | "in_progress" | "decision" | "done" | "discarded";

export type GraphView = { cycles: Array<Array<number>>, truncated: Array<number>, };

export type IgnoredDueView = { number: number, value: string, };

export type ControllerView = { last_round: string | null, stale: boolean, budget: ControllerBudgetView | null, to_open: Array<ControllerToOpenView>, to_resume: Array<ControllerToResumeView>, };

export type ControllerBudgetView = { rounds: number, since: string | null, reason: string | null, };

export type ControllerToOpenView = { name: string, folder: string | null, prompt: string | null, reason: string | null, };

export type ControllerToResumeView = { project: string | null, session: string | null, resumes: string | null, how: string | null, reason: string | null, };

export type DossierView = { project: string, folder: string, problem: DossierProblem, branch: string | null, ahead: number | null, detail: string | null, };

export type DossierProblem = "folder_missing" | "not_a_repo" | "no_remote" | "no_upstream" | "detached_head" | "git_failed" | "not_pushed";

export type OutsideTrackerView = { total: number, items: Array<OutsideTrackerItemView>, more: number, };

export type OutsideTrackerItemView = { check: string, project: string, where: string | null, text: string, why: string | null, };

export type Document = { blocks: Array<Block>, 
/**
 * The text was longer than the cap (`text::DOCUMENT_LIMIT`), and its end is missing.
 */
truncated: boolean, };

export type Block = { "kind": "paragraph", content: Array<Inline>, } | { "kind": "heading", level: number, content: Array<Inline>, } | { "kind": "quote", blocks: Array<Block>, } | { "kind": "code", text: string, } | { "kind": "list", ordered: boolean, items: Array<Array<Block>>, } | { "kind": "table", head: Array<Array<Inline>>, rows: Array<Array<Array<Inline>>>, } | { "kind": "rule" };

export type Inline = { "kind": "text", text: string, } | { "kind": "emphasis", content: Array<Inline>, } | { "kind": "strong", content: Array<Inline>, } | { "kind": "code", text: string, } | { "kind": "break" } | { "kind": "link", content: Array<Inline>, target: LinkTarget, } | { "kind": "image", alt: string, host: string | null, };

export type LinkTarget = { "kind": "external", url: string, host: string, confirm: boolean, } | { "kind": "refused", reason: LinkRefusal, };

export type LinkRefusal = "invalid" | "scheme" | "credentials" | "too_long";

export type Settings = { direction: Direction, density: Density, 
/**
 * The language chosen in the app, or `null` when none was: then the system's.
 */
language: LanguageTag | null, };

export type SettingsChange = { direction?: Direction, density?: Density, language?: LanguageTag, };

export type Direction = "montaggio" | "foglio" | "nero";

export type Density = "compact" | "comfortable";

export type LanguageTag = "en" | "it";
