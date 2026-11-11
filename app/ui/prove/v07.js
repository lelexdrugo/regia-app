// V07 on the DOM, in Chrome, under the app's CSP (threat model §3.7 and V07): the harness renders the core's
// hostile corpus with the app's renderer and judges it with src/lib/v07.ts, the check app-smoke runs on each
// engine. Here two more witnesses watch: the browser reports every request and navigation of the page, and the
// server every path it was asked for. The negative control: the same corpus through a deliberately unsafe
// renderer (harness/main.ts) must fail.
import { open, show } from "./views.js";

export const v07 = {
  browser: true,
  async run({ page, harness, server }) {
    const requests = [];
    const navigations = [];
    page.on("Network.requestWillBeSent", (params) => requests.push(params.request.url));
    page.on("Page.frameRequestedNavigation", (params) => navigations.push(params.url));
    await page.send("Network.enable");
    await open(page, harness, "#/blank?data=none");

    const outcome = {};
    for (const renderer of ["app", "unsafe"]) {
      await show(page, `#/blank?data=${renderer}`);
      const [sent, served, errors] = [requests.length, server.requests.length, page.consoleErrors.length];
      navigations.length = 0;
      await show(page, `#/v07?renderer=${renderer}&lang=en`);
      const checks = (await page.evaluate("window.harnessChecks")) ?? [];
      outcome[renderer] = {
        checks,
        failed: checks.filter((check) => !check.ok),
        requests: [...requests.slice(sent), ...server.requests.slice(served).map((path) => `served ${path}`)],
        navigations: [...navigations],
        errors: page.consoleErrors.slice(errors),
      };
    }

    const { app, unsafe } = outcome;
    const appOk = app.checks.length > 0 && app.failed.length === 0 && app.requests.length === 0 && app.navigations.length === 0 && app.errors.length === 0;
    console.log(`the app's renderer: ${app.checks.length - app.failed.length} of ${app.checks.length} checks passed; requests ${app.requests.length}; navigations ${app.navigations.length}; console errors ${app.errors.length}`);
    for (const check of app.failed) console.log(`  FAIL ${check.name}: ${check.detail}`);
    for (const request of app.requests) console.log(`  request: ${request}`);
    for (const error of app.errors) console.log(`  error: ${error}`);

    const caught = unsafe.failed.length > 0;
    console.log(`negative control, the unsafe renderer: ${caught ? "caught" : "NOT CAUGHT"}, ${unsafe.failed.length} of ${unsafe.checks.length} checks failed; requests ${unsafe.requests.length}; navigations ${unsafe.navigations.length}`);
    for (const check of unsafe.failed.slice(0, 12)) console.log(`  ${check.name}: ${check.detail.slice(0, 150)}`);
    for (const request of [...new Set(unsafe.requests)].slice(0, 5)) console.log(`  request: ${request}`);
    return appOk && caught;
  },
};
