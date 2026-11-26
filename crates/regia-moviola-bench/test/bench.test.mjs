// The bench's own tests, in Node, without a window: the synthetic timelines, the verdicts, the server's rules and
// what the machine says it is. `npm test`.
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { analyze, engineClass, pool, sanityVerdict, summarise, target } from '../scripts/analyze.mjs';
import { glxSummary, isWsl } from '../scripts/machine.mjs';
import { safeName, serve } from '../scripts/serve.mjs';
import { dimensions, parseScale, syntheticTimeline } from '../src/timeline.js';

test('the scales of T5 have the counts of traguardi.md: 155, 1,550, 15,500 and 12,400 nodes', () => {
  const nodes = (s) => dimensions(s).milestones + dimensions(s).aggregates;
  assert.deepEqual([1, 10, 100, '10d'].map(nodes), [155, 1550, 15500, 12400]);
  assert.deepEqual([1, 10, 100].map((s) => dimensions(s).lanes), [8, 16, 32]);
  // x10d: the lanes and days of x10, eight times the milestones and aggregates
  assert.equal(dimensions('10d').lanes, dimensions(10).lanes);
  assert.equal(dimensions('10d').days, dimensions(10).days);
  assert.equal(Math.round(dimensions(10).days), 91);
});

test('a scale is a number or a number and d, nothing else', () => {
  assert.deepEqual(parseScale('10d'), { base: 10, crowd: 8 });
  assert.deepEqual(parseScale(100), { base: 100, crowd: 1 });
  for (const bad of ['', 'x10', '10x', '-1', '0', 'd']) assert.throws(() => parseScale(bad));
});

