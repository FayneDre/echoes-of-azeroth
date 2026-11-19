// Release notes of the launcher (`release-notes.json`, one entry per version,
// one line per change, English and French). The same file feeds the update
// feed (`tools/ci/app-release.mjs`), so the notes of an update are shown
// before it is installed and again, from the new build, after the restart.

import notes from "./release-notes.json";
import { i18n } from "./i18n/i18n.svelte";

/** Notes by locale: `en` always, `fr` when translated. */
export type ReleaseNotes = Record<string, string[]>;

const bundled: Record<string, ReleaseNotes> = notes;

/** The notes of a version shipped in this build, if any. */
export function bundledNotes(version: string): ReleaseNotes | null {
  return bundled[version] ?? null;
}

/** The lines in the player's language (English as the fallback). */
export function noteLines(notes: ReleaseNotes | null): string[] {
  if (!notes) return [];
  return notes[i18n.locale] ?? notes.en ?? Object.values(notes)[0] ?? [];
}

// The last launcher version that ran (per-viewer convenience: losing it only
// skips one "what's new" dialog).
const LAST = "rk.lastVersion";
// Written by launchers up to 0.3.1 just before installing an update.
const LEGACY = "rk.whatsNew";

/**
 * Records `current` as seen and tells whether it was just updated to: an
 * earlier version ran before. A first install has nothing new to show.
 */
export function justUpdated(current: string): boolean {
  try {
    const last = localStorage.getItem(LAST);
    const legacy = localStorage.getItem(LEGACY);
    localStorage.setItem(LAST, current);
    localStorage.removeItem(LEGACY);
    if (last !== null) return last !== current;
    if (legacy === null) return false;
    const saved: unknown = JSON.parse(legacy);
    return typeof saved === "object" && saved !== null && "version" in saved && saved.version === current;
  } catch {
    return false;
  }
}
