// The whole set of measurements for one machine, one directory per measurement under results/<host>-<date>/:
//   main      the spec: 5 renderers x scales 1/10/100 x 3 repetitions (+ screenshots)
//   software  the same with the GPU switched off (Windows, Linux)
//   stress    scales 10d (the days of x10 with 8 times the nodes), 300 and 1000, beyond the spec: where each renderer starts to give way
//   headroom  scale 100, every frame drawn 10 times: exposes GPU/raster cost hidden by the display refresh rate
//   sanity    the control: a deliberately slowed canvas must come out slower
// Usage: node scripts/run-suite.mjs [--only main,sanity] [--host-label name] [--main-env KEY=VALUE]
import { spawnSync } from 'node:child_process';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const opt = (n, d) => { const i = args.indexOf(`--${n}`); return i >= 0 ? args[i + 1] : d; };
const only = opt('only', 'main,software,stress,headroom,sanity').split(',');
const host = opt('host-label', { win32: 'windows', darwin: 'macos', linux: 'linux' }[process.platform]);
const stamp = new Date().toISOString().slice(0, 10);
const base = join(root, 'results', `${host}-${stamp}`);
const mainEnv = opt('main-env') ? ['--env', opt('main-env')] : [];
const gpuRenderers = 'canvas2d,canvas2d-lod,webgl2-raw,pixi-webgl,pixi-webgpu';

const steps = {
  main: [...mainEnv, '--label', `${host}-main`, '--out', join(base, 'main')],
  software: ['--software', '--label', `${host}-software`, '--out', join(base, 'software')],
  stress: [...mainEnv, '--scales', '10d,300,1000', '--no-shots', '--label', `${host}-stress`, '--out', join(base, 'stress')],
  headroom: [...mainEnv, '--scales', '100', '--repeat', '10', '--renderers', gpuRenderers, '--no-shots', '--label', `${host}-headroom`, '--out', join(base, 'headroom')],
  sanity: [...mainEnv, '--sanity', '--label', `${host}-sanity`, '--out', join(base, 'sanity')],
};
let built = false, failed = 0;
for (const name of only) {
  if (name === 'software' && process.platform === 'darwin') { console.log('skip software on macOS'); continue; }
  console.log(`\n=== ${name} ===`);
  const r = spawnSync('node', [join(root, 'scripts', 'run-bench.mjs'), ...(built ? ['--skip-build'] : []), ...steps[name]], { stdio: 'inherit', cwd: root });
  built = true;
  if (r.status !== 0) { console.error(`step ${name} failed (${r.status})`); failed++; }
}
console.log(`\nresults in ${base}`);
process.exit(failed ? 1 : 0);
