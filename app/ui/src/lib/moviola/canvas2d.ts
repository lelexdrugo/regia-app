// The moviola's renderer, the Canvas 2D that #398 of the dossier chose (docs/app/moviola/scelta-renderer.md): one
// frame of the scene (scene.ts) under the camera, then a layer in screen space. Copied from the renderer spike,
// commit 84871ea of regia-app's spike/moviola-renderer: src/renderers/canvas2d.js (`frame`: the world transform,
// one path per kind, the nodes in view found by binary search on x) and src/renderers/common.js
// (`drawScreenLayer`: veil, playhead, timecode, labels, the ring of the node under the pointer). The marks are the
// prototype's (the dossier's tools/moviola/static/moviola.js: `glifo`, the rings of `yAnello`, `badge`, the lanes'
// envelopes and `arco` of `disegnaPalco`, the tape of `disegnaNastro` with its joints), in the seven shapes and the
// tokens of the design system. Text is drawn by the system's text engine; its words come from the catalog.
//
// The level of detail (#489). Measured on one machine (WebView2 158 on Skia Ganesh over Direct3D 11, an Intel Arc
// GPU, density 2; the dossier's report of #489): a path that spans the stage, one for the rings in view, cost a few
// ms to rasterise with only a few dozen marks in it, about three times less at density 1, and much less split by
// region. So
// the world is drawn part by part (`drawWorldDirect`, cells of `CELL` px), or blitted from tiles drawn once for the
// zoom (tiles.ts). Under the zoom of groups (lod.ts) each lane's milestones are drawn in groups, a group of one as its
// glyph. The tape's marks that do not move with the playhead are drawn once, in two layers, for each size, direction
// and language.
import { AT_RISK, groupHalfHeight, groupLevel, groupRange, type Groups, SINGLE_RADIUS, size, WAITING } from "./lod";
import type { Layout, Lane, Envelope, Model, TimeScale } from "./model";
import { ringOffset } from "./model";
import type { Palette } from "./palette";
import { type Camera, KINDS, LABEL_ZOOM, type Labels, lowerBound, PLATE, type Reserved, type Scene, toScreenX, toScreenY, toWorldX, type View } from "./scene";
import type { MilestoneKind } from "./types";

const TAU = Math.PI * 2;

/** The tape's height, under the stage, in CSS pixels. */
export const TAPE = 46;
/** The band at the top of the stage that holds the days. */
export const AXIS = 22;
/** A glyph's radius, in world units: the prototype's in its variant A. */
export const GLYPH = 7;
/** The side of a part of the stage drawn on its own, in CSS pixels (#489): a path spans at most one. */
export const CELL = 256;

/** The words the canvas draws, from the catalog: the frame never formats anything itself. */
export type CanvasText = {
  day: (t: number) => string;
  timecode: (t: number) => string;
  laneName: (lane: Lane) => string;
  laneStatus: (lane: Lane) => string;
};

export type Tape = { scale: TimeScale; x0: number; x1: number };

/** The tape's marks that do not move with the playhead (#489): under the part in view, and over it. */
export type TapeLayers = { under: HTMLCanvasElement; over: HTMLCanvasElement; top: number };

export type Frame = {
  model: Model;
  layout: Layout;
  scene: Scene;
  /** Scene indices by kind, each ascending, so each kind's nodes in view are a binary search away. */
  buckets: Uint32Array[];
  envelopes: Envelope[];
  /** Model indices of each day's first node. */
  days: number[];
  cam: Camera;
  /** The stage, above the tape, in CSS pixels. */
  view: View;
  width: number;
  height: number;
  dpr: number;
  palette: Palette;
  /** The playhead's instant. */
  t: number;
  /** Scene indices, or -1. */
  selected: number;
  hover: number;
  labels: Labels;
  /** Each scene node's label, already cut to its length. */
  labelText: (node: number) => string;
  text: CanvasText;
  tape: Tape;
  /** Each scene node's emphasis (#598), or `null` when every node is drawn in full: `FADED` for a lane other than
   * the one chosen, `FILTERED` for a node the filters leave out. */
  fade: Uint8Array | null;
  /** The lane chosen in the overview, or -1: the others' envelopes, names and nodes are dimmed. */
  chosen: number;
  /** The groups of this zoom (#489, lod.ts), or `null` at the zoom of detail, with the lanes' envelopes through
   * them; the group under the pointer, or -1. */
  groups: Groups | null;
  groupEnvelopes: Envelope[] | null;
  hoverGroup: number;
  /** The tape's layers once drawn, `null` until the next frame draws them. */
  tapeLayers: TapeLayers | null;
  /** Each lane's name and state, measured once in this frame's faces, `null` until the next frame measures them. */
  nameWidths: Float32Array | null;
  /** The groups of one's glyphs, drawn once in this frame's tokens and density, `null` until a frame draws one. */
  sprites: Map<number, HTMLCanvasElement> | null;
  /** Whether the world is drawn part by part and the groups of one from sprites (stage.ts's PARTS), or in one pass. */
  parts: boolean;
};

/** The alpha of each emphasis (#598): in full, dimmed as the prototype's `attenuato`, filtered out as its
 * `nascosto`, almost gone but in its place, so the structure does not move. */
export const FADE = [1, 0.28, 0.04] as const;
export const FADED = 1;

/** The emphases a frame draws, one pass each: only the full one when nothing is dimmed or filtered out. */
const levels = (f: Frame): readonly (0 | 1 | 2)[] => (f.fade ? [0, 1, 2] : [0]);
const fadeOf = (f: Frame, k: number): number => (f.fade ? (f.fade[k] as number) : 0);
const laneAlpha = (f: Frame, lane: number): number => (f.chosen >= 0 && lane !== f.chosen ? FADE[FADED] : 1);

