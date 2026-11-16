// The moviola's model and layout: the prototype's viewer (the dossier's tools/moviola/static/moviola.js, functions
// `prepara`, `scalaTempo`, `interp`, `indiciFinestra`, `layoutA`, `semiSpessore`, `yAnello`) ported over the
// core's timeline. Pure: no DOM, no clock unless given, so the proofs run it in Node (prove/moviola.js) against
// the prototype's own answers on the same fixtures (harness/moviola.json).
import type { Aggregate, DirectionStatus, LinkedMilestone, ModelUse, Timeline } from "./types";

export const HOUR = 3600e3;

/** One ring per round of each task a milestone points at; a task with no round declared gives one, undeclared. */
export type Ring = { declared: boolean; model: ModelUse | null };

/** How a lane is coloured: the main one, an abandoned one, or the n-th of the others' palette. */
export type LaneTone = { kind: "main" } | { kind: "abandoned" } | { kind: "branch"; hue: number };

export type Lane = {
  id: string;
  index: number;
  /** The direction's record, `null` for the main lane and for a lane no record names. */
  title: string | null;
  reason: string | null;
  recorded: boolean;
  status: DirectionStatus;
  /** When the lane opens: its opening milestone's instant, or the first milestone's. */
  opened: number;
  /** Derived: the first abandonment or rejoin with a parent in this lane, if the lane is not active. */
  closed: number | null;
  closedBy: string | null;
  tone: LaneTone;
  /** Its nodes' indices, in time order. */
  nodes: number[];
};

export type Node = {
  milestone: LinkedMilestone;
  index: number;
  t: number;
  lane: number;
  children: string[];
  work: { task: number; aggregate: Aggregate | null }[];
  rings: Ring[];
  /** ✓ a check passed, ? a check did not conclude, D3 three defects found, 1/3 one of three closed. */
  badges: string[];
  /** Has a check, or a count of defects (even 0), that someone declared. */
  declared: boolean;
};

export type Model = {
  timeline: Timeline;
  nodes: Node[];
  byId: Map<string, Node>;
  lanes: Lane[];
  t0: number;
  t1: number;
};

const declaredValue = (value: number | null | undefined): boolean => value !== null && value !== undefined;

/** `prepara`: lanes (the main one, then the directions by the instant of their opening milestone, then the lanes
 * only milestones name, in order), the nodes with their children, rings and badges, and the span of time. */
