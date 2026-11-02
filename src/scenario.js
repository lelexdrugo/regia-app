// The scripted scenario: identical for every renderer. Pure functions of (scene, phase, elapsed ms).
import { hitTest, lowerBound, DAY } from './dataset.js';

export const PHASES = ['pan', 'zoom', 'playhead', 'hover'];
export const VIEW = { w: 1280, h: 720 };
export const LABEL_ZOOM = 0.5;      // labels appear from this zoom
export const MAX_LABELS = 120;      // labels drawn per frame at most
export const LABEL_FONT = '12px system-ui, "Segoe UI", sans-serif';
export const MONO_FONT = '12px ui-monospace, Consolas, monospace';
const Z_PAN = 0.6, PAN_SPEED = 3000, Z_HOVER = 1.4, Z_MAX = 2.4;

const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const tri = (u) => 1 - Math.abs(2 * (u % 1) - 1);      // 0..1..0
const fitZoom = (scene) => Math.min(VIEW.w / (scene.extent.x1 - scene.extent.x0 + 2 * DAY), VIEW.h / (scene.extent.y1 + 40));

export function cameraAt(scene, phase, ms, durMs) {
  const e = scene.extent, midX = (e.x0 + e.x1) / 2, midY = (e.y0 + e.y1) / 2;
  const u = clamp(ms / durMs, 0, 1);
  switch (phase) {
    case 'pan': {
      // constant speed ping-pong along x at a working zoom, with a slow vertical drift
      const halfW = VIEW.w / 2 / Z_PAN, span = Math.max(1, e.x1 - e.x0 - 2 * halfW);
      const dist = (PAN_SPEED * ms) / 1000;
      const p = span > 1 ? tri(dist / (2 * span)) : 0.5;
      const cx = e.x1 - e.x0 <= 2 * halfW ? midX : e.x0 + halfW + p * span;
      return { cx, cy: midY + Math.sin(u * Math.PI * 2) * Math.min(60, (e.y1 - e.y0) / 4), z: Z_PAN, playheadX: null, hoverId: -1, timecode: null };
    }
    case 'zoom': {
      // from "everything in view" to the label zoom and back, log-spaced, anchored on the centre of the data
      const zf = fitZoom(scene);
      const z = zf * Math.pow(Z_MAX / zf, tri(u));
      return { cx: midX, cy: midY, z, playheadX: null, hoverId: -1, timecode: null };
    }
    case 'playhead': {
      // the camera follows the playhead as it scrubs the whole timeline and back
      const span = e.x1 - e.x0;
      const px = e.x0 + tri(u) * span;
      return { cx: clamp(px, e.x0 + 300, Math.max(e.x0 + 300, e.x1 - 300)), cy: midY, z: 0.8, playheadX: px, hoverId: -1, timecode: timecodeAt(px) };
    }
    case 'hover': {
      // fixed camera in the middle of the data; the pointer visits the labelled nodes in view, in order
      const cam = { cx: midX, cy: midY, z: Z_HOVER, playheadX: null, hoverId: -1, timecode: null };
      const pts = hoverPath(scene, cam);
      if (pts.length) {
        // out and back along the points, three times, so that the pointer crosses every labelled node in view
        const f = tri(u * 3) * (pts.length - 1);
        const k = Math.min(pts.length - 2, Math.floor(f));
        const a = pts[k], b = pts[k + 1], t = f - k;
        cam.pointer = { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
        cam.hoverId = hitTest(scene, (cam.pointer.x - VIEW.w / 2) / cam.z + cam.cx, (cam.pointer.y - VIEW.h / 2) / cam.z + cam.cy, 14 / cam.z);
      }
      return cam;
    }
  }
}

const pathCache = new WeakMap();
function hoverPath(scene, cam) {
  if (pathCache.has(scene)) return pathCache.get(scene);
  const { x, y, labeled } = scene.nodes;
  const pts = [];
  const lo = lowerBound(scene.nodes.x, cam.cx - VIEW.w / 2 / cam.z), hi = lowerBound(scene.nodes.x, cam.cx + VIEW.w / 2 / cam.z);
  for (const i of labeled) {
    if (i < lo || i >= hi) continue;
    const sy = (y[i] - cam.cy) * cam.z + VIEW.h / 2;
    if (sy < 20 || sy > VIEW.h - 20) continue;
    pts.push({ x: (x[i] - cam.cx) * cam.z + VIEW.w / 2, y: sy });
    if (pts.length >= 40) break;
  }
  if (pts.length < 2) { pts.push({ x: 200, y: 300 }, { x: 1000, y: 400 }); }
  pathCache.set(scene, pts);
  return pts;
}

export function timecodeAt(px) {
  const d = px / DAY, day = Math.floor(d), h = Math.floor((d - day) * 24), m = Math.floor((((d - day) * 24) - h) * 60);
  return `D+${String(day).padStart(3, '0')} ${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
}

// Choose the labels to draw this frame. Shared by all renderers, so its cost is common and part of every JS time.
// Greedy, left to right, skipping labels that would overlap one already placed (coarse occupancy grid).
const occ = new Uint8Array(Math.ceil(VIEW.w / 8) * Math.ceil(VIEW.h / 16));
export function pickLabels(scene, cam, out) {
  out.n = 0;
  if (cam.z < LABEL_ZOOM) return out;
  occ.fill(0);
  const { x, y, titles, labeled } = scene.nodes;
  const halfW = VIEW.w / 2 / cam.z + 40, halfH = VIEW.h / 2 / cam.z + 20;
  const lo = lowerBound(x, cam.cx - halfW), hi = lowerBound(x, cam.cx + halfW);
  const cols = Math.ceil(VIEW.w / 8);
  // `labeled` is sorted by node index and nodes are sorted by x: binary search into it
  let a = 0, b = labeled.length;
  while (a < b) { const m = (a + b) >> 1; if (labeled[m] < lo) a = m + 1; else b = m; }
  for (let k = a; k < labeled.length && out.n < MAX_LABELS; k++) {
    const i = labeled[k]; if (i >= hi) break;
    if (Math.abs(y[i] - cam.cy) > halfH) continue;
    const sx = (x[i] - cam.cx) * cam.z + VIEW.w / 2 + 12, sy = (y[i] - cam.cy) * cam.z + VIEW.h / 2;
    const wpx = Math.min(titles[i].length, 36) * 6.4;
    const c0 = Math.max(0, Math.floor(sx / 8)), c1 = Math.min(cols - 1, Math.floor((sx + wpx) / 8)), r = Math.floor((sy - 8) / 16);
    if (r < 0 || r * cols >= occ.length) continue;
    let free = true; for (let c = c0; c <= c1; c++) if (occ[r * cols + c]) { free = false; break; }
    if (!free) continue;
    for (let c = c0; c <= c1; c++) occ[r * cols + c] = 1;
    out.id[out.n] = i; out.sx[out.n] = sx; out.sy[out.n] = sy; out.n++;
  }
  return out;
}
export const newLabelBuf = () => ({ n: 0, id: new Uint32Array(MAX_LABELS), sx: new Float32Array(MAX_LABELS), sy: new Float32Array(MAX_LABELS) });
export const labelText = (scene, i) => { const t = scene.nodes.titles[i]; return t.length > 36 ? t.slice(0, 35) + '…' : t; };