/** The fill and stroke of each kind, as the UX prototype's `node-mark` rules give them. */
const STYLE: Record<MilestoneKind, { fill: keyof Palette | null; stroke: keyof Palette; dash: boolean }> = {
  milestone: { fill: "node", stroke: "node", dash: false },
  decision: { fill: "surface", stroke: "node", dash: false },
  gate: { fill: "accent", stroke: "accent", dash: false },
  exception: { fill: "negative", stroke: "negative", dash: false },
  exploration: { fill: null, stroke: "node", dash: true },
  rejoin: { fill: null, stroke: "node", dash: false },
  abandonment: { fill: null, stroke: "node", dash: false },
};

/** The tick of each kind on the tape: the turning points in their colour, the steps in the node's. A group (#489)
 * takes its most telling kind's. */
const TICK: Record<MilestoneKind, keyof Palette> = {
  milestone: "node",
  decision: "node",
  gate: "accent",
  exception: "negative",
  exploration: "muted",
  rejoin: "positive",
  abandonment: "faint",
};

/** Adds one glyph to the current path: the seven shapes of the design system. */
function glyph(ctx: CanvasRenderingContext2D, kind: MilestoneKind, x: number, y: number, r: number): void {
  const d = r * 1.25;
  switch (kind) {
    case "decision":
      ctx.moveTo(x, y - d);
      ctx.lineTo(x + d, y);
      ctx.lineTo(x, y + d);
      ctx.lineTo(x - d, y);
      ctx.closePath();
      break;
    case "gate":
      ctx.rect(x - r * 0.9, y - r * 0.9, r * 1.8, r * 1.8);
      break;
    case "exception":
      ctx.moveTo(x, y - d);
      ctx.lineTo(x + d, y + r);
      ctx.lineTo(x - d, y + r);
      ctx.closePath();
      break;
    case "rejoin":
      ctx.moveTo(x - r, y - r);
      ctx.lineTo(x + r, y);
      ctx.lineTo(x - r, y + r);
      break;
    case "abandonment":
      ctx.moveTo(x - r, y - r);
      ctx.lineTo(x + r, y + r);
      ctx.moveTo(x + r, y - r);
      ctx.lineTo(x - r, y + r);
      break;
    default:
      ctx.moveTo(x + r, y);
      ctx.arc(x, y, r, 0, TAU);
  }
}

/** First index of `points`, ascending in x, with `x >= v`. */
function lowerBoundBy(points: readonly { x: number }[], v: number): number {
  let lo = 0;
  let hi = points.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if ((points[m] as { x: number }).x < v) lo = m + 1;
    else hi = m;
  }
  return lo;
}

/** The scene's indices by kind, each ascending. */
export function bucketByKind(scene: Scene): Uint32Array[] {
  const lists: number[][] = KINDS.map(() => []);
  for (let i = 0; i < scene.n; i++) lists[scene.kind[i] as number]?.push(i);
  return lists.map((list) => Uint32Array.from(list));
}

/** The text for a label: the spike's `labelText`, a title cut at 36 characters. */
export function cut(title: string): string {
  return title.length > 36 ? `${title.slice(0, 35)}…` : title;
}

function plate(ctx: CanvasRenderingContext2D, colour: string, x: number, y: number, w: number, h: number, alpha = 1): void {
  ctx.globalAlpha = 0.85 * alpha;
  ctx.fillStyle = colour;
  ctx.fillRect(x, y, w, h);
  ctx.globalAlpha = 1;
}

/** The world a mark of a node may reach beyond the node's centre, in world units: rings and the lane's envelope
 * above and below, badges on the right, a group's mark at a few pixels whatever the zoom. A part of the stage draws
 * every node within this of it. */
export function worldMargin(f: Frame): [number, number] {
  return [Math.max(100, 24 / f.cam.z), Math.max(110, 24 / f.cam.z)];
}

/** A canvas of its own, off the page. */
function layer(width: number, height: number): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  return canvas;
}

/** Where each lane's name and state are drawn this frame, held at the stage's left edge as the stage moves under
 * them: `[lane, x, y, width]`, the plate's left, the name's line and the plate's width, in CSS pixels. Labels keep off
 * them (scene.ts's `pickLabels`). */
export function namePlates(f: Frame, ctx: CanvasRenderingContext2D): [number, number, number, number][] {
  const { model, layout, cam, view, palette: p } = f;
  if (!f.nameWidths) {
    const widths = new Float32Array(model.lanes.length);
    for (const lane of model.lanes) {
      ctx.font = `500 12px ${p.font}`;
      const wName = ctx.measureText(f.text.laneName(lane)).width;
      ctx.font = `11px ${p.font}`;
      const wStatus = ctx.measureText(f.text.laneStatus(lane)).width;
      widths[lane.index] = Math.max(wName, wStatus) + 8;
    }
    f.nameWidths = widths;
  }
  const nameX = Math.max(8, toScreenX(cam, view, 12));
  const out: [number, number, number, number][] = [];
  for (const lane of model.lanes) {
    const sy = toScreenY(cam, view, layout.laneY[lane.index] as number);
    if (sy < AXIS || sy > view.h + 12) continue;
    out.push([lane.index, nameX - 4, sy, f.nameWidths[lane.index] as number]);
  }
  return out;
}

/** The places labels keep off: the lanes' names and the days' band. */
export function reservedPlaces(f: Frame, ctx: CanvasRenderingContext2D): Reserved {
  const places: [number, number, number, number][] = [[0, 0, f.view.w, AXIS]];
  for (const [, x, sy, w] of namePlates(f, ctx)) places.push([x, sy - 17, x + w, sy + 15]);
  return places;
}

