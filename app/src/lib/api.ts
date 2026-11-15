// Typed wrappers around Tauri commands and events. Keep in sync with
// app/src-tauri/src/commands.rs (and the serialized types of eoa-launcher).

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
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
  /** The process whose input is the server console, if any. */
  console: string | null;
  modules: ModuleSummary[];
}

/** Failure returned by a command: `code` maps to the `error.<code>` message. */
export interface CommandError {
  code: string;
  params: Record<string, string>;
  detail: string;
}

/** Normalises anything a command may reject with into a CommandError. */
export function asCommandError(error: unknown): CommandError {
  if (typeof error === "object" && error !== null && "code" in error && "detail" in error) {
    const params =
      "params" in error && typeof error.params === "object" && error.params !== null
        ? (error.params as Record<string, string>)
        : {};
    return { code: String(error.code), params, detail: String(error.detail) };
  }
  return { code: "unknown", params: {}, detail: String(error) };
}

export type OptionValue = boolean | number | string;

export interface InstanceRecord {
  format: number;
  id: string;
  emulator: string;
  flavor: string;
  modules: string[];
  network: "solo" | "lan";
  ports: Record<string, number>;
  dbPort: number;
  created: number;
  regenerate?: string[];
  build?: string;
  options?: Record<string, Record<string, OptionValue>>;
}

export interface InstanceView {
  record: InstanceRecord;
  /** Operation interrupted halfway: running the same action again resumes it. */
  unfinished: string | null;
  /** The installation completed once (a server build is recorded). */
  installed: boolean;
  running: boolean;
}

export interface ClientView {
  id: string;
  path: string;
  locale: string;
  managed: string[];
  problem: CommandError | null;
}

export interface LauncherOverview {
  root: string;
  instances: InstanceView[];
  clients: ClientView[];
  busy: string | null;
}

interface OptionChoice {
  id: string;
  label: LocalizedText;
  help?: LocalizedText | null;
}

export type ModuleOption =
  | { type: "boolean"; id: string; label: LocalizedText; help?: LocalizedText | null; default: boolean }
  | {
      type: "integer";
      id: string;
      label: LocalizedText;
      help?: LocalizedText | null;
      default: number;
      min?: number | null;
      max?: number | null;
    }
  | {
      type: "choice";
      id: string;
      label: LocalizedText;
      help?: LocalizedText | null;
      default: string;
      choices: OptionChoice[];
    };

export interface ModuleOptions {
  module: string;
  options: { option: ModuleOption; value: OptionValue }[];
}

/** A database backup of an instance (`backups/<id>/<name>`). */
export interface Backup {
  name: string;
  kind: "manual" | "update" | "repair" | "restore" | "other";
  /** Unix seconds. */
  created: number;
  size: number;
  /** Automatic dumps: only the newest few are kept. */
  automatic: boolean;
}

export interface CrashReport {
  id: string;
  text: string;
}

/** A newer launcher version, from the signed update feed. */
export interface AppUpdate {
  version: string;
  notes: string | null;
}

export type UpdateOutcome =
  | { status: "upToDate"; build: string }
  | { status: "updated"; from: string | null; to: string };

export type OperationEvent =
  | { kind: "stepStarted"; index: number; total: number; step: string; stepKind: string; resumed: boolean }
  | { kind: "stepSkipped"; index: number; total: number; step: string }
  | { kind: "stepFinished"; index: number; total: number; step: string }
  | { kind: "stepProgress"; step: string; done: number; total: number | null }
  | { kind: "rollbackStarted"; reason: string }
  | { kind: "stepUndone"; step: string }
  | { kind: "finalizeFailed"; step: string; error: string };

export type SessionEvent =
  | { kind: "databaseStarting" }
  | { kind: "databaseReady" }
  | { kind: "processStarting"; process: string }
  | { kind: "processReady"; process: string; afterMs: number }
  | { kind: "ready"; address: string; authPort: number }
  | { kind: "processStopping"; process: string }
  | { kind: "processStopped"; process: string; how: "clean" | "killed" | "alreadyExited" }
  | { kind: "databaseStopped" }
  | { kind: "warning"; message: string }
  | { kind: "log"; process: string; stderr: boolean; text: string }
  | { kind: "logSkipped"; lines: number }
  | { kind: "gameStarted" }
  | { kind: "gameExited"; code: number | null }
  | { kind: "processExited"; process: string; code: number | null }
  | { kind: "failed"; error: CommandError }
  | { kind: "ended"; clean: boolean };

