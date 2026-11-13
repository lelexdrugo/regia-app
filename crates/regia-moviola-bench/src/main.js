// The bench's page: it asks the runner (scripts/run-bench.mjs) what to measure, runs the bench (harness.js) on the
// product's renderer, sends back results.json and the screenshots, and ends the run through the host window
// (`/exit?code=N`, which src/main.rs turns into its exit code). The page writes nothing itself.
import '../../../app/ui/src/tokens.css';
import '../../../app/ui/src/app.css';
import './bench.css';

import tauri from '../../../app/src-tauri/tauri.conf.json';
import { catalog } from '../../../app/ui/src/lib/i18n';
import { pageInfo, runBench } from './harness.js';

const query = new URLSearchParams(location.search);
const token = query.get('token') ?? '';
const status = document.getElementById('status');
const say = (text) => {
  status.textContent = text;
};

async function send(name, body, type) {
  const response = await fetch(`/${name}?token=${encodeURIComponent(token)}`, { method: 'POST', body, headers: { 'Content-Type': type } });
  if (!response.ok) throw new Error(`sending ${name}: ${response.status}`);
}

async function main() {
  const response = await fetch(`/config.json?token=${encodeURIComponent(token)}`);
  if (!response.ok) throw new Error(`no configuration from the runner (${response.status}): start the bench with npm run bench`);
  const cfg = await response.json();
  // The canvas draws with the design system's faces: loaded before the first frame that is measured.
  await Promise.all(['12px "Schibsted Grotesk"', '600 12px "Schibsted Grotesk"', '12px "IBM Plex Mono"'].map((face) => document.fonts.load(face)));
  const strings = catalog('en', tauri.productName);
  const started = new Date();
  const result = await runBench(cfg, strings, document.getElementById('root'), say);
  const doc = {
    schema: 'moviola-bench/v2',
    started: started.toISOString(),
    wall_s: (Date.now() - started.getTime()) / 1000,
    config: cfg,
    page: pageInfo(query),
    view: result.view,
    blocks: result.blocks,
  };
  await send('results.json', JSON.stringify(doc), 'application/json');
  for (const s of result.shots) if (s.blob) await send(`shots/${s.name}.png`, s.blob, 'image/png');
  say('done');
  location.href = '/exit?code=0';
}

main().catch(async (e) => {
  say(`failed: ${e?.message || e}`);
  try {
    await send('error.txt', String(e?.stack || e), 'text/plain');
  } catch {
    // the runner reports the missing results
  }
  location.href = '/exit?code=1';
});
