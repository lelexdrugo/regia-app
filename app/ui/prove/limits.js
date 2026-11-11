// The limits of every view: the UX prototype's prove-limits, on the views the app has. Each view and state, in
// it, en and the pseudo-locale, at three window sizes. A problem is what a person would see: content outside the
// window, a control cut by its box, text squeezed into a column, a raw value (undefined, NaN, [object), a
// parameter left in braces, markup that became elements. The negative control: the harness's control page,
// with a too wide element, a raw value and a parameter left in braces.
import { open, show, VIEWS } from "./views.js";

const SIZES = [
  [1440, 900, "1440x900"],
  [1024, 700, "1024x700, the smallest window"],
  [720, 450, "720x450, 1440x900 at 200 %"],
];
const LANGUAGES = ["it", "en", "xx"];

const SCAN = `(() => {
  const found = [];
  const width = innerWidth;
  const name = (element) => element.tagName.toLowerCase() + (element.className ? "." + String(element.className).split(" ")[0] : "");
  const scroller = (element) => {
    for (let parent = element.parentElement; parent; parent = parent.parentElement) {
      if (/(auto|scroll|hidden)/.test(getComputedStyle(parent).overflowX)) return parent;
    }
    return null;
  };
  for (const element of document.querySelectorAll("#root *")) {
    const box = element.getBoundingClientRect();
    if (!box.width || !box.height) continue;
    const parent = scroller(element);
    const unscrolled = !parent || parent === document.body || parent === document.documentElement;
    if ((box.right > width + 1 || box.left < -1) && unscrolled) found.push("outside the window: " + name(element));
    if (parent && getComputedStyle(parent).overflowX === "hidden" && element.matches("button, a, input, select, textarea")) {
      const limit = parent.getBoundingClientRect();
      if (limit.width > 0 && box.right > limit.right + 1) found.push("a control cut by its box: " + name(element));
    }
  }
  for (const element of document.querySelectorAll("#root p, #root span, #root li, #root td, #root th, #root code")) {
    const text = element.textContent.trim();
    if (element.children.length || !text.includes(" ")) continue;
    const box = element.getBoundingClientRect();
    if (!box.width) continue;
    const line = parseFloat(getComputedStyle(element).lineHeight) || 18;
    const words = text.split(/\\s+/).length;
    if (words >= 4 && box.height / line >= words * 0.7 && box.width < 90) found.push("text squeezed into a column: " + text.slice(0, 30));
  }
  const text = document.getElementById("root").innerText;
  for (const raw of ["undefined", "NaN", "[object"]) if (text.includes(raw)) found.push("a raw value on screen: " + raw);
  if (/\\{\\s*\\$?[\\w-]+\\s*\\}/.test(text)) found.push("a parameter left in braces");
  if (document.querySelector("#root script, #root img, #root iframe, #root svg, #root style, #root form")) found.push("markup that became elements");
  return [...new Set(found)];
})()`;

export const limits = {
  browser: true,
  async run({ page, harness }) {
    const errors = page.consoleErrors.length;
    const failures = [];
    let checked = 0;
    await open(page, harness, `${VIEWS[0]}&lang=it`);
    for (const [width, height, label] of SIZES) {
      await page.resize(width, height);
      for (const language of LANGUAGES) {
        for (const view of VIEWS) {
          const route = `${view}&lang=${language}`;
          await show(page, route);
          for (const problem of await page.evaluate(SCAN)) failures.push(`${label} · ${language} · ${view} · ${problem}`);
          checked += 1;
        }
      }
    }

    await page.resize(1440, 900);
    await show(page, "#/brief?data=demo&lang=en");
    const started = Date.now();
    await show(page, "#/brief?data=huge&lang=en");
    const drawn = Date.now() - started;
    const rows = await page.evaluate(`document.querySelectorAll("li[data-number]").length`);

    await page.resize(1024, 700);
    await show(page, "#/control?data=planted&lang=en");
    const control = await page.evaluate(SCAN);
    const planted = ["outside the window", "a raw value on screen: undefined", "a parameter left in braces"];
    const missed = planted.filter((kind) => !control.some((problem) => problem.startsWith(kind)));

    console.log(`combinations checked: ${checked} (${SIZES.length} windows, ${LANGUAGES.length} languages, ${VIEWS.length} views and states)`);
    console.log(`the brief of 1,284 tasks: ${rows} rows drawn in ${drawn} ms`);
    console.log(`problems: ${failures.length}`);
    for (const failure of failures.slice(0, 40)) console.log(`  ${failure}`);
    console.log(`negative control, ${planted.length} planted mistakes: ${missed.length === 0 ? "all caught" : `missed ${JSON.stringify(missed)}`} ${JSON.stringify(control)}`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return failures.length === 0 && missed.length === 0 && newErrors.length === 0 && rows === 1284;
  },
};
