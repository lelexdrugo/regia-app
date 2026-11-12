// What the brief screen shows of the core's brief, and nothing it decides: the order, the measures and the due
// marks are the core's (regia-core's brief, due and graph modules). The view only splits the waiting tasks in two
// groups, as the CLI's text does, and keeps each task's place in the core's list.
import type { BriefView, DossierProblem, TaskView } from "./bindings";
import type { Catalog } from "./i18n";

/** The keys of the core's lists that the queue's rows carry, as `app-smoke` reads them back (`data-section`). */
export type SectionKey = "waiting_on_owner" | "ready" | "in_progress";

/** A row of the queue: the task, its list in the core's brief and its place there (`data-rank`). */
export interface Ranked {
  task: TaskView;
  section: SectionKey;
  rank: number;
}

/** A decision taken in the app today: #477 (F34) records them, and this version takes none. */
export interface Done {
  number: number;
  title: string;
  outcome: "approve" | "discard" | "defer" | "option";
  /** The option chosen, for `option`. */
  choice: string | null;
  written: boolean;
}

function ranked(tasks: TaskView[], section: SectionKey): Ranked[] {
  return tasks.map((task, rank) => ({ task, section, rank }));
}

/**
 * The queue's groups. A task that needs a session is in the core's `waiting_on_owner` and in its `session`: it
 * shows in the session group, not twice, and keeps its place in `waiting_on_owner`.
 */
export function groups(brief: BriefView) {
  const waiting = ranked(brief.waiting_on_owner, "waiting_on_owner");
  const session = new Set(brief.session.map((task) => task.number));
  return {
    waiting: waiting.filter((row) => !session.has(row.task.number)),
    session: waiting.filter((row) => session.has(row.task.number)),
    ready: ranked(brief.ready, "ready"),
    inProgress: ranked(brief.in_progress, "in_progress"),
  };
}

/** The finding under the title: how many wait, and the first of the short decisions, else the first session. */
export function finding(brief: BriefView, strings: Catalog): string {
  const { waiting, session } = groups(brief);
  const top = waiting[0] ?? session[0];
  if (!top) {
    return strings.message("brief-finding", { n: 0, top: "", unlocks: 0 });
  }
  return strings.message("brief-finding", { n: brief.waiting_on_owner.length, top: String(top.task.number), unlocks: top.task.unblocks ?? 0 });
}

/**
 * The graph's measure, as the prototype's CLI writes it after each task: "in a cycle", or what the task
 * unblocks with the total next to it when the total is larger, and the chain; nothing when nothing is downstream.
 */
export function measure(task: TaskView, strings: Catalog): string | null {
  if (task.cycle) {
    return strings.message("queue-cycle");
  }
  const unblocks = task.unblocks ?? 0;
  const total = task.unblocks_total ?? unblocks;
  if (total === 0) {
    return null;
  }
  const what =
    total === unblocks
      ? strings.message("queue-unlocks", { n: unblocks })
      : strings.message("queue-unlocks-total", { n: unblocks, total });
  return `${what} · ${strings.message("queue-chain", { n: task.chain ?? 0 })}`;
}

/** The strip: one frame per step of the chain, eight at most, lit for what the answer unblocks at once. */
export function frames(task: TaskView): { lit: boolean[]; more: number } | null {
  const chain = task.chain ?? 0;
  if (task.cycle || measureless(task)) {
    return null;
  }
  const shown = Math.min(chain, 8);
  const lit = Math.min(task.unblocks ?? 0, shown);
  return { lit: Array.from({ length: shown }, (_, index) => index < lit), more: Math.max(0, chain - 8) };
}

function measureless(task: TaskView): boolean {
  const unblocks = task.unblocks ?? 0;
  return (task.unblocks_total ?? unblocks) === 0;
}

/**
 * A day `YYYY-MM-DD` in the catalog's language. Built from its parts, at noon UTC and shown in UTC: a day has no
 * time zone, and `new Date("2026-01-12")` would be the 11th west of Greenwich. Anything else stays as it came.
 */
export function day(value: string, strings: Catalog): string {
  const parts = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!parts) {
    return value;
  }
  const date = new Date(Date.UTC(Number(parts[1]), Number(parts[2]) - 1, Number(parts[3]), 12));
  return new Intl.DateTimeFormat(strings.language, { day: "numeric", month: "short", timeZone: "UTC" }).format(date);
}

/** A moment the controller wrote, in this machine's time, or `null` if it is not one: then it shows as written. */
export function moment(value: string, strings: Catalog): string | null {
  const time = Date.parse(value);
  if (Number.isNaN(time)) {
    return null;
  }
  return new Intl.DateTimeFormat(strings.language, { dateStyle: "medium", timeStyle: "short" }).format(time);
}

/** The time the tracker was read, as the stamp shows it. */
export function clock(time: Date, strings: Catalog): string {
  return new Intl.DateTimeFormat(strings.language, { hour: "2-digit", minute: "2-digit" }).format(time);
}

/** The due mark of a row, or none: a date far away is not marked, as in the CLI's text. */
export function due(task: TaskView, strings: Catalog): { overdue: boolean; text: string } | null {
  if (!task.due || !(task.due_soon || task.overdue)) {
    return null;
  }
  const id = task.overdue ? "queue-overdue" : "queue-due";
  return { overdue: task.overdue, text: strings.message(id, { day: day(task.due, strings) }) };
}

const PROBLEMS: Record<Exclude<DossierProblem, "not_pushed">, string> = {
  folder_missing: "dossier-missing",
  not_a_repo: "dossier-not-repo",
  no_remote: "dossier-noremote",
  no_upstream: "dossier-no-upstream",
  detached_head: "dossier-detached",
  git_failed: "dossier-git-failed",
};

/** What is wrong with a dossier, in the catalog's words. */
export function problem(kind: DossierProblem, ahead: number | null, strings: Catalog): string {
  return kind === "not_pushed" ? strings.message("dossier-unpushed", { n: ahead ?? 0 }) : strings.message(PROBLEMS[kind]);
}

/**
 * The marker of the first brief on screen, for the T2 milestone of the dossier's `docs/app/piano/traguardi.md`:
 * `data-brief` on the brief's screen, `loading` until the core's answer is in the DOM, then `ready` or `failed`.
 */
export type Marker = "loading" | "ready" | "failed";
