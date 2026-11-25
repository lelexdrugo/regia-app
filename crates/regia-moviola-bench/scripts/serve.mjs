// The bench's server, on the loopback for one run: the built page (dist/) under the app's own CSP, with
// `connect-src 'self'` in place of the IPC the page does not have; the run's configuration; and the files the page
// sends back, written under the run's folder only. It stands in for the spike's `bench_save` command (src-tauri of
// commit 84871ea): the host window writes nothing. A token in the page's address keeps any other local page out.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, extname, join, resolve, sep } from 'node:path';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.woff2': 'font/woff2',
};
const LIMIT = 64 * 1024 * 1024;

/** app/src-tauri/tauri.conf.json's policy, with the page's own origin in place of the IPC's. */
export function policy(configuration) {
  const csp = JSON.parse(readFileSync(configuration, 'utf8')).app.security.csp;
  return csp
    .split(';')
    .map((directive) => directive.trim())
    .map((directive) => (directive.startsWith('connect-src ') ? "connect-src 'self'" : directive))
    .join('; ');
}

/** The names the page may write: results.json, error.txt, shots/<name>.png, nothing else and nowhere else. */
export function safeName(name) {
  if (name === 'results.json' || name === 'error.txt') return true;
  const m = /^shots\/([A-Za-z0-9][A-Za-z0-9_+-]*)\.png$/.exec(name);
  return m !== null;
}

/** Serves `root` and receives into `out`; `onExit(code)` is called when the page asks to end the run. */
export function serve({ root, out, config, csp, token, onExit = () => {} }) {
  const base = resolve(root);
  const saved = new Set();
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://127.0.0.1');
    const path = decodeURIComponent(url.pathname);
    const headers = { 'Content-Security-Policy': csp, 'Cache-Control': 'no-store' };
    const allowed = url.searchParams.get('token') === token;
    if (request.method === 'POST') {
      const name = path.slice(1);
      if (!allowed || !safeName(name)) {
        response.writeHead(403, headers).end();
        request.resume();
        return;
      }
      const chunks = [];
      let size = 0;
      request.on('data', (chunk) => {
        size += chunk.length;
        if (size > LIMIT) request.destroy();
        else chunks.push(chunk);
      });
      request.on('end', () => {
        const file = join(out, ...name.split('/'));
        mkdirSync(dirname(file), { recursive: true });
        writeFileSync(file, Buffer.concat(chunks));
        saved.add(name);
        response.writeHead(204, headers).end();
      });
      return;
    }
    if (path === '/config.json') {
      if (!allowed) {
        response.writeHead(403, headers).end();
        return;
      }
      response.writeHead(200, { ...headers, 'Content-Type': TYPES['.json'] }).end(JSON.stringify(config));
      return;
    }
    if (path === '/exit') {
      // The host window ends the run on this address before it loads; this answer is for a host that did not.
      response.writeHead(200, { ...headers, 'Content-Type': TYPES['.html'] }).end('<!doctype html><title>done</title>');
      onExit(Number(url.searchParams.get('code') ?? 1));
      return;
    }
    const file = resolve(base, `.${path === '/' ? '/index.html' : path}`);
    let body;
    try {
      if (!file.startsWith(base + sep)) throw new Error('outside the page');
      body = readFileSync(file);
    } catch {
      response.writeHead(404, headers).end();
      return;
    }
    response.writeHead(200, { ...headers, 'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(body);
  });
  return new Promise((ready) => {
    server.listen(0, '127.0.0.1', () => {
      ready({
        origin: `http://127.0.0.1:${server.address().port}`,
        saved,
        close: () => new Promise((closed) => server.close(closed)),
      });
    });
  });
}
