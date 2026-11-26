// The colours and faces the Canvas draws with: the design system's tokens (tokens.css), read from the page as the
// direction on <html> sets them. A new direction is a new read and a redraw (stage.ts), never a reload, and no
// colour is written here: a mark the tokens have no colour for (a hue per branch, one per model) is not coloured.
//
// Forced colours (#490): under a contrast theme of the system (Windows' contrast themes, `forced-colors: active`) the
// browser repaints the page in the system's few colours, but neither the tokens, which are custom properties, nor a
// canvas's pixels. So the palette is then the system's colours, each resolved on a probe element that keeps its own
// colour: the grounds Canvas, the ink CanvasText, the rules and the quiet marks GrayText, the selection, the
// playhead and the accent Highlight. Kinds stay apart by their glyphs, and health by its rings, not by hue.

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

type Colour = Exclude<keyof Palette, "direction" | "font" | "mono">;

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

/** The media query of the system's forced colours. */
export const FORCED_COLORS = "(forced-colors: active)";

/** Whether the page is under the system's forced colours now: then `readPalette` gives the system's colours. */
export const forcedColors = (): boolean => typeof matchMedia === "function" && matchMedia(FORCED_COLORS).matches;

/** Each colour of the palette under forced colours, as the system names it (CSS Color 4, system colours). A lane's band
 * is the ground, and its edge a rule: the lanes are drawn by their edges. */
export const SYSTEM: Record<Colour, string> = {
  ground: "Canvas",
  surface: "Canvas",
  raised: "Canvas",
  lane: "Canvas",
  pillBg: "Canvas",
  text: "CanvasText",
  node: "CanvasText",
  lineControl: "CanvasText",
  pillInk: "CanvasText",
  positive: "CanvasText",
  negative: "CanvasText",
  border: "GrayText",
  rule: "GrayText",
  laneEdge: "GrayText",
  muted: "GrayText",
  faint: "GrayText",
  accent: "Highlight",
};

/** The system colours, as the page resolves them now: a probe that keeps its own colour (`forced-color-adjust:
 * none`), read once per name. */
function systemColours(root: HTMLElement): Map<string, string> {
  const probe = document.createElement("span");
  probe.setAttribute("aria-hidden", "true");
  probe.style.cssText = "position:absolute;width:0;height:0;overflow:hidden;forced-color-adjust:none";
  root.append(probe);
  const out = new Map<string, string>();
  for (const name of new Set(Object.values(SYSTEM))) {
    probe.style.color = name;
    out.set(name, getComputedStyle(probe).color);
  }
  probe.remove();
  return out;
}

export function readPalette(root: HTMLElement = document.documentElement): Palette {
  const style = getComputedStyle(root);
  const read = (name: string): string => style.getPropertyValue(name).trim();
  const palette = { direction: root.dataset["dir"] ?? "" } as Palette;
  for (const [key, name] of Object.entries(TOKENS) as [keyof typeof TOKENS, string][]) {
    palette[key] = read(name);
  }
  if (forcedColors()) {
    const system = systemColours(root);
    for (const [key, name] of Object.entries(SYSTEM) as [Colour, string][]) {
      palette[key] = system.get(name) ?? palette[key];
    }
  }
  return palette;
}
