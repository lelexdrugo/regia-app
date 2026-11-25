// The stage's level of detail (#489): under a zoom where a milestone's marks fall to a few pixels, each lane's
// milestones are drawn in groups, one for each stretch of the lane a few pixels wide, instead of one by one. Nothing
// is left out: every milestone is in exactly one group, a group of one is the milestone's own glyph, and the screen
// says how many moments the groups in view hold (Timeline.svelte). The renderer spike's level of detail skipped the
// nodes under a pixel, so the overview of x10 lost all of them (the dossier's docs/app/moviola/scelta-renderer.md, C3
// of its review). The groups are cut on the world's x at the zoom, not on the view, so a pan does not move them. Pure,
// and types are all this file imports, so the proofs' Node runs it as it is.
import type { Envelope, Model } from "./model";
import type { Camera, Scene, View } from "./scene";

/** Under this zoom the stage draws groups: a column of the layout is under 17 px and a glyph's radius under 2.5. */
export const GROUP_ZOOM = 0.35;
/** The stretch of a lane one group covers, in CSS pixels on screen. */
export const BIN = 8;
/** A group of one is its milestone's glyph, never smaller than this radius in CSS pixels. */
export const SINGLE_RADIUS = 2.5;

/** The kinds of the scene (scene.ts's KINDS: milestone, decision, gate, exception, exploration, rejoin, abandonment)
 * by how much a group shows them, in the labels' order (scene.ts's `labelRank`): a gate, an exception, a decision, an
 * abandonment, a rejoin, an exploration, a milestone. A group takes the kind of its most telling milestone. */
const RANK = [0, 4, 6, 5, 1, 2, 3];

/** A milestone at risk, or waiting for the owner, in a group: the group carries the halo. */
export const AT_RISK = 1;
export const WAITING = 2;

export type Groups = {
  /** The zoom they were cut at. */
  z: number;
  n: number;
  /** Each group's lane, its first and last milestone's x, and its lane's y, in world units, by x0 ascending. */
  lane: Uint16Array;
  x0: Float64Array;
  x1: Float64Array;
  y: Float64Array;
  /** Its milestones, as scene indices: `members[start[g]]` to `members[start[g + 1] - 1]`. */
  start: Uint32Array;
  members: Uint32Array;
  /** The kind of its most telling milestone (an index of KINDS), and `AT_RISK | WAITING` of its milestones' health. */
  kind: Uint8Array;
  health: Uint8Array;
  /** The most rounds one of its milestones has: the thickness its lane's envelope takes there. */
  rings: Uint16Array;
  /** The group of each scene node. */
  of: Int32Array;
  /** The widest group, in world units: what a search of the groups in view looks back by. */
  widest: number;
  /** Parent to child between two different groups, once for each pair, as group indices. */
  edges: { n: number; a: Uint32Array; b: Uint32Array };
};

/** How many milestones a group holds. */
export const size = (groups: Groups, g: number): number => (groups.start[g + 1] as number) - (groups.start[g] as number);

/** The zoom the groups of zoom `z` are cut at: `z` down to a quarter of an octave, so a group is `BIN` to 9.5 px wide on
 * screen and a zoom does not cut them again at every frame. */
export const groupZoom = (z: number): number => 2 ** (Math.floor(Math.log2(z) * 4) / 4);

/** The groups at zoom `z`: in each lane, the milestones whose x falls in the same stretch of `BIN / z` world units. The
 * scene is ordered by x, so one pass opens each group at its first milestone, and the groups come out by x0. */
export function groupsAt(scene: Scene, model: Model, z: number): Groups {
  const n = scene.n;
  const span = BIN / z;
  const lanes = model.lanes.length;
  const current = new Int32Array(lanes).fill(-1);
  const bin = new Float64Array(lanes).fill(Number.NaN);
  const of = new Int32Array(n);
  const lane: number[] = [];
  const x0: number[] = [];
  const x1: number[] = [];
  const y: number[] = [];
  const kind: number[] = [];
  const health: number[] = [];
  const rings: number[] = [];
  const counts: number[] = [];
  for (let k = 0; k < n; k++) {
    const l = scene.lane[k] as number;
    const x = scene.x[k] as number;
    const b = Math.floor(x / span);
    let g = current[l] as number;
    if (g < 0 || bin[l] !== b) {
      g = lane.length;
      current[l] = g;
      bin[l] = b;
      lane.push(l);
      x0.push(x);
      x1.push(x);
      y.push(scene.y[k] as number);
      kind.push(scene.kind[k] as number);
      health.push(0);
      rings.push(0);
      counts.push(0);
    }
    of[k] = g;
    x1[g] = x;
    counts[g] = (counts[g] as number) + 1;
    const k2 = scene.kind[k] as number;
    if ((RANK[k2] as number) > (RANK[kind[g] as number] as number)) kind[g] = k2;
    const node = model.nodes[scene.node[k] as number];
    if (node) {
      const h = node.milestone.health;
      health[g] = (health[g] as number) | (h === "at_risk" ? AT_RISK : h === "waiting_on_owner" ? WAITING : 0);
      rings[g] = Math.max(rings[g] as number, node.rings.length);
    }
  }
  const total = lane.length;
  const start = new Uint32Array(total + 1);
  for (let g = 0; g < total; g++) start[g + 1] = (start[g] as number) + (counts[g] as number);
  const fill = start.slice(0, total);
  const members = new Uint32Array(n);
  for (let k = 0; k < n; k++) {
    const g = of[k] as number;
    members[fill[g] as number] = k;
    fill[g] = (fill[g] as number) + 1;
  }
  let widest = 0;
  for (let g = 0; g < total; g++) widest = Math.max(widest, (x1[g] as number) - (x0[g] as number));
  // The scene's edges between groups: within one group an edge is inside its mark, and two milestones of one group
  // with two of another make one edge.
  const seen = new Set<number>();
  const ea: number[] = [];
  const eb: number[] = [];
  for (let i = 0; i < scene.edges.n; i++) {
    const a = of[scene.edges.a[i] as number] as number;
    const b = of[scene.edges.b[i] as number] as number;
    if (a === b) continue;
    const key = a * total + b;
    if (seen.has(key)) continue;
    seen.add(key);
    ea.push(a);
    eb.push(b);
  }
  return {
    z,
    n: total,
    lane: Uint16Array.from(lane),
    x0: Float64Array.from(x0),
    x1: Float64Array.from(x1),
    y: Float64Array.from(y),
    start,
    members,
    kind: Uint8Array.from(kind),
    health: Uint8Array.from(health),
    rings: Uint16Array.from(rings),
    of,
    widest,
    edges: { n: ea.length, a: Uint32Array.from(ea), b: Uint32Array.from(eb) },
  };
}

