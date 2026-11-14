// Typed wrappers around Tauri commands. Keep in sync with app/src-tauri/src/commands.rs.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
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

/** Failure returned by a command: `code` maps to the `error.<code>` message. */
export interface CommandError {
  code: string;
  detail: string;
}

/** Normalises anything a command may reject with into a CommandError. */
export function asCommandError(error: unknown): CommandError {
  if (typeof error === "object" && error !== null && "code" in error && "detail" in error) {
    return { code: String(error.code), detail: String(error.detail) };
  }
  return { code: "unknown", detail: String(error) };
}

export const inDesktopApp = isTauri;

/** Opens a web page in the default browser (allowed URLs: capabilities/default.json). */
export function openExternal(url: string): Promise<void> {
  return openUrl(url);
}

export function catalogOverview(): Promise<EmulatorOverview[]> {
  return invoke<EmulatorOverview[]>("catalog_overview");
}

/** Window chrome: the launcher draws its own title bar (decorations off). */
export const appWindow = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
  isMaximized: () => getCurrentWindow().isMaximized(),
  /** Calls `handler` after every resize (maximise/restore included); returns an unsubscribe. */
  onResized: (handler: () => void) => getCurrentWindow().onResized(handler),
};