export function prepare(timeline: Timeline, now: number = Date.now()): Model {
  const milestones = timeline.milestones;
  const times = milestones.map((m) => Date.parse(m.ts));
  const indexOf = new Map(milestones.map((m, i) => [m.id, i]));
  const at = (id: string | null): number | undefined => {
    const i = id === null ? undefined : indexOf.get(id);
    return i === undefined ? undefined : times[i];
  };
  const aggregates = new Map(timeline.aggregates.map((a) => [a.task, a]));
  const records = new Map(timeline.directions.map((d) => [d.id, d]));
  const order = [
    timeline.main_direction,
    ...timeline.directions
      .slice()
      .sort((a, b) => (at(a.opened_by) || 0) - (at(b.opened_by) || 0))
      .map((d) => d.id),
  ];
  for (const m of milestones) if (!order.includes(m.direction)) order.push(m.direction);

  const lanes: Lane[] = order.map((id, index) => {
    const record = records.get(id);
    const opening = record ? at(record.opened_by) : undefined;
    const opened = opening !== undefined ? opening : milestones.length ? (times[0] as number) : 0;
    const closer = milestones.findIndex(
      (m) =>
        (m.kind === "abandonment" || m.kind === "rejoin") &&
        m.parents.some((p) => {
          const i = indexOf.get(p);
          return i !== undefined && milestones[i]?.direction === id;
        }),
    );
    const status: DirectionStatus = record ? record.status : "active";
    // An active lane, and the main one, never closes.
    const closedBy = status !== "active" && closer >= 0 ? closer : null;
    const tone: LaneTone =
      id === timeline.main_direction ? { kind: "main" } : status === "abandoned" ? { kind: "abandoned" } : { kind: "branch", hue: (index - 1) % 5 };
    return {
      id,
      index,
      title: record ? record.title : null,
      reason: record ? record.reason : null,
      recorded: record !== undefined,
      status,
      opened,
      closed: closedBy === null ? null : (times[closedBy] as number),
      closedBy: closedBy === null ? null : (milestones[closedBy] as LinkedMilestone).id,
      tone,
      nodes: [],
    };
  });
  const laneOf = new Map(lanes.map((lane) => [lane.id, lane]));

  const nodes: Node[] = milestones.map((m, index) => {
    const lane = laneOf.get(m.direction) as Lane;
    lane.nodes.push(index);
    const work = m.refs.issue.map((task) => ({ task, aggregate: aggregates.get(task) ?? null }));
    const rings: Ring[] = [];
    for (const { aggregate } of work) {
      const models: (ModelUse | null)[] = aggregate && aggregate.models.length ? aggregate.models : [null];
      if (!aggregate || aggregate.round === null) {
        rings.push({ declared: false, model: models[0] ?? null });
        continue;
      }
      for (let r = 0; r < aggregate.round; r++) rings.push({ declared: true, model: models[r % models.length] ?? null });
    }
    const checks = work.flatMap(({ aggregate }) => aggregate?.checks ?? []);
    const defects = m.process?.defects ?? null;
    const badges: string[] = [];
    if (checks.some((c) => c.outcome === "ok")) badges.push("✓");
    if (checks.some((c) => c.outcome === "non-concluso")) badges.push("?");
    if (defects?.found) badges.push(declaredValue(defects.closed) ? `${defects.closed}/${defects.found}` : `D${defects.found}`);
    const declared =
      checks.length > 0 ||
      (m.process?.checks ?? []).length > 0 ||
      declaredValue(defects?.found) ||
      work.some(({ aggregate }) => declaredValue(aggregate?.defects?.found));
    return {
      milestone: m,
      index,
      t: times[index] as number,
      lane: lane.index,
      children: milestones.filter((x) => x.parents.includes(m.id)).map((x) => x.id),
      work,
      rings,
      badges,
      declared,
    };
  });
  const last = nodes[nodes.length - 1];
  return {
    timeline,
    nodes,
    byId: new Map(nodes.map((n) => [n.milestone.id, n])),
    lanes,
    t0: nodes[0] ? nodes[0].t : now,
    t1: last ? last.t : now,
  };
}

/** `interp`: piecewise linear, `a` ascending, clamped at both ends. */
export function interpolate(a: readonly number[], b: readonly number[], v: number): number {
  const first = a[0] as number;
  const last = a[a.length - 1] as number;
  if (v <= first) return b[0] as number;
  if (v >= last) return b[b.length - 1] as number;
  let i = 1;
  while ((a[i] as number) < v) i++;
  const lo = a[i - 1] as number;
  const q = (v - lo) / ((a[i] as number) - lo || 1);
  return (b[i - 1] as number) + q * ((b[i] as number) - (b[i - 1] as number));
}

/** A gap of time wider than the threshold, drawn as a joint of fixed width (✂). */
export type Joint = { x: number; from: number; to: number };

export type TimeScale = { x: (t: number) => number; t: (x: number) => number; joints: Joint[] };

/** `scalaTempo`: x grows with time; gaps beyond `threshold` become joints `joint` wide; `pad` of margin at both
 * ends (0 for a window, whose ends are already the edges). */