/** A source of a frame's world drawn already (tiles.ts): `false` when the frame has to draw it itself. */
export type WorldSource = { draw(ctx: CanvasRenderingContext2D, f: Frame): boolean };

export function drawFrame(ctx: CanvasRenderingContext2D, f: Frame, world: WorldSource | null = null): void {
  const { scene, model, layout, cam, view, palette: p, dpr } = f;
  const { z } = cam;
  const W = view.w;
  const H = view.h;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = p.surface;
  ctx.fillRect(0, 0, f.width, f.height);

  // The stage, clipped above the tape.
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, W, H);
  ctx.clip();

  // The days, as lines across the stage.
  ctx.strokeStyle = p.rule;
  ctx.lineWidth = 1;
  ctx.setLineDash([2, 5]);
  ctx.beginPath();
  const dayX = f.days.map((i) => toScreenX(cam, view, (layout.x[i] as number) - Math.min(layout.step / 2, 60)));
  for (const sx of dayX) {
    if (sx < 0 || sx > W) continue;
    ctx.moveTo(Math.round(sx) + 0.5, AXIS);
    ctx.lineTo(Math.round(sx) + 0.5, H);
  }
  ctx.stroke();
  ctx.setLineDash([]);

  // The world: from tiles drawn at this zoom, or part by part.
  if (!world || !world.draw(ctx, f)) drawWorldDirect(ctx, f);

  // The screen layer.
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.globalAlpha = 1;
  const labels = f.labels;
  if (labels.n) {
    ctx.font = `12px ${p.font}`;
    ctx.textBaseline = "middle";
    for (let i = 0; i < labels.n; i++) {
      // The picker placed it already, on the node's right or, near the stage's right edge, on its left.
      const sx = labels.sx[i] as number;
      const sy = labels.sy[i] as number;
      plate(ctx, p.surface, sx - PLATE.x, sy - PLATE.y, (labels.w[i] as number) + 2 * PLATE.x, 2 * PLATE.y);
      ctx.fillStyle = p.muted;
      ctx.fillText(f.labelText(labels.id[i] as number), sx, sy);
    }
    ctx.textBaseline = "alphabetic";
  }

  // The lanes' names, held at the left edge while the stage moves under them.
  for (const [index, x, sy, w] of namePlates(f, ctx)) {
    const lane = model.lanes[index] as Lane;
    plate(ctx, p.surface, x, sy - 17, w, 32);
    ctx.globalAlpha = laneAlpha(f, index);
    ctx.font = `500 12px ${p.font}`;
    ctx.fillStyle = p.text;
    ctx.fillText(f.text.laneName(lane), x + 4, sy - 4);
    ctx.font = `11px ${p.font}`;
    ctx.fillStyle = p.faint;
    ctx.fillText(f.text.laneStatus(lane), x + 4, sy + 11);
    ctx.globalAlpha = 1;
  }

  // The days' band at the top.
  plate(ctx, p.surface, 0, 0, W, AXIS);
  ctx.font = `11px ${p.font}`;
  ctx.fillStyle = p.faint;
  let lastX = -1e9;
  f.days.forEach((i, k) => {
    const sx = dayX[k] as number;
    if (sx < -60 || sx > W || sx - lastX < 44) return;
    lastX = sx;
    ctx.fillText(f.text.day((model.nodes[i] as Model["nodes"][number]).t), Math.max(4, sx + 4), 15);
  });

  // The playhead: the future under a veil, the line, its head and the timecode.
  const px = toScreenX(cam, view, layout.xAt(f.t));
  if (px < W) {
    ctx.globalAlpha = 0.55;
    ctx.fillStyle = p.ground;
    ctx.fillRect(Math.max(0, px), AXIS, W - Math.max(0, px), H - AXIS);
    ctx.globalAlpha = 1;
  }
  if (px >= -8 && px <= W + 8) {
    ctx.fillStyle = p.accent;
    ctx.fillRect(px - 0.75, AXIS, 1.5, H - AXIS);
    ctx.beginPath();
    ctx.moveTo(px - 6, AXIS);
    ctx.lineTo(px + 6, AXIS);
    ctx.lineTo(px, AXIS + 9);
    ctx.closePath();
    ctx.fill();
    ctx.font = `500 11px ${p.mono}`;
    const code = f.text.timecode(f.t);
    const right = px > W - 140;
    ctx.textAlign = right ? "end" : "start";
    ctx.fillText(code, right ? px - 7 : px + 7, AXIS + 16);
    ctx.textAlign = "start";
  }

  // The selected node, and the one under the pointer; at the zoom of groups, the group under it.
  const groups = f.groups;
  if (groups && f.hoverGroup >= 0 && f.hoverGroup < groups.n) {
    const g = f.hoverGroup;
    const hh = groupHalfHeight(size(groups, g)) + 4;
    const a = toScreenX(cam, view, groups.x0[g] as number);
    const b = toScreenX(cam, view, groups.x1[g] as number);
    const sy = toScreenY(cam, view, groups.y[g] as number);
    ctx.strokeStyle = p.muted;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.roundRect(a - hh, sy - hh, b - a + 2 * hh, 2 * hh, hh);
    ctx.stroke();
  }
  for (const [index, colour, width] of [[groups ? -1 : f.hover, p.muted, 1], [f.selected, p.accent, 2]] as const) {
    if (index < 0) continue;
    const sx = toScreenX(cam, view, scene.x[index] as number);
    const sy = toScreenY(cam, view, scene.y[index] as number);
    ctx.strokeStyle = colour;
    ctx.lineWidth = width;
    ctx.beginPath();
    ctx.arc(sx, sy, Math.max(11, (GLYPH + 4) * z), 0, TAU);
    ctx.stroke();
  }
  ctx.restore();

  drawTape(ctx, f, toWorldX(cam, view, 0), toWorldX(cam, view, W));
}

