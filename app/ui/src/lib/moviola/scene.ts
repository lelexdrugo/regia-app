// The scene the Canvas draws, in world units (CSS pixels at zoom 1): typed arrays ordered by x, so that what is in
// view, under the playhead or under the pointer is a binary search away. Copied from the renderer spike, commit
// 84871ea of regia-app's spike/moviola-renderer (src/dataset.js: `lowerBound`, `buildGrid`, `hitTest`;
// src/scenario.js: `pickLabels`; src/renderers/common.js: the camera), over the prototype's layout (model.ts).
import type { Layout, Model } from "./model";
import type { MilestoneKind } from "./types";

export const KINDS: readonly MilestoneKind[] = ["milestone", "decision", "gate", "exception", "exploration", "rejoin", "abandonment"];

/** The camera: the world point at the centre of the view, and the zoom. */
export type Camera = { cx: number; cy: number; z: number };

/** The view, in CSS pixels. */
export type View = { w: number; h: number };

export type Grid = { cw: number; ch: number; start: Uint32Array; items: Uint32Array };

export type Scene = {
  n: number;
  /** Nodes by x: the model's node each one is, and its centre, radius and kind. */
  node: Uint32Array;
  x: Float64Array;
  y: Float64Array;
  r: Float32Array;
  kind: Uint8Array;
  lane: Uint16Array;
  /** Nodes whose label may be drawn (the turning points, as the prototype's labels), ascending. */
  labeled: Uint32Array;
  /** Parent to child, with the scene index of each end (#598: an edge goes with a node filtered out). They come in the
   * order of the children, so by `x2` ascending; `span` is the longest, from parent to child in x (#489): the edges
   * that reach into a part of the stage are a binary search away. */
  edges: { n: number; x1: Float64Array; y1: Float64Array; x2: Float64Array; y2: Float64Array; a: Uint32Array; b: Uint32Array; span: number };
  /** One band per lane, from its opening to its closing or the stage's end. */
  bands: { n: number; x: Float64Array; y: Float64Array; w: Float64Array; h: Float64Array };
  extent: { x0: number; y0: number; x1: number; y1: number };
  grid: Grid;
};

export const NODE_RADIUS = 8;
const CELL = 32;

/** First index with `arr[i] >= v`, `arr` ascending. */
export function lowerBound(arr: ArrayLike<number>, v: number): number {
  let lo = 0;
  let hi = arr.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if ((arr[m] as number) < v) lo = m + 1;
    else hi = m;
  }
  return lo;
}

export function buildScene(model: Model, layout: Layout): Scene {
  const n = model.nodes.length;
  const order = Array.from({ length: n }, (_, i) => i).sort((a, b) => (layout.x[a] as number) - (layout.x[b] as number) || a - b);
  const at = new Int32Array(n);
  order.forEach((i, k) => (at[i] = k));
  const x = new Float64Array(n);
  const y = new Float64Array(n);
  const r = new Float32Array(n).fill(NODE_RADIUS);
  const kind = new Uint8Array(n);
  const lane = new Uint16Array(n);
  const labeled: number[] = [];
  order.forEach((i, k) => {
    const node = model.nodes[i];
    if (!node) return;
    x[k] = layout.x[i] as number;
    y[k] = layout.y[i] as number;
    kind[k] = KINDS.indexOf(node.milestone.kind);
    lane[k] = node.lane;
    if (node.milestone.kind !== "milestone") labeled.push(k);
  });
  const ex1: number[] = [];
  const ey1: number[] = [];
  const ex2: number[] = [];
  const ey2: number[] = [];
  const ea: number[] = [];
  const eb: number[] = [];
  for (const node of model.nodes) {
    for (const parent of node.milestone.parents) {
      const from = model.byId.get(parent);
      if (!from) continue;
      ex1.push(layout.x[from.index] as number);
      ey1.push(layout.y[from.index] as number);
      ex2.push(layout.x[node.index] as number);
      ey2.push(layout.y[node.index] as number);
      ea.push(at[from.index] as number);
      eb.push(at[node.index] as number);
    }
  }
  const end = layout.width - 8;
  const bands = model.lanes.map((l) => {
    const x0 = Math.max(0, layout.xAt(l.opened) - layout.step / 4);
    const x1 = l.closed === null ? end : layout.xAt(l.closed);
    const h = layout.laneHeight[l.index] as number;
    return { x: x0, y: (layout.laneY[l.index] as number) - h / 2, w: Math.max(0, x1 - x0), h };
  });
  const scene: Scene = {
    n,
    node: Uint32Array.from(order),
    x,
    y,
    r,
    kind,
    lane,
    labeled: Uint32Array.from(labeled),
    edges: {
      n: ex1.length,
      x1: Float64Array.from(ex1),
      y1: Float64Array.from(ey1),
      x2: Float64Array.from(ex2),
      y2: Float64Array.from(ey2),
      a: Uint32Array.from(ea),
      b: Uint32Array.from(eb),
      span: ex1.reduce((most, x, i) => Math.max(most, Math.abs((ex2[i] as number) - x)), 0),
    },
    bands: {
      n: bands.length,
      x: Float64Array.from(bands.map((b) => b.x)),
      y: Float64Array.from(bands.map((b) => b.y)),
      w: Float64Array.from(bands.map((b) => b.w)),
      h: Float64Array.from(bands.map((b) => b.h)),
    },
    extent: { x0: 0, y0: 0, x1: layout.width, y1: layout.height },
    grid: { cw: 0, ch: 0, start: new Uint32Array(1), items: new Uint32Array(0) },
  };
  scene.grid = buildGrid(scene);
  return scene;
}

