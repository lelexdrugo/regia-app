// Consolidated table of several result directories, for the dossier.
// Usage: node scripts/tabella.mjs "Windows, WebView2, GPU=results/windows-2026-11-02/main" "Windows, software=results/windows-2026-11-02/software" ...
// Per renderer and scale: median / p95 of the pooled frames of all repetitions (all phases) and the worst phase p95.
import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

const f = (v, d = 1) => (Number.isFinite(v) ? v.toFixed(d) : 'n/a');
const out = [];
for (const arg of process.argv.slice(2)) {
  const i = arg.lastIndexOf('='); const title = arg.slice(0, i), dir = arg.slice(i + 1);
  if (!existsSync(join(dir, 'summary.json'))) { out.push(`### ${title}\n\n(no data: ${dir})\n`); continue; }
  const rows = JSON.parse(readFileSync(join(dir, 'summary.json'), 'utf8'));
  const doc = JSON.parse(readFileSync(join(dir, 'results.json'), 'utf8'));
  const h = doc.host;
  out.push(`### ${title}`, '');
  out.push(`WebView: ${h.webview}; refresh interval ${f(doc.vsync_ms, 1)} ms (${f(1000 / doc.vsync_ms, 0)} Hz); dpr ${doc.dpr}; WebGL2 reported by the page: ${h.webgl?.webgl2 ? h.webgl.renderer : 'none'}; WebGPU: ${h.webgpu?.available ? [h.webgpu.vendor, h.webgpu.architecture].filter(Boolean).join(' ') + (h.webgpu.fallbackAdapter ? ' (software fallback adapter)' : '') : 'not available'}.`, '');
  out.push('| renderer | scale | median / p95 (ms) | worst phase p95 | frames late | script ms (median) |', '|---|---|---|---|---|---|');
  for (const r of rows.sort((a, b) => parseFloat(a.scale) - parseFloat(b.scale) || String(a.scale).localeCompare(String(b.scale)) || a.renderer.localeCompare(b.renderer))) {
    const worst = Math.max(...Object.values(r.by_phase).map((p) => p.p95));
    out.push(`| ${r.renderer} | x${r.scale} | **${f(r.all.median)}** / ${f(r.all.p95)} | ${f(worst)} | ${f(r.dropped_pct, 0)} % | ${f(r.js.median, 2)} |`);
  }
  const miss = new Set((doc.failures || []).map((x) => x.renderer));
  if (miss.size) out.push('', `Not run here: ${[...miss].join(', ')} (${[...new Set(doc.failures.map((x) => x.error))].join('; ')}).`);
  out.push('');
}
console.log(out.join('\n'));
