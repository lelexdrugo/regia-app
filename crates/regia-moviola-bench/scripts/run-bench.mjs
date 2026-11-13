// The one command that measures the moviola's time per frame on this machine (README.md):
//
//   npm ci && npm run bench                  # T5 at x1, x10, x100 and x10d, 3 repetitions, then the sanity check
//   npm run bench -- --software              # the same with the GPU switched off (Windows, Linux)
//   npm run bench -- --only main --scales 300,1000 --no-shots   # stress beyond the spec
//   npm run bench -- --only main --scales 100 --repeat 10 --no-shots   # headroom: each frame drawn 10 times
//   npm run bench -- --env GALLIUM_DRIVER=d3d12   # more environment for the window (WSLg: the GPU through Mesa d3d12)
//   npm run bench -- --xvfb                  # Linux: in a virtual display, even where there is one
//
// The renderer spike's scripts/run-bench.mjs (commit 84871ea): it builds the page (vite) and the host window
// (cargo, the workspace's own Tauri), serves the page on the loopback (scripts/serve.mjs), opens it in the window,
// receives results.json and the screenshots, adds what the machine is (scripts/machine.mjs), then writes raw.csv,
// summary.json and summary.md (scripts/analyze.mjs) into results/<label>-<time>/.
// Exit: 0 done; 1 the run failed or the sanity check did not pass; 2 a bad option; 3 the run took too long.
import { spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { analyze } from './analyze.mjs';
import { describeMachine } from './machine.mjs';
import { policy, serve } from './serve.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repo = resolve(root, '../..');
const args = process.argv.slice(2);
const flag = (name) => args.includes(`--${name}`);
const option = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const fail = (message, code = 2) => {
  console.error(`moviola-bench: ${message}`);
  process.exit(code);
};

const platform = process.platform;
const software = flag('software');
const blocks = option('only', 'main,sanity').split(',').map((s) => s.trim()).filter(Boolean);
for (const b of blocks) if (!['main', 'sanity'].includes(b)) fail(`--only: main, sanity or both, not ${b}`);
const number = (name, fallback, min) => {
  const v = Number(option(name, fallback));
  if (!Number.isFinite(v) || v < min) fail(`--${name}: a number of at least ${min}`);
  return v;
};
const scales = option('scales', '1,10,100,10d').split(',').map((s) => s.trim()).filter(Boolean);
for (const s of scales) if (!/^\d+(\.\d+)?d?$/.test(s)) fail(`--scales: numbers, or a number and d (10d), not ${s}`);

// Environment for the window: what --software means on each system, the Linux default of the spike, and --env.
const env = {};
if (platform === 'linux') env.WEBKIT_DISABLE_DMABUF_RENDERER = process.env.WEBKIT_DISABLE_DMABUF_RENDERER ?? '1';
if (software) {
  if (platform === 'win32') env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--disable-gpu';
  else if (platform === 'linux') env.LIBGL_ALWAYS_SOFTWARE = '1';
  else console.warn('moviola-bench: --software has no effect on macOS');
}
args.forEach((a, i) => {
  if (a !== '--env') return;
  const [k, ...v] = (args[i + 1] ?? '').split('=');
  if (!k || !v.length) fail('--env: KEY=VALUE');
  env[k] = v.join('=');
});
for (const k of ['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', 'LIBGL_ALWAYS_SOFTWARE', 'GALLIUM_DRIVER', 'WEBKIT_DISABLE_COMPOSITING_MODE', 'GDK_BACKEND']) {
  if (process.env[k] && env[k] === undefined) env[k] = process.env[k];
}

const machine = describeMachine(repo);
const system = { win32: 'windows', darwin: 'macos', linux: 'linux' }[platform] ?? platform;
const label = option('label', `${system}${machine.wsl ? '-wsl' : ''}${software ? '-software' : ''}`);
if (!/^[A-Za-z0-9_.+-]+$/.test(label)) fail('--label: letters, digits and . _ + - only');
const stamp = new Date().toISOString().replace(/[-:]/g, '').slice(0, 13);
const out = resolve(option('out', join(root, 'results', `${label}-${stamp}`)));
mkdirSync(out, { recursive: true });

const config = {
  label,
  blocks,
  scales: scales.map((s) => (/^\d+$/.test(s) ? Number(s) : s)),
  reps: number('reps', 3, 1),
  phase_ms: number('phase-ms', 2500, 200),
  warmup_ms: number('warmup-ms', 1000, 0),
  repeat: number('repeat', 1, 1),
  shots: !flag('no-shots'),
  software,
  env,
};

const run = (cmd, a, options = {}) => {
  const r = spawnSync(cmd, a, { stdio: 'inherit', cwd: root, ...options });
  if (r.status !== 0) fail(`${cmd} ${a.join(' ')} failed (${r.status ?? r.error?.message})`, 1);
  return r;
};

let exe = null;
if (!flag('skip-build')) {
  const { build } = await import('vite');
  await build({ root, configFile: join(root, 'vite.config.js') });
  // The host's path from cargo itself: it holds under CARGO_TARGET_DIR too.
  const r = run('cargo', ['build', '-p', 'regia-moviola-bench', '--locked', '--message-format=json-render-diagnostics'], { stdio: ['ignore', 'pipe', 'inherit'], encoding: 'utf8', maxBuffer: 64 * 2 ** 20 });
  for (const line of r.stdout.split('\n')) {
    if (!line.startsWith('{')) continue;
    const m = JSON.parse(line);
    if (m.reason === 'compiler-artifact' && m.target?.name === 'regia-moviola-bench' && m.executable) exe = m.executable;
  }
  writeFileSync(join(root, 'dist', 'host.txt'), exe ?? '');
} else if (existsSync(join(root, 'dist', 'host.txt'))) {
  exe = readFileSync(join(root, 'dist', 'host.txt'), 'utf8').trim();
}
if (!exe || !existsSync(exe)) fail('the host window is not built: run without --skip-build', 1);
if (!existsSync(join(root, 'dist', 'index.html'))) fail('the page is not built: run without --skip-build', 1);

const token = randomBytes(16).toString('hex');
let exitAsked = null;
let child = null;
const server = await serve({
  root: join(root, 'dist'),
  out,
  config,
  csp: policy(join(repo, 'app', 'src-tauri', 'tauri.conf.json')),
  token,
  onExit: (code) => {
    exitAsked = code;
    // A host that did not end the run itself on /exit is ended here, once the page had its time.
    setTimeout(() => child?.kill(), 10000);
  },
});

const hostEnv = { ...process.env, ...env, MOVIOLA_BENCH_URL: `${server.origin}/index.html?token=${token}` };
let cmd = exe;
let cmdArgs = [];
if (platform === 'linux' && (flag('xvfb') || (!process.env.DISPLAY && !process.env.WAYLAND_DISPLAY))) {
  cmd = 'xvfb-run';
  cmdArgs = ['-a', '-s', '-screen 0 1600x1000x24', exe];
}
console.log(`moviola-bench: ${label}, blocks ${blocks.join(' and ')}, scales ${scales.join(', ')}, ${config.reps} repetitions`);
console.log(`  ${machine.system}, ${machine.cpu}; GPU ${machine.gpu?.renderer ?? 'unknown'}${machine.wsl ? '; WSL' : ''}`);
console.log(`  results in ${out}`);
console.log('  keep the window in view and the machine idle until it closes');
const timeoutS = number('timeout-s', 2400, 10);
let timedOut = false;
child = spawn(cmd, cmdArgs, { env: hostEnv, stdio: 'inherit' });
const timer = setTimeout(() => {
  timedOut = true;
  console.error(`moviola-bench: no end after ${timeoutS} s: stopping the window`);
  child.kill();
}, timeoutS * 1000);
const status = await new Promise((done) => {
  child.on('exit', (code) => done(code));
  child.on('error', (e) => {
    console.error(`moviola-bench: cannot start ${cmd}: ${e.message}`);
    done(1);
  });
});
clearTimeout(timer);
await server.close();

const { glxinfo, ...described } = machine;
if (glxinfo) writeFileSync(join(out, 'glxinfo.txt'), `${glxinfo}\n`);
if (timedOut) fail('the run took too long', 3);
if (server.saved.has('error.txt')) {
  console.error(readFileSync(join(out, 'error.txt'), 'utf8'));
  fail('the page failed', 1);
}
if (!server.saved.has('results.json')) fail(`no results.json: the window exited with ${status} before the end`, 1);
const doc = JSON.parse(readFileSync(join(out, 'results.json'), 'utf8'));
doc.machine = described;
doc.host_exit = { code: status, asked: exitAsked };
writeFileSync(join(out, 'results.json'), JSON.stringify(doc));
const result = analyze(out);
console.log(result.lines.join('\n'));
console.log(`moviola-bench: send back ${join(out, 'results.json')}`);
process.exit(result.exit);
