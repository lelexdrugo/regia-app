// The list of moments in the page (#490): a window of the list (moments.ts) as the canvas's own children, which a
// browser keeps out of the picture and gives to the accessibility tree, the canvas being a listbox. The canvas keeps
// the focus and names the moment chosen with aria-activedescendant, so a screen reader reads it as the arrows choose
// it and the focus ring stays on the stage. Each option says its place in the whole list (aria-posinset and
// aria-setsize), not in the window; at the zoom of groups each group of the stage is a group of options, named once
// with its moments, its lane, its span, its kinds and its health (#489/V9). The window is rebuilt only when the moment
// chosen, the list or the words change, never at a frame. Words come from the catalog; titles and directions' ids are
// text from others, and sit in data-raw.
import type { ListGroup, Order } from "./moments";
import { windowAt } from "./moments";
import type { Model } from "./model";

/** The words of an entry, said one after the other: a word of the catalog, or text from others (`raw`). */
export type Words = { text: string; raw?: boolean }[];

export type ListText = {
  /** A moment's words, by its model index: kind, title, instant, lane, and its health when it is not healthy. */
  moment: (index: number) => Words;
  /** A group's words, the group `position` of `total` groups of several moments. */
  group: (group: ListGroup, position: number, total: number) => Words;
};

/**
 * How the list is in the page: `window`, the product's; `capped`, the renderer spike's, its first 200 entries only,
 * and `none`, no list at all, as before #490: the harness's negative controls (prove/access.js).
 */
export type ListMode = "window" | "capped" | "none";

/** The spike's cap: its hidden list stopped at this many items (src/a11y.js of commit 84871ea). */
export const SPIKE_CAP = 200;

export class MomentList {
  private readonly host: HTMLElement;
  private readonly model: Model;
  private readonly mode: ListMode;
  private text: ListText;
  private order: Order | null = null;
  private drawn = "";
  /** Windows rebuilt so far, for the proofs and the bench. */
  builds = 0;

  constructor(host: HTMLElement, model: Model, text: ListText, mode: ListMode = "window") {
    this.host = host;
    this.model = model;
    this.text = text;
    this.mode = mode;
  }

  /** The id of a moment's option, by its model index. */
  static id(index: number): string {
    return `moviola-moment-${index}`;
  }

  /** Other words (another language): the window is written again at the next render. */
  setText(text: ListText): void {
    this.text = text;
    this.drawn = "";
  }

  /**
   * The window around the moment chosen (a model index, -1 for none; then around the entry `anchor`). Returns the id
   * of the option of the moment chosen, or "" when the window has none. Nothing is written when nothing changed.
   */
  render(order: Order, chosen: number, anchor: number): string {
    if (this.mode === "none") return "";
    const at = chosen >= 0 ? (order.place[chosen] as number) : -1;
    const total = order.index.length;
    const [from, to] = this.mode === "capped" ? [0, Math.min(SPIKE_CAP, total)] : windowAt(order, at >= 0 ? at : Math.max(0, anchor));
    const key = `${from} ${to} ${chosen}`;
    const active = at >= from && at < to ? MomentList.id(chosen) : "";
    if (order === this.order && key === this.drawn) return active;
    this.order = order;
    this.drawn = key;
    this.builds += 1;
    const out = document.createDocumentFragment();
    let box: Node = out;
    let current = -1;
    for (let e = from; e < to; e++) {
      const index = order.index[e] as number;
      const g = order.groupOf ? (order.groupOf[e] as number) : -1;
      // A group of one is the moment alone, in the stage as in the list.
      const found = g >= 0 && order.groups ? (order.groups[g] as ListGroup) : null;
      const group = found && found.count > 1 ? found : null;
      if (!group) box = out;
      if (group && g !== current) {
        current = g;
        const element = document.createElement("div");
        const label = document.createElement("div");
        label.id = `moviola-group-${group.group}`;
        label.setAttribute("aria-hidden", "true");
        write(label, this.text.group(group, group.ordinal, order.several));
        element.setAttribute("role", "group");
        element.setAttribute("aria-labelledby", label.id);
        element.dataset["group"] = String(group.group);
        element.append(label);
        out.append(element);
        box = element;
      }
      const option = document.createElement("div");
      option.id = MomentList.id(index);
      option.setAttribute("role", "option");
      option.setAttribute("aria-selected", index === chosen ? "true" : "false");
      option.setAttribute("aria-posinset", String(group ? e - group.first + 1 : e + 1));
      option.setAttribute("aria-setsize", String(group ? group.count : total));
      option.dataset["id"] = this.model.nodes[index]?.milestone.id ?? "";
      write(option, this.text.moment(index));
      box.appendChild(option);
    }
    this.host.replaceChildren(out);
    return active;
  }
}

/** The words of an entry into `element`, one span each, with a comma between: text from others in data-raw. */
function write(element: HTMLElement, words: Words): void {
  words.forEach((word, i) => {
    if (i > 0) element.append(", ");
    const span = document.createElement("span");
    span.textContent = word.text;
    if (word.raw) span.setAttribute("data-raw", "");
    element.append(span);
  });
}
