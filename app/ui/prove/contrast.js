// WCAG contrast of the colour tokens (src/tokens.css), in each of the design system's three directions, for
// every pair the design uses: the UX prototype's prove-contrast, on the app's copy of the tokens. Text needs
// 4.5:1; a mark that carries meaning but is not text needs 3:1 (WCAG 1.4.11). The negative control: the
// faintest text of the default direction made fainter still must fall under the threshold.
import { readFileSync } from "node:fs";
import { join } from "node:path";

const DIRECTIONS = ["montaggio", "foglio", "nero"];

// [foreground, background, need, use]
const PAIRS = [
  ["text", "ground", 4.5, "text on the ground"],
  ["text", "surface", 4.5, "text on a panel"],
  ["text", "raised", 4.5, "text on an option"],
  ["text", "active", 4.5, "text on the chosen row"],
  ["muted", "ground", 4.5, "secondary text on the ground"],
  ["muted", "surface", 4.5, "secondary text on a panel"],
  ["muted", "raised", 4.5, "secondary text on an option"],
  ["muted", "active", 4.5, "secondary text on the chosen row"],
  ["faint", "ground", 4.5, "metadata on the ground"],
  ["faint", "surface", 4.5, "metadata on a panel"],
  ["faint", "raised", 4.5, "metadata on an option"],
  ["faint", "active", 4.5, "metadata on the chosen row"],
  ["accent", "ground", 4.5, "timecodes and counts on the ground"],
  ["accent", "surface", 4.5, "timecodes on a panel"],
  ["accent-ink", "accent", 4.5, "text on the accent (selection)"],
  ["primary-ink", "primary", 4.5, "the main action"],
  ["positive", "ground", 4.5, "a positive outcome"],
  ["negative", "ground", 4.5, "a negative outcome on the ground"],
  ["negative", "surface", 4.5, "a negative outcome on a panel"],
  ["pill-accent-ink", "pill-accent-bg", 4.5, "the Decision label"],
  ["pill-positive-ink", "pill-positive-bg", 4.5, "a positive label"],
  ["pill-negative-ink", "pill-negative-bg", 4.5, "a negative label"],
  ["pill-neutral-ink", "pill-neutral-bg", 4.5, "a neutral label"],
  ["line-control", "surface", 3, "the border of controls (not text)"],
  ["line-control", "ground", 3, "the border of fields (not text)"],
  ["accent", "active", 3, "the selection line (not text)"],
  ["node", "surface", 3, "the timeline's marks (not text)"],
  ["node", "lane", 3, "a mark on a lane (not text)"],
  ["accent", "lane", 3, "a gate on a lane (not text)"],
  ["negative", "lane", 3, "an exception on a lane (not text)"],
];

function tokensOf(css, direction) {
  const body = css.match(new RegExp(`\\[data-dir="${direction}"\\]\\s*\\{([^}]*)\\}`))?.[1] ?? "";
  return Object.fromEntries([...body.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})/g)].map((match) => [match[1], match[2]]));
}

function luminance(hex) {
  const [r, g, b] = [1, 3, 5]
    .map((at) => parseInt(hex.slice(at, at + 2), 16) / 255)
    .map((value) => (value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function ratio(a, b) {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}

/** Each pair's ratio in each direction; a token a direction lacks counts as a failure. */
function measure(tokens) {
  return PAIRS.map(([foreground, background, need, use]) => ({
    pair: `${foreground} / ${background}`,
    need,
    use,
    ratios: DIRECTIONS.map((direction) => {
      const [fg, bg] = [tokens[direction][foreground], tokens[direction][background]];
      return fg && bg ? ratio(fg, bg) : Number.NaN;
    }),
  }));
}

const under = (rows) => rows.flatMap((row) => row.ratios.filter((value) => !(value >= row.need)));

export const contrast = {
  browser: false,
  async run({ ui }) {
    const css = readFileSync(join(ui, "src/tokens.css"), "utf8");
    const tokens = Object.fromEntries(DIRECTIONS.map((direction) => [direction, tokensOf(css, direction)]));
    const rows = measure(tokens);
    console.log(`${"pair".padEnd(34)} need ${DIRECTIONS.map((direction) => direction.padEnd(10)).join("")}use`);
    for (const row of rows) {
      const cells = row.ratios.map((value) => `${Number.isNaN(value) ? "missing" : value.toFixed(2)}${value >= row.need ? "" : " NO"}`);
      console.log(`${row.pair.padEnd(34)} ${String(row.need).padEnd(4)} ${cells.map((cell) => cell.padEnd(10)).join("")}${row.use}`);
    }
    const failed = under(rows).length;
    console.log(`pairs: ${rows.length} in ${DIRECTIONS.length} directions; under the threshold: ${failed}`);

    const altered = structuredClone(tokens);
    altered.montaggio.faint = "#5b544a";
    const caught = under(measure(altered)).length;
    console.log(`negative control, montaggio's --faint at #5b544a: ${caught} under the threshold (${caught > 0 ? "caught" : "NOT CAUGHT"})`);
    return failed === 0 && caught > 0;
  },
};