/** The world under the frame's camera, drawn straight onto the stage one cell at a time: each cell clipped on whole
 * device pixels, so two cells meet without a seam, and each with paths of its own, so none spans the stage. The
 * world's origin lands on a whole device pixel, as with the tiles (tiles.ts), so the two ways draw the same pixels. */
export function drawWorldDirect(ctx: CanvasRenderingContext2D, f: Frame): void {
  const { cam, view, dpr } = f;
  const s = dpr * cam.z;
  const tx = Math.round(dpr * (view.w / 2 - cam.cx * cam.z));
  const ty = Math.round(dpr * (view.h / 2 - cam.cy * cam.z));
  const wide = Math.round(view.w * dpr);
  const high = Math.round(view.h * dpr);
  const [mx, my] = worldMargin(f);
  if (!f.parts) {
    // In one pass, as before #489, where parts cost more than they save (stage.ts's PARTS).
    ctx.save();
    ctx.setTransform(s, 0, 0, s, tx, ty);
    drawWorld(ctx, f, -tx / s - mx, (wide - tx) / s + mx, -ty / s - my, (high - ty) / s + my);
    ctx.restore();
    return;
  }
  const step = Math.max(1, Math.round(CELL * dpr));
  for (let dy = 0; dy < high; dy += step) {
    for (let dx = 0; dx < wide; dx += step) {
      const w = Math.min(step, wide - dx);
      const h = Math.min(step, high - dy);
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.beginPath();
      ctx.rect(dx, dy, w, h);
      ctx.clip();
      ctx.setTransform(s, 0, 0, s, tx, ty);
      drawWorld(ctx, f, (dx - tx) / s - mx, (dx + w - tx) / s + mx, (dy - ty) / s - my, (dy + h - ty) / s + my);
      ctx.restore();
    }
  }
}

/**
 * The world between `x0` and `x1`, `y0` and `y1` in world units, under the context's transform, which maps the world
 * at the frame's zoom: the lanes' envelopes and lines, the edges, then the nodes' marks, or at the zoom of groups the
 * groups' (lod.ts). Everything a mark of a node in that part may reach is drawn, and nothing else.
 */
