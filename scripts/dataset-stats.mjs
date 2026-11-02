// Reads the real `.regia/moviola/` stores (read-only) and writes the numeric shape of the data
// that drives the synthetic generator (src/dataset.js). Only counts and distributions are
// written, never titles or other content.
// Usage: node scripts/dataset-stats.mjs <dossier-dir> [<dossier-dir> ...] > src/shape.json
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const dirs = process.argv.slice(2);
if (!dirs.length) { console.error('usage: dataset-stats.mjs <dossier-dir>...'); process.exit(2); }

const jsonl = (p) => (existsSync(p) ? readFileSync(p, 'utf8').split('\n').filter((l) => l.trim()).map((l) => JSON.parse(l)) : []);
const counter = (arr) => arr.reduce((m, k) => ((m[k] = (m[k] || 0) + 1), m), {});
const quant = (a, q) => { const s = [...a].sort((x, y) => x - y); return s.length ? s[Math.min(s.length - 1, Math.floor(q * s.length))] : 0; };

const ms = [], ag = [], dirsAll = [];
let snapshots = 0;
for (const d of dirs) {
  const base = join(d, '.regia', 'moviola');
  ms.push(...jsonl(join(base, 'milestones.jsonl')));
  ag.push(...jsonl(join(base, 'aggregati.jsonl')));
  dirsAll.push(...jsonl(join(base, 'direzioni.jsonl')));
  if (existsSync(join(base, 'snapshot'))) snapshots += readdirSync(join(base, 'snapshot')).filter((f) => f.endsWith('.json')).length;
}
const ts = [...ms.map((m) => Date.parse(m.ts)), ...ag.map((a) => Date.parse(a.finestra?.[0]))].filter(Number.isFinite);
const spanDays = (Math.max(...ts) - Math.min(...ts)) / 864e5;
const titleLen = ms.map((m) => (m.titolo || '').length);
const perDirection = Object.values(counter(ms.map((m) => `${m.progetto}/${m.direzione}`))).sort((a, b) => b - a);
const lanes = new Set([...ms.map((m) => `${m.progetto}/${m.direzione}`), ...ag.map((a) => `${a.progetto}/task`)]).size;
const durMin = ag.map((a) => (Date.parse(a.finestra[1]) - Date.parse(a.finestra[0])) / 6e4).filter(Number.isFinite);
const out = {
  note: 'numeric shape of the real moviola stores, derived by scripts/dataset-stats.mjs',
  sources: dirs.length,
  spanDays: Math.round(spanDays * 10) / 10,
  milestones: ms.length,
  aggregates: ag.length,
  directions: dirsAll.length,
  lanes,
  snapshots,
  nodes: ms.length + ag.length,
  milestoneKinds: counter(ms.map((m) => m.tipo)),
  parentsPerMilestone: counter(ms.map((m) => m.genitori.length)),
  laneShare: perDirection.map((n) => Math.round((n / ms.length) * 100) / 100),
  titleLength: { mean: Math.round(titleLen.reduce((a, b) => a + b, 0) / titleLen.length), p50: quant(titleLen, 0.5), p95: quant(titleLen, 0.95), max: Math.max(...titleLen) },
  rounds: counter(ag.map((a) => a.round)),
  modelsPerTask: counter(ag.map((a) => a.modelli.length)),
  taskDurationMin: { p50: Math.round(quant(durMin, 0.5)), p95: Math.round(quant(durMin, 0.95)) },
};
console.log(JSON.stringify(out, null, 2));
