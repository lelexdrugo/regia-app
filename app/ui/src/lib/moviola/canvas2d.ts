// The moviola's renderer, the Canvas 2D that #398 of the dossier chose (docs/app/moviola/scelta-renderer.md): one
// frame of the scene (scene.ts) under the camera, then a layer in screen space. Copied from the renderer spike,
// commit 84871ea of regia-app's spike/moviola-renderer: src/renderers/canvas2d.js (`frame`: the world transform,
// one path per kind, the nodes in view found by binary search on x) and src/renderers/common.js
// (`drawScreenLayer`: veil, playhead, timecode, labels, the ring of the node under the pointer). The marks are the
// prototype's (the dossier's tools/moviola/static/moviola.js: `glifo`, the rings of `yAnello`, `badge`, the lanes'
// envelopes and `arco` of `disegnaPalco`, the tape of `disegnaNastro` with its joints), in the seven shapes and the
// tokens of the design system. Text is drawn by the system's text engine; its words come from the catalog.
import type { Layout, Lane, Envelope, Model, TimeScale } from "./model";
import { ringOffset } from "./model";
import type { Palette } from "./palette";
import { type Camera, KINDS, LABEL_ZOOM, type Labels, lowerBound, type Scene, toScreenX, toScreenY, toWorldX, type View } from "./scene";
import type { MilestoneKind } from "./types";

const TAU = Math.PI * 2;

/** The tape's height, under the stage, in CSS pixels. */
export const TAPE = 46;
/** The band at the top of the stage that holds the days. */
export const AXIS = 22;
/** A glyph's radius, in world units: the prototype's in its variant A. */
export const GLYPH = 7;

/** The words the canvas draws, from the catalog: the frame never formats anything itself. */
export type CanvasText = {
  day: (t: number) => string;
  timecode: (t: number) => string;
  laneName: (lane: Lane) => string;
  laneStatus: (lane: Lane) => string;
};

export type Tape = { scale: TimeScale; x0: number; x1: number };

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

/** The tick of each kind on the tape: the turning points in their colour, the steps in the node's. */
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

