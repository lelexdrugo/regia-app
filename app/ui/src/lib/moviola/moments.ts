// The list of moments (#490): what the stage shows, as a screen reader and the keyboard reach it. The renderer
// spike kept a hidden list of the moments in view and stopped it at 200 items (the dossier's
// docs/app/moviola/risultati.md, C4 of its review): at x10 dense 264 were in view, at x100 2,700. Here the list holds
// every moment the filters keep, and only a window of it is in the page (listbox.ts), so its cost does not grow with
// the story. Its order is the order the arrows walk. At the zoom of detail it is time's, the model's. At the zoom of
// groups (lod.ts) it is the groups', each group's moments together, so a group is one stretch of the list and a
// screen reader can name it once, on entering it (#489/V9). Pure, and types are all it imports, so the proofs' Node
// runs it as it is.
import type { Groups } from "./lod";
import type { Model } from "./model";
import type { Scene } from "./scene";

/** How many entries of the list are in the page at once: the one chosen, and about as many on each side. */
export const WINDOW = 31;
/** PageUp and PageDown at the zoom of detail: this many moments back or on. At the zoom of groups, a group. */
export const PAGE = 10;

/** A group of the list, at the zoom of groups: a lod.ts group, as many of its moments as the filters keep. */
export type ListGroup = {
  /** The lod.ts group. */
  group: number;
  lane: number;
  /** Its first entry in the list, and how many entries it has. */
  first: number;
  count: number;
  /** The first and the last of its moments' instants. */
  from: number;
  to: number;
  /** How many of its moments have each kind (scene.ts's KINDS), are at risk, or wait for the owner. */
  kinds: number[];
  atRisk: number;
  waiting: number;
  /** Its place among the groups of several moments, from 1; 0 for a group of one, which the stage draws as the
   * moment's own glyph and the list as the moment alone. */
  ordinal: number;
};

export type Order = {
  /** Model indices of the moments the filters keep, in the list's order. */
  index: Int32Array;
  /** Each model index's place in `index`, -1 for a moment the filters leave out. */
  place: Int32Array;
  /** At the zoom of groups, the groups that keep a moment, in the list's order, and each entry's group among them;
   * `null` at the zoom of detail. */
  groups: ListGroup[] | null;
  groupOf: Int32Array | null;
  /** How many of those groups hold several moments. */
  several: number;
};

/** The list's order (`kept`: the moments the filters keep, `null` for all; `groups`: the stage's groups, `null` at
 * the zoom of detail). `scene.node` gives the model index of each scene node, `scene.kind` its kind. */
export function listOrder(model: Model, scene: Pick<Scene, "node" | "kind">, kept: readonly boolean[] | null, groups: Groups | null): Order {
  const n = model.nodes.length;
  const place = new Int32Array(n).fill(-1);
  const keeps = (index: number): boolean => kept === null || kept[index] === true;
  const out: number[] = [];
  if (!groups) {
    for (let index = 0; index < n; index++) {
      if (!keeps(index)) continue;
      place[index] = out.length;
      out.push(index);
    }
    return { index: Int32Array.from(out), place, groups: null, groupOf: null, several: 0 };
  }
  const list: ListGroup[] = [];
  const of: number[] = [];
  for (let g = 0; g < groups.n; g++) {
    let entry: ListGroup | null = null;
    for (let m = groups.start[g] as number; m < (groups.start[g + 1] as number); m++) {
      const k = groups.members[m] as number;
      const index = scene.node[k] as number;
      const node = model.nodes[index];
      if (!node || !keeps(index)) continue;
      if (!entry) {
        entry = { group: g, lane: groups.lane[g] as number, first: out.length, count: 0, from: node.t, to: node.t, kinds: [0, 0, 0, 0, 0, 0, 0], atRisk: 0, waiting: 0, ordinal: 0 };
        list.push(entry);
      }
      place[index] = out.length;
      out.push(index);
      of.push(list.length - 1);
      entry.count += 1;
      entry.from = Math.min(entry.from, node.t);
      entry.to = Math.max(entry.to, node.t);
      const kind = scene.kind[k] as number;
      entry.kinds[kind] = (entry.kinds[kind] as number) + 1;
      if (node.milestone.health === "at_risk") entry.atRisk += 1;
      else if (node.milestone.health === "waiting_on_owner") entry.waiting += 1;
    }
  }
  let several = 0;
  for (const entry of list) if (entry.count > 1) entry.ordinal = ++several;
  return { index: Int32Array.from(out), place, groups: list, groupOf: Int32Array.from(of), several };
}

/** The entries in the page: `WINDOW` of them around `at`, an entry's place, fewer when the list is shorter. */
export function windowAt(order: Order, at: number, size = WINDOW): [number, number] {
  const total = order.index.length;
  if (total <= size) return [0, total];
  const from = Math.min(Math.max(0, at - (size >> 1)), total - size);
  return [from, from + size];
}

/** The entry an arrow or a page key leads to from `at` (-1: none chosen yet, so `start` is where the walk begins):
 * `step` entries on or back; with `group`, at the zoom of groups, the first entry of the next or the previous group
 * (from the middle of a group, back is the start of that group). The ends hold. */
export function nextEntry(order: Order, at: number, step: number, group = false, start = 0): number {
  const total = order.index.length;
  if (total === 0) return -1;
  if (at < 0) return Math.min(total - 1, Math.max(0, start));
  if (group && order.groups && order.groupOf) {
    const g = order.groupOf[at] as number;
    const first = (order.groups[g] as ListGroup).first;
    if (step > 0) return g + 1 < order.groups.length ? (order.groups[g + 1] as ListGroup).first : first;
    if (at > first) return first;
    return g > 0 ? (order.groups[g - 1] as ListGroup).first : first;
  }
  return Math.min(total - 1, Math.max(0, at + step));
}
