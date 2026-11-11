// How the app looks: the design system's direction and the density, on <html> as data-dir and data-density,
// which tokens.css reads. The app's index.html arrives with the saved ones already there (settings.rs in
// app/src-tauri), so the first paint is in them; a change is drawn here at once, then saved.
import type { Density, Direction } from "./bindings";

export const DIRECTIONS: readonly Direction[] = ["montaggio", "foglio", "nero"];
export const DENSITIES: readonly Density[] = ["compact", "comfortable"];

export interface Look {
  direction: Direction;
  density: Density;
}

/** What the last save of a setting said. */
export type Saving = { kind: "idle" } | { kind: "saved" } | { kind: "failed"; reason: string };

export function draw(look: Look): void {
  document.documentElement.dataset["dir"] = look.direction;
  document.documentElement.dataset["density"] = look.density;
}