export function drawFrame(ctx: CanvasRenderingContext2D, f: Frame): void {
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

  const x0 = cam.cx - W / 2 / z - 60;
  const x1 = cam.cx + W / 2 / z + 60;
  const y0 = cam.cy - H / 2 / z - 60;
  const y1 = cam.cy + H / 2 / z + 60;
  ctx.setTransform(dpr * z, 0, 0, dpr * z, dpr * (W / 2 - cam.cx * z), dpr * (H / 2 - cam.cy * z));

  // Lanes: the envelope of their rings, then the line, dashed once abandoned, dotted once rejoined. Only the part
  // in view is a path: a dash is laid along the whole of one, in view or not.
  for (const e of f.envelopes) {
    const lane = model.lanes[e.lane] as Lane;
    if (e.x1 < x0 || e.x0 > x1) continue;
    const dim = laneAlpha(f, e.lane);
    const all = e.points;
    const from = Math.max(0, lowerBoundBy(all, x0) - 1);
    const to = Math.min(all.length, lowerBoundBy(all, x1) + 1);
    const pts = all.slice(from, to);
    const a = pts[0] as { x: number; w: number };
    const b = pts[pts.length - 1] as { x: number; w: number };
    ctx.beginPath();
    ctx.moveTo(a.x, e.y - a.w);
    for (let i = 1; i < pts.length; i++) {
      const p0 = pts[i - 1] as { x: number; w: number };
      const p1 = pts[i] as { x: number; w: number };
      const mx = (p0.x + p1.x) / 2;
      ctx.bezierCurveTo(mx, e.y - p0.w, mx, e.y - p1.w, p1.x, e.y - p1.w);
    }
    ctx.lineTo(b.x, e.y + b.w);
    for (let i = pts.length - 1; i > 0; i--) {
      const p0 = pts[i] as { x: number; w: number };
      const p1 = pts[i - 1] as { x: number; w: number };
      const mx = (p0.x + p1.x) / 2;
      ctx.bezierCurveTo(mx, e.y + p0.w, mx, e.y + p1.w, p1.x, e.y + p1.w);
    }
    ctx.closePath();
    ctx.globalAlpha = dim;
    ctx.fillStyle = p.lane;
    ctx.fill();
    ctx.globalAlpha = 0.6 * dim;
    ctx.strokeStyle = p.laneEdge;
    ctx.lineWidth = 0.8 / z;
    ctx.stroke();
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

  // Edges, parent to child: within a lane in the lane's mark, across lanes dashed and faint; an edge goes with a
  // node filtered out.
  const edges = scene.edges;
  const fade = f.fade;
  for (const across of [false, true]) {
    ctx.strokeStyle = across ? p.faint : p.muted;
    ctx.lineWidth = 1.3 / z;
    ctx.setLineDash(across ? [4 / z, 3 / z] : []);
    for (const gone of fade ? [false, true] : [false]) {
      ctx.globalAlpha = (across ? 1 : 0.75) * (gone ? FADE[2] : 1);
      ctx.beginPath();
      for (let i = 0; i < edges.n; i++) {
        const ax = edges.x1[i] as number;
        const ay = edges.y1[i] as number;
        const bx = edges.x2[i] as number;
        const by = edges.y2[i] as number;
        if ((ay !== by) !== across) continue;
        if ((ax < x0 && bx < x0) || (ax > x1 && bx > x1)) continue;
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

  // The screen layer.
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  const labels = f.labels;
  if (labels.n) {
    ctx.font = `12px ${p.font}`;
    ctx.textBaseline = "middle";
    for (let i = 0; i < labels.n; i++) {
      const id = labels.id[i] as number;
      const text = f.labelText(id);
      const w = ctx.measureText(text).width;
      // A label that would leave the stage goes to the node's left.
      const sx = (labels.sx[i] as number) + w > W - 4 ? (labels.sx[i] as number) - 24 - w : (labels.sx[i] as number);
      const sy = labels.sy[i] as number;
      plate(ctx, p.surface, sx - 3, sy - 8, w + 6, 16);
      ctx.fillStyle = p.muted;
      ctx.fillText(text, sx, sy);
    }
    ctx.textBaseline = "alphabetic";
  }

  // The lanes' names, held at the left edge while the stage moves under them.
  const nameX = Math.max(8, toScreenX(cam, view, 12));
  for (const lane of model.lanes) {
    const sy = toScreenY(cam, view, layout.laneY[lane.index] as number);
    if (sy < AXIS || sy > H + 12) continue;
    const name = f.text.laneName(lane);
    const status = f.text.laneStatus(lane);
    const dim = laneAlpha(f, lane.index);
    ctx.font = `500 12px ${p.font}`;
    const wName = ctx.measureText(name).width;
    ctx.font = `11px ${p.font}`;
    const wStatus = ctx.measureText(status).width;
    plate(ctx, p.surface, nameX - 4, sy - 17, Math.max(wName, wStatus) + 8, 32);
    ctx.globalAlpha = dim;
    ctx.font = `500 12px ${p.font}`;
    ctx.fillStyle = p.text;
    ctx.fillText(name, nameX, sy - 4);
    ctx.font = `11px ${p.font}`;
    ctx.fillStyle = p.faint;
    ctx.fillText(status, nameX, sy + 11);
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

  // The selected node, and the one under the pointer.
  for (const [index, colour, width] of [[f.hover, p.muted, 1], [f.selected, p.accent, 2]] as const) {
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

/** The tape under the stage (`disegnaNastro`): the whole story on the time scale, the days, the joints (✂) where
 * nothing happened for more than twelve hours, a tick per milestone, the part of the story in view, the playhead. */
function drawTape(ctx: CanvasRenderingContext2D, f: Frame, left: number, right: number): void {
  const { palette: p, tape, model, layout } = f;
  const top = f.view.h;
  const W = f.width;
  const scale = tape.scale;
  ctx.fillStyle = p.ground;
  ctx.fillRect(0, top, W, TAPE);
  ctx.fillStyle = p.border;
  ctx.fillRect(0, top, W, 1);
  // Perforations, above and below: it is film.
  ctx.fillStyle = p.raised;
  ctx.beginPath();
  for (let x = 4; x < W; x += 14) {
    ctx.roundRect(x, top + 3, 7, 4, 1.5);
    ctx.roundRect(x, top + TAPE - 7, 7, 4, 1.5);
  }
  ctx.fill();
  // The part of the story the stage shows.
  const a = scale.x(layout.tAt(left));
  const b = scale.x(layout.tAt(right));
  ctx.globalAlpha = 0.7;
  ctx.fillStyle = p.raised;
  ctx.fillRect(a, top + 9, Math.max(2, b - a), TAPE - 18);
  ctx.globalAlpha = 1;
  // Days.
  ctx.strokeStyle = p.rule;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.font = `10px ${p.font}`;
  ctx.fillStyle = p.faint;
  let lastX = -1e9;
  for (const i of f.days) {
    const t = (model.nodes[i] as Model["nodes"][number]).t;
    const x = Math.round(scale.x(t) - 3) + 0.5;
    ctx.moveTo(x, top + 10);
    ctx.lineTo(x, top + TAPE - 10);
    if (x - lastX >= 44) {
      lastX = x;
      ctx.fillText(f.text.day(t), x + 2, top + 19);
    }
  }
  ctx.stroke();
  ctx.textAlign = "center";
  for (const joint of scale.joints) ctx.fillText("✂", joint.x, top + 37);
  ctx.textAlign = "start";
  // A tick per milestone, in its kind's colour: one path per kind.
  for (const kind of KINDS) {
    ctx.strokeStyle = p[TICK[kind]];
    ctx.lineWidth = kind === "milestone" ? 1 : 2;
    ctx.beginPath();
    for (const node of model.nodes) {
      if (node.milestone.kind !== kind) continue;
      const x = scale.x(node.t);
      ctx.moveTo(x, top + 23);
      ctx.lineTo(x, top + TAPE - 9);
    }
    ctx.stroke();
  }
  const x = scale.x(f.t);
  ctx.fillStyle = p.accent;
  ctx.fillRect(x - 0.75, top + 7, 1.5, TAPE - 14);
  ctx.beginPath();
  ctx.moveTo(x - 5, top + 7);
  ctx.lineTo(x + 5, top + 7);
  ctx.lineTo(x, top + 14);
  ctx.closePath();
  ctx.fill();
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
};

/** `disegnaStriscia`: the axis of the whole story with its days, joints and a tick per milestone, then the window,
 * the rest under a veil, with a handle at each end. Every milestone is on it, whatever the filters keep. */
export function drawStrip(ctx: CanvasRenderingContext2D, s: StripFrame): void {
  const { palette: p, scale, model } = s;
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
  let lastX = -1e9;
  for (const i of s.days) {
    const t = (model.nodes[i] as Model["nodes"][number]).t;
    const x = Math.round(scale.x(t) - 2) + 0.5;
    ctx.moveTo(x, 12);
    ctx.lineTo(x, 29);
    if (x - lastX < 40) continue;
    lastX = x;
    const end = x > s.x1 - 30;
    ctx.textAlign = end ? "end" : "start";
    ctx.fillText(s.text.day(t), end ? x - 2 : x + 2, 9);
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
