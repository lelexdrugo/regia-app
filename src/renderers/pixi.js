// Tauri's default-deny CSP forbids eval: this module swaps Pixi's generated shader-sync code for a static version
import 'pixi.js/unsafe-eval';
import { Application, Graphics, BitmapText, Container, Rectangle } from 'pixi.js';
import { BG, KIND_COLORS, VIEW, labelText, nodeText, toScreenX, tooltipGeometry } from './common.js';
import { MAX_LABELS } from '../scenario.js';

const css = (hex) => parseInt(hex.slice(1), 16);
const ACCENT = 0xe8a33d, INK = 0xd9d2c5;
const FONT = { fontFamily: 'system-ui, "Segoe UI", sans-serif', fontSize: 12, fill: INK };
const MONO = { fontFamily: 'ui-monospace, Consolas, monospace', fontSize: 12, fill: ACCENT };

// PixiJS v8 with the backend forced to WebGL or WebGPU. Retained scene: three Graphics for the world (bands, edges, nodes),
// moved by one container transform; labels are a sticky pool of BitmapText.
export class PixiRenderer {
  constructor(backend) { this.backend = backend; this.id = 'pixi-' + backend; }

  async init(host, scene, dpr) {
    this.scene = scene; this.dpr = dpr;
    const app = new Application();
    await app.init({
      width: VIEW.w, height: VIEW.h, resolution: dpr, autoDensity: true, antialias: true, background: css(BG),
      preference: [this.backend], powerPreference: 'high-performance', autoStart: false, sharedTicker: false,
    });
    const actual = app.renderer.name || (app.renderer.type === 2 ? 'webgpu' : 'webgl');
    if (!String(actual).includes(this.backend)) { app.destroy(true, { children: true }); throw new Error(`asked for ${this.backend}, Pixi gave ${actual}`); }
    app.ticker.stop();
    host.appendChild(app.canvas); this.app = app;

    const world = new Container(); this.world = world; app.stage.addChild(world);
    const b = scene.bands, bands = new Graphics();
    for (let i = 0; i < b.n; i++) bands.rect(b.x[i], b.y[i], b.w[i], b.h[i]);
    bands.fill({ color: ACCENT, alpha: 0.16 }); world.addChild(bands);

    const e = scene.edges, edges = new Graphics();
    for (let i = 0; i < e.n; i++) edges.moveTo(e.x1[i], e.y1[i]).lineTo(e.x2[i], e.y2[i]);
    edges.stroke({ color: 0xa09682, alpha: 0.55, width: 1, pixelLine: true }); world.addChild(edges);

    const { x, y, r, shape, kind } = scene.nodes;
    for (let k = 0; k < 8; k++) {
      const g = new Graphics(); let any = false;
      for (let i = 0; i < scene.N; i++) {
        if (kind[i] !== k) continue; any = true;
        if (shape[i] === 0) g.circle(x[i], y[i], r[i]);
        else if (shape[i] === 1) { const d = r[i] * 1.3; g.poly([x[i], y[i] - d, x[i] + d, y[i], x[i], y[i] + d, x[i] - d, y[i]], true); }
        else g.rect(x[i] - r[i], y[i] - r[i], r[i] * 2, r[i] * 2);
      }
      if (any) { g.fill(KIND_COLORS[k]); world.addChild(g); }
    }

    const ov = new Container(); app.stage.addChild(ov); this.ov = ov;
    this.veil = new Graphics().rect(0, 0, 1, VIEW.h).fill({ color: css(BG), alpha: 0.72 }); this.veil.visible = false; ov.addChild(this.veil);
    this.head = new Graphics().rect(0, 0, 2, VIEW.h).fill(ACCENT); this.head.visible = false; ov.addChild(this.head);
    this.tc = new BitmapText({ text: '', style: MONO }); this.tc.y = 4; this.tc.visible = false; ov.addChild(this.tc);
    this.pool = Array.from({ length: MAX_LABELS }, () => { const t = new BitmapText({ text: ' ', style: FONT }); t.visible = false; ov.addChild(t); return t; });
    this.slotOf = new Map(); this.slotId = new Int32Array(MAX_LABELS).fill(-1);
    this.ring = new Graphics().circle(0, 0, 11).stroke({ color: ACCENT, width: 2 }); this.ring.visible = false; ov.addChild(this.ring);
    this.tipBox = new Graphics(); this.tipBox.visible = false; ov.addChild(this.tipBox);
    this.tipText = new BitmapText({ text: ' ', style: FONT }); this.tipText.visible = false; ov.addChild(this.tipText);
    this.tipW = -1;

    const info = { backend: this.backend, pixi: app.renderer.name };
    const gl = app.renderer.gl;
    if (gl) { const dbg = gl.getExtension('WEBGL_debug_renderer_info'); info.gpu = dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER); info.version = gl.getParameter(gl.VERSION); }
    else if (navigator.gpu) {
      try { const a = await navigator.gpu.requestAdapter(); const i = a?.info || (await a?.requestAdapterInfo?.()); info.gpu = [i?.vendor, i?.architecture, i?.device, i?.description].filter(Boolean).join(' / '); info.fallbackAdapter = !!a?.isFallbackAdapter; } catch { /* informational only */ }
    }
    return info;
  }

  frame(cam, labels) {
    const z = cam.z;
    this.world.position.set(VIEW.w / 2 - cam.cx * z, VIEW.h / 2 - cam.cy * z); this.world.scale.set(z);
    if (cam.playheadX != null) {
      const sx = toScreenX(cam, cam.playheadX), vx = Math.max(0, sx);
      this.veil.visible = sx < VIEW.w; this.veil.x = vx; this.veil.scale.x = Math.max(0, VIEW.w - vx);
      this.head.visible = true; this.head.x = sx - 1;
      this.tc.visible = true; this.tc.x = Math.min(VIEW.w - 120, Math.max(4, sx + 6)); if (this.tc.text !== cam.timecode) this.tc.text = cam.timecode;
    } else { this.veil.visible = false; this.head.visible = false; this.tc.visible = false; }

    // sticky label slots: a label keeps its BitmapText while it stays visible
    const want = new Set(); for (let i = 0; i < labels.n; i++) want.add(labels.id[i]);
    for (let s = 0; s < MAX_LABELS; s++) { const id = this.slotId[s]; if (id >= 0 && !want.has(id)) { this.slotOf.delete(id); this.slotId[s] = -1; this.pool[s].visible = false; } }
    let free = 0;
    for (let i = 0; i < labels.n; i++) {
      const id = labels.id[i]; let s = this.slotOf.get(id);
      if (s === undefined) {
        while (this.slotId[free] >= 0) free++;
        s = free; this.slotId[s] = id; this.slotOf.set(id, s);
        this.pool[s].text = labelText(this.scene, id);
      }
      const t = this.pool[s]; t.visible = true; t.position.set(labels.sx[i], labels.sy[i] - 7);
    }
    if (cam.hoverId >= 0) {
      const text = nodeText(this.scene, cam.hoverId);
      const g = tooltipGeometry(text, cam, this.scene, cam.hoverId, (s) => s.length * 6.3);
      this.ring.visible = true; this.ring.position.set(g.sx, g.sy);
      if (this.tipText.text !== text) { this.tipText.text = text; }
      this.tipBox.visible = true; this.tipBox.position.set(g.x, g.y);
      if (this.tipW !== g.w) { this.tipW = g.w; this.tipBox.clear().rect(0, 0, g.w, g.h).fill({ color: 0x1e1a16 }).stroke({ color: ACCENT, width: 1 }); }
      this.tipText.visible = true; this.tipText.position.set(g.x + 8, g.y + 4);
    } else { this.ring.visible = false; this.tipBox.visible = false; this.tipText.visible = false; }
    this.app.renderer.render(this.app.stage);
  }

  async snapshot() {
    const url = await this.app.renderer.extract.base64({ target: this.app.stage, frame: new Rectangle(0, 0, VIEW.w, VIEW.h), resolution: this.dpr });
    return url.replace(/^data:image\/png;base64,/, '');
  }
  destroy() { this.app.destroy(true, { children: true }); }
}