export function drawWorld(ctx: CanvasRenderingContext2D, f: Frame, x0: number, x1: number, y0: number, y1: number): void {
  const { scene, model, cam, palette: p } = f;
  const { z } = cam;
  const groups = f.groups;

  // Lanes: the envelope of their rings, then the line, dashed once abandoned, dotted once rejoined. Only the part
  // in view is a path: a dash is laid along the whole of one, in view or not. At the zoom of groups the envelope
  // goes through the groups, in straight lines and with no edge: its curves cost more than the rest of the stage.
  for (const e of groups ? (f.groupEnvelopes ?? f.envelopes) : f.envelopes) {
    const lane = model.lanes[e.lane] as Lane;
    if (e.x1 < x0 || e.x0 > x1) continue;
    const all = e.points;
    const from = Math.max(0, lowerBoundBy(all, x0) - 1);
    const to = Math.min(all.length, lowerBoundBy(all, x1) + 1);
    const pts = all.slice(from, to);
    if (pts.length === 0) continue;
    const thick = pts.reduce((most, q) => Math.max(most, q.w), 0);
    if (e.y + thick < y0 || e.y - thick > y1) continue;
    const dim = laneAlpha(f, e.lane);
    const a = pts[0] as { x: number; w: number };
    const b = pts[pts.length - 1] as { x: number; w: number };
    ctx.beginPath();
    ctx.moveTo(a.x, e.y - a.w);
    for (let i = 1; i < pts.length; i++) {
      const p0 = pts[i - 1] as { x: number; w: number };
      const p1 = pts[i] as { x: number; w: number };
      const mx = (p0.x + p1.x) / 2;
      if (groups) ctx.lineTo(p1.x, e.y - p1.w);
      else ctx.bezierCurveTo(mx, e.y - p0.w, mx, e.y - p1.w, p1.x, e.y - p1.w);
    }
    ctx.lineTo(b.x, e.y + b.w);
    for (let i = pts.length - 1; i > 0; i--) {
      const p0 = pts[i] as { x: number; w: number };
      const p1 = pts[i - 1] as { x: number; w: number };
      const mx = (p0.x + p1.x) / 2;
      if (groups) ctx.lineTo(p1.x, e.y + p1.w);
      else ctx.bezierCurveTo(mx, e.y + p0.w, mx, e.y + p1.w, p1.x, e.y + p1.w);
    }
    ctx.closePath();
    ctx.globalAlpha = dim;
    ctx.fillStyle = p.lane;
    ctx.fill();
    ctx.strokeStyle = p.laneEdge;
    if (!groups) {
      ctx.globalAlpha = 0.6 * dim;
      ctx.lineWidth = 0.8 / z;
      ctx.stroke();
    }
    ctx.globalAlpha = dim;
    const start = Math.max(e.x0, x0);
    ctx.setLineDash(lane.status === "abandoned" ? [5 / z, 4 / z] : lane.status === "rejoined" ? [1 / z, 4 / z] : []);
    // The dashes stay where they are on the lane while the camera moves.
    ctx.lineDashOffset = start - e.x0;
    ctx.lineWidth = 1 / z;
    ctx.beginPath();
    ctx.moveTo(start, e.y);
    ctx.lineTo(Math.min(e.x1, x1), e.y);
    ctx.stroke();
    ctx.lineDashOffset = 0;
    ctx.setLineDash([]);
  }
  ctx.globalAlpha = 1;

  if (groups) {
    drawGroups(ctx, f, groups, x0, x1, y0, y1);
    return;
  }

  // Edges, parent to child: within a lane in the lane's mark, across lanes dashed and faint; an edge goes with a
  // node filtered out. They come by the child's x: those that reach into this part are a binary search away.
  const edges = scene.edges;
  const fade = f.fade;
  const first = lowerBound(edges.x2, x0 - edges.span);
  for (const across of [false, true]) {
    ctx.strokeStyle = across ? p.faint : p.muted;
    ctx.lineWidth = 1.3 / z;
    ctx.setLineDash(across ? [4 / z, 3 / z] : []);
    for (const gone of fade ? [false, true] : [false]) {
      ctx.globalAlpha = (across ? 1 : 0.75) * (gone ? FADE[2] : 1);
      ctx.beginPath();
      for (let i = first; i < edges.n; i++) {
        const ax = edges.x1[i] as number;
        const ay = edges.y1[i] as number;
        const bx = edges.x2[i] as number;
        const by = edges.y2[i] as number;
        if (bx > x1 + edges.span) break;
        if ((ay !== by) !== across) continue;
        if ((ax < x0 && bx < x0) || (ax > x1 && bx > x1)) continue;
        if ((ay < y0 && by < y0) || (ay > y1 && by > y1)) continue;
        if (fade && (fade[edges.a[i] as number] === 2 || fade[edges.b[i] as number] === 2) !== gone) continue;
        const mx = (ax + bx) / 2;
        ctx.moveTo(ax, ay);
        ctx.bezierCurveTo(mx, ay, mx, by, bx, by);
      }
      ctx.stroke();
    }
  }
  ctx.globalAlpha = 1;
  ctx.setLineDash([]);

  const lo = lowerBound(scene.x, x0);
  const hi = lowerBound(scene.x, x1);

  // Every mark of a node below, one pass per emphasis: a single one when nothing is dimmed or filtered out.
  for (const level of levels(f)) {
    // Rings, one per round, above and below the glyph in turn: declared in the muted ink, undeclared dashed.
    ctx.globalAlpha = FADE[level];
    for (const declared of [true, false]) {
      ctx.strokeStyle = declared ? p.muted : p.faint;
      ctx.lineWidth = 1.4;
      ctx.setLineDash(declared ? [] : [2, 1.6]);
      ctx.beginPath();
      for (let k = lo; k < hi; k++) {
        const y = scene.y[k] as number;
        if (y < y0 || y > y1 || fadeOf(f, k) !== level) continue;
        const x = scene.x[k] as number;
        const rings = (model.nodes[scene.node[k] as number] as Model["nodes"][number]).rings;
        rings.forEach((ring, i) => {
          if (ring.declared !== declared) return;
          const ry = y + ringOffset(i);
          ctx.moveTo(x + 4.4, ry);
          ctx.arc(x, ry, 4.4, 0, TAU);
        });
      }
      ctx.stroke();
    }
    ctx.setLineDash([]);

    // Health: a halo for a milestone at risk, or waiting for the owner.
    ctx.lineWidth = 2;
    ctx.globalAlpha = 0.7 * FADE[level];
    for (const [health, colour] of [["at_risk", p.negative], ["waiting_on_owner", p.accent]] as const) {
      ctx.strokeStyle = colour;
      ctx.beginPath();
      for (let k = lo; k < hi; k++) {
        const y = scene.y[k] as number;
        if (y < y0 || y > y1 || fadeOf(f, k) !== level) continue;
        if ((model.nodes[scene.node[k] as number] as Model["nodes"][number]).milestone.health !== health) continue;
        const x = scene.x[k] as number;
        ctx.moveTo(x + GLYPH + 6, y);
        ctx.arc(x, y, GLYPH + 6, 0, TAU);
      }
      ctx.stroke();
    }

    // Glyphs, one path per kind.
    ctx.globalAlpha = FADE[level];
    ctx.lineWidth = 1.5;
    KINDS.forEach((kind, k) => {
      const list = f.buckets[k];
      if (!list || list.length === 0) return;
      const style = STYLE[kind];
      ctx.beginPath();
      for (let q = lowerBound(list, lo); q < list.length; q++) {
        const i = list[q] as number;
        if (i >= hi) break;
        const y = scene.y[i] as number;
        if (y < y0 || y > y1 || fadeOf(f, i) !== level) continue;
        glyph(ctx, kind, scene.x[i] as number, y, GLYPH);
      }
      if (style.fill) {
        ctx.fillStyle = p[style.fill];
        ctx.fill();
      }
      ctx.strokeStyle = p[style.stroke];
      ctx.setLineDash(style.dash ? [2, 2] : []);
      ctx.stroke();
    });
    ctx.setLineDash([]);
  }
  ctx.globalAlpha = 1;

  // Badges, where they can be read: ✓ a check passed, ? one did not conclude, D3 or 1/3 the defects.
  if (z >= LABEL_ZOOM) {
    ctx.font = `500 9px ${p.mono}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (let k = lo; k < hi; k++) {
      const y = scene.y[k] as number;
      if (y < y0 || y > y1) continue;
      const badges = (model.nodes[scene.node[k] as number] as Model["nodes"][number]).badges;
      ctx.globalAlpha = FADE[fadeOf(f, k) as 0 | 1 | 2];
      let bx = (scene.x[k] as number) + 10;
      for (const badge of badges) {
        const w = 7 + badge.length * 6.2;
        ctx.fillStyle = p.pillBg;
        ctx.beginPath();
        ctx.roundRect(bx, y - 21, w, 13, 6.5);
        ctx.fill();
        ctx.fillStyle = p.pillInk;
        ctx.fillText(badge, bx + w / 2, y - 14.5);
        bx += w + 2;
      }
    }
    ctx.globalAlpha = 1;
    ctx.textAlign = "start";
    ctx.textBaseline = "alphabetic";
  }
}

/** The side of a group of one's sprite, in CSS pixels: its glyph at `SINGLE_RADIUS` and the halo around it. */
const SPRITE = 16;

/** A group of one's glyph, in the frame's tokens and density, with the halo of its health: drawn once for each kind
 * and health, then copied wherever such a group is. At the zoom of groups its size on screen does not change. */
function sprite(f: Frame, kind: number, health: number): HTMLCanvasElement {
  const sprites = f.sprites ?? (f.sprites = new Map());
  const key = kind * 4 + health;
  const known = sprites.get(key);
  if (known) return known;
  const { palette: p, dpr } = f;
  const side = Math.ceil(SPRITE * dpr);
  const canvas = layer(side, side);
  const ctx = canvas.getContext("2d") as CanvasRenderingContext2D;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  const m = SPRITE / 2;
  ctx.lineWidth = 1.5;
  ctx.globalAlpha = 0.7;
  for (const [bit, colour, r] of [[AT_RISK, p.negative, SINGLE_RADIUS + 3], [WAITING, p.accent, SINGLE_RADIUS + 4.5]] as const) {
    if (!(health & bit)) continue;
    ctx.strokeStyle = colour;
    ctx.beginPath();
    ctx.arc(m, m, r, 0, TAU);
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  const name = KINDS[kind] as MilestoneKind;
  const style = STYLE[name];
  ctx.beginPath();
  glyph(ctx, name, m, m, SINGLE_RADIUS);
  if (style.fill) {
    ctx.fillStyle = p[style.fill];
    ctx.fill();
  }
  ctx.lineWidth = 1.2;
  ctx.strokeStyle = p[style.stroke];
  ctx.setLineDash(style.dash ? [1, 1] : []);
  ctx.stroke();
  sprites.set(key, canvas);
  return canvas;
}

/**
 * The groups' marks (#489), in the primitives the Canvas draws cheapest, since at this zoom the stage is all marks:
 * the edges across lanes, straight from group to group (within a lane the lane's own line stands for them); a halo,
 * a rectangle, around a group with a milestone at risk or waiting for the owner; then each group, one pass per
 * emphasis. A group of one is its milestone's glyph, at `SINGLE_RADIUS` on screen, from a sprite; a larger one a bar
 * from its first milestone to its last, taller for each doubling, in the tape's ink of its most telling kind.
 */
function drawGroups(ctx: CanvasRenderingContext2D, f: Frame, groups: Groups, x0: number, x1: number, y0: number, y1: number): void {
  const { palette: p, cam } = f;
  const z = cam.z;
  const fade = f.fade;
  const [lo, hi] = groupRange(groups, x0, x1);
  const centre = (g: number): number => ((groups.x0[g] as number) + (groups.x1[g] as number)) / 2;

  const edges = groups.edges;
  ctx.strokeStyle = p.faint;
  ctx.lineWidth = 1.3 / z;
  ctx.setLineDash([4 / z, 3 / z]);
  for (const gone of fade ? [false, true] : [false]) {
    ctx.globalAlpha = gone ? FADE[2] : 1;
    ctx.beginPath();
    for (let i = 0; i < edges.n; i++) {
      const a = edges.a[i] as number;
      const b = edges.b[i] as number;
      const ay = groups.y[a] as number;
      const by = groups.y[b] as number;
      if (ay === by) continue;
      const ax = centre(a);
      const bx = centre(b);
      if ((ax < x0 && bx < x0) || (ax > x1 && bx > x1)) continue;
      if ((ay < y0 && by < y0) || (ay > y1 && by > y1)) continue;
      if (fade && (groupLevel(groups, fade, a) === 2 || groupLevel(groups, fade, b) === 2) !== gone) continue;
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
    }
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.setLineDash([]);

  const inPart: number[] = [];
  for (let g = lo; g < hi; g++) {
    const y = groups.y[g] as number;
    if ((groups.x1[g] as number) < x0 || y < y0 || y > y1) continue;
    inPart.push(g);
  }
  const levelOf = fade ? new Map(inPart.map((g) => [g, groupLevel(groups, fade, g)])) : null;
  // Where the world lands in device pixels: a sprite is copied on whole ones, at its own resolution.
  const at = ctx.getTransform();
  const half = Math.ceil(SPRITE * f.dpr) / 2;
  for (const level of levels(f)) {
    const mine = levelOf ? inPart.filter((g) => levelOf.get(g) === level) : inPart;
    if (mine.length === 0) continue;
    // The halos of the bars.
    ctx.lineWidth = 1.5 / z;
    ctx.globalAlpha = 0.7 * FADE[level];
    for (const [bit, colour] of [[AT_RISK, p.negative], [WAITING, p.accent]] as const) {
      ctx.strokeStyle = colour;
      ctx.beginPath();
      for (const g of mine) {
        const n = size(groups, g);
        if (n < 2 || !((groups.health[g] as number) & bit)) continue;
        const r = (groupHalfHeight(n) + (bit === AT_RISK ? 2.5 : 4)) / z;
        const a = groups.x0[g] as number;
        const b = groups.x1[g] as number;
        ctx.rect(a - r, (groups.y[g] as number) - r, b - a + 2 * r, 2 * r);
      }
      ctx.stroke();
    }
    // The bars, in their kind's ink.
    ctx.globalAlpha = FADE[level];
    for (const g of mine) {
      const n = size(groups, g);
      if (n < 2) continue;
      const hh = groupHalfHeight(n) / z;
      const a = groups.x0[g] as number;
      const b = groups.x1[g] as number;
      ctx.fillStyle = p[TICK[KINDS[groups.kind[g] as number] as MilestoneKind]];
      ctx.fillRect(a - hh, (groups.y[g] as number) - hh, b - a + 2 * hh, 2 * hh);
    }
    // The groups of one: from sprites, or where copying a canvas costs more than drawing (stage.ts's PARTS), their
    // glyphs and halos as paths, one for each kind.
    if (f.parts) {
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      for (const g of mine) {
        if (size(groups, g) !== 1) continue;
        const dx = at.a * (groups.x0[g] as number) + at.e;
        const dy = at.d * (groups.y[g] as number) + at.f;
        ctx.drawImage(sprite(f, groups.kind[g] as number, groups.health[g] as number), Math.round(dx - half), Math.round(dy - half));
      }
      ctx.restore();
      continue;
    }
    const r = SINGLE_RADIUS / z;
    ctx.lineWidth = 1.5 / z;
    ctx.globalAlpha = 0.7 * FADE[level];
    for (const [bit, colour, ring] of [[AT_RISK, p.negative, 3], [WAITING, p.accent, 4.5]] as const) {
      ctx.strokeStyle = colour;
      ctx.beginPath();
      for (const g of mine) {
        if (size(groups, g) !== 1 || !((groups.health[g] as number) & bit)) continue;
        const x = groups.x0[g] as number;
        ctx.moveTo(x + r + ring / z, groups.y[g] as number);
        ctx.arc(x, groups.y[g] as number, r + ring / z, 0, TAU);
      }
      ctx.stroke();
    }
    ctx.globalAlpha = FADE[level];
    ctx.lineWidth = 1.2 / z;
    KINDS.forEach((kind, k) => {
      const style = STYLE[kind];
      ctx.beginPath();
      let any = false;
      for (const g of mine) {
        if ((groups.kind[g] as number) !== k || size(groups, g) !== 1) continue;
        glyph(ctx, kind, groups.x0[g] as number, groups.y[g] as number, r);
        any = true;
      }
      if (!any) return;
      if (style.fill) {
        ctx.fillStyle = p[style.fill];
        ctx.fill();
      }
      ctx.strokeStyle = p[style.stroke];
      ctx.stroke();
    });
  }
  ctx.globalAlpha = 1;
}

/** The tape under the stage (`disegnaNastro`): the whole story on the time scale, the days, the joints (✂) where
 * nothing happened for more than twelve hours, a tick per milestone, the part of the story in view, the playhead.
 * The lines and ticks, which do not move with the playhead, are drawn once, in two layers (#489): under the part in
 * view, and over it. Its words are drawn on every frame, on the stage's own opaque canvas, as before. */
function drawTape(ctx: CanvasRenderingContext2D, f: Frame, left: number, right: number): void {
  const { palette: p, tape, layout, model, dpr } = f;
  const top = f.view.h;
  const layers = f.tapeLayers ?? (f.tapeLayers = tapeLayers(f));
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.drawImage(layers.under, 0, layers.top);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  // The part of the story the stage shows.
  const a = tape.scale.x(layout.tAt(left));
  const b = tape.scale.x(layout.tAt(right));
  ctx.globalAlpha = 0.7;
  ctx.fillStyle = p.raised;
  ctx.fillRect(a, top + 9, Math.max(2, b - a), TAPE - 18);
  ctx.globalAlpha = 1;
  // The days' labels and the joints, where the days' lines are.
  const scale = tape.scale;
  ctx.font = `10px ${p.font}`;
  ctx.fillStyle = p.faint;
  let lastX = -1e9;
  for (const i of f.days) {
    const t = (model.nodes[i] as Model["nodes"][number]).t;
    const x = Math.round(scale.x(t) - 3) + 0.5;
    if (x - lastX < 44) continue;
    lastX = x;
    ctx.fillText(f.text.day(t), x + 2, top + 19);
  }
  ctx.textAlign = "center";
  for (const joint of scale.joints) ctx.fillText("✂", joint.x, top + 37);
  ctx.textAlign = "start";
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.drawImage(layers.over, 0, layers.top);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  const x = tape.scale.x(f.t);
  ctx.fillStyle = p.accent;
  ctx.fillRect(x - 0.75, top + 7, 1.5, TAPE - 14);
  ctx.beginPath();
  ctx.moveTo(x - 5, top + 7);
  ctx.lineTo(x + 5, top + 7);
  ctx.lineTo(x, top + 14);
  ctx.closePath();
  ctx.fill();
}

/** The tape's two layers, on whole device pixels from the tape's top down: the ground and its perforations, then the
 * days' lines and the ticks. */
function tapeLayers(f: Frame): TapeLayers {
  const { palette: p, tape, model, dpr } = f;
  const W = f.width;
  const oy = f.view.h * dpr;
  const top = Math.floor(oy);
  const frac = oy - top;
  const wide = Math.round(W * dpr);
  const high = Math.ceil(TAPE * dpr + frac);
  const under = layer(wide, high);
  const over = layer(wide, high);
  const u = under.getContext("2d", { alpha: false }) as CanvasRenderingContext2D;
  const o = over.getContext("2d") as CanvasRenderingContext2D;
  u.fillStyle = p.ground;
  u.fillRect(0, 0, wide, high);
  u.setTransform(dpr, 0, 0, dpr, 0, frac);
  o.setTransform(dpr, 0, 0, dpr, 0, frac);
  u.fillStyle = p.border;
  u.fillRect(0, 0, W, 1);
  // Perforations, above and below: it is film.
  u.fillStyle = p.raised;
  u.beginPath();
  for (let x = 4; x < W; x += 14) {
    u.roundRect(x, 3, 7, 4, 1.5);
    u.roundRect(x, TAPE - 7, 7, 4, 1.5);
  }
  u.fill();
  // Days.
  const scale = tape.scale;
  o.strokeStyle = p.rule;
  o.lineWidth = 1;
  o.beginPath();
  for (const i of f.days) {
    const x = Math.round(scale.x((model.nodes[i] as Model["nodes"][number]).t) - 3) + 0.5;
    o.moveTo(x, 10);
    o.lineTo(x, TAPE - 10);
  }
  o.stroke();
  // A tick per milestone, in its kind's colour: one path per kind.
  for (const kind of KINDS) {
    o.strokeStyle = p[TICK[kind]];
    o.lineWidth = kind === "milestone" ? 1 : 2;
    o.beginPath();
    for (const node of model.nodes) {
      if (node.milestone.kind !== kind) continue;
      const x = scale.x(node.t);
      o.moveTo(x, 23);
      o.lineTo(x, TAPE - 9);
    }
    o.stroke();
  }
  return { under, over, top };
}

/** The strip's height (#598), in CSS pixels. */
export const STRIP = 30;

/** The strip «time window» (#598): the whole story, as `disegnaStriscia` draws it, and the part the stage shows. */
export type StripFrame = {
  model: Model;
  scale: TimeScale;
  x0: number;
  x1: number;
  width: number;
  dpr: number;
  palette: Palette;
  text: CanvasText;
  /** Model indices of each day's first node. */
  days: number[];
  /** The part of the story in view, in the strip's x, or `null` when the whole story is. */
  band: [number, number] | null;
  /** The window a hand is drawing, until it lets go. */
  preview: [number, number] | null;
  /** The axis, its days, joints and ticks, drawn once (#489), `null` until the next frame draws them. */
  layer: HTMLCanvasElement | null;
};

/** `disegnaStriscia`: the axis of the whole story with its days, joints and a tick per milestone, then the window,
 * the rest under a veil, with a handle at each end. Every milestone is on it, whatever the filters keep. */
export function drawStrip(ctx: CanvasRenderingContext2D, s: StripFrame): void {
  const { palette: p } = s;
  const axis = s.layer ?? (s.layer = stripLayer(s));
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.drawImage(axis, 0, 0);
  ctx.setTransform(s.dpr, 0, 0, s.dpr, 0, 0);
  // The window: what lies outside it under a veil, its edge in the lamp's colour, and a handle at each end. The whole
  // story has no handles: a drag anywhere draws a window.
  const window = s.preview ?? s.band;
  const [a, b] = window ?? [s.x0, s.x1];
  ctx.globalAlpha = 0.6;
  ctx.fillStyle = p.ground;
  ctx.fillRect(s.x0, 11, Math.max(0, a - s.x0), 18);
  ctx.fillRect(b, 11, Math.max(0, s.x1 - b), 18);
  ctx.globalAlpha = window ? 1 : 0.35;
  ctx.strokeStyle = p.accent;
  // The width the ticks of the last kind left, before the axis was drawn once (#489).
  ctx.lineWidth = 2;
  ctx.strokeRect(Math.round(a) + 0.5, 11.5, Math.max(1, Math.round(b - a)), 17);
  ctx.globalAlpha = 1;
  if (window) {
    ctx.fillStyle = p.accent;
    ctx.beginPath();
    ctx.roundRect(a - 3, 9, 6, 22, 2);
    ctx.roundRect(b - 3, 9, 6, 22, 2);
    ctx.fill();
  }
}

/** The strip's axis: what does not move with the window. */
function stripLayer(s: StripFrame): HTMLCanvasElement {
  const { palette: p, scale, model } = s;
  const canvas = layer(Math.round(s.width * s.dpr), Math.round(STRIP * s.dpr));
  const ctx = canvas.getContext("2d", { alpha: false }) as CanvasRenderingContext2D;
  ctx.setTransform(s.dpr, 0, 0, s.dpr, 0, 0);
  ctx.fillStyle = p.surface;
  ctx.fillRect(0, 0, s.width, STRIP);
  ctx.fillStyle = p.border;
  ctx.fillRect(s.x0, 21, s.x1 - s.x0, 1);
  // The days: a line each, a label where it fits; the last one ends inside the strip.
  ctx.strokeStyle = p.rule;
  ctx.lineWidth = 1;
  ctx.setLineDash([2, 3]);
  ctx.beginPath();
  ctx.font = `10px ${p.mono}`;
  ctx.fillStyle = p.faint;
  let lastRight = -1e9;
  for (const i of s.days) {
    const t = (model.nodes[i] as Model["nodes"][number]).t;
    const x = Math.round(scale.x(t) - 2) + 0.5;
    ctx.moveTo(x, 12);
    ctx.lineTo(x, 29);
    // A label where it fits whole, never over the one before: the day is in the window's text too.
    const label = s.text.day(t);
    const w = ctx.measureText(label).width;
    const left = Math.min(x + 2, s.x1 - w);
    if (left < lastRight + 6) continue;
    lastRight = left + w;
    ctx.fillText(label, left, 9);
  }
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.textAlign = "center";
  for (const joint of scale.joints) ctx.fillText("✂", joint.x, 27);
  ctx.textAlign = "start";
  for (const kind of KINDS) {
    ctx.strokeStyle = p[TICK[kind]];
    ctx.lineWidth = kind === "milestone" ? 1 : 2;
    ctx.beginPath();
    for (const node of model.nodes) {
      if (node.milestone.kind !== kind) continue;
      const x = scale.x(node.t);
      ctx.moveTo(x, 15);
      ctx.lineTo(x, 27);
    }
    ctx.stroke();
  }
  return canvas;
}
