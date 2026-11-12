// The app's commands, one wrapper each, written by hand over the types ts-rs generates (bindings.ts). A
// command that is not here is not called: the capability allows exactly these (V02).
import { invoke } from "@tauri-apps/api/core";

import type { AppInfo, BriefView, Settings, SettingsChange } from "./bindings";
import type { ProjectSummary, Snapshot, Timeline } from "./moviola/types";

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("get_app_info");
}

/** Rejects with the core's error, as text. */
export function getBrief(): Promise<BriefView> {
  return invoke<BriefView>("get_brief");
}

/** The moviola's portfolio (L0): the projects with a folder here, by id. Rejects with the core's error, as text. */
export function listProjects(): Promise<ProjectSummary[]> {
  return invoke<ProjectSummary[]>("list_projects");
}

/** A project's timeline, its sidecars left out; `from` included and `to` excluded, each a date or an instant. The
 * moviola's commands take their arguments in snake_case. */
export function getTimeline(projectId: string, from: string | null = null, to: string | null = null): Promise<Timeline> {
  return invoke<Timeline>("get_timeline", { project_id: projectId, from, to });
}

/** A milestone's sidecar, `null` when the project has no such milestone or the milestone no sidecar. */
export function getSnapshot(projectId: string, milestoneId: string): Promise<Snapshot | null> {
  return invoke<Snapshot | null>("get_snapshot", { project_id: projectId, milestone_id: milestoneId });
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/** Saves the fields given and answers what is saved; rejects with the reason, as text. */
export function setSettings(settings: SettingsChange): Promise<Settings> {
  return invoke<Settings>("set_settings", { settings });
}
