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
}

/** A project's id as the app's commands accept one (app/src-tauri/src/moviola.rs); anything else is no project. */
const PROJECT = /^[A-Za-z0-9._-]{1,64}$/;

function known<T extends string>(list: readonly T[], value: string | undefined): value is T {
  return value !== undefined && (list as readonly string[]).includes(value);
}

/** `#/settings/general?x=1` is the settings' general area, `#/moviola/alpha` the lanes of the project alpha. An
 * address the app does not know is the brief. */
export function parse(hash: string): Route {
  const [path = ""] = hash.replace(/^#\/?/, "").split("?");
  const [first, second] = path.split("/");
  const section: Section = known(SECTIONS, first) ? first : "brief";
  const area = section === "settings" ? (known(SETTINGS_AREAS, second) ? second : SETTINGS_AREAS[0]) : null;
  const project = section === "moviola" && second !== undefined && PROJECT.test(second) ? second : null;
  return { section, area, project };
}

export function href(section: Section): string {
  return `#/${section}`;
}

export function projectHref(project: string): string {
  return `#/moviola/${project}`;
}

export function same(a: Route, b: Route): boolean {
  return a.section === b.section && a.area === b.area && a.project === b.project;
}