export function timeScale(times: readonly number[], x0: number, x1: number, threshold = 12 * HOUR, joint = 34, pad = HOUR): TimeScale {
  const ts = [...new Set(times)].sort((a, b) => a - b);
  if (ts.length < 2) {
    const c = (x0 + x1) / 2;
    return { x: () => c, t: () => ts[0] || 0, joints: [] };
  }
  const knots = [(ts[0] as number) - pad, ...ts, (ts[ts.length - 1] as number) + pad];
  let live = 0;
  let count = 0;
  for (let i = 1; i < knots.length; i++) {
    const gap = (knots[i] as number) - (knots[i - 1] as number);
    if (gap > threshold) count++;
    else live += gap;
  }
  const k = Math.max(1e-9, (x1 - x0 - count * joint) / Math.max(live, 1));
  const xs = [x0];
  const joints: Joint[] = [];
  for (let i = 1; i < knots.length; i++) {
    const gap = (knots[i] as number) - (knots[i - 1] as number);
    const dx = gap > threshold ? joint : gap * k;
    if (gap > threshold) joints.push({ x: (xs[i - 1] as number) + dx / 2, from: knots[i - 1] as number, to: knots[i] as number });
    xs.push((xs[i - 1] as number) + dx);
  }
  return { x: (t) => interpolate(knots, xs, t), t: (x) => interpolate(xs, knots, x), joints };
}

/** The margin at each end of the tape and the strip (#486/V18): a share of the story, between a minute and the
 * prototype's hour. */
export const MARGIN = { share: 0.05, min: 60e3, max: HOUR } as const;
/** The most of the width the two margins may take together, as long as a joint can make room for the story. */
export const MARGINS_ROOM = 0.1;

/**
 * The tape's scale (#486/V18): `timeScale`, with a margin of 5 % of the story at each end (`MARGIN`) instead of an
 * hour, and joints that widen when the margins would take more than a tenth of the width. On a sparse story every
 * gap between milestones is a joint, so the margins are the only time left to stretch, and with the prototype's
 * scale they took the whole tape whatever their length: on alpha the story was 130 px of 1,420. Here the joints
 * widen just enough for the margins to keep a tenth, so the story takes the rest. A story whose margins already fit
 * keeps the prototype's joints, and its scale is the prototype's, with an hour of margin from 20 hours of story up.
 */
export function tapeScale(times: readonly number[], x0: number, x1: number, threshold = 12 * HOUR, joint = 26): TimeScale {
  const ts = [...new Set(times)].sort((a, b) => a - b);
  if (ts.length < 2) {
    const c = (x0 + x1) / 2;
    return { x: () => c, t: () => ts[0] || 0, joints: [] };
  }
  const first = ts[0] as number;
  const last = ts[ts.length - 1] as number;
  const pad = Math.min(MARGIN.max, Math.max(MARGIN.min, (last - first) * MARGIN.share));
  const knots = [first - pad, ...ts, last + pad];
  let live = 0;
  let count = 0;
  for (let i = 1; i < knots.length; i++) {
    const gap = (knots[i] as number) - (knots[i - 1] as number);
    if (gap > threshold) count++;
    else live += gap;
  }
  const width = x1 - x0;
  // A joint as wide as it must be for the two margins, `2 * pad * k`, to stay within their room.
  const room = count > 0 ? (width - (MARGINS_ROOM * width * live) / (2 * pad)) / count : 0;
  const wide = Math.max(joint, room);
  const k = Math.max(1e-9, (width - count * wide) / Math.max(live, 1));
  const xs = [x0];
  const joints: Joint[] = [];
  for (let i = 1; i < knots.length; i++) {
    const gap = (knots[i] as number) - (knots[i - 1] as number);
    const dx = gap > threshold ? wide : gap * k;
    if (gap > threshold) joints.push({ x: (xs[i - 1] as number) + dx / 2, from: knots[i - 1] as number, to: knots[i] as number });
    xs.push((xs[i - 1] as number) + dx);
  }
  return { x: (t) => interpolate(knots, xs, t), t: (x) => interpolate(xs, knots, x), joints };
}

/** What the project was at an instant, as the prototype viewer's panel «alla data» says it, without the sidecar,
 * which the screen reads for `last`. */
export type AtDate = {
  /** The milestones up to the instant, and those after it. */
  before: number;
  after: number;
  /** The last milestone up to the instant: its health is the project's, its sidecar's open decisions too. */
  last: Node | null;
  /** The lanes open at the instant, derived: opened, not yet closed, and with a milestone by then (the main lane
   * from its start). */
  open: Lane[];
};

