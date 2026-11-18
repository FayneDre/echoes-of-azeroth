// Typed wrappers around Tauri commands and events. Keep in sync with
// app/src-tauri/src/commands.rs (and the serialized types of rk-launcher).

import { getVersion } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
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
  /** Ready-made console commands of the GM panel. */
  consoleActions: ConsoleAction[];
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
  /** Name the player gave the realm (ADR 0020). */
  name?: string;
}

/** A realm written to one file to move it to another computer (ADR 0020). */
export interface ExportedRealm {
  format: number;
  emulator: string;
  flavor: string;
  modules: string[];
  options: Record<string, Record<string, OptionValue>>;
  name: string | null;
  network: "solo" | "lan";
  build: string | null;
  exported: number;
  launcher: string;
}

export interface ExportSummary {
  realm: ExportedRealm;
  dumpSize: number;
}

/** Extension of exported realm files. */
export const REALM_EXTENSION = "rkrealm";

/** A ready-made console command of the GM panel (emulator manifest). */
export interface ConsoleAction {
  id: string;
  label: LocalizedText;
  description?: LocalizedText;
  command: string;
  params: ConsoleParam[];
}

export type ConsoleParam = { id: string; label: LocalizedText } & (
  | { type: "character" }
  | { type: "text"; maxLength?: number }
  | { type: "number"; min: number; max: number; default?: number }
  | { type: "location" }
  | { type: "choice"; choices: { value: string; label: LocalizedText }[] }
);

export interface AccountView {
  id: number;
  username: string;
  gmLevel: number;
}

export interface OnlineCharacter {
  name: string;
  level: number;
  race: string;
  class: string;
  bot: boolean;
}

