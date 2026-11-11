// Serves the harness to the proofs under the app's own CSP: the policy of app/src-tauri/tauri.conf.json, with
// `connect-src 'none'` for the IPC the harness does not have. It also keeps every path it was asked for, a
// witness the page cannot hide. Test only.
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
};

/** tauri.conf.json's policy, as the app sends it, without the sources of the IPC. */
export function policy(configuration) {
  const csp = JSON.parse(readFileSync(configuration, "utf8")).app.security.csp;
  return csp
    .split(";")
    .map((directive) => directive.trim())
    .map((directive) => (directive.startsWith("connect-src ") ? "connect-src 'none'" : directive))
    .join("; ");
}

export function serve(root, csp) {
  const base = resolve(root);
  const requests = [];
  const server = createServer((request, response) => {
    const path = decodeURIComponent(new URL(request.url ?? "/", "http://127.0.0.1").pathname);
    requests.push(path);
    const file = resolve(base, `.${path}`);
    const headers = { "Content-Security-Policy": csp, "Cache-Control": "no-store" };
    let body;
    try {
      if (!file.startsWith(base + sep)) {
        throw new Error("outside the harness");
      }
      body = readFileSync(file);
    } catch {
      response.writeHead(404, headers).end();
      return;
    }
    response.writeHead(200, { ...headers, "Content-Type": TYPES[extname(file)] ?? "application/octet-stream" }).end(body);
  });
  return new Promise((ready) => {
    server.listen(0, "127.0.0.1", () => {
      ready({
        origin: `http://127.0.0.1:${server.address().port}`,
        requests,
        close: () => new Promise((closed) => server.close(closed)),
      });
    });
  });
}
