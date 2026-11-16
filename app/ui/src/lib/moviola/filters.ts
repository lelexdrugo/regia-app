// The moviola's filters (#598): the prototype viewer's `visibileFiltri` (the dossier's tools/moviola/static/moviola.js)
// with its two granularities, over the core's timeline. Basic hides kinds; advanced also keeps only who worked (an
// agent and a model, a report's citation, or none declared), one lane, one health, or the moments with a check or a
// count of defects declared. A moment filtered out stays where it is on the stage, almost gone, so the structure does
// not move. Pure, with types alone imported, so the proofs run it in Node against the prototype's answers.
import type { Model, Node } from "./model";
import type { Health, MilestoneKind, ModelUse } from "./types";

/** The seven kinds, in the prototype's order (scene.ts's KINDS: a value from there would not load in Node). */
export const ALL_KINDS: readonly MilestoneKind[] = ["milestone", "decision", "gate", "exception", "exploration", "rejoin", "abandonment"];

export type Mode = "basic" | "advanced";

/** «Who» as the filter knows a model's use: one value per agent, model and citation, "none" for none declared. */
export const NONE = "none";

export type Filters = {
  /** Chosen, or `null`: basic, unless an advanced filter is on (`modoFiltri`). */
  mode: Mode | null;
  /** The kinds hidden. */
  hidden: readonly MilestoneKind[];
  /** "" for anyone, `NONE`, or a `whoKey`. */
  who: string;
  /** "" for every lane, or a lane's id. */
  lane: string;
  /** "" for every health. */
  health: Health | "";
  /** Only the moments with a check or a count of defects declared. */
  declared: boolean;
};

export const NO_FILTERS: Filters = { mode: null, hidden: [], who: "", lane: "", health: "", declared: false };

/** `avanzatiAttivi`. */
export function advancedOn(filters: Filters): boolean {
  return filters.who !== "" || filters.lane !== "" || filters.health !== "" || filters.declared;
}

/** `modoFiltri`, for the lanes: basic until a choice or an advanced filter says otherwise. */
export function modeOf(filters: Filters): Mode {
  return filters.mode ?? (advancedOn(filters) ? "advanced" : "basic");
}

/** Something is filtered out, or could be: what makes «Clear the filters» and the count appear (`disegnaAzzera`). */
export function filtering(filters: Filters): boolean {
  return filters.hidden.length > 0 || advancedOn(filters);
}

/** Back to basic: the advanced filters go with it, so none stays on out of sight. */
export function basic(filters: Filters): Filters {
  return { ...filters, mode: "basic", who: "", lane: "", health: "", declared: false };
}

/** A model's use as the filter tells it apart: the prototype's label (`etichettaModello`) without its words, an
 * empty field for one not declared. */
export function whoKey(use: Pick<ModelUse, "agent" | "model" | "via">): string {
  return JSON.stringify([use.agent || "", use.model || "", use.via === "report"]);
}

/** The parts of a `whoKey`, for the screen to say. */
export function whoParts(key: string): { agent: string | null; model: string | null; cited: boolean } {
  const [agent, model, cited] = JSON.parse(key) as [string, string, boolean];
  return { agent: agent || null, model: model || null, cited };
}

/** Who worked on a moment: the uses of the aggregates of the tasks it names, or `NONE` (`m.modelli`). */
export function whoOf(node: Node): string[] {
  const keys = new Set<string>();
  for (const { aggregate } of node.work) for (const use of aggregate?.models ?? []) keys.add(whoKey(use));
  return keys.size ? [...keys] : [NONE];
}

/** Every «who» of the project, `NONE` last (`P.modelli`). */
export function whoOptions(model: Model): string[] {
  const keys = new Set(model.nodes.flatMap(whoOf));
  const none = keys.delete(NONE);
  return [...[...keys].sort(), ...(none ? [NONE] : [])];
}

/** `visibileFiltri`: the moment passes every filter on. */
export function visible(node: Node, filters: Filters): boolean {
  const m = node.milestone;
  if (filters.hidden.includes(m.kind)) return false;
  if (filters.who !== "" && !whoOf(node).includes(filters.who)) return false;
  if (filters.lane !== "" && m.direction !== filters.lane) return false;
  if (filters.health !== "" && m.health !== filters.health) return false;
  if (filters.declared && !node.declared) return false;
  return true;
}

/** The filters an address asks for: `filters=advanced|basic`, `hide=<kind>,…`; anything else is left out. */
export function fromAddress(mode: string | null, hide: string | null): Filters {
  const hidden = (hide ?? "").split(",").filter((kind): kind is MilestoneKind => (ALL_KINDS as readonly string[]).includes(kind));
  return { ...NO_FILTERS, mode: mode === "advanced" || mode === "basic" ? mode : null, hidden };
}