function buildGrid(scene: Scene): Grid {
  const cw = Math.ceil(scene.extent.x1 / CELL) + 2;
  const ch = Math.ceil(scene.extent.y1 / CELL) + 2;
  const start = new Uint32Array(cw * ch + 1);
  const cell = new Uint32Array(scene.n);
  const clamp = (v: number, hi: number): number => Math.min(hi, Math.max(0, v));
  for (let i = 0; i < scene.n; i++) {
    const c = clamp(Math.floor((scene.y[i] as number) / CELL), ch - 1) * cw + clamp(Math.floor((scene.x[i] as number) / CELL), cw - 1);
    cell[i] = c;
    start[c + 1] = (start[c + 1] as number) + 1;
  }
  for (let c = 0; c < cw * ch; c++) start[c + 1] = (start[c + 1] as number) + (start[c] as number);
  const fill = start.slice(0, cw * ch);
  const items = new Uint32Array(scene.n);
  for (let i = 0; i < scene.n; i++) {
    const c = cell[i] as number;
    items[fill[c] as number] = i;
    fill[c] = (fill[c] as number) + 1;
  }
  return { cw, ch, start, items };
}

/** A scene node's emphasis that takes it off the stage for the pointer and the labels: filtered out (#598). */
export const FILTERED = 2;

/** The nearest node within `radius` world units of (wx, wy), as a scene index, or -1; none filtered out by `fade`. */
export function hitTest(scene: Scene, wx: number, wy: number, radius: number, fade: Uint8Array | null = null): number {
  const g = scene.grid;
  const c0 = Math.max(0, Math.floor((wx - radius) / CELL));
  const c1 = Math.min(g.cw - 1, Math.floor((wx + radius) / CELL));
  const r0 = Math.max(0, Math.floor((wy - radius) / CELL));
  const r1 = Math.min(g.ch - 1, Math.floor((wy + radius) / CELL));
  let best = -1;
  let bd = radius * radius;
  for (let r = r0; r <= r1; r++) {
    for (let c = c0; c <= c1; c++) {
      const cell = r * g.cw + c;
      for (let k = g.start[cell] as number; k < (g.start[cell + 1] as number); k++) {
        const i = g.items[k] as number;
        if (fade && fade[i] === FILTERED) continue;
        const dx = (scene.x[i] as number) - wx;
        const dy = (scene.y[i] as number) - wy;
        const d = dx * dx + dy * dy;
        if (d < bd) {
          bd = d;
          best = i;
        }
      }
    }
  }
  return best;
}

export const toScreenX = (cam: Camera, view: View, wx: number): number => (wx - cam.cx) * cam.z + view.w / 2;
export const toScreenY = (cam: Camera, view: View, wy: number): number => (wy - cam.cy) * cam.z + view.h / 2;
export const toWorldX = (cam: Camera, view: View, sx: number): number => (sx - view.w / 2) / cam.z + cam.cx;
export const toWorldY = (cam: Camera, view: View, sy: number): number => (sy - view.h / 2) / cam.z + cam.cy;

/** The scene's nodes in view, as a range of scene indices: what the renderer draws. */
export function visibleRange(scene: Scene, cam: Camera, view: View, margin = 60): [number, number] {
  const half = view.w / 2 / cam.z + margin;
  return [lowerBound(scene.x, cam.cx - half), lowerBound(scene.x, cam.cx + half)];
}

export const LABEL_ZOOM = 0.5;
export const MAX_LABELS = 120;

/** The labels of a frame: each one's scene node, its text's left end and its middle on screen, and its width. */
export type Labels = { n: number; id: Uint32Array; sx: Float32Array; sy: Float32Array; w: Float32Array };

