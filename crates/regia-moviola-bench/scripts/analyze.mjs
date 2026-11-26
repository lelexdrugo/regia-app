// results.json -> raw.csv (one row per block, renderer, scale, repetition and phase), summary.json and summary.md:
// the renderer spike's scripts/analyze.mjs (commit 84871ea), with the rows of T5 (docs/app/piano/traguardi.md of the
// dossier) and their thresholds where this engine is the one the row names, and the verdict of the sanity check.
// The thresholds and the late frames use the display's interval that the frames ran at (`displayInterval`).
// Exits 1 when the sanity check fails or did not run in full, since then the numbers do not count (criteri.md).
//
//   node scripts/analyze.mjs <results-dir>...
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const PHASES = ['pan', 'zoom', 'playhead', 'hover'];
/** A cell with fewer frames than this is not reported: percentiles of a handful of frames mean nothing. */
export const MIN_N = 30;
/** A frame shorter than this share of the calibrated interval cannot come from a display at the calibrated rate. */
export const FAST = 0.75;
/** The share of a block's frames that must be that short for the display to have run faster than calibrated. */
export const FAST_SHARE = 0.25;

const q = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
const fmt = (v, d = 1) => (Number.isFinite(v) ? v.toFixed(d) : 'n/a');

/**
 * The display's refresh interval for a block's verdicts (README.md, "Read the result"). The harness calibrates it
 * before the block on a second of empty frames, and Windows lowers a dynamic refresh rate while nothing moves: the
 * calibration can read 60 Hz on a 120 Hz display while the stage's frames run at 120 (#489). A display cannot hand
 * over a frame sooner than its interval, and jitter puts few frames there (at most 2 % under 0.75 intervals on the
 * macOS runner, the jitteriest seen). So when at least a quarter of the block's frames are shorter than 0.75
 * calibrated intervals, the display ran faster, and its interval is the median of those frames; otherwise the
 * calibration stands. The interval only ever goes down: a slow renderer cannot raise it.
 */
export function displayInterval(calibrated, frames) {
  const fast = frames.filter((v) => v < FAST * calibrated).sort((a, b) => a - b);
  const share = frames.length ? fast.length / frames.length : 0;
  const ran = frames.length >= MIN_N && share >= FAST_SHARE ? q(fast, 0.5) : null;
  return { ms: ran ?? calibrated, calibrated, fast_pct: 100 * share, corrected: ran !== null };
}

/** The share of frames later than 1.5 display intervals, in %. */
const lateShare = (dts, interval) => (dts.length ? (100 * dts.filter((x) => x > 1.5 * interval).length) / dts.length : NaN);

export function pool(arrays) {
  const all = arrays.flat().sort((a, b) => a - b);
  const max = all.length ? all[all.length - 1] : NaN;
  if (all.length < MIN_N) return { n: all.length, median: NaN, p95: NaN, p99: NaN, max };
  return { n: all.length, median: q(all, 0.5), p95: q(all, 0.95), p99: q(all, 0.99), max };
}

