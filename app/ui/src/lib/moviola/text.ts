// The moviola's words: the catalog's ids for the core's kinds, health and lane states, the one format of a moment
// (the catalog's date-day-time, #439/D30: the same on every screen), and the order of the portfolio. Pure, over a
// Catalog, so the proofs run it in Node.
import type { Catalog } from "../i18n";
import type { ListText, Words } from "./listbox";
import type { Lane, Model } from "./model";
import { KINDS } from "./scene";
import type { DirectionStatus, Health, MilestoneKind, ProjectSummary } from "./types";

/** The core's kinds by their names in the catalog, which kept the UX prototype's. */
export const KIND_ID: Record<MilestoneKind, string> = {
  milestone: "type-milestone",
  decision: "type-decision",
  gate: "type-gate",
  exception: "type-exception",
  exploration: "type-exploration",
  rejoin: "type-return",
  abandonment: "type-abandoned",
};

export const HEALTH_ID: Record<Health, string> = {
  healthy: "health-healthy",
  at_risk: "health-at-risk",
  waiting_on_owner: "health-waiting",
};

export const LANE_ID: Record<DirectionStatus, string> = {
  active: "moviola-lane-active",
  abandoned: "moviola-lane-abandoned",
  rejoined: "moviola-lane-rejoined",
};

/** A moment as every screen writes it, in this machine's time zone. */
export function timecode(strings: Catalog, t: number): string {
  return strings.message("date-day-time", { when: new Date(t) });
}

export function day(strings: Catalog, t: number): string {
  return strings.message("date-day", { when: new Date(t) });
}

/** A day of this machine's calendar, for the axis: the first milestone of each day carries its label. */
export function dayKey(t: number): string {
  const date = new Date(t);
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
}

/** A lane's name: the main lane is a word of the catalog, the others their direction's id, as the prototype. */
export function laneName(strings: Catalog, model: Model, lane: Lane): string {
  return lane.id === model.timeline.main_direction ? strings.message("moviola-lane-main") : lane.id;
}

/** The words of the list of moments (#490, listbox.ts): a moment's kind, title, instant, lane, and its health when it
 * is not healthy; a group's place among the groups of several, its moments, lane, span, kinds and health (#489/V9).
 * Titles and directions' ids are text from others. */
export function listText(strings: Catalog, model: Model): ListText {
  const laneWords = (index: number): Words[number] => {
    const lane = model.lanes[index];
    return !lane || lane.id === model.timeline.main_direction ? { text: strings.message("moviola-lane-main") } : { text: lane.id, raw: true };
  };
  return {
    moment: (index) => {
      const node = model.nodes[index] as Model["nodes"][number];
      const m = node.milestone;
      const said: Words = [{ text: strings.message(KIND_ID[m.kind]) }, { text: m.title, raw: true }, { text: timecode(strings, node.t) }, laneWords(node.lane)];
      if (m.health !== "healthy") said.push({ text: strings.message(HEALTH_ID[m.health]) });
      return said;
    },
    group: (group, position, total) => {
      const span = group.from === group.to ? timecode(strings, group.from) : strings.message("moviola-list-span", { from: timecode(strings, group.from), to: timecode(strings, group.to) });
      const said: Words = [
        { text: strings.message("moviola-list-group", { position, total }) },
        { text: strings.message("moviola-list-moments", { n: group.count }) },
        laneWords(group.lane),
        { text: span },
      ];
      KINDS.forEach((kind, k) => {
        const n = group.kinds[k] ?? 0;
        if (n > 0) said.push({ text: strings.message("moviola-list-kind", { kind: strings.message(KIND_ID[kind]), n }) });
      });
      if (group.atRisk > 0) said.push({ text: strings.message("moviola-list-at-risk", { n: group.atRisk }) });
      if (group.waiting > 0) said.push({ text: strings.message("moviola-list-waiting", { n: group.waiting }) });
      return said;
    },
  };
}

/** The prototype's L0 (`disegnaL0`): the projects with a story first, then the rest, each group in the core's order. */
export function portfolioOrder(projects: readonly ProjectSummary[]): ProjectSummary[] {
  return [...projects].sort((a, b) => Number(a.milestones === 0) - Number(b.milestones === 0));
}

/** The registry's title without the prefix `[id] ` some carry, or the id when there is none. */
export function projectName(project: ProjectSummary): string {
  return (project.title || project.id).replace(/^\[[^\]]+\]\s*/, "");
}