export const newLabels = (): Labels => ({
  n: 0,
  id: new Uint32Array(MAX_LABELS),
  sx: new Float32Array(MAX_LABELS),
  sy: new Float32Array(MAX_LABELS),
  w: new Float32Array(MAX_LABELS),
});

/** The plate under a label's text, as the renderer draws it (canvas2d.ts): 3 px either side, 8 above and below. */
export const PLATE = { x: 3, y: 8 } as const;
/** A label's text starts this far from its node's centre, on its right, or ends as far on its left. */
export const LABEL_GAP = 12;

/** A label's rank (#489): first a milestone waiting for the owner or at risk, then by kind, a gate, an exception, a
 * decision, an abandonment, a rejoin, an exploration. Scene kinds are indices of KINDS. */
const KIND_RANK = [0, 4, 6, 5, 1, 2, 3];
export const labelRank = (scene: Scene, i: number, urgent = false): number => (urgent ? 10 : 0) + (KIND_RANK[scene.kind[i] as number] as number);

/** A place on screen no label may cover: `[x0, y0, x1, y1]`, in CSS pixels. */
export type Reserved = readonly (readonly [number, number, number, number])[];

/**
 * The labels to draw this frame (#489): the turning points in view, the highest rank first (`rank`, `labelRank` by
 * default) and from left to right within a rank. A label goes to its node's right, or to its left where it would
 * leave the stage, and is skipped when its plate would cover one already placed or a reserved place. The plates are
 * tested where they are drawn, on a grid of 8×16 px cells, every cell a plate touches: the spike tested one row and
 * the place on the right, so two labels could overlap (the dossier's docs/app/moviola/risultati.md, §8). `width`
 * gives a label's width in pixels; a node filtered out by `fade` has none.
 */
export function pickLabels(
  scene: Scene,
  cam: Camera,
  view: View,
  width: (node: number) => number,
  out: Labels,
  fade: Uint8Array | null = null,
  rank: (node: number) => number = (i) => labelRank(scene, i),
  reserved: Reserved = [],
): Labels {
  out.n = 0;
  if (cam.z < LABEL_ZOOM) return out;
  const cols = Math.ceil(view.w / 8);
  const rows = Math.ceil(view.h / 16);
  const occupied = new Uint8Array(cols * rows);
  const cells = (x0: number, y0: number, x1: number, y1: number): [number, number, number, number] => [
    Math.max(0, Math.floor(x0 / 8)),
    Math.min(cols - 1, Math.floor(x1 / 8)),
    Math.max(0, Math.floor(y0 / 16)),
    Math.min(rows - 1, Math.floor(y1 / 16)),
  ];
  for (const [x0, y0, x1, y1] of reserved) {
    const [c0, c1, r0, r1] = cells(x0, y0, x1, y1);
    for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) occupied[r * cols + c] = 1;
  }
  const halfW = view.w / 2 / cam.z + 40;
  const halfH = view.h / 2 / cam.z + 20;
  const lo = lowerBound(scene.x, cam.cx - halfW);
  const hi = lowerBound(scene.x, cam.cx + halfW);
  const candidates: number[] = [];
  for (let k = lowerBound(scene.labeled, lo); k < scene.labeled.length; k++) {
    const i = scene.labeled[k] as number;
    if (i >= hi) break;
    if (fade && fade[i] === FILTERED) continue;
    if (Math.abs((scene.y[i] as number) - cam.cy) > halfH) continue;
    candidates.push(i);
  }
  const ranks = new Map(candidates.map((i) => [i, rank(i)]));
  candidates.sort((a, b) => (ranks.get(b) as number) - (ranks.get(a) as number) || a - b);
  for (const i of candidates) {
    if (out.n >= MAX_LABELS) break;
    const nx = toScreenX(cam, view, scene.x[i] as number);
    const sy = toScreenY(cam, view, scene.y[i] as number);
    if (sy - PLATE.y < 0 || sy + PLATE.y > view.h) continue;
    const w = width(i);
    const right = nx + LABEL_GAP;
    const sx = right + w > view.w - 4 ? nx - LABEL_GAP - w : right;
    const [c0, c1, r0, r1] = cells(sx - PLATE.x, sy - PLATE.y, sx + w + PLATE.x, sy + PLATE.y);
    let free = true;
    for (let r = r0; r <= r1 && free; r++) {
      for (let c = c0; c <= c1; c++) {
        if (occupied[r * cols + c]) {
          free = false;
          break;
        }
      }
    }
    if (!free) continue;
    for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) occupied[r * cols + c] = 1;
    out.id[out.n] = i;
    out.sx[out.n] = sx;
    out.sy[out.n] = sy;
    out.w[out.n] = w;
    out.n++;
  }
  return out;
}