export const inDesktopApp = isTauri;

/** Opens a web page in the default browser (allowed URLs: capabilities/default.json). */
export function openExternal(url: string): Promise<void> {
  return openUrl(url);
}

/** Native folder picker; `null` when cancelled. */
export async function pickFolder(title: string): Promise<string | null> {
  const picked = await openDialog({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

export const api = {
  catalogOverview: () => invoke<EmulatorOverview[]>("catalog_overview"),
  overview: () => invoke<LauncherOverview>("launcher_overview"),
  registerClient: (client: string, path: string, locale: string | null) =>
    invoke<ClientView>("register_client", { client, path, locale }),
  install: (request: { id: string; emulator: string; modules: string[]; lan: boolean }) =>
    invoke<InstanceRecord>("install_instance", { request }),
  update: (id: string) => invoke<UpdateOutcome>("update_instance", { id }),
  repair: (id: string) => invoke<InstanceRecord>("repair_instance", { id }),
  resume: (id: string) => invoke<InstanceRecord>("resume_instance", { id }),
  rebuildsFor: (id: string, disable: string[]) => invoke<string[]>("rebuilds_for", { id, disable }),
  setModules: (id: string, enable: string[], disable: string[]) =>
    invoke<InstanceRecord>("set_modules", { id, enable, disable }),
  options: (id: string) => invoke<ModuleOptions[]>("instance_options", { id }),
  setOptions: (id: string, module: string, values: [string, string][]) =>
    invoke<InstanceRecord>("set_options", { id, module, values }),
  cancelOperation: () => invoke<void>("cancel_operation"),
  createAccount: (id: string, username: string, password: string, gmLevel: number) =>
    invoke<number>("create_account", { id, username, password, gmLevel }),
  start: (id: string, play: boolean, keepRunning: boolean) =>
    invoke<void>("start_instance", { id, play, keepRunning }),
  stop: (id: string) => invoke<void>("stop_instance", { id }),
  console: (id: string, line: string) => invoke<void>("console_command", { id, line }),
  joinLan: (emulator: string, address: string) => invoke<void>("join_lan", { emulator, address }),
  backups: (id: string) => invoke<Backup[]>("list_backups", { id }),
  backupNow: (id: string) => invoke<Backup>("backup_now", { id }),
  deleteBackup: (id: string, name: string) => invoke<void>("delete_backup", { id, name }),
  restoreBackup: (id: string, name: string) => invoke<InstanceRecord>("restore_backup", { id, name }),
  openBackupsFolder: (id: string) => invoke<void>("open_backups_folder", { id }),
  crashReports: () => invoke<CrashReport[]>("crash_reports"),
  dismissCrashReport: (id: string) => invoke<void>("dismiss_crash_report", { id }),
  /** `null` when up to date or when this build has no update feed. */
  checkAppUpdate: () => invoke<AppUpdate | null>("check_app_update"),
  /** Downloads, verifies and installs; the launcher restarts. */
  installAppUpdate: () => invoke<void>("install_app_update"),
};

export const events = {
  operation: (handler: (instance: string, event: OperationEvent) => void): Promise<UnlistenFn> =>
    listen<{ instance: string; event: OperationEvent }>("operation", (e) =>
      handler(e.payload.instance, e.payload.event),
    ),
  session: (handler: (instance: string, events: SessionEvent[]) => void): Promise<UnlistenFn> =>
    listen<{ instance: string; events: SessionEvent[] }>("session", (e) =>
      handler(e.payload.instance, e.payload.events),
    ),
  closing: (handler: () => void): Promise<UnlistenFn> => listen("closing", () => handler()),
};

/** Window chrome: the launcher draws its own title bar (decorations off). */
export const appWindow = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
  isMaximized: () => getCurrentWindow().isMaximized(),
  /** Calls `handler` after every resize (maximise/restore included); returns an unsubscribe. */
  onResized: (handler: () => void) => getCurrentWindow().onResized(handler),
};
