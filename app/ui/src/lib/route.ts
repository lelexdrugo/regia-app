// Where the app is: the address after #, and nothing else. The sidebar, the keyboard and a reload agree on it,
// and a query after ? is left to whoever wrote it (the harness's fixtures).

export const SECTIONS = ["brief", "daily", "moviola", "console", "settings"] as const;
export type Section = (typeof SECTIONS)[number];

/** The areas of the settings this version has. */
export const SETTINGS_AREAS = ["general"] as const;
export type SettingsArea = (typeof SETTINGS_AREAS)[number];

export interface Route {
  section: Section;
  /** Only for the settings: the area shown. */
  area: SettingsArea | null;
  /** Only for the moviola: the project whose lanes are shown, or none for the portfolio. */
  project: string | null;
  /** Only for a project's moviola: the moment open at first, or none. */
  moment: string | null;
  /** Only for a project's moviola: the search the address asks for (`?q=`), or none. */
  query: string | null;
}

/** A project's id, or a milestone's, as the app's commands accept one (app/src-tauri/src/moviola.rs); anything else
 * is none. */
const PROJECT = /^[A-Za-z0-9._-]{1,64}$/;

function known<T extends string>(list: readonly T[], value: string | undefined): value is T {
  return value !== undefined && (list as readonly string[]).includes(value);
}

/** `#/settings/general?x=1` is the settings' general area, `#/moviola/alpha` the lanes of the project alpha,
 * `#/moviola/alpha/<id>` the same with that moment open, `#/moviola/alpha?q=probe` with a search. An address the
 * app does not know is the brief. */
export function parse(hash: string): Route {
  const [path = "", search = ""] = hash.replace(/^#\/?/, "").split("?");
  const [first, second, third] = path.split("/");
  const section: Section = known(SECTIONS, first) ? first : "brief";
  const area = section === "settings" ? (known(SETTINGS_AREAS, second) ? second : SETTINGS_AREAS[0]) : null;
  const project = section === "moviola" && second !== undefined && PROJECT.test(second) ? second : null;
  const moment = project !== null && third !== undefined && PROJECT.test(third) ? third : null;
  const query = project !== null ? (new URLSearchParams(search).get("q")?.slice(0, 200) ?? null) : null;
  return { section, area, project, moment, query };
}

export function href(section: Section): string {
  return `#/${section}`;
}

export function projectHref(project: string): string {
  return `#/moviola/${project}`;
}

export function same(a: Route, b: Route): boolean {
  return a.section === b.section && a.area === b.area && a.project === b.project && a.moment === b.moment && a.query === b.query;
}
