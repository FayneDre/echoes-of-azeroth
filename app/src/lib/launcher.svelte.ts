// Live launcher state for the UI: installed instances, registered clients,
// operation progress and running sessions, all fed by commands and events.
// Only real states: nothing here is guessed or simulated.

import {
  api,
  asCommandError,
  events,
  inDesktopApp,
  type CommandError,
  type InstanceView,
  type LauncherOverview,
  type OperationEvent,
  type SessionEvent,
} from "./api";
import { t, type LocalizedText, type MessageKey } from "./i18n/i18n.svelte";
import { notify } from "./toasts.svelte";

/** Log lines kept per process (older ones are dropped; the log files keep all). */
const LOG_LIMIT = 1500;

export interface OperationProgress {
  instance: string;
  index: number;
  total: number;
  step: string;
  stepKind: string;
  /** Bytes (downloads) or items done in the current step. */
  done: number | null;
  size: number | null;
  /** Download speed in bytes per second (smoothed), once measured. */
  rate: number | null;
  rollingBack: boolean;
  /** Title of the progress panel, when the change is not an install or update. */
  title: MessageKey | null;
}

/** When the last progress of the current step arrived (performance.now()). */
let lastProgressAt = 0;

export type ProcessState = "starting" | "ready" | "stopping" | "stopped" | "exited";

export interface LogLine {
  id: number;
  stderr: boolean;
  text: string;
}

export interface SessionState {
  phase: "starting" | "ready" | "stopping" | "ended";
  processes: Record<string, ProcessState>;
  logs: Record<string, LogLine[]>;
  address: string | null;
  authPort: number | null;
  game: "none" | "running" | "exited";
  /** The servers stay up when the running game exits. */
  keepRunning: boolean;
  warnings: string[];
  error: CommandError | null;
  /** Unexpected end (crash, failed start). */
  failed: boolean;
  /** Characters in the world (players, bots), when the emulator tells. */
  population: { players: number; bots: number } | null;
  /** When every server became ready (ms since the epoch). */
  readyAt: number | null;
  /** How far the start went (manifest milestones), until ready. */
  startup: { process: string; progress: number; label: LocalizedText } | null;
}

function newSession(): SessionState {
  return {
    phase: "starting",
    processes: {},
    logs: {},
    address: null,
    authPort: null,
    game: "none",
    keepRunning: true,
    warnings: [],
    error: null,
    failed: false,
    population: null,
    readyAt: null,
    startup: null,
  };
}

let overview = $state<LauncherOverview | null>(null);
let overviewError = $state<CommandError | null>(null);
let progress = $state<OperationProgress | null>(null);
let sessions = $state<Record<string, SessionState>>({});
let closing = $state(false);
let nextLine = 0;

function pushLog(session: SessionState, process: string, stderr: boolean, text: string) {
  const lines = (session.logs[process] ??= []);
  lines.push({ id: nextLine++, stderr, text });
  if (lines.length > LOG_LIMIT) lines.splice(0, lines.length - LOG_LIMIT);
}

function applySession(instance: string, list: SessionEvent[]) {
  const session = (sessions[instance] ??= newSession());
  for (const event of list) {
    switch (event.kind) {
      case "databaseStarting":
        session.processes.database = "starting";
        break;
      case "databaseReady":
        session.processes.database = "ready";
        break;
      case "processStarting":
        session.processes[event.process] = "starting";
        break;
      case "processReady":
        session.processes[event.process] = "ready";
        break;
      case "startup":
        if (session.phase === "starting") session.startup = { process: event.process, progress: event.progress, label: event.label };
        break;
      case "ready":
        session.phase = "ready";
        session.startup = null;
        session.readyAt = Date.now();
        session.address = event.address;
        session.authPort = event.authPort;
        break;
      case "processStopping":
        session.phase = "stopping";
        session.processes[event.process] = "stopping";
        break;
      case "processStopped":
        // A process that had already ended keeps its "ended unexpectedly" state.
        if (event.how !== "alreadyExited") session.processes[event.process] = "stopped";
        break;
      case "databaseStopped":
        session.processes.database = "stopped";
        break;
      case "warning":
        session.warnings.push(event.message);
        // Shown once, when it happens (not again each time a page mounts).
        notify(event.message, "warning");
        break;
      case "log":
        pushLog(session, event.process, event.stderr, event.text);
        break;
      case "logSkipped":
        break;
      case "gameStarted":
        session.game = "running";
        session.keepRunning = event.keepRunning;
        break;
      case "gameExited":
        session.game = "exited";
        break;
      case "population":
        session.population = { players: event.players, bots: event.bots };
        break;
      case "processExited":
        session.processes[event.process] = "exited";
        session.failed = true;
        break;
      case "failed":
        session.error = event.error;
        session.failed = true;
        break;
      case "ended":
        session.phase = "ended";
        session.population = null;
        if (!event.clean) session.failed = true;
        void refresh();
        break;
    }
  }
}

