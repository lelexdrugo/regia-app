// The whole set of measurements for one machine, one folder each under results/<host>-<date>/: the renderer spike's
// scripts/run-suite.mjs (commit 84871ea), on the product's renderer.
//   bench     what `npm run bench` does: T5 at x1, x10, x100 and x10d, then the sanity check
//   software  the same with the GPU switched off (Windows, Linux; skipped on macOS)
//   stress    x300 and x1000, beyond the spec: where the renderer starts to give way
//   headroom  x100 with every frame drawn 10 times: the cost of drawing that the refresh interval hides
// Usage: npm run bench:suite [-- --only bench,stress] [--host-label name] [--env KEY=VALUE]... [--xvfb]
//        [--reps N] [--phase-ms N] [--warmup-ms N] [--timeout-s N]
// Exit: 0 when every step passed, 1 when one failed (each step is still run).
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const option = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const only = option('only', 'bench,software,stress,headroom').split(',');
const system = { win32: 'windows', darwin: 'macos', linux: 'linux' }[process.platform] ?? process.platform;
const host = option('host-label', system);
const base = join(root, 'results', `${host}-${new Date().toISOString().slice(0, 10)}`);

// The options every step shares, passed on as they are.
const shared = [];
args.forEach((a, i) => {
  if (['--env', '--reps', '--phase-ms', '--warmup-ms', '--timeout-s'].includes(a)) shared.push(a, args[i + 1]);
  if (a === '--xvfb') shared.push(a);
});

const steps = {
  bench: ['--label', `${host}-bench`, '--out', join(base, 'bench')],
  software: ['--software', '--label', `${host}-software`, '--out', join(base, 'software')],
  stress: ['--only', 'main', '--scales', '300,1000', '--no-shots', '--label', `${host}-stress`, '--out', join(base, 'stress')],
  headroom: ['--only', 'main', '--scales', '100', '--repeat', '10', '--no-shots', '--label', `${host}-headroom`, '--out', join(base, 'headroom')],
};
let built = false;
let failed = 0;
for (const name of only) {
  if (!steps[name]) {
    console.error(`unknown step ${name}: bench, software, stress or headroom`);
    process.exit(2);
  }
  if (name === 'software' && process.platform === 'darwin') {
    console.log('skip software on macOS: there is no switch for its GPU');
    continue;
  }
  console.log(`\n=== ${name} ===`);
  const r = spawnSync(process.execPath, [join(root, 'scripts', 'run-bench.mjs'), ...(built ? ['--skip-build'] : []), ...steps[name], ...shared], { stdio: 'inherit', cwd: root });
  built = true;
  if (r.status !== 0) {
    console.error(`step ${name} failed (${r.status})`);
    failed++;
  }
}
console.log(`\nresults in ${base}`);
process.exit(failed ? 1 : 0);
