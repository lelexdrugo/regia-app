import { BG, BAND_CSS, EDGE_CSS, VEIL_CSS, INK, ACCENT, KIND_CSS, VIEW, LABEL_FONT, MONO_FONT, labelText, nodeText, toScreenX, toScreenY, tooltipGeometry } from './common.js';
import { MAX_LABELS } from '../scenario.js';

const NS = 'http://www.w3.org/2000/svg';
const el = (name, attrs = {}, parent) => { const e = document.createElementNS(NS, name); for (const k in attrs) e.setAttribute(k, attrs[k]); if (parent) parent.appendChild(e); return e; };
const f = (v) => Math.round(v * 100) / 100;

// Plain SVG: every band, edge and node is a DOM element, the world is one <g> whose transform changes per frame.
// Labels, veil, playhead, hover and timecode are a screen-space overlay with a fixed pool of <text> elements.
// Milestone nodes carry role="img" and an aria-label; task nodes are decorative (aria-hidden on their group).
export class SvgRenderer {
  constructor() { this.id = 'svg'; }

  async init(host, scene, dpr = 1) {
    this.scene = scene; this.dpr = dpr;
    const svg = el('svg', { width: VIEW.w, height: VIEW.h, viewBox: `0 0 ${VIEW.w} ${VIEW.h}`, role: 'img', 'aria-label': 'moviola timeline' }, host);
    svg.style.background = BG;
    const style = el('style', {}, svg);
    style.textContent = `text{font:${LABEL_FONT};fill:${INK};dominant-baseline:central} .mono{font:${MONO_FONT};fill:${ACCENT}}`;
    this.svg = svg;
    const world = el('g', {}, svg); this.world = world;

    // bands: one path per lane, one sub-path per rectangle
    const bg = el('g', { fill: BAND_CSS }, world);
    const b = scene.bands; const byLane = new Map();
    for (let i = 0; i < b.n; i++) { const l = b.lane[i]; byLane.set(l, (byLane.get(l) || '') + `M${f(b.x[i])} ${f(b.y[i])}h${f(b.w[i])}v${f(b.h[i])}h${-f(b.w[i])}z`); }
    for (const d of byLane.values()) el('path', { d }, bg);

    // edges
    const eg = el('g', { stroke: EDGE_CSS, 'stroke-width': 1, fill: 'none' }, world);
    const e = scene.edges;
    for (let i = 0; i < e.n; i++) el('line', { x1: f(e.x1[i]), y1: f(e.y1[i]), x2: f(e.x2[i]), y2: f(e.y2[i]), 'vector-effect': 'non-scaling-stroke' }, eg);

    // nodes
    const ng = el('g', {}, world); const mg = el('g', {}, world); ng.setAttribute('aria-hidden', 'true');
    const { x, y, r, shape, kind, titles } = scene.nodes;
    for (let i = 0; i < scene.N; i++) {
      const parent = titles[i] ? mg : ng; let n;
      if (shape[i] === 0) n = el('circle', { cx: f(x[i]), cy: f(y[i]), r: r[i] }, parent);
      else if (shape[i] === 1) { const d = r[i] * 1.3; n = el('path', { d: `M${f(x[i])} ${f(y[i] - d)}L${f(x[i] + d)} ${f(y[i])}L${f(x[i])} ${f(y[i] + d)}L${f(x[i] - d)} ${f(y[i])}z` }, parent); }
      else n = el('rect', { x: f(x[i] - r[i]), y: f(y[i] - r[i]), width: r[i] * 2, height: r[i] * 2 }, parent);
      n.setAttribute('fill', KIND_CSS[kind[i]]);
      if (titles[i]) { n.setAttribute('role', 'img'); n.setAttribute('aria-label', titles[i]); }
    }
    this.elementCount = svg.querySelectorAll('*').length;

    // screen-space overlay
    const ov = el('g', {}, svg);
    this.veil = el('rect', { x: 0, y: 0, width: 0, height: VIEW.h, fill: VEIL_CSS, display: 'none' }, ov);
    this.head = el('rect', { x: 0, y: 0, width: 2, height: VIEW.h, fill: ACCENT, display: 'none' }, ov);
    this.tc = el('text', { class: 'mono', display: 'none', y: 12 }, ov);
    this.pool = Array.from({ length: MAX_LABELS }, () => { const t = el('text', { display: 'none' }, ov); t.setAttribute('aria-hidden', 'true'); return t; });
    this.hoverRing = el('circle', { r: 11, fill: 'none', stroke: ACCENT, 'stroke-width': 2, display: 'none' }, ov);
    this.tipBox = el('rect', { height: 22, fill: 'rgb(30,26,22)', stroke: ACCENT, display: 'none' }, ov);
    this.tipText = el('text', { display: 'none' }, ov);
    this.shownLabels = 0; this.lastText = new Array(MAX_LABELS).fill('');
    return { backend: 'svg', notes: `${this.elementCount} DOM elements` };
  }

