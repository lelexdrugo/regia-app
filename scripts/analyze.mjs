// results.json -> raw.csv (one row per renderer, scale, repetition, phase) + summary.md (pooled median and p95).
// Usage: node scripts/analyze.mjs <results-dir>...
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const q = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
const MIN_N = 30; // a cell with fewer frames than this is not reported (percentiles of a handful of frames mean nothing)
const pool = (arrs) => { const all = arrs.flat().sort((a, b) => a - b); return all.length >= MIN_N ? { n: all.length, median: q(all, 0.5), p95: q(all, 0.95), p99: q(all, 0.99), max: all[all.length - 1] } : { n: all.length, median: NaN, p95: NaN, p99: NaN, max: all.length ? all[all.length - 1] : NaN }; };
const fmt = (v, d = 1) => (Number.isFinite(v) ? v.toFixed(d) : 'n/a');

for (const dir of process.argv.slice(2)) {
  const doc = JSON.parse(readFileSync(join(dir, 'results.json'), 'utf8'));
  const rows = doc.rows;

  // raw.csv
  const cols = ['renderer', 'scale', 'rep', 'phase', 'repeat', 'nodes', 'lanes', 'frames', 'dt_median', 'dt_p95', 'dt_p99', 'dt_max', 'js_median', 'js_p95', 'labels_median', 'dropped_pct', 'init_ms', 'backend', 'gpu'];
  const csv = [cols.join(',')].concat(rows.map((r) => cols.map((c) => { const v = r[c]; return typeof v === 'number' ? Math.round(v * 1000) / 1000 : JSON.stringify(v ?? ''); }).join(','))).join('\n');
  writeFileSync(join(dir, 'raw.csv'), csv + '\n');

  const host = doc.host, gpuLine = host.webgl?.webgl2 ? `${host.webgl.renderer} (${host.webgl.vendor})` : 'no WebGL2';
  const lines = [];
  lines.push(`# ${doc.mode === 'sanity' ? 'Sanity check' : 'Benchmark'}: ${host.env?.MOVIOLA_BENCH_LABEL || ''} (${host.os} ${host.arch})`, '');
  lines.push(`- webview: ${host.webview}; Tauri ${host.tauri}; ${host.userAgent}`);
  lines.push(`- display: ${host.screen.join('x')} @ dpr ${host.dpr}; refresh interval measured ${fmt(doc.vsync_ms, 2)} ms (${fmt(1000 / doc.vsync_ms, 0)} Hz); stage ${doc.view.w}x${doc.view.h}`);
  lines.push(`- WebGL2 context: ${gpuLine}`);
  lines.push(`- WebGPU: ${host.webgpu?.available ? `${[host.webgpu.vendor, host.webgpu.architecture, host.webgpu.device, host.webgpu.description].filter(Boolean).join(' / ')}${host.webgpu.fallbackAdapter ? ' (fallback adapter = software)' : ''}` : `not available${host.webgpu?.reason ? ` (${host.webgpu.reason})` : ''}`}`);
  lines.push(`- environment flags: ${JSON.stringify(Object.fromEntries(Object.entries(host.env || {}).filter(([k, v]) => v && k !== 'MOVIOLA_BENCH_LABEL')))}`);
  lines.push(`- run: ${doc.started}, ${fmt(doc.wall_s, 0)} s; ${doc.config.reps} repetitions x ${doc.config.phase_ms} ms per phase; scales ${doc.config.scales.join(', ')}`);
  lines.push(`- dataset: ${Object.entries(doc.dims).map(([s, d]) => `x${s} = ${d.nodes} nodes, ${d.lanes} lanes, ${d.days} days`).join('; ')}`, '');

  const groups = new Map();
  for (const r of rows) { const k = `${r.renderer}${(r.repeat||1)>1?' (x'+r.repeat+' draws/frame)':''}|${r.scale}`; if (!groups.has(k)) groups.set(k, []); groups.get(k).push(r); }
  const phases = ['pan', 'zoom', 'playhead', 'hover'];
  lines.push('Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.', '');
  lines.push(`| renderer | scale | backend / what rendered | ${phases.map((p) => p).join(' | ')} | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |`);
  lines.push(`|---|---|---|${phases.map(() => '---').join('|')}|---|---|---|---|---|---|`);
  const summary = [];
  for (const [k, rs] of [...groups].sort((a, b) => parseFloat(a[0].split('|')[1]) - parseFloat(b[0].split('|')[1]) || a[0].split('|')[1].localeCompare(b[0].split('|')[1]) || a[0].localeCompare(b[0]))) {
    const [renderer, scale] = k.split('|');
    const byPhase = phases.map((p) => pool(rs.filter((r) => r.phase === p).map((r) => r.samples.dt)));
    const all = pool(rs.map((r) => r.samples.dt));
    const js = pool(rs.map((r) => r.samples.js));
    const frames = rs.reduce((a, r) => a + r.frames, 0), dropped = rs.reduce((a, r) => a + (r.dropped_pct * r.frames) / 100, 0);
    const repMedians = [...new Set(rs.map((r) => r.rep))].map((rep) => pool(rs.filter((r) => r.rep === rep).map((r) => r.samples.dt)).median);
    const spread = Math.max(...repMedians) - Math.min(...repMedians);
    const what = rs[0].gpu ? `${rs[0].backend}: ${rs[0].gpu}` : rs[0].backend;
    summary.push({ renderer, scale: /^\d+$/.test(scale) ? Number(scale) : scale, what, byPhase, all, js, dropped: (100 * dropped) / frames, spread });
    lines.push(`| ${renderer} | x${scale} | ${what} | ${byPhase.map((b) => `${fmt(b.median)} / ${fmt(b.p95)}`).join(' | ')} | **${fmt(all.median)} / ${fmt(all.p95)}** | ${fmt(all.max, 0)} | ${fmt(js.median, 2)} | ${fmt((100 * dropped) / frames, 0)} % | ${new Set(rs.map((r) => r.rep)).size} | ${fmt(spread)} |`);
  }
  lines.push('');

  if (doc.failures?.length) {
    lines.push('## Configurations that did not run', '', '| renderer | scale | rep | reason |', '|---|---|---|---|');
    const seen = new Set();
    for (const f of doc.failures) { const key = `${f.renderer}|${f.scale}|${f.error}`; if (seen.has(key)) continue; seen.add(key); lines.push(`| ${f.renderer} | x${f.scale} | ${f.rep} | ${f.error.replace(/\|/g, '/')} |`); }
    lines.push('');
  }
  if (doc.a11y?.length) {
    lines.push('## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)', '', '| scale | camera | items | ms per rebuild |', '|---|---|---|---|');
    for (const a of doc.a11y) lines.push(`| x${a.scale} | ${a.camera} | ${a.items} | ${a.camera === 'native' ? 'native: ' + (a.elements ?? '') : fmt(a.ms, 3)} |`);
    lines.push('');
  }

  // sanity check: deliberately slowed canvas must be slower in the numbers
  if (doc.mode === 'sanity') {
    const get = (r) => summary.find((s) => s.renderer === r);
    const base = get('canvas2d'), busy = get('canvas2d+busyv'), shadow = get('canvas2d+shadow');
    lines.push('## Verdict', '');
    if (base && busy) {
      const extra = 1.6 * doc.vsync_ms;
      const jsOk = busy.js.median >= base.js.median + 0.75 * extra;
      const dtOk = busy.all.median >= base.all.median * 1.3;
      lines.push(`- canvas2d + ${fmt(extra)} ms (1.6 refresh intervals) of busy work per frame: script median ${fmt(base.js.median, 2)} -> ${fmt(busy.js.median, 2)} ms (${jsOk ? 'PASS' : 'FAIL'}: expected at least +${fmt(0.75 * extra)} ms); frame median ${fmt(base.all.median)} -> ${fmt(busy.all.median)} ms (${dtOk ? 'PASS' : 'FAIL'}: expected at least x1.3).`);
    }
    if (base && shadow) {
      const dtOk = shadow.all.median >= base.all.median * 1.15 || shadow.all.p95 >= base.all.p95 * 1.3;
      lines.push(`- canvas2d + shadowBlur on every node (a real drawing cost, not script time): script median ${fmt(base.js.median, 2)} -> ${fmt(shadow.js.median, 2)} ms; frame median ${fmt(base.all.median)} -> ${fmt(shadow.all.median)} ms; frame p95 ${fmt(base.all.p95)} -> ${fmt(shadow.all.p95)} ms (${dtOk ? 'PASS' : 'NOT SEEN'}: expected a visible rise in frame median x1.15 or p95 x1.3; when the engine rasterises on a GPU fast enough the cost hides under the refresh interval, and then the headroom runs, with repeated drawing, are the control for the GPU side).`);
    }
    lines.push('');
  }
  writeFileSync(join(dir, 'summary.md'), lines.join('\n') + '\n');
  writeFileSync(join(dir, 'summary.json'), JSON.stringify(summary.map((s) => ({ renderer: s.renderer, scale: s.scale, what: s.what, all: s.all, js: s.js, dropped_pct: s.dropped, by_phase: Object.fromEntries(phases.map((p, i) => [p, s.byPhase[i]])) })), null, 1));
  console.log(lines.join('\n'));
}
