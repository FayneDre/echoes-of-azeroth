// Display names for catalog ids that the UI translates itself.

import { t, type MessageKey } from "./i18n/i18n.svelte";

const PROCESS_KEYS: Record<string, MessageKey> = {
  database: "process.database",
  authserver: "process.authserver",
  worldserver: "process.worldserver",
};

/** Translated name of a server process; unknown ids (new emulators) show as is. */
export function processLabel(id: string): string {
  const key = PROCESS_KEYS[id];
  return key ? t(key) : id;
}
