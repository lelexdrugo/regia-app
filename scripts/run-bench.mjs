// The one command that reproduces the numbers on this machine:
//
//   npm ci && npm run bench                  # full run: 5 renderers x scales 1/10/100 x 3 repetitions
//   npm run bench -- --software              # same, with the GPU switched off (the Linux/Windows fallback case)
//   npm run bench -- --scales 300,1000 --reps 3   # stress tiers beyond the x100 of the spec
//   npm run bench -- --repeat 10 --scales 100 --renderers canvas2d,webgl2-raw,pixi-webgl,pixi-webgpu   # headroom: each frame drawn 10 times
//   npm run bench -- --sanity                # the control: a deliberately slowed canvas must come out slower
//
// It builds the page (vite) and the Tauri window (cargo), runs the window with MOVIOLA_BENCH_* set,
// then turns results.json into raw.csv and summary.md (scripts/analyze.mjs).
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import os from 'node:os';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const flag = (n) => args.includes(`--${n}`);
const opt = (n, d) => { const i = args.indexOf(`--${n}`); return i >= 0 ? args[i + 1] : d; };

const platform = process.platform;
const software = flag('software');
const label = opt('label', `${platform === 'win32' ? 'windows' : platform === 'darwin' ? 'macos' : 'linux'}${software ? '-software' : ''}${flag('sanity') ? '-sanity' : ''}`);
const stamp = new Date().toISOString().replace(/[-:]/g, '').slice(0, 13);
const out = resolve(opt('out', join(root, 'results', `${label}-${stamp}`)));
mkdirSync(out, { recursive: true });

const run = (cmd, a, o = {}) => {
  const r = spawnSync(cmd, a, { stdio: 'inherit', cwd: root, shell: platform === 'win32' && /^(npm|npx)$/.test(cmd), ...o });
  if (r.status !== 0) { console.error(`${cmd} ${a.join(' ')} failed (${r.status})`); process.exit(r.status || 1); }
};

if (!flag('skip-build')) {
  run('npm', ['run', 'build:web']);
  run('cargo', ['build', '--release', '--manifest-path', join(root, 'src-tauri', 'Cargo.toml')]);
}
const exe = join(root, 'src-tauri', 'target', 'release', platform === 'win32' ? 'moviola-bench.exe' : 'moviola-bench');
if (!existsSync(exe)) { console.error(`missing ${exe}`); process.exit(1); }

const env = { ...process.env, MOVIOLA_BENCH_OUT: out, MOVIOLA_BENCH_LABEL: label };
for (const [k, o] of [['RENDERERS', 'renderers'], ['SCALES', 'scales'], ['REPS', 'reps'], ['PHASE_MS', 'phase-ms'], ['WARMUP_MS', 'warmup-ms'], ['REPEAT', 'repeat']]) if (opt(o)) env[`MOVIOLA_BENCH_${k}`] = opt(o);
if (flag('sanity')) env.MOVIOLA_BENCH_SANITY = '1';
if (flag('no-shots')) env.MOVIOLA_BENCH_SHOTS = '0';
if (software) {
  if (platform === 'win32') env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--disable-gpu';
  else if (platform === 'linux') { env.LIBGL_ALWAYS_SOFTWARE = '1'; env.WEBKIT_DISABLE_DMABUF_RENDERER = '1'; }
  else console.warn('--software has no effect on this OS');
}
console.log(`running ${exe}\n  out: ${out}\n  host: ${os.type()} ${os.release()} ${os.arch()}, ${os.cpus()[0]?.model}`);

let cmd = exe, cmdArgs = [];
if (platform === 'linux' && (flag('xvfb') || (!process.env.DISPLAY && !process.env.WAYLAND_DISPLAY))) { cmd = 'xvfb-run'; cmdArgs = ['-a', '-s', '-screen 0 1600x1000x24', exe]; }
const r = spawnSync(cmd, cmdArgs, { stdio: 'inherit', env });
console.log(`window exited with ${r.status}`);
if (!existsSync(join(out, 'results.json'))) { console.error('no results.json: the run did not finish'); process.exit(r.status || 1); }
run('node', [join(root, 'scripts', 'analyze.mjs'), out]);
process.exit(r.status === 0 ? 0 : r.status || 1);