test('a timeline is the same for the same scale, in time order, with parents before their children', () => {
  const a = syntheticTimeline(10);
  assert.equal(JSON.stringify(a), JSON.stringify(syntheticTimeline(10)));
  assert.notEqual(JSON.stringify(syntheticTimeline('10d').milestones.slice(0, 5)), JSON.stringify(a.milestones.slice(0, 5)));
  assert.equal(a.milestones.length, 270);
  assert.equal(a.aggregates.length, 1280);
  const index = new Map(a.milestones.map((m, i) => [m.id, i]));
  a.milestones.forEach((m, i) => {
    if (i > 0) assert.ok(Date.parse(a.milestones[i - 1].ts) <= Date.parse(m.ts));
    for (const p of m.parents) assert.ok(index.get(p) < i, `${m.id} has a later parent ${p}`);
    assert.match(m.title, /^#\d{3} /);
  });
  const named = a.milestones.reduce((n, m) => n + m.refs.issue.length, 0);
  assert.equal(named, a.aggregates.length, 'every aggregate is named by one milestone');
  assert.ok(a.directions.every((d) => /^branch-\d+$/.test(d.id)));
});

const row = (block, renderer, phase, dt, js, rep = 1, scale = 10) => ({ block, renderer, scale, rep, phase, repeat: 1, frames: dt.length, dropped_pct: 0, samples: { dt, js } });
const steady = (v, n = 40) => Array.from({ length: n }, () => v);

test('pooled percentiles need thirty frames', () => {
  assert.ok(Number.isNaN(pool([steady(10, 29)]).median));
  assert.equal(pool([steady(10, 20), steady(20, 20)]).median, 20);
});

test('the sanity check passes when the slowed configuration is slower, and fails when it is not', () => {
  const vsync = 10;
  const make = (busyDt, busyJs) =>
    summarise(['pan', 'zoom', 'playhead', 'hover'].flatMap((p) => [row('sanity', 'canvas2d', p, steady(10), steady(0.5)), row('sanity', 'canvas2d+busyv', p, steady(busyDt), steady(busyJs))]));
  const get = (s, r) => s.find((x) => x.renderer === r);
  const slowed = make(20, 16.5);
  assert.equal(sanityVerdict(get(slowed, 'canvas2d'), get(slowed, 'canvas2d+busyv'), null, vsync).pass, true);
  // negative control: busy work that does not reach the numbers
  const deaf = make(10, 0.6);
  const v = sanityVerdict(get(deaf, 'canvas2d'), get(deaf, 'canvas2d+busyv'), null, vsync);
  assert.equal(v.pass, false);
  assert.equal(v.jsOk, false);
  assert.equal(v.dtOk, false);
  assert.equal(sanityVerdict(get(deaf, 'canvas2d'), undefined, null, vsync).pass, false);
});

test('a threshold of T5 applies only on the engine its row names', () => {
  const s = (scale, p95, latePan = 0) => ({
    scale, all: { median: 10, p95 }, by_phase: Object.fromEntries(['pan', 'zoom', 'playhead', 'hover'].map((p) => [p, { p95 }])), late_by_phase: { pan: latePan },
  });
  assert.equal(target(s('1', 10.9), 10, 'gpu').pass, true);
  assert.equal(target(s('1', 11.1), 10, 'gpu').pass, false);
  assert.equal(target(s('10d', 10, 1.5), 10, 'gpu').pass, false);
  assert.equal(target(s('10', 40), 10, 'gpu').threshold, null);
  assert.equal(target(s('10', 40), 10, 'software').pass, true);
  assert.equal(target(s('10', 51), 10, 'software').pass, false);
  assert.equal(target(s('100', 40), 10, 'software').threshold, null);
});

test('llvmpipe, WARP or --software make a run software; a GPU name does not', () => {
  assert.equal(engineClass({ machine: { gpu: { renderer: 'llvmpipe (LLVM 20.1.2, 256 bits)', accelerated: 'no' } }, page: { gpu: { renderer: 'Apple GPU' } } }), 'software');
  assert.equal(engineClass({ page: { gpu: { renderer: 'ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11)' } } }), 'software');
  assert.equal(engineClass({ config: { software: true }, page: { gpu: { renderer: 'ANGLE (Intel, Arc)' } } }), 'software');
  assert.equal(engineClass({ machine: { gpu: { renderer: 'Intel(R) Arc(TM) 140T GPU' } }, page: { gpu: { renderer: 'ANGLE (Intel, Arc)' } } }), 'gpu');
});

/** The probe of a GitHub Linux runner: glxinfo found no display, WebKit's WebGL said "Apple GPU". */
const runner = (virtual) => ({
  config: { software: false },
  machine: { platform: 'linux', virtual, gpu: { renderer: null, accelerated: null, vendor: null } },
  page: { gpu: { webgl: true, renderer: 'Apple GPU', vendor: 'Apple Inc.' } },
});

test('a Linux virtual machine whose renderer nothing read is software, not gpu (#488/V23)', () => {
  assert.equal(engineClass(runner('amazon')), 'software');
  assert.equal(engineClass(runner('microsoft')), 'software');
  assert.equal(engineClass({ ...runner('none'), config: { xvfb: true } }), 'software', 'a window in Xvfb has no GPU');
  // controls: bare metal, an unknown answer and the macOS runner keep gpu; a renderer read wins
  assert.equal(engineClass(runner('none')), 'gpu');
  assert.equal(engineClass(runner('unknown')), 'gpu');
  assert.equal(engineClass({ ...runner('yes'), machine: { platform: 'darwin', virtual: 'yes', gpu: { renderer: null } } }), 'gpu');
  assert.equal(engineClass({ ...runner('kvm'), machine: { platform: 'linux', virtual: 'kvm', gpu: { renderer: 'NVIDIA GeForce RTX 4090/PCIe/SSE2', accelerated: 'yes' } } }), 'gpu');
});

/** analyze() on a results.json written to a folder of its own: summary.md's lines and summary.json. */
function analyzed(doc) {
  const dir = mkdtempSync(join(tmpdir(), 'moviola-bench-analyze-'));
  try {
    writeFileSync(join(dir, 'results.json'), JSON.stringify(doc));
    analyze(dir);
    return { md: readFileSync(join(dir, 'summary.md'), 'utf8').split('\n'), json: JSON.parse(readFileSync(join(dir, 'summary.json'), 'utf8')) };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
/** A block as the harness writes it: rows of the four phases at each scale, late frames counted on `vsync`. */
const block = (name, vsync, scales, dts, renderer = 'canvas2d') => ({
  name, vsync_ms: vsync, failures: [], infos: [],
  rows: scales.flatMap((scale) => ['pan', 'zoom', 'playhead', 'hover'].map((phase) => ({
    block: name, renderer, scale, rep: 1, phase, repeat: 1, frames: dts.length,
    dropped_pct: (100 * dts.filter((x) => x > 1.5 * vsync).length) / dts.length, samples: { dt: dts, js: dts.map(() => 0.5) },
  }))),
});
const verdict = (md, scale) => md.find((l) => l.startsWith(`| x${scale} |`))?.split('|').at(-2).trim();

test("the runner's engine class follows into summary.md and summary.json, with the software row of T5", () => {
  const { md, json } = analyzed({ ...runner('amazon'), config: { label: 'ci-linux', software: false }, blocks: [block('main', 16, [1, 10], steady(16))] });
  assert.equal(json.engine, 'software');
  assert.match(md.find((l) => l.startsWith('- engine class:')), /\*\*software\*\* \(no renderer read, on a virtual machine: amazon\)/);
  assert.match(md.find((l) => l.startsWith('| x10 |')), /median <= 33 ms, p95 <= 50 ms \| PASS \|$/);
  assert.match(md.find((l) => l.startsWith('| x1 |')), /none on this engine/);
});

test('a display that ran faster than calibrated is judged at the rate of its frames: calibrated 60 Hz, used 120 Hz', () => {
  // Windows' dynamic refresh rate: the empty frames of the calibration at 60 Hz, the stage's at 120, one in ten
  // missing a refresh. At 120 Hz that misses T5 (p95 16.7 against 9.1 ms, 10 % late in pan); at 60 Hz it would pass.
  const drr = Array.from({ length: 40 }, (_, i) => (i % 10 === 9 ? 16.7 : 8.3));
  const { md, json } = analyzed({
    config: { label: 'windows', reps: 1 },
    machine: { platform: 'win32', virtual: 'no', wsl: false, gpu: { renderer: 'Intel(R) Arc(TM) GPU' } },
    page: { gpu: { webgl: true, renderer: 'ANGLE (Intel, Intel(R) Arc(TM) GPU Direct3D11)' } },
    blocks: [block('main', 16.7, [1, '10d'], drr)],
  });
  assert.equal(verdict(md, '1'), 'FAIL');
  assert.equal(verdict(md, '10d'), 'FAIL');
  assert.match(md.find((l) => l.startsWith('| x10d |')), /\| 10\.0 % \| 10\.0 % \|/, 'late frames counted on 8.3 ms');
  assert.match(md.find((l) => l.startsWith('- display:')), /16\.70 ms \(60 Hz\) before main, but 90 % of its frames ran at 8\.30 ms \(120 Hz\)/);
  assert.equal(json.intervals?.main?.ms, 8.3);
});

test('WSL and the rasteriser are read from what the system prints', () => {
  assert.equal(isWsl('Linux version 6.6.87.2-microsoft-standard-WSL2 (root@...)', {}), true);
  assert.equal(isWsl('Linux version 6.8.0-45-generic (buildd@lcy02)', {}), false);
  assert.equal(isWsl('', { WSL_DISTRO_NAME: 'Ubuntu-24.04' }), true);
  const glx = 'name of display: :0\nOpenGL vendor string: Mesa\nOpenGL renderer string: llvmpipe (LLVM 20.1.2, 256 bits)\n    Accelerated: no\n';
  assert.deepEqual(glxSummary(glx), { renderer: 'llvmpipe (LLVM 20.1.2, 256 bits)', accelerated: 'no', vendor: 'Mesa' });
});

test('the page may write results.json, error.txt and PNG screenshots, and nothing else', () => {
  for (const ok of ['results.json', 'error.txt', 'shots/x1-detail.png', 'shots/x100-overview.png']) assert.equal(safeName(ok), true, ok);
  for (const bad of ['', 'summary.md', '../results.json', 'shots/../x.png', 'shots/.png', 'shots/a/b.png', '/etc/passwd', 'shots\\x.png', 'C:/x.png']) {
    assert.equal(safeName(bad), false, bad);
  }
});

test('the server needs the run token to give the configuration or take a file', async () => {
  const out = mkdtempSync(join(tmpdir(), 'moviola-bench-test-'));
  const server = await serve({ root: out, out, config: { reps: 1 }, csp: "default-src 'none'", token: 'secret' });
  try {
    assert.equal((await fetch(`${server.origin}/config.json`)).status, 403);
    const config = await fetch(`${server.origin}/config.json?token=secret`);
    assert.equal(config.headers.get('content-security-policy'), "default-src 'none'");
    assert.deepEqual(await config.json(), { reps: 1 });
    assert.equal((await fetch(`${server.origin}/results.json`, { method: 'POST', body: '{}' })).status, 403);
    assert.equal((await fetch(`${server.origin}/summary.md?token=secret`, { method: 'POST', body: 'x' })).status, 403);
    assert.equal((await fetch(`${server.origin}/results.json?token=secret`, { method: 'POST', body: '{"ok":1}' })).status, 204);
    assert.equal(readFileSync(join(out, 'results.json'), 'utf8'), '{"ok":1}');
    assert.deepEqual([...server.saved], ['results.json']);
    assert.equal((await fetch(`${server.origin}/../../secret`)).status, 404);
  } finally {
    await server.close();
    rmSync(out, { recursive: true, force: true });
  }
});
