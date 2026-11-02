import { BG, BAND_CSS, EDGE_CSS, KIND_CSS, VIEW, busyWait, bucketByKind, bytesToBase64Png, drawScreenLayer } from './common.js';
import { lowerBound } from '../dataset.js';

const TAU = Math.PI * 2;

// Canvas 2D, written directly, with viewport culling by binary search on the x-sorted nodes.
// Variants for the sanity check: `busy<ms>` spins that many milliseconds per frame, `shadow` turns on shadowBlur.
export class Canvas2DRenderer {
  constructor(variant = '') { this.variant = variant; this.id = 'canvas2d' + (variant ? '+' + variant : ''); }

  async init(host, scene, dpr) {
    this.scene = scene; this.dpr = dpr;
    const c = document.createElement('canvas');
    c.width = Math.round(VIEW.w * dpr); c.height = Math.round(VIEW.h * dpr);
    c.style.width = VIEW.w + 'px'; c.style.height = VIEW.h + 'px';
    host.appendChild(c);
    this.canvas = c; this.ctx = c.getContext('2d', { alpha: false });
    this.buckets = bucketByKind(scene);
    this.busy = /^busy(\d+)/.exec(this.variant)?.[1] | 0;
    this.shadow = this.variant === 'shadow';
    return { backend: 'canvas2d', notes: 'CanvasRenderingContext2D' };
  }

  frame(cam, labels) {
    const { ctx, scene, dpr } = this;
    const { z } = cam, W = VIEW.w, H = VIEW.h;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = BG; ctx.fillRect(0, 0, W, H);

    const x0 = cam.cx - W / 2 / z - 60, x1 = cam.cx + W / 2 / z + 60, y0 = cam.cy - H / 2 / z - 60, y1 = cam.cy + H / 2 / z + 60;
    ctx.setTransform(dpr * z, 0, 0, dpr * z, dpr * (W / 2 - cam.cx * z), dpr * (H / 2 - cam.cy * z));

    // bands
    const b = scene.bands;
    ctx.fillStyle = BAND_CSS; ctx.beginPath();
    for (let i = 0; i < b.n; i++) { if (b.x[i] > x1 || b.x[i] + b.w[i] < x0 || b.y[i] > y1 || b.y[i] + b.h[i] < y0) continue; ctx.rect(b.x[i], b.y[i], b.w[i], b.h[i]); }
    ctx.fill();

    // edges
    const e = scene.edges;
    ctx.strokeStyle = EDGE_CSS; ctx.lineWidth = 1 / z; ctx.beginPath();
    for (let i = 0; i < e.n; i++) {
      if ((e.x1[i] < x0 && e.x2[i] < x0) || (e.x1[i] > x1 && e.x2[i] > x1)) continue;
      ctx.moveTo(e.x1[i], e.y1[i]); ctx.lineTo(e.x2[i], e.y2[i]);
    }
    ctx.stroke();

    // nodes, one path per kind
    const { x, y, r, shape } = scene.nodes;
    const lo = lowerBound(x, x0), hi = lowerBound(x, x1);
    if (this.shadow) { ctx.shadowBlur = 6 * dpr; ctx.shadowColor = 'rgba(232,163,61,0.8)'; }
    for (let k = 0; k < 8; k++) {
      const list = this.buckets[k]; if (!list.length) continue;
      ctx.fillStyle = KIND_CSS[k]; ctx.beginPath();
      let s = 0, t = list.length;
      while (s < t) { const m = (s + t) >> 1; if (list[m] < lo) s = m + 1; else t = m; }
      for (let q = s; q < list.length; q++) {
        const i = list[q]; if (i >= hi) break;
        if (y[i] < y0 || y[i] > y1) continue;
        const px = x[i], py = y[i], rr = r[i];
        if (shape[i] === 0) { ctx.moveTo(px + rr, py); ctx.arc(px, py, rr, 0, TAU); }
        else if (shape[i] === 1) { ctx.moveTo(px, py - rr * 1.3); ctx.lineTo(px + rr * 1.3, py); ctx.lineTo(px, py + rr * 1.3); ctx.lineTo(px - rr * 1.3, py); ctx.closePath(); }
        else ctx.rect(px - rr, py - rr, rr * 2, rr * 2);
      }
      ctx.fill();
    }
    ctx.shadowBlur = 0;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    drawScreenLayer(ctx, cam, labels, scene);
    if (this.busy) busyWait(this.busy);
  }

  async snapshot() { return bytesToBase64Png(this.canvas.toDataURL('image/png')); }
  destroy() { this.canvas.remove(); this.ctx = null; }
}
