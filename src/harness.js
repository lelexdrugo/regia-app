import { generate, dimensions } from './dataset.js';
import { PHASES, VIEW, cameraAt, pickLabels, newLabelBuf } from './scenario.js';
import { measureShadowList } from './a11y.js';
import { Canvas2DRenderer } from './renderers/canvas2d.js';
import { SvgRenderer } from './renderers/svg.js';
import { WebGL2Renderer } from './renderers/webgl2.js';
import { PixiRenderer } from './renderers/pixi.js';

export const RENDERERS = {
  svg: () => new SvgRenderer(),
  canvas2d: () => new Canvas2DRenderer(),
  'webgl2-raw': () => new WebGL2Renderer(),
  'pixi-webgl': () => new PixiRenderer('webgl'),
  'pixi-webgpu': () => new PixiRenderer('webgpu'),
  // sanity-check variants, deliberately slowed (see scripts/analyze.mjs)
  'canvas2d+busy16': () => new Canvas2DRenderer('busy16'),
  'canvas2d+shadow': () => new Canvas2DRenderer('shadow'),
};
export const DEFAULT_RENDERERS = ['svg', 'canvas2d', 'webgl2-raw', 'pixi-webgl', 'pixi-webgpu'];
export const SANITY_RENDERERS = ['canvas2d', 'canvas2d+busy16', 'canvas2d+shadow'];

const raf = () => new Promise((r) => requestAnimationFrame(r));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const q = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
const stats = (arr) => { const s = [...arr].sort((a, b) => a - b); return { median: q(s, 0.5), p95: q(s, 0.95), p99: q(s, 0.99), max: s[s.length - 1], mean: s.reduce((a, b) => a + b, 0) / s.length }; };

// refresh interval of the display, measured: median rAF delta with no work
async function calibrateVsync(ms = 1000) {
  const dts = []; let prev = await raf(); const end = performance.now() + ms;
  while (performance.now() < end) { const t = await raf(); dts.push(t - prev); prev = t; }
  return stats(dts).median;
}

