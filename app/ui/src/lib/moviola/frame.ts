// The frame of a moment (L3): what the prototype's viewer shows when a milestone is open (the dossier's
// tools/moviola/static/moviola.js, `disegnaL3` with `conLinkIssue`, `elenco` and `voceReport`), over the core's
// timeline and the milestone's sidecar. Pure, so prove/moviola.js runs it in Node against the frames the prototype
// draws on the same fixtures (harness/moviola.json, recorded by prove/moviola-record.mjs).
import type { Lane, Model, Node } from "./model";
import type { Aggregate, Deviation, DirectionStatus, IssueLink, Links, Snapshot } from "./types";

/** The sidecar of the moment open, as `get_snapshot` answered: still coming, read, none written, or refused. */
export type Notes = { kind: "loading" } | { kind: "ready" } | { kind: "none" } | { kind: "failed"; reason: string };

/** A run of a text: plain, or a `#N` with the address of its issue. */
export type Run = { text: string; url: string | null };

/** A direction the moment opens or closes, as its record has it. */
export type LaneEvent = { id: string; title: string | null; status: DirectionStatus; reason: string | null };

export type Frame = {
  node: Node;
  lane: Lane;
  /** The sidecar's summary for a reader without context (#409, R17); older milestones have none. */
  inBrief: string | null;
  /** The why: the sidecar's direction, and the lanes the moment opens or closes, with their reasons. */
  why: { direction: string | null; opens: LaneEvent | null; closes: LaneEvent[] };
  changed: Run[][];
  openDecisions: Run[][];
  risks: Run[][];
  next: string | null;
  /** A paper's claims and deviations; empty for the other projects. */
  claims: string[];
  deviations: Deviation[];
  /** The milestone's own process, as the chronicler computed it from the aggregates. */
  process: { round: number | null; checks: string[]; found: number | null; closed: number | null };
  /** Who worked, by task: each task the milestone points at, with its aggregate if there is one. */
  work: { task: number; aggregate: Aggregate | null }[];
  links: Links;
  projectUrl: string | null;
  parents: Node[];
  children: Node[];
  previous: Node | null;
  following: Node | null;
};

/**
 * `conLinkIssue`: every `#N` of a text becomes a link to its issue, at the address the core resolved for it among
 * the milestone's issues; a `#N` the milestone does not point at takes the same repository as the others; with no
 * address at all it stays text. A `#N` followed by a letter or a digit is not one. Text is never markup: the view
 * puts each run in the DOM as text.
 */
export function issueRuns(text: string, issues: readonly IssueLink[]): Run[] {
  const url = new Map<string, string>();
  let base = "";
  for (const issue of issues) {
    const n = /^#(\d+)$/.exec(issue.text);
    if (!n || !issue.url) continue;
    url.set(n[1] as string, issue.url);
    base = base || issue.url.replace(/\d+$/, "");
  }
  const out: Run[] = [];
  text.split(/(#\d+)(?!\w)/).forEach((piece, i) => {
    if (!piece) return;
    const href = i % 2 === 1 ? (url.get(piece.slice(1)) ?? (base ? base + piece.slice(1) : null)) : null;
    const last = out[out.length - 1];
    if (href === null && last && last.url === null) last.text += piece;
    else out.push({ text: piece, url: href });
  });
  return out;
}

/** The frame of the node at `index`, with its sidecar (`null` when the milestone has none): `disegnaL3`. */
export function frame(model: Model, index: number, snapshot: Snapshot | null): Frame | null {
  const node = model.nodes[index];
  if (!node) return null;
  const m = node.milestone;
  const lane = model.lanes[node.lane] as Lane;
  const record = (id: string) => model.timeline.directions.find((d) => d.id === id) ?? null;
  const event = (of: Lane): LaneEvent => ({ id: of.id, title: record(of.id)?.title ?? null, status: of.status, reason: of.reason });
  const own = record(lane.id);
  const closing = m.kind === "abandonment" || m.kind === "rejoin";
  const list = (items: readonly string[]) => items.map((item) => issueRuns(item, m.links.issue));
  const known = (ids: readonly string[]) => ids.map((id) => model.byId.get(id)).filter((n): n is Node => n !== undefined);
  return {
    node,
    lane,
    inBrief: snapshot?.in_brief || null,
    why: {
      direction: snapshot?.direction || null,
      opens: own && own.opened_by === m.id ? event(lane) : null,
      closes: closing ? model.lanes.filter((l) => l.closedBy === m.id).map(event) : [],
    },
    changed: list(snapshot?.changed ?? []),
    openDecisions: list(snapshot?.open_decisions ?? []),
    risks: list(snapshot?.risks ?? []),
    next: snapshot?.next || null,
    claims: snapshot?.changed_claims ?? [],
    deviations: snapshot?.deviations ?? [],
    process: {
      round: m.process?.round ?? null,
      checks: m.process?.checks ?? [],
      found: m.process?.defects?.found ?? null,
      closed: m.process?.defects?.closed ?? null,
    },
    work: node.work,
    links: m.links,
    projectUrl: model.timeline.project_url || null,
    parents: known(m.parents),
    children: known(node.children),
    previous: model.nodes[index - 1] ?? null,
    following: model.nodes[index + 1] ?? null,
  };
}
