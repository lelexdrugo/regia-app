// «Copy for chat» (#487, CONTRATTO §8 of the dossier): the prototype viewer's `pacchetto` and `accorcia`. Five
// lines under 1,500 characters, pointers only: the moment, what it is about, its references, the question to
// complete, the instruction to read the references first. When they do not fit, the summary is shortened first,
// then the references are dropped from the last, whole (never half an address), with a line that counts them.
// The words come from the catalog; the proofs pass the prototype's own and compare the text byte for byte
// (prove/moviola.js). The prototype cites its server's page of the snapshot; here it is the sidecar's path in the
// dossier, as #485 plans for the core's `copy_context_pack`.
import type { Links, Snapshot } from "./types";

export const PACK_LIMIT = 1500;

/** `accorcia`: at most `n` characters, cut at a whole word (never in the middle) and closed with «…». */
export function shorten(text: string | null | undefined, n: number): string {
  const t = String(text || "").replace(/\s+/g, " ").trim();
  if (t.length <= n) return t;
  const cut = t.slice(0, n - 1);
  const space = cut.lastIndexOf(" ");
  return (space > n / 2 ? cut.slice(0, space) : cut).replace(/[\s,;:.]+$/, "") + "…";
}

/** The pack's fixed words, with their arguments: the catalog's in the app, the prototype's in the proofs. */
export type PackWords = {
  head: (args: { project: string; id: string; kind: string; day: string }) => string;
  looking: (what: string) => string;
  references: (list: string) => string;
  none: string;
  more: (n: number) => string;
  question: string;
  skill: string;
};

export type PackMoment = { project: string; id: string; kind: string; day: string; title: string };

/** The references of a moment, in the prototype's order: issues, commits, reports (the path where the file is here,
 * the text where not), then the sidecar. */
export function packReferences(links: Links, sidecar: string | null): string[] {
  return [
    ...links.issue.map((x) => x.url || x.text),
    ...links.commit.map((x) => x.url || x.text),
    ...links.report.map((x) => (x.exists === false ? x.text : x.path || x.text)),
    ...(sidecar ? [sidecar] : []),
  ];
}

/** The summary the pack carries: `in_breve`, or the why, or the first thing that changed. */
export function packSummary(snapshot: Snapshot | null): string {
  return snapshot?.in_brief || snapshot?.direction || snapshot?.changed[0] || "";
}

/** `pacchetto`. */
export function pack(moment: PackMoment, summary: string, refs: readonly string[], words: PackWords): string {
  const build = (title: string, sum: string, n: number): string => {
    const items = refs.slice(0, n);
    if (n < refs.length) items.push(words.more(refs.length - n));
    const seen = title && !/[.!?…]$/.test(title) ? `${title}.` : title;
    return [
      words.head(moment),
      words.looking(`${seen}${sum ? ` ${sum}` : ""}`),
      words.references(items.length ? items.join(", ") : words.none),
      words.question,
      words.skill,
    ].join("\n");
  };
  for (const [nt, ns] of [
    [200, 260],
    [200, 160],
    [200, 100],
    [120, 0],
  ] as const) {
    const title = shorten(moment.title, nt);
    const sum = shorten(summary, ns);
    for (let n = refs.length; n >= 0; n--) {
      const text = build(title, sum, n);
      if (text.length <= PACK_LIMIT) return text;
      // The summary is shortened first, then the references go.
      if (n === refs.length && ns > 100) break;
    }
  }
  return build(shorten(moment.title, 60), "", 0);
}

/** The sidecar's path in the dossier, which the pack cites in place of the prototype's page. */
export const sidecarPath = (id: string): string => `.regia/moviola/snapshot/${id}.json`;

/** The moment's day, `YYYY-MM-DD`, in this machine's time zone (the prototype's is Europe/Rome's). */
export function localDay(t: number): string {
  const date = new Date(t);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())}`;
}
