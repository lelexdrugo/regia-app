// The app's commands, one wrapper each, written by hand over the types ts-rs generates (bindings.ts). A
// command that is not here is not called: the capability allows exactly these (V02).
import { invoke } from "@tauri-apps/api/core";

import type { AppInfo, BriefView, Settings, SettingsChange } from "./bindings";

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("get_app_info");
}

/** Rejects with the core's error, as text. */
export function getBrief(): Promise<BriefView> {
  return invoke<BriefView>("get_brief");
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/** Saves the fields given and answers what is saved; rejects with the reason, as text. */
export function setSettings(settings: SettingsChange): Promise<Settings> {
  return invoke<Settings>("set_settings", { settings });
}