/** The panel «alla data» of the prototype's viewer: the project at the instant `t`. */
export function stateAt(model: Model, t: number): AtDate {
  const before = model.nodes.filter((node) => node.t <= t);
  const main = model.timeline.main_direction;
  const open = model.lanes.filter(
    (lane) =>
      lane.opened <= t &&
      !(lane.closed !== null && lane.closed <= t) &&
      (lane.id === main || lane.nodes.some((i) => (model.nodes[i] as Node).t <= t)),
  );
  return { before: before.length, after: model.nodes.length - before.length, last: before[before.length - 1] ?? null, open };
}

/** A window of time, in ms: the zoom of the stage. */
export type TimeWindow = { from: number; to: number };

/** The narrowest window, as the prototype's (`MIN_FINESTRA`). */
export const MIN_WINDOW = 20 * 60e3;

/**
 * `impostaFinestra`: the window from `from` to `to`, either way round, at least twenty minutes wide, within an hour
 * of the first and the last milestone; one that holds every milestone is no window, the whole story.
 */
export function chooseWindow(model: Model, from: number, to: number): TimeWindow | null {
  let a = Math.min(from, to);
  let b = Math.max(from, to);
  if (b - a < MIN_WINDOW) {
    const c = (a + b) / 2;
    a = c - MIN_WINDOW / 2;
    b = c + MIN_WINDOW / 2;
  }
  a = Math.max(model.t0 - HOUR, a);
  b = Math.min(model.t1 + HOUR, b);
  return a <= model.t0 && b >= model.t1 ? null : { from: a, to: b };
}

/** A direction as the overview lists it (`disegnaL1`): its lane, the milestone that opened it and the one that
 * closed it, when its record names them. */
export type LaneCard = { lane: Lane; openedBy: Node | null; closedBy: Node | null };

export function laneCards(model: Model): LaneCard[] {
  return model.lanes.map((lane) => {
    const record = model.timeline.directions.find((d) => d.id === lane.id);
    const opener = record && record.opened_by !== null ? model.byId.get(record.opened_by) : undefined;
    const closer = lane.closedBy !== null && lane.status !== "active" ? model.byId.get(lane.closedBy) : undefined;
    return { lane, openedBy: opener ?? null, closedBy: closer ?? null };
  });
}

/** `indiciFinestra`: the first and the last milestone inside the window, at least two apart by one. */
export function windowIndices(model: Model, window: TimeWindow | null): [number, number] {
  const n = model.nodes.length;
  if (!window || n < 2) return [0, n - 1];
  let i0 = model.nodes.findIndex((m) => m.t >= window.from);
  let i1 = -1;
  model.nodes.forEach((m, i) => {
    if (m.t <= window.to) i1 = i;
  });
  if (i0 < 0) i0 = n - 1;
  if (i1 < 0) i1 = 0;
  if (i1 < i0) [i0, i1] = [i1, i0];
  if (i1 === i0) {
    if (i1 < n - 1) i1++;
    else i0--;
  }
  return [i0, i1];
}

/** `yAnello`: rings above and below the glyph in turn, 0 → −14, 1 → +14, 2 → −24, … */
export const ringOffset = (i: number): number => (i % 2 ? 1 : -1) * (14 + 10 * Math.floor(i / 2));

/** `semiSpessore`: half the thickness a lane needs for `n` rings. */
export const halfThickness = (n: number): number => 9 + 10 * Math.ceil(n / 2);

export type Layout = {
  /** Each node's centre, by index. */
  x: number[];
  y: number[];
  /** Each lane's centre and height, by lane index. */
  laneY: number[];
  laneHeight: number[];
  xAt: (t: number) => number;
  tAt: (x: number) => number;
  width: number;
  height: number;
  step: number;
  left: number;
  i0: number;
  i1: number;
  /** With a window: the part of the stage the nodes may use. */
  clip: [number, number] | null;
};