function applyOperation(instance: string, event: OperationEvent) {
  switch (event.kind) {
    case "stepStarted":
      progress = {
        instance,
        index: event.index,
        total: event.total,
        step: event.step,
        stepKind: event.stepKind,
        done: null,
        size: null,
        rate: null,
        rollingBack: progress?.instance === instance ? progress.rollingBack : false,
        title: progress?.instance === instance ? progress.title : null,
      };
      break;
    case "stepSkipped":
    case "stepFinished":
      if (progress?.instance === instance) {
        progress.index = event.index + 1;
        progress.total = event.total;
        progress.done = null;
        progress.size = null;
        progress.rate = null;
      }
      break;
    case "stepProgress":
      if (progress?.instance === instance) {
        const now = performance.now();
        // Speed from the second sample of a download on (the first one may
        // include what an interrupted download had already fetched).
        if (progress.stepKind === "download" && progress.done !== null && now > lastProgressAt) {
          const instant = ((event.done - progress.done) * 1000) / (now - lastProgressAt);
          if (instant >= 0) progress.rate = progress.rate === null ? instant : progress.rate * 0.8 + instant * 0.2;
        }
        lastProgressAt = now;
        progress.done = event.done;
        progress.size = event.total;
      }
      break;
    case "rollbackStarted":
      if (progress?.instance === instance) progress.rollingBack = true;
      break;
    default:
      break;
  }
}

export async function refresh(): Promise<void> {
  if (!inDesktopApp()) return;
  try {
    overview = await api.overview();
    overviewError = null;
  } catch (error) {
    overviewError = asCommandError(error);
  }
}

let started = false;

/** Subscribes to backend events once and loads the first overview. */
export function connect(): void {
  if (started || !inDesktopApp()) return;
  started = true;
  void events.operation(applyOperation);
  void events.session(applySession);
  void events.closing(() => (closing = true));
  void refresh();
}

/**
 * Runs a change (install, modules, options, update) with its progress shown;
 * refreshes the overview afterwards, whatever the result.
 */
export async function runOperation<T>(
  instance: string,
  run: () => Promise<T>,
  title: MessageKey | null = null,
): Promise<T> {
  progress = {
    instance,
    index: 0,
    total: 0,
    step: "",
    stepKind: "",
    done: null,
    size: null,
    rate: null,
    rollingBack: false,
    title,
  };
  try {
    return await run();
  } finally {
    progress = null;
    await refresh();
  }
}

/** Starts an instance (and the game with `play`); progress arrives as events. */
export async function startSession(id: string, play: boolean, keepRunning: boolean): Promise<void> {
  sessions[id] = newSession();
  try {
    await api.start(id, play, keepRunning);
  } catch (error) {
    sessions[id].error = asCommandError(error);
    sessions[id].failed = true;
    sessions[id].phase = "ended";
    throw error;
  } finally {
    await refresh();
  }
}

/** Realm shown for each game version (emulator id -> instance id), ADR 0020. */
let chosen = $state<Record<string, string>>(loadChosen());

function loadChosen(): Record<string, string> {
  try {
    const raw = localStorage.getItem("rk.chosenRealm");
    return raw ? (JSON.parse(raw) as Record<string, string>) : {};
  } catch {
    return {};
  }
}

/** Shows the realm `id` for its game version (after a copy or an import too). */
export function chooseRealm(emulator: string, id: string): void {
  chosen[emulator] = id;
  try {
    localStorage.setItem("rk.chosenRealm", JSON.stringify(chosen));
  } catch {
    // Remembering the choice is a convenience only.
  }
}

/** A realm's display name: the one the player gave; the first realm of a
 * game version (its id is the emulator's) is "My realm"; else its id. */
export function realmLabel(instance: InstanceView): string {
  if (instance.record.name) return instance.record.name;
  return instance.record.id === instance.record.emulator ? t("realms.defaultName") : instance.record.id;
}

export const launcher = {
  /** The realms of one game version, oldest first. */
  realmsOf(emulator: string): InstanceView[] {
    return (overview?.instances ?? [])
      .filter((i) => i.record.emulator === emulator)
      .sort((a, b) => a.record.created - b.record.created);
  },
  /** The realm shown for a game version: the chosen one, else the oldest. */
  instanceFor(emulator: string | null | undefined): InstanceView | null {
    if (!emulator) return null;
    const realms = this.realmsOf(emulator);
    return realms.find((i) => i.record.id === chosen[emulator]) ?? realms[0] ?? null;
  },
  get overview() {
    return overview;
  },
  get error() {
    return overviewError;
  },
  get progress() {
    return progress;
  },
  get closing() {
    return closing;
  },
  session(id: string): SessionState | null {
    return sessions[id] ?? null;
  },
  /** Running = a session that has not ended (the backend is the reference). */
  isRunning(id: string): boolean {
    const session = sessions[id];
    if (session) return session.phase !== "ended";
    return overview?.instances.find((i) => i.record.id === id)?.running ?? false;
  },
};
