// The colours and faces the Canvas draws with: the design system's tokens (tokens.css), read from the page as the
// direction on <html> sets them. A new direction is a new read and a redraw (stage.ts), never a reload, and no
// colour is written here: a mark the tokens have no colour for (a hue per branch, one per model) is not coloured.

export type Palette = {
  /** The direction the tokens were read in, as `data-dir` on <html> named it. */
  direction: string;
  ground: string;
  surface: string;
  raised: string;
  border: string;
  rule: string;
  lineControl: string;
  text: string;
  muted: string;
  faint: string;
  accent: string;
  positive: string;
  negative: string;
  lane: string;
  laneEdge: string;
  node: string;
  pillBg: string;
  pillInk: string;
  font: string;
  mono: string;
};

const TOKENS = {
  ground: "--ground",
  surface: "--surface",
  raised: "--raised",
  border: "--border",
  rule: "--rule",
  lineControl: "--line-control",
  text: "--text",
  muted: "--muted",
  faint: "--faint",
  accent: "--accent",
  positive: "--positive",
  negative: "--negative",
  lane: "--lane",
  laneEdge: "--lane-edge",
  node: "--node",
  pillBg: "--pill-neutral-bg",
  pillInk: "--pill-neutral-ink",
  font: "--font",
  mono: "--font-mono",
} as const satisfies Record<Exclude<keyof Palette, "direction">, string>;

export function readPalette(root: HTMLElement = document.documentElement): Palette {
  const style = getComputedStyle(root);
  const read = (name: string): string => style.getPropertyValue(name).trim();
  const palette = { direction: root.dataset["dir"] ?? "" } as Palette;
  for (const [key, name] of Object.entries(TOKENS) as [keyof typeof TOKENS, string][]) {
    palette[key] = read(name);
  }
  return palette;
}