/** First group with `x0 >= v`. */
function firstFrom(groups: Groups, v: number): number {
  let lo = 0;
  let hi = groups.n;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if ((groups.x0[m] as number) < v) lo = m + 1;
    else hi = m;
  }
  return lo;
}

/** The groups that may reach into the world's x from `x0` to `x1`, as a range of group indices. */
export function groupRange(groups: Groups, x0: number, x1: number): [number, number] {
  return [firstFrom(groups, x0 - groups.widest), firstFrom(groups, x1)];
}

/** A group's half height on screen, in CSS pixels: a little taller for each doubling of its milestones. */
export const groupHalfHeight = (count: number): number => Math.min(7, Math.max(3, 2 + Math.log2(count)));

/** The emphasis a group is drawn with (canvas2d.ts's FADE): the fullest of its milestones'. */
export function groupLevel(groups: Groups, fade: Uint8Array | null, g: number): 0 | 1 | 2 {
  if (!fade) return 0;
  let level = 2;
  for (let i = groups.start[g] as number; i < (groups.start[g + 1] as number); i++) {
    level = Math.min(level, fade[groups.members[i] as number] as number);
    if (level === 0) break;
  }
  return level as 0 | 1 | 2;
}

/** The groups in a view and the milestones they hold, for the screen's words: in view by their lane's y and by x. */
export function inView(groups: Groups, cam: Camera, view: View): { groups: number; moments: number } {
  const halfW = view.w / 2 / cam.z;
  const halfH = view.h / 2 / cam.z;
  const [lo, hi] = groupRange(groups, cam.cx - halfW, cam.cx + halfW);
  let count = 0;
  let moments = 0;
  for (let g = lo; g < hi; g++) {
    if ((groups.x1[g] as number) < cam.cx - halfW) continue;
    if (Math.abs((groups.y[g] as number) - cam.cy) > halfH) continue;
    count += 1;
    moments += size(groups, g);
  }
  return { groups: count, moments };
}

/** The group nearest the world point (wx, wy) within `radius` world units of its mark, or -1; one whose milestones
 * the filters all leave out is out of reach, as such a milestone is (scene.ts's hitTest). */
export function groupAt(groups: Groups, wx: number, wy: number, radius: number, fade: Uint8Array | null = null): number {
  const [lo, hi] = groupRange(groups, wx - radius, wx + radius);
  let best = -1;
  let bd = radius;
  for (let g = lo; g < hi; g++) {
    const dx = Math.max(0, (groups.x0[g] as number) - wx, wx - (groups.x1[g] as number));
    const d = Math.hypot(dx, (groups.y[g] as number) - wy);
    if (d <= bd && groupLevel(groups, fade, g) !== 2) {
      bd = d;
      best = g;
    }
  }
  return best;
}

/** The lanes' envelopes at the groups' zoom: each group a point as thick as its thickest milestone, between the two
 * thin ends of the lane, so the path has one point for each group instead of one for each milestone. */
export function groupEnvelopes(envelopes: readonly Envelope[], groups: Groups, thickness: (rings: number) => number): Envelope[] {
  const points = new Map<number, { x: number; w: number }[]>();
  for (let g = 0; g < groups.n; g++) {
    const lane = groups.lane[g] as number;
    const list = points.get(lane) ?? [];
    list.push({ x: ((groups.x0[g] as number) + (groups.x1[g] as number)) / 2, w: thickness(groups.rings[g] as number) });
    points.set(lane, list);
  }
  return envelopes.map((e) => {
    const ends = [e.points[0], e.points[e.points.length - 1]].filter((p): p is { x: number; w: number } => p !== undefined);
    return { ...e, points: [...ends, ...(points.get(e.lane) ?? [])].sort((a, b) => a.x - b.x) };
  });
}