/** A lane as `disegnaPalco` draws it: a line at `y` from `x0` to `x1`, and around it an envelope through `points`,
 * each node's half thickness for its rings, thin at the two ends. A lane with no node has neither. */
export type Envelope = { lane: number; y: number; x0: number; x1: number; points: { x: number; w: number }[] };

export function laneEnvelope(model: Model, layout: Layout, lane: Lane): Envelope | null {
  const nodes = lane.nodes.map((i) => ({ x: layout.x[i] as number, w: halfThickness((model.nodes[i] as Node).rings.length) }));
  const first = nodes[0];
  const last = nodes[nodes.length - 1];
  if (!first || !last) return null;
  // The main lane starts before the first column, under its name; a closed one ends where it closed.
  const x0 = lane.id === model.timeline.main_direction ? Math.min(layout.left - 14, first.x) : first.x;
  const x1 = lane.closed !== null ? layout.xAt(lane.closed) : Math.max(layout.width - 20, last.x);
  const points = [{ x: x0, w: 3 }, ...nodes, { x: x1, w: 3 }].sort((a, b) => a.x - b.x);
  return { lane: lane.index, y: layout.laneY[lane.index] as number, x0, x1, points };
}

/** The axis of `disegnaPalco` and `disegnaNastro`: the first node of each day, as `key` tells the days apart. */
export function dayStarts(model: Model, key: (t: number) => string): number[] {
  const seen = new Set<string>();
  const out: number[] = [];
  for (const node of model.nodes) {
    const k = key(node.t);
    if (seen.has(k)) continue;
    seen.add(k);
    out.push(node.index);
  }
  return out;
}

/** `layoutA`: one column per milestone, lanes as rows; with a window, the milestones inside fill the stage. */
export function layout(model: Model, width: number, height: number, window: TimeWindow | null = null): Layout {
  const left = 150;
  const right = 26;
  const n = model.nodes.length;
  const zoom = window !== null && n > 1;
  const [i0, i1] = windowIndices(model, window);
  const margin = zoom ? 44 : 0;
  const step = zoom ? (width - left - right - 2 * margin) / Math.max(i1 - i0, 1) : Math.max(48, (width - left - right) / Math.max(n - 1, 1));
  // The comfortable lane height, or a narrower one when the stage is short: no scrolling.
  const thickness = model.lanes.map((lane) => 2 * halfThickness(Math.max(1, ...lane.nodes.map((i) => (model.nodes[i] as Node).rings.length))));
  const total = (top: number, min: number, pad: number, gap: number): number => top + thickness.reduce((s, x) => s + Math.max(min, x + pad) + gap, 0) + 10;
  const [top, min, pad, gap] = total(70, 64, 14, 18) <= height ? [70, 64, 14, 18] : [56, 50, 4, 6];
  const laneHeight = thickness.map((x) => Math.max(min, x + pad));
  const laneY: number[] = [];
  let y = top;
  for (const h of laneHeight) {
    laneY.push(y + h / 2);
    y += h + gap;
  }
  const xs = model.nodes.map((_, i) => left + margin + step * (i - i0));
  const ys = model.nodes.map((node) => laneY[node.lane] as number);
  const times = model.nodes.map((node) => node.t);
  const firstT = times[0] as number;
  const lastT = times[times.length - 1] as number;
  const xAt = (t: number): number =>
    n < 2
      ? left
      : t <= firstT
        ? (xs[0] as number) - (step / 2) * Math.min(1, (firstT - t) / (6 * HOUR))
        : t >= lastT
          ? (xs[xs.length - 1] as number) + (step / 2) * Math.min(1, (t - lastT) / (6 * HOUR))
          : interpolate(times, xs, t);
  const tAt = (x: number): number => (n < 2 ? firstT : interpolate(xs, times, x));
  return {
    x: xs,
    y: ys,
    laneY,
    laneHeight,
    xAt,
    tAt,
    width: zoom ? width : Math.max(width, left + step * (n - 1) + right),
    height: Math.max(height, y + 10),
    step,
    left,
    i0,
    i1,
    clip: zoom ? [left - 22, width] : null,
  };
}
