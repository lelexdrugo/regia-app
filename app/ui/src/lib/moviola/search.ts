// The search of a moment (#487, #397/D12): a word, a #number. The prototype viewer has none; the UX prototype's
// (the dossier's docs/app/ux/app/screens-moviola.js, `matches`) wants every word of the query in the moment, and
// lists the moments found newest first, so Enter opens the newest. Here a word is looked for in what the timeline
// holds and the screen shows (the title, the kind's name, the lane's name), without case or accents; a `#N` is a
// task the moment names, or a `#N` in its title. The sidecars' text is not searched: it is read one moment at a
// time. Pure, so the proofs run it in Node.
import type { Model, Node } from "./model";

export type Query = { words: string[]; tasks: number[] };

/** Lower case, without accents: `Perché` and `perche` are the same word. */
export function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/** The words of a query, and its `#N`. An empty query is none. */
export function parseQuery(text: string): Query {
  const words: string[] = [];
  const tasks: number[] = [];
  for (const token of text.trim().split(/\s+/)) {
    if (!token) continue;
    const task = /^#(\d{1,9})$/.exec(token);
    if (task) tasks.push(Number(task[1]));
    else words.push(fold(token));
  }
  return { words, tasks };
}

export const isEmpty = (query: Query): boolean => query.words.length === 0 && query.tasks.length === 0;

/** The names the screen gives a moment's kind and lane, in the language in use. */
export type Names = { kind: (node: Node) => string; lane: (node: Node) => string };

export function matches(node: Node, query: Query, names: Names): boolean {
  const m = node.milestone;
  const hay = fold([m.title, names.kind(node), names.lane(node)].join(" "));
  return (
    query.words.every((word) => hay.includes(word)) &&
    query.tasks.every((task) => m.refs.issue.includes(task) || new RegExp(`#${task}(?!\\w)`).test(m.title))
  );
}

/** The moments found, newest first (two on the same instant: the later in the timeline first). */
export function search(model: Model, query: Query, names: Names): Node[] {
  if (isEmpty(query)) return [];
  return model.nodes.filter((node) => matches(node, query, names)).sort((a, b) => b.t - a.t || b.index - a.index);
}

/** `,` and `.` within the moments found: the one before or after the moment chosen, in time; from none, the
 * first or the last. */
export function stepWithin(found: readonly Node[], selected: number, direction: 1 | -1): Node | null {
  const ordered = [...found].sort((a, b) => a.t - b.t || a.index - b.index);
  if (ordered.length === 0) return null;
  const at = ordered.findIndex((node) => node.index === selected);
  if (at < 0) return (direction > 0 ? ordered[0] : ordered[ordered.length - 1]) ?? null;
  return ordered[Math.max(0, Math.min(ordered.length - 1, at + direction))] ?? null;
}