  frame(cam, labels) {
    const z = cam.z;
    this.world.setAttribute('transform', `matrix(${z} 0 0 ${z} ${VIEW.w / 2 - cam.cx * z} ${VIEW.h / 2 - cam.cy * z})`);
    if (cam.playheadX != null) {
      const sx = toScreenX(cam, cam.playheadX), vx = Math.max(0, sx);
      this.veil.setAttribute('display', sx < VIEW.w ? 'inline' : 'none'); this.veil.setAttribute('x', vx); this.veil.setAttribute('width', Math.max(0, VIEW.w - vx));
      this.head.setAttribute('display', 'inline'); this.head.setAttribute('x', sx - 1);
      this.tc.setAttribute('display', 'inline'); this.tc.setAttribute('x', Math.min(VIEW.w - 120, Math.max(4, sx + 6))); if (this.tc.textContent !== cam.timecode) this.tc.textContent = cam.timecode;
    } else { this.veil.setAttribute('display', 'none'); this.head.setAttribute('display', 'none'); this.tc.setAttribute('display', 'none'); }
    const n = labels.n;
    for (let i = 0; i < n; i++) {
      const t = this.pool[i], s = labelText(this.scene, labels.id[i]);
      if (this.lastText[i] !== s) { t.textContent = s; this.lastText[i] = s; }
      t.setAttribute('x', f(labels.sx[i])); t.setAttribute('y', f(labels.sy[i])); if (i >= this.shownLabels) t.setAttribute('display', 'inline');
    }
    for (let i = n; i < this.shownLabels; i++) this.pool[i].setAttribute('display', 'none');
    this.shownLabels = n;
    if (cam.hoverId >= 0) {
      const text = nodeText(this.scene, cam.hoverId);
      const g = tooltipGeometry(text, cam, this.scene, cam.hoverId, (s) => s.length * 6.3);
      this.hoverRing.setAttribute('display', 'inline'); this.hoverRing.setAttribute('cx', g.sx); this.hoverRing.setAttribute('cy', g.sy);
      this.tipBox.setAttribute('display', 'inline'); this.tipBox.setAttribute('x', g.x); this.tipBox.setAttribute('y', g.y); this.tipBox.setAttribute('width', g.w);
      this.tipText.setAttribute('display', 'inline'); this.tipText.setAttribute('x', g.x + 8); this.tipText.setAttribute('y', g.y + g.h / 2); if (this.tipText.textContent !== text) this.tipText.textContent = text;
    } else { this.hoverRing.setAttribute('display', 'none'); this.tipBox.setAttribute('display', 'none'); this.tipText.setAttribute('display', 'none'); }
  }

  async snapshot() {
    const clone = this.svg.cloneNode(true);
    clone.setAttribute('xmlns', NS);
    const xml = new XMLSerializer().serializeToString(clone);
    const img = new Image();
    await new Promise((res, rej) => { img.onload = res; img.onerror = () => rej(new Error('svg image load failed')); img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(xml); });
    const c = document.createElement('canvas'); c.width = Math.round(VIEW.w * this.dpr); c.height = Math.round(VIEW.h * this.dpr); c.getContext('2d').drawImage(img, 0, 0, c.width, c.height); // vector: re-rasterised at the device resolution, like the window
    return c.toDataURL('image/png').replace(/^data:image\/png;base64,/, '');
  }
  destroy() { this.svg.remove(); }
}
