// The moviola's words: the catalog's ids for the core's kinds, health and lane states, the one format of a moment
// (the catalog's date-day-time, #439/D30: the same on every screen), and the order of the portfolio. Pure, over a
// Catalog, so the proofs run it in Node.
import type { Catalog } from "../i18n";
import type { Lane, Model } from "./model";
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

/** The prototype's L0 (`disegnaL0`): the projects with a story first, then the rest, each group in the core's order. */
export function portfolioOrder(projects: readonly ProjectSummary[]): ProjectSummary[] {
  return [...projects].sort((a, b) => Number(a.milestones === 0) - Number(b.milestones === 0));
}

/** The registry's title without the prefix `[id] ` some carry, or the id when there is none. */
export function projectName(project: ProjectSummary): string {
  return (project.title || project.id).replace(/^\[[^\]]+\]\s*/, "");
}