/** Pooled numbers per block, renderer, repeat and scale. */
export function summarise(rows) {
  const groups = new Map();
  for (const r of rows) {
    const key = `${r.block}|${r.renderer}|${r.repeat ?? 1}|${r.scale}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(r);
  }
  return [...groups.values()].map((rs) => {
    const late = (list) => {
      const frames = list.reduce((a, r) => a + r.frames, 0);
      return frames ? (100 * list.reduce((a, r) => a + (r.dropped_pct * r.frames) / 100, 0)) / frames : NaN;
    };
    const reps = [...new Set(rs.map((r) => r.rep))];
    const repMedians = reps.map((rep) => pool(rs.filter((r) => r.rep === rep).map((r) => r.samples.dt)).median);
    return {
      block: rs[0].block,
      renderer: rs[0].renderer,
      repeat: rs[0].repeat ?? 1,
      scale: String(rs[0].scale),
      by_phase: Object.fromEntries(PHASES.map((p) => [p, pool(rs.filter((r) => r.phase === p).map((r) => r.samples.dt))])),
      late_by_phase: Object.fromEntries(PHASES.map((p) => [p, late(rs.filter((r) => r.phase === p))])),
      all: pool(rs.map((r) => r.samples.dt)),
      js: pool(rs.map((r) => r.samples.js)),
      late_pct: late(rs),
      reps: reps.length,
      spread: Math.max(...repMedians) - Math.min(...repMedians),
    };
  });
}

/** A hypervisor's name (systemd-detect-virt, sysctl, the system's model): anything but none, no and unknown. */
const isVirtual = (name) => Boolean(name) && !['none', 'no', 'unknown'].includes(String(name).trim().toLowerCase());
/** A renderer's name as read. WebKit answers every page's WebGL "Apple GPU", whatever renders: that is no reading. */
const reading = (name) => (name && !/^apple gpu$/i.test(String(name).trim()) ? name : null);

/** The engine class and, where it is not read from a renderer, the reason, for summary.md (README.md, "Read the result"). */
function classify(doc) {
  const machine = doc.machine ?? {};
  const text = `${doc.page?.gpu?.renderer ?? ''} ${machine.gpu?.renderer ?? ''}`;
  if (doc.config?.software) return { engine: 'software', why: ' (--software)' };
  if (/llvmpipe|softpipe|swrast|swiftshader|basic render driver/i.test(text) || /^no$/i.test(machine.gpu?.accelerated ?? '')) {
    return { engine: 'software', why: '' };
  }
  if (machine.platform === 'linux' && doc.config?.xvfb) return { engine: 'software', why: ' (the window ran in Xvfb, which has no GPU)' };
  if (machine.platform === 'linux' && !reading(machine.gpu?.renderer) && !reading(doc.page?.gpu?.renderer) && isVirtual(machine.virtual)) {
    return { engine: 'software', why: ` (no renderer read, on a virtual machine: ${machine.virtual})` };
  }
  return { engine: 'gpu', why: '' };
}

/**
 * Software or GPU, from what rendered: the system's GPU (glxinfo on Linux, since WebKit masks WebGL's) and the page's.
 * On Linux, a window in Xvfb rasterises on the processor whatever the machine has, and so does a virtual machine whose
 * renderer nothing read (glxinfo found no display, WebKit said "Apple GPU"): a GitHub runner has no GPU (#488/V23).
 * A Linux that is not virtual keeps gpu when nothing is read.
 */
export function engineClass(doc) {
  return classify(doc).engine;
}

/** The row of T5 for a scale on this engine: its threshold and the verdict, or no threshold here. */
export function target(s, vsync, engine) {
  const limit = 1.1 * vsync;
  const worst = Math.max(...PHASES.map((p) => s.by_phase[p].p95));
  if (engine === 'gpu' && s.scale === '1') return { threshold: `p95 <= ${fmt(limit)} ms (1.1 intervals)`, pass: s.all.p95 <= limit };
  if (engine === 'gpu' && s.scale === '100') return { threshold: `p95 <= ${fmt(limit)} ms in each phase`, pass: worst <= limit };
  if (engine === 'gpu' && s.scale === '10d') {
    return { threshold: `late in pan <= 1 %, p95 <= ${fmt(limit)} ms`, pass: s.late_by_phase.pan <= 1 && s.all.p95 <= limit };
  }
  if (engine === 'software' && s.scale === '10') return { threshold: 'median <= 33 ms, p95 <= 50 ms', pass: s.all.median <= 33 && s.all.p95 <= 50 };
  return { threshold: null, pass: null };
}

/** criteri.md, E3: 1.6 refresh intervals of busy work must show in the script time (+75 % of it) and in the frame
 * (median x1.3). The shadow is a cost of drawing: it shows where the canvas rasterises on the processor. */
export function sanityVerdict(base, busy, shadow, vsync) {
  if (!base || !busy) return { pass: false, missing: true };
  const extra = 1.6 * vsync;
  const jsOk = busy.js.median >= base.js.median + 0.75 * extra;
  const dtOk = busy.all.median >= base.all.median * 1.3;
  const rises = shadow ? shadow.all.median >= base.all.median * 1.15 || shadow.all.p95 >= base.all.p95 * 1.3 : null;
  return { pass: jsOk && dtOk, extra, jsOk, dtOk, rises };
}

export function analyze(dir) {
  const doc = JSON.parse(readFileSync(join(dir, 'results.json'), 'utf8'));
  const blocks = doc.blocks ?? [];
  const intervals = Object.fromEntries(
    blocks.map((b) => [b.name, displayInterval(b.vsync_ms, b.rows.filter((r) => r.renderer === 'canvas2d').flatMap((r) => r.samples?.dt ?? []))]),
  );
  // In a block whose frames ran faster than calibrated, the late frames are counted again on the interval that ran.
  const rows = blocks.flatMap((b) => {
    const interval = intervals[b.name];
    return interval.corrected ? b.rows.map((r) => ({ ...r, dropped_pct: lateShare(r.samples?.dt ?? [], interval.ms) })) : b.rows;
  });
  const cols = ['block', 'renderer', 'scale', 'rep', 'phase', 'repeat', 'frames', 'dt_median', 'dt_p95', 'dt_p99', 'dt_max', 'js_median', 'js_p95', 'dropped_pct', 'init_ms'];
  const csv = [cols.join(',')].concat(rows.map((r) => cols.map((c) => (typeof r[c] === 'number' ? Math.round(r[c] * 1000) / 1000 : JSON.stringify(r[c] ?? ''))).join(',')));
  writeFileSync(join(dir, 'raw.csv'), `${csv.join('\n')}\n`);

  const summary = summarise(rows);
  const { engine, why } = classify(doc);
  const m = doc.machine ?? {};
  const page = doc.page ?? {};
  const label = doc.config?.label ?? '';
  const reference = !m.wsl && ['no', 'none'].includes(m.virtual) && !label.startsWith('ci-');
  const vsyncOf = (name) => blocks.find((b) => b.name === name)?.vsync_ms;
  const calibration = (b) => {
    const measured = `${fmt(b.vsync_ms, 2)} ms (${fmt(1000 / b.vsync_ms, 0)} Hz) before ${b.name}`;
    const i = intervals[b.name];
    return i.corrected ? `${measured}, but ${fmt(i.fast_pct, 0)} % of its frames ran at ${fmt(i.ms, 2)} ms (${fmt(1000 / i.ms, 0)} Hz), the interval its verdicts use` : measured;
  };
  const infoOf = (block, scale) => blocks.find((b) => b.name === block)?.infos.find((i) => String(i.scale) === scale && i.renderer === 'canvas2d');

  const lines = [`# Moviola bench: ${label}`, ''];
  lines.push(`- system: ${m.system ?? '?'} (${m.platform ?? '?'} ${m.arch ?? ''}); ${m.cpu ?? '?'}, ${m.cpus ?? '?'} threads, ${m.memory_gb ?? '?'} GB`);
  lines.push(`- WSL: ${m.wsl ? `yes${m.wslg ? ', with WSLg' : ''}` : 'no'}; virtual machine: ${m.virtual ?? 'unknown'}`);
  lines.push(`- GPU, as the system names it: ${m.gpu?.renderer ?? 'unknown'}${m.gpu?.accelerated ? ` (accelerated: ${m.gpu.accelerated})` : ''}`);
  lines.push(`- GPU, as the page's WebGL names it: ${page.gpu?.webgl ? page.gpu.renderer : 'no WebGL'} (WebKit always answers "Apple GPU")`);
  lines.push(`- engine class: **${engine}**${why}`);
  lines.push(`- webview: ${page.webview ?? '?'}; Tauri ${page.tauri ?? '?'}; ${page.userAgent ?? ''}`);
  lines.push(`- display: ${(page.screen ?? []).join('x')} at dpr ${page.dpr}; refresh interval measured ${blocks.map(calibration).join(', ')}; stage ${doc.view?.w}x${doc.view?.h} and the tape ${doc.view?.tape}`);
  lines.push(`- commit: ${m.commit ? `${m.commit.sha}${m.commit.clean ? '' : ' (with local changes)'}` : 'unknown'}; node ${m.node ?? '?'}`);
  lines.push(`- run: ${doc.started}, ${fmt(doc.wall_s, 0)} s; ${doc.config?.reps} repetitions x ${doc.config?.phase_ms} ms per phase, warm-up ${doc.config?.warmup_ms} ms; environment ${JSON.stringify(doc.config?.env ?? {})}`);
  lines.push('');

  const main = summary.filter((s) => s.block === 'main');
  if (main.length) {
    const vsync = intervals.main.ms;
    lines.push('## T5: time per frame (docs/app/piano/traguardi.md)', '');
    lines.push('Frame time in ms, median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. The display refresh interval is the floor: one missed refresh costs a whole interval. Late = longer than 1.5 intervals. Script ms is the time inside the draw (camera, labels, frame), not the GPU or the compositor after it.');
    lines.push(`A threshold shows where this engine is the one the row of T5 names. Its verdict counts ${reference ? 'here: this is not a runner, a VM or WSL' : '**only as information here**: a runner, a VM or WSL records the number without a verdict'}.`, '');
    lines.push(`| scale | milestones + aggregates | lanes | ${PHASES.join(' | ')} | all phases | max | late in pan | late, all | script ms (median) | threshold here | verdict |`);
    lines.push(`|---|---|---|${PHASES.map(() => '---').join('|')}|---|---|---|---|---|---|---|`);
    const order = ['1', '10', '100', '10d'];
    const rank = (s) => (order.includes(s.scale) ? order.indexOf(s.scale) : 10 + parseFloat(s.scale));
    for (const s of [...main].sort((a, b) => rank(a) - rank(b) || a.repeat - b.repeat)) {
      const info = infoOf('main', s.scale);
      const t = s.repeat === 1 ? target(s, vsync, engine) : { threshold: null, pass: null };
      const nodes = info ? `${info.milestones} + ${info.aggregates} = ${info.milestones + info.aggregates}` : '?';
      lines.push(`| x${s.scale}${s.repeat > 1 ? ` (x${s.repeat} draws)` : ''} | ${nodes} | ${info?.lanes ?? '?'} | ${PHASES.map((p) => `${fmt(s.by_phase[p].median)} / ${fmt(s.by_phase[p].p95)}`).join(' | ')} | **${fmt(s.all.median)} / ${fmt(s.all.p95)}** | ${fmt(s.all.max, 0)} | ${fmt(s.late_by_phase.pan, 1)} % | ${fmt(s.late_pct, 1)} % | ${fmt(s.js.median, 2)} | ${t.threshold ?? 'none on this engine'} | ${t.pass === null ? '' : t.pass ? 'PASS' : 'FAIL'} |`);
    }
    lines.push('');
  }

  let exit = 0;
  const sanity = summary.filter((s) => s.block === 'sanity');
  if (blocks.some((b) => b.name === 'sanity')) {
    // The busy work the harness spun: 1.6 calibrated intervals, whatever the display ran at.
    const vsync = vsyncOf('sanity');
    const get = (r) => sanity.find((s) => s.renderer === r);
    const base = get('canvas2d');
    const busy = get('canvas2d+busyv');
    const shadow = get('canvas2d+shadow');
    const v = sanityVerdict(base, busy, shadow, vsync);
    lines.push('## Sanity check (criteri.md, E3), at x10', '');
    if (v.missing) {
      lines.push('- **FAIL**: the sanity block did not run in full (see below): the numbers above do not count.');
    } else {
      lines.push(`- canvas2d + ${fmt(v.extra)} ms (1.6 refresh intervals) of busy work per frame: script median ${fmt(base.js.median, 2)} -> ${fmt(busy.js.median, 2)} ms (${v.jsOk ? 'PASS' : 'FAIL'}: at least +${fmt(0.75 * v.extra)} ms expected); frame median ${fmt(base.all.median)} -> ${fmt(busy.all.median)} ms (${v.dtOk ? 'PASS' : 'FAIL'}: at least x1.3 expected).`);
      if (shadow) {
        lines.push(`- canvas2d + a blurred shadow on every fill and stroke (a cost of drawing, not of script): script median ${fmt(base.js.median, 2)} -> ${fmt(shadow.js.median, 2)} ms; frame median ${fmt(base.all.median)} -> ${fmt(shadow.all.median)} ms, p95 ${fmt(base.all.p95)} -> ${fmt(shadow.all.p95)} ms (${v.rises ? 'RISES' : 'NOT SEEN'}; information only: where a GPU rasterises fast enough the cost hides under the refresh interval).`);
      }
      lines.push('', `**Verdict: ${v.pass ? 'PASS**: the slowed configuration comes out slower, so these numbers measure what they say.' : 'FAIL**: the slowed configuration does not come out slower, so the numbers above do not count.'}`);
    }
    lines.push('');
    if (!v.pass) exit = 1;
  }

  const failures = blocks.flatMap((b) => b.failures);
  if (failures.length) {
    lines.push('## Configurations that did not run', '', '| block | renderer | scale | rep | reason |', '|---|---|---|---|---|');
    for (const f of failures) lines.push(`| ${f.block} | ${f.renderer} | x${f.scale} | ${f.rep} | ${String(f.error).replace(/\|/g, '/')} |`);
    lines.push('');
  }
  writeFileSync(join(dir, 'summary.md'), `${lines.join('\n')}\n`);
  writeFileSync(join(dir, 'summary.json'), JSON.stringify({ engine, reference, intervals, summary }, null, 1));
  return { lines, exit, engine, summary };
}

const invoked = process.argv[1] && resolve(process.argv[1]).toLowerCase() === fileURLToPath(import.meta.url).toLowerCase();
if (invoked) {
  let exit = 0;
  const dirs = process.argv.slice(2);
  if (!dirs.length) {
    console.error('usage: node scripts/analyze.mjs <results-dir>...');
    process.exit(2);
  }
  for (const dir of dirs) {
    const r = analyze(dir);
    console.log(r.lines.join('\n'));
    exit = Math.max(exit, r.exit);
  }
  process.exit(exit);
}