export interface InstanceView {
  record: InstanceRecord;
  /** Operation interrupted halfway: running the same action again resumes it. */
  unfinished: string | null;
  /** The installation completed once (a server build is recorded). */
  installed: boolean;
  /** Its folder was renamed or moved: resuming writes its paths again. */
  moved: boolean;
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
      /** Decimal (rates). */
      type: "number";
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

/** What an uninstall did. */
export interface UninstallReport {
  backup: Backup | null;
  sharedRemoved: boolean;
  warnings: string[];
}

/** Options key of the realm settings (`setOptions(id, REALM_OPTIONS, ...)`). */
export const REALM_OPTIONS = "realm";

/** A section of realm settings (rates, realm type...) with its values. */
export interface SettingsSection {
  id: string;
  label: LocalizedText;
  options: { option: ModuleOption; value: OptionValue }[];
}

/** A database backup of an instance (`backups/<id>/<name>`). */
/** Whether other computers of the network can reach a realm (ADR 0022). */
export interface FirewallStatus {
  /** Rules can be made on this system (Windows). */
  supported: boolean;
  /** Every server program has its rule. */
  allowed: boolean;
  /** Windows treats this network as public: the rules do not apply there. */
  publicNetwork: boolean;
}

export interface Backup {
  name: string;
  kind: "manual" | "daily" | "update" | "repair" | "restore" | "import" | "other";
  /** Unix seconds. */
  created: number;
  size: number;
  /** Automatic dumps: only the newest few are kept. */
  automatic: boolean;
}

/** Disk space a first install needs (bytes). */
export interface SpaceEstimate {
  /** Still to download. */
  download: number;
  /** Taken on the disk, downloads included (margin excluded). */
  needed: number;
  /** Free on the disk of the install location, when readable. */
  available: number | null;
  /** The disk is known to be too small (a safety margin included). */
  tooSmall: boolean;
}

/** A game folder found on this computer. */
export interface FoundClient {
  path: string;
  locales: string[];
}

/** The launcher's own settings (not a realm's). */
export interface Preferences {
  /** Back up the player data once a day, when the servers stop. */
  dailyBackup: boolean;
  /** Hide the launcher in the notification area while the game runs. */
  hideWhilePlaying: boolean;
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
  | { kind: "gameStarted"; keepRunning: boolean }
  | { kind: "gameExited"; code: number | null }
  | { kind: "population"; players: number; bots: number }
  | { kind: "startup"; process: string; progress: number; label: LocalizedText }
  | { kind: "processExited"; process: string; code: number | null }
  | { kind: "failed"; error: CommandError }
  | { kind: "ended"; clean: boolean };

export const inDesktopApp = isTauri;

/** Version of the running launcher. */
export const appVersion = (): Promise<string> => getVersion();

/** Copies text to the clipboard (with a fallback for older webviews). */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // Fall back to a selection copy below.
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.append(area);
  area.select();
  const copied = document.execCommand("copy");
  area.remove();
  if (!copied) throw new Error("clipboard unavailable");
}

/** Opens a web page in the default browser (allowed URLs: capabilities/default.json). */
export function openExternal(url: string): Promise<void> {
  return openUrl(url);
}

/** Native folder picker; `null` when cancelled. */
export async function pickFolder(title: string): Promise<string | null> {
  const picked = await openDialog({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

/** Native file picker for an exported realm; `null` when cancelled. */
export async function pickRealmFile(title: string, filterName: string): Promise<string | null> {
  const picked = await openDialog({
    directory: false,
    multiple: false,
    title,
    filters: [{ name: filterName, extensions: [REALM_EXTENSION] }],
  });
  return typeof picked === "string" ? picked : null;
}

/** Native "save as" for an exported realm; `null` when cancelled. */
export async function pickRealmTarget(title: string, filterName: string, defaultName: string): Promise<string | null> {
  const picked = await saveDialog({
    title,
    defaultPath: `${defaultName}.${REALM_EXTENSION}`,
    filters: [{ name: filterName, extensions: [REALM_EXTENSION] }],
  });
  return typeof picked === "string" ? picked : null;
}

/** Where realms, downloads and backups are installed. */
export interface DataLocation {
  root: string;
  default: string;
  /** No realm is installed yet: it can change to an empty folder. */
  changeable: boolean;
  /** It can point at another RealmKeeper folder (renamed or moved). */
  relocatable: boolean;
  /** The chosen folder, no longer found (renamed, moved, drive unplugged). */
  missing: string | null;
}

export const api = {
  dataLocation: () => invoke<DataLocation>("data_location"),
  /** `folder` picked by the player, used as is (empty or an RealmKeeper folder), `null` = default. */
  setDataLocation: (folder: string | null) => invoke<DataLocation>("set_data_location", { folder }),
  catalogOverview: () => invoke<EmulatorOverview[]>("catalog_overview"),
  listAccounts: (id: string) => invoke<AccountView[]>("list_accounts", { id }),
  setGmLevel: (id: string, username: string, gmLevel: number) =>
    invoke<void>("set_gm_level", { id, username, gmLevel }),
  /** `null` while the servers are stopped. */
  onlineCharacters: (id: string) => invoke<OnlineCharacter[] | null>("online_characters", { id }),
  teleportLocations: (id: string) => invoke<string[]>("teleport_locations", { id }),
  /** Types the action's command in the server console; returns the line sent. */
  runConsoleAction: (id: string, action: string, values: Record<string, string>) =>
    invoke<string>("run_console_action", { id, action, values }),
  overview: () => invoke<LauncherOverview>("launcher_overview"),
  registerClient: (client: string, path: string, locale: string | null) =>
    invoke<ClientView>("register_client", { client, path, locale }),
  /** Looks for the game in the usual places (drives, personal folders). */
  findClients: (client: string) => invoke<FoundClient[]>("find_clients", { client }),
  installSpace: (emulator: string) => invoke<SpaceEstimate>("install_space", { emulator }),
  install: (request: { id: string; emulator: string; modules: string[]; lan: boolean }) =>
    invoke<InstanceRecord>("install_instance", { request }),
  update: (id: string) => invoke<UpdateOutcome>("update_instance", { id }),
  repair: (id: string) => invoke<InstanceRecord>("repair_instance", { id }),
  resume: (id: string) => invoke<InstanceRecord>("resume_instance", { id }),
  rebuildsFor: (id: string, disable: string[]) => invoke<string[]>("rebuilds_for", { id, disable }),
  setModules: (id: string, enable: string[], disable: string[]) =>
    invoke<InstanceRecord>("set_modules", { id, enable, disable }),
  options: (id: string) => invoke<ModuleOptions[]>("instance_options", { id }),
  realmSettings: (id: string) => invoke<SettingsSection[]>("realm_settings", { id }),
  /** Removes a realm; with `keepBackup`, its player data is backed up first. */
  uninstall: (id: string, keepBackup: boolean) =>
    invoke<UninstallReport>("uninstall_instance", { id, keepBackup }),
  setOptions: (id: string, module: string, values: [string, string][]) =>
    invoke<InstanceRecord>("set_options", { id, module, values }),
  cancelOperation: () => invoke<void>("cancel_operation"),
  createAccount: (id: string, username: string, password: string, gmLevel: number) =>
    invoke<number>("create_account", { id, username, password, gmLevel }),
  start: (id: string, play: boolean, keepRunning: boolean) =>
    invoke<void>("start_instance", { id, play, keepRunning }),
  stop: (id: string) => invoke<void>("stop_instance", { id }),
  console: (id: string, line: string) => invoke<void>("console_command", { id, line }),
  /** Launches the game for an instance whose servers are up (or starting). */
  launchGame: (id: string) => invoke<void>("launch_game", { id }),
  joinLan: (emulator: string, address: string) => invoke<void>("join_lan", { emulator, address }),
  backups: (id: string) => invoke<Backup[]>("list_backups", { id }),
  backupNow: (id: string) => invoke<Backup>("backup_now", { id }),
  deleteBackup: (id: string, name: string) => invoke<void>("delete_backup", { id, name }),
  restoreBackup: (id: string, name: string) => invoke<InstanceRecord>("restore_backup", { id, name }),
  openBackupsFolder: (id: string) => invoke<void>("open_backups_folder", { id }),
  firewallStatus: (id: string) => invoke<FirewallStatus>("firewall_status", { id }),
  /** Windows asks for administrator rights. */
  allowFirewall: (id: string) => invoke<FirewallStatus>("allow_firewall", { id }),
  removeFirewall: (id: string) => invoke<FirewallStatus>("remove_firewall", { id }),
  /** Copies a stopped realm to a new one (ADR 0020). */
  duplicate: (source: string, id: string, name: string | null) =>
    invoke<InstanceRecord>("duplicate_instance", { source, id, name }),
  exportRealm: (id: string, path: string) => invoke<ExportSummary>("export_instance", { id, path }),
  readExport: (path: string) => invoke<ExportSummary>("read_export", { path }),
  importRealm: (path: string, id: string, name: string | null) =>
    invoke<InstanceRecord>("import_instance", { path, id, name }),
  rename: (id: string, name: string | null) => invoke<InstanceRecord>("rename_instance", { id, name }),
  /** Support report to paste into a bug report (personal folders redacted). */
  diagnosticReport: () => invoke<string>("diagnostic_report"),
  preferences: () => invoke<Preferences>("preferences"),
  setPreferences: (preferences: Preferences) => invoke<Preferences>("set_preferences", { preferences }),
  /** Tray menu, and what Windows shows while it waits for the servers or stays awake. */
  setTrayTexts: (texts: { show: string; stop: string; quit: string; shutdown: string; awake: string; ready: string }) =>
    invoke<void>("set_tray_texts", texts),
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
