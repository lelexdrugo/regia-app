// The moviola's answers as the core sends them: regia-core's `moviola` module (records.rs, links.rs, mod.rs),
// serialized by serde with every key present and `null` for none. Written by hand until the app's generated
// bindings (bindings.ts, from app/src-tauri/src/ipc.rs) list these types: keep the two in step.

export type MilestoneKind = "milestone" | "decision" | "gate" | "exception" | "exploration" | "rejoin" | "abandonment";

export type Health = "healthy" | "at_risk" | "waiting_on_owner";

export type DirectionStatus = "active" | "abandoned" | "rejoined";

export type ReportKind = "relative" | "data" | "absolute" | "outside";

export type Refs = { issue: number[]; commit: string[]; report: string[] };

export type ProcessDefects = { found: number | null; closed: number | null };

export type Process = { round: number | null; checks: string[]; defects: ProcessDefects | null };

export type Author = { role: string | null; agent: string | null; model: string | null; session: string | null };

export type IssueLink = { text: string; url: string | null };

export type CommitLink = { text: string; url: string | null; hash: string };

export type ReportLink = { text: string; path: string | null; kind: ReportKind; exists: boolean };

export type Links = { issue: IssueLink[]; commit: CommitLink[]; report: ReportLink[] };

export type Milestone = {
  id: string;
  ts: string;
  kind: MilestoneKind;
  title: string;
  direction: string;
  parents: string[];
  health: Health;
  opens_direction: string | null;
  refs: Refs;
  process: Process | null;
  author: Author | null;
};

/** A milestone of a timeline: the record, flattened, and its links. */
export type LinkedMilestone = Milestone & { links: Links };

export type Direction = {
  id: string;
  title: string;
  opened_by: string | null;
  status: DirectionStatus;
  reason: string | null;
};

export type ModelUse = {
  agent: string | null;
  model: string | null;
  effort: string | null;
  sessions: number | null;
  via: string | null;
};

export type Check = { kind: string | null; outcome: string | null; defects: string[]; model: string | null };

export type AggregateDefects = { found: number | null; closed: number | null; closes: string[] };

export type Tokens = { input: number | null; output: number | null; cache_read: number | null; cache_write: number | null };

export type Aggregate = {
  project: string | null;
  task: number;
  window: string[];
  round: number | null;
  declared_subagents: number | null;
  models: ModelUse[];
  checks: Check[];
  defects: AggregateDefects | null;
  tokens: Tokens | null;
};

/** `get_timeline`: the milestones by instant, then id; the directions and the aggregates in their files' order. */
export type Timeline = {
  project: string;
  project_url: string | null;
  /** The implicit direction, which has no record: its lane comes first. */
  main_direction: string;
  milestones: LinkedMilestone[];
  directions: Direction[];
  aggregates: Aggregate[];
};

export type LastMilestone = { id: string; ts: string; kind: MilestoneKind; title: string; direction: string };

/** `list_projects`: one project of the portfolio (L0). */
export type ProjectSummary = {
  id: string;
  title: string | null;
  milestones: number;
  directions: number;
  health: Health | null;
  last: LastMilestone | null;
  from: string | null;
  to: string | null;
};

export type Deviation = { plan: string | null; deviation: string | null; reason: string | null; impact: string | null };

/** `get_snapshot`: a milestone's sidecar. */
export type Snapshot = {
  id: string | null;
  ts: string | null;
  in_brief: string | null;
  changed: string[];
  direction: string | null;
  open_decisions: string[];
  risks: string[];
  next: string | null;
  changed_claims: string[];
  deviations: Deviation[];
  process: Process | null;
};
