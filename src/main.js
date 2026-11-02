import { invoke } from '@tauri-apps/api/core';
import { runBenchmark, hostInfo, DEFAULT_RENDERERS, SANITY_RENDERERS, RENDERERS } from './harness.js';

const status = document.getElementById('status');
const say = (s) => { status.textContent = s; };

async function main() {
  const cfg = await invoke('bench_config');
  if (!cfg.out) { say('idle: set MOVIOLA_BENCH_OUT to run the benchmark (npm run bench)'); return; }
  const list = (v, d) => (v ? v.split(',').map((s) => s.trim()).filter(Boolean) : d);
  const renderers = cfg.sanity ? SANITY_RENDERERS : list(cfg.renderers, DEFAULT_RENDERERS);
  for (const r of renderers) if (!RENDERERS[r]) throw new Error(`unknown renderer ${r}`);
  const run = {
    renderers,
    scales: cfg.sanity ? [10] : list(cfg.scales, ['1', '10', '100']).map(Number),
    reps: Number(cfg.reps || 3),
    phase_ms: Number(cfg.phase_ms || 2500),
    warmup_ms: Number(cfg.warmup_ms || 1000),
    repeat: Number(cfg.repeat || 1),
    shots: cfg.shots !== false && !cfg.sanity,
  };
  const t0 = Date.now();
  const result = await runBenchmark(run, say);
  const host = await hostInfo(cfg.host);
  const shots = result.shots; delete result.shots;
  const doc = { schema: 'moviola-bench/v1', started: new Date(t0).toISOString(), wall_s: (Date.now() - t0) / 1000, mode: cfg.sanity ? 'sanity' : 'bench', config: run, host, ...result };
  await invoke('bench_save', { name: 'results.json', data: JSON.stringify(doc), base64: false });
  for (const s of shots) await invoke('bench_save', { name: `shots/${s.name}.png`, data: s.b64, base64: true });
  say('done');
  await invoke('bench_exit', { code: 0 });
}

main().catch(async (e) => {
  say('failed: ' + (e?.message || e));
  try { await invoke('bench_save', { name: 'error.txt', data: String(e?.stack || e), base64: false }); } catch { /* nothing else to do */ }
  try { await invoke('bench_exit', { code: 1 }); } catch { /* ignore */ }
});
