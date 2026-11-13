// Typed wrappers around Tauri commands. Keep in sync with app/src-tauri/src/commands.rs.

import { invoke, isTauri } from "@tauri-apps/api/core";
import type { LocalizedText } from "./i18n/i18n.svelte";

export type ModuleCategory =
  | "bots"
  | "economy"
  | "progression"
  | "gameplay"
  | "quality-of-life"
  | "cosmetic"
  | "administration";

export interface ModuleSummary {
  id: string;
  name: LocalizedText;
  description: LocalizedText;
  category: ModuleCategory;
  homepage: string;
  mandatory: boolean;
}

export interface EmulatorOverview {
  id: string;
  name: string;
  description: LocalizedText;
  modules: ModuleSummary[];
}

export const inDesktopApp = isTauri;

export function catalogOverview(): Promise<EmulatorOverview[]> {
  return invoke<EmulatorOverview[]>("catalog_overview");
}
