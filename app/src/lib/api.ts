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

/** How the side bar presents a client (original emblem, never a Blizzard logo). */
export interface ClientSummary {
  id: string;
  name: LocalizedText;
  shortName: LocalizedText;
  version: string;
  build: number;
  emblem: string;
  accent: string;
}

export interface EmulatorOverview {
  id: string;
  name: string;
  description: LocalizedText;
  client: ClientSummary | null;
  /** Server processes in start order (the database is implicit and first). */
  processes: string[];
  modules: ModuleSummary[];
}

export const inDesktopApp = isTauri;

export function catalogOverview(): Promise<EmulatorOverview[]> {
  return invoke<EmulatorOverview[]>("catalog_overview");
}
