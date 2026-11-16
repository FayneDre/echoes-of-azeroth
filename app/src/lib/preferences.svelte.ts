// The launcher's own settings (daily backup, hide while playing), shared by
// the pages that show them. Loaded once; each change is saved at once.

import { api, inDesktopApp, type Preferences } from "./api";

let current = $state<Preferences | null>(null);
let requested = false;

/** Loads the preferences once (later calls do nothing). */
export function loadPreferences(): void {
  if (requested || !inDesktopApp()) return;
  requested = true;
  api
    .preferences()
    .then((value) => (current = value))
    .catch(() => (requested = false));
}

export const preferences = {
  /** `null` until loaded. */
  get value(): Preferences | null {
    return current;
  },
  /** Saves a change; the shown value follows what was saved. */
  async set(change: Partial<Preferences>): Promise<void> {
    if (!current) return;
    current = await api.setPreferences({ ...current, ...change });
  },
};
