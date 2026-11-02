import { KIND_CSS, KIND_COLORS } from '../dataset.js';
import { VIEW, LABEL_FONT, MONO_FONT, labelText } from '../scenario.js';
import { nodeText } from '../dataset.js';

export const BG = '#14110f';
export const BAND_CSS = 'rgba(232,163,61,0.16)';
export const EDGE_CSS = 'rgba(160,150,130,0.55)';
export const VEIL_CSS = 'rgba(20,17,15,0.72)';
export const INK = '#d9d2c5';
export const ACCENT = '#e8a33d';
export { KIND_CSS, KIND_COLORS, VIEW, LABEL_FONT, MONO_FONT, labelText, nodeText };

export function busyWait(ms) { const end = performance.now() + ms; while (performance.now() < end) { /* spin */ } }

// world -> screen
export const toScreenX = (cam, wx) => (wx - cam.cx) * cam.z + VIEW.w / 2;
export const toScreenY = (cam, wy) => (wy - cam.cy) * cam.z + VIEW.h / 2;

// node indices per kind, ascending (nodes are sorted by x, so every bucket is too)
const bucketCache = new WeakMap();
export function bucketByKind(scene) {
  if (bucketCache.has(scene)) return bucketCache.get(scene);
  const lists = Array.from({ length: 8 }, () => []);
  for (let i = 0; i < scene.N; i++) lists[scene.nodes.kind[i]].push(i);
  const out = lists.map((l) => Uint32Array.from(l));
  bucketCache.set(scene, out);
  return out;
}

export function tooltipGeometry(text, cam, scene, hoverId, measure) {
  const sx = toScreenX(cam, scene.nodes.x[hoverId]), sy = toScreenY(cam, scene.nodes.y[hoverId]);
  const w = Math.ceil(measure(text)) + 16, h = 22;
  const x = Math.min(VIEW.w - w - 4, Math.max(4, sx + 14)), y = Math.min(VIEW.h - h - 4, Math.max(4, sy - h - 10));
  return { sx, sy, x, y, w, h };
}

// the visually hidden list a screen reader would get for the canvas-based renderers (see src/a11y.js)
export function bytesToBase64Png(dataUrl) { return dataUrl.replace(/^data:image\/png;base64,/, ''); }

// The screen-space layer shared by Canvas 2D and the WebGL2 overlay: veil, playhead, timecode, labels, hover ring and tooltip.
// Expects the context transform to be the device-pixel-ratio scale only.
export function drawScreenLayer(ctx, cam, labels, scene) {
  const W = VIEW.w, H = VIEW.h, TAU = Math.PI * 2;
    if (cam.playheadX != null) {
    const sx = toScreenX(cam, cam.playheadX);
    if (sx < W) { ctx.fillStyle = VEIL_CSS; ctx.fillRect(Math.max(0, sx), 0, W - Math.max(0, sx), H); }
    ctx.fillStyle = ACCENT; ctx.fillRect(sx - 1, 0, 2, H);
    ctx.font = MONO_FONT; ctx.fillStyle = ACCENT; ctx.fillText(cam.timecode, Math.min(W - 120, Math.max(4, sx + 6)), 16);
  }
  if (labels.n) {
    ctx.font = LABEL_FONT; ctx.fillStyle = INK; ctx.textBaseline = 'middle';
    for (let i = 0; i < labels.n; i++) ctx.fillText(labelText(scene, labels.id[i]), labels.sx[i], labels.sy[i]);
    ctx.textBaseline = 'alphabetic';
  }
  if (cam.hoverId >= 0) {
    const text = nodeText(scene, cam.hoverId);
    ctx.font = LABEL_FONT;
    const g = tooltipGeometry(text, cam, scene, cam.hoverId, (s) => ctx.measureText(s).width);
    ctx.strokeStyle = ACCENT; ctx.lineWidth = 2; ctx.beginPath(); ctx.arc(g.sx, g.sy, 11, 0, TAU); ctx.stroke();
    ctx.fillStyle = 'rgba(30,26,22,0.96)'; ctx.fillRect(g.x, g.y, g.w, g.h);
    ctx.strokeStyle = ACCENT; ctx.lineWidth = 1; ctx.strokeRect(g.x + .5, g.y + .5, g.w - 1, g.h - 1);
    ctx.fillStyle = INK; ctx.textBaseline = 'middle'; ctx.fillText(text, g.x + 8, g.y + g.h / 2); ctx.textBaseline = 'alphabetic';
  }
}