function runPhase(r, scene, phase, durMs, repeat = 1) {
  return new Promise((resolve, reject) => {
    const labels = newLabelBuf(); const dts = [], js = [], lab = []; let t0 = null, prev = null, n = 0, lastTs = performance.now();
    const watchdog = setInterval(() => { if (performance.now() - lastTs > 5000) { clearInterval(watchdog); reject(new Error('requestAnimationFrame stalled (window hidden or occluded?)')); } }, 1000);
    const step = (ts) => {
      lastTs = performance.now();
      if (t0 === null) t0 = ts;
      const ms = ts - t0;
      if (ms >= durMs) { clearInterval(watchdog); resolve({ dts, js, lab }); return; }
      const a = performance.now();
      const cam = cameraAt(scene, phase, ms, durMs);
      pickLabels(scene, cam, labels);
      const b = performance.now();
      for (let k = 0; k < repeat; k++) r.frame(cam, labels);
      const c = performance.now();
      if (prev !== null && ++n > 2) { dts.push(ts - prev); js.push(c - a); lab.push(b - a); }
      prev = ts;
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
}

async function renderOnce(r, scene, phase, u, dur) {
  const labels = newLabelBuf(); const cam = cameraAt(scene, phase, u * dur, dur);
  pickLabels(scene, cam, labels); r.frame(cam, labels); await raf(); r.frame(cam, labels);
  return r.snapshot();
}

function probeGpu() {
  const c = document.createElement('canvas'); const gl = c.getContext('webgl2');
  if (!gl) return { webgl2: false };
  const d = gl.getExtension('WEBGL_debug_renderer_info');
  const out = { webgl2: true, renderer: d ? gl.getParameter(d.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER), vendor: d ? gl.getParameter(d.UNMASKED_VENDOR_WEBGL) : gl.getParameter(gl.VENDOR), version: gl.getParameter(gl.VERSION) };
  gl.getExtension('WEBGL_lose_context')?.loseContext();
  return out;
}
async function probeWebGpu() {
  if (!navigator.gpu) return { available: false };
  try {
    const a = await navigator.gpu.requestAdapter();
    if (!a) return { available: false, reason: 'no adapter' };
    const i = a.info || (await a.requestAdapterInfo?.()) || {};
    return { available: true, fallbackAdapter: !!a.isFallbackAdapter, vendor: i.vendor, architecture: i.architecture, device: i.device, description: i.description };
  } catch (e) { return { available: false, reason: String(e) }; }
}

export async function hostInfo(base) {
  return {
    ...base,
    userAgent: navigator.userAgent, dpr: window.devicePixelRatio, screen: [screen.width, screen.height], inner: [innerWidth, innerHeight],
    cores: navigator.hardwareConcurrency, deviceMemoryGb: navigator.deviceMemory ?? null,
    webgl: probeGpu(), webgpu: await probeWebGpu(),
    fontsProbe: getComputedStyle(document.body).fontFamily,
  };
}

// cfg: { renderers, scales, reps, phase_ms, warmup_ms, shots, onStatus }
export async function runBenchmark(cfg, onStatus = () => {}) {
  const stage = document.getElementById('stage');
  const rows = [], failures = [], shots = [], a11y = [], infos = [];
  const vsync = await calibrateVsync();
  const dpr = window.devicePixelRatio || 1;
  const scenes = new Map();
  const sceneFor = (s) => { if (!scenes.has(s)) { const t = performance.now(); const sc = generate(s); sc.buildMs = performance.now() - t; scenes.set(s, sc); } return scenes.get(s); };
  const dims = Object.fromEntries(cfg.scales.map((s) => { const d = dimensions(s); return [s, d]; }));

  for (let rep = 1; rep <= cfg.reps; rep++) {
    for (const scale of cfg.scales) {
      const scene = sceneFor(scale);
      for (const id of cfg.renderers) {
        const tag = `${id} x${scale} rep ${rep}/${cfg.reps}`;
        onStatus(tag);
        let r; stage.replaceChildren();
        try {
          r = RENDERERS[id]();
          const t0 = performance.now();
          const info = await r.init(stage, scene, dpr);
          const initMs = performance.now() - t0;
          await raf(); await raf();
          // warm-up: the pan phase, not recorded
          await runPhase(r, scene, 'pan', cfg.warmup_ms);
          for (const phase of PHASES) {
            const { dts, js, lab } = await runPhase(r, scene, phase, cfg.phase_ms, cfg.repeat);
            const d = stats(dts), j = stats(js), l = stats(lab);
            rows.push({
              renderer: id, scale, rep, phase, repeat: cfg.repeat, nodes: scene.N, lanes: scene.lanes, frames: dts.length,
              dt_median: d.median, dt_p95: d.p95, dt_p99: d.p99, dt_max: d.max,
              js_median: j.median, js_p95: j.p95, labels_median: l.median,
              dropped_pct: (100 * dts.filter((x) => x > 1.5 * vsync).length) / dts.length,
              init_ms: initMs, backend: info.backend, gpu: info.gpu ?? null,
              samples: { dt: dts.map((v) => Math.round(v * 10) / 10), js: js.map((v) => Math.round(v * 100) / 100) },
            });
          }
          if (rep === 1) {
            if (cfg.shots && (scale === 1 || scale === 100)) {
              shots.push({ name: `${id}-x${scale}-detail`, b64: await renderOnce(r, scene, 'hover', 0.4, cfg.phase_ms) });
              if (scale === 100) shots.push({ name: `${id}-x${scale}-overview`, b64: await renderOnce(r, scene, 'zoom', 0, cfg.phase_ms) });
            }
            if (id === 'canvas2d') {
              // overview camera is the worst case for the shadow list (all milestones that fit), detail camera the common one
              const detail = cameraAt(scene, 'hover', 0, 1000);
              a11y.push({ scale, camera: 'detail', ...measureShadowList(stage, scene, detail) });
              a11y.push({ scale, camera: 'label-zoom-wide', ...measureShadowList(stage, scene, { ...detail, z: 0.5 }) });
            }
            if (id === 'svg') a11y.push({ scale, camera: 'native', items: scene.M, ms: 0, elements: info.notes });
          }
          if (rep === 1) infos.push({ renderer: id, scale, ...info });
        } catch (e) {
          failures.push({ renderer: id, scale, rep, error: String(e?.message || e) });
          onStatus(`${tag} FAILED: ${e?.message || e}`);
        } finally {
          try { r?.destroy(); } catch { /* ignore */ }
          stage.replaceChildren();
          await sleep(150);
        }
      }
    }
  }
  return { vsync_ms: vsync, dpr, view: VIEW, dims, scene_build_ms: Object.fromEntries([...scenes].map(([s, sc]) => [s, sc.buildMs])), rows, failures, a11y, infos, shots };
}
