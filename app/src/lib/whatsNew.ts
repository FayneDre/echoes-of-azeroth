// Release notes of a launcher update, kept across the restart that installs
// it (per-viewer convenience: losing them only skips the notice).

const KEY = "rk.whatsNew";

export interface WhatsNewNotes {
  version: string;
  notes: string | null;
}

export function saveWhatsNew(notes: WhatsNewNotes): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(notes));
  } catch {
    // Storage unavailable: no notice after the restart.
  }
}

export function readWhatsNew(): WhatsNewNotes | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const value: unknown = JSON.parse(raw);
    if (typeof value === "object" && value !== null && "version" in value && typeof value.version === "string") {
      const notes = "notes" in value && typeof value.notes === "string" ? value.notes : null;
      return { version: value.version, notes };
    }
  } catch {
    // Unreadable: treated as absent.
  }
  return null;
}

export function clearWhatsNew(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // Nothing to clear.
  }
}
