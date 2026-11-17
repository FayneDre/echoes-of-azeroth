// Messages shown over the page (bottom right), never in its flow: errors
// of an action, confirmations and warnings. The layout of a page never
// moves because something happened (CLAUDE.md, UI conventions).

import type { CommandError } from "./api";

export type ToastKind = "error" | "warning" | "info";

export interface Toast {
  id: number;
  kind: ToastKind;
  /** An error, shown with its help and technical details. */
  error: CommandError | null;
  /** A plain message (info, warning). */
  text: string | null;
  retry: (() => void) | null;
}

/** Plain messages close by themselves; errors stay until closed. */
const LIFETIME_MS: Record<ToastKind, number | null> = { error: null, warning: 12_000, info: 6_000 };
/** Older ones leave first: the stack never covers the page. */
const MAX_SHOWN = 3;

let list = $state<Toast[]>([]);
let next = 1;

export const toasts = {
  get list(): Toast[] {
    return list;
  },
};

export function showToast(toast: Omit<Toast, "id">): number {
  const id = next++;
  // The same message again replaces the old one instead of piling up.
  const key = (t: Omit<Toast, "id">) =>
    JSON.stringify([t.kind, t.text, t.error?.code, t.error?.params, t.error?.detail]);
  const shown = [...list.filter((t) => key(t) !== key(toast)), { ...toast, id }];
  // Errors stay until closed: plain messages leave first when there are too many.
  while (shown.length > MAX_SHOWN) {
    const plain = shown.findIndex((t) => t.kind !== "error");
    shown.splice(plain === -1 ? 0 : plain, 1);
  }
  list = shown;
  const lifetime = LIFETIME_MS[toast.kind];
  if (lifetime !== null) setTimeout(() => removeToast(id), lifetime);
  return id;
}

export function removeToast(id: number) {
  list = list.filter((t) => t.id !== id);
}

/** An action failed: the player sees why, with help and a retry. */
export function notifyError(error: CommandError, retry?: () => void): number {
  return showToast({ kind: "error", error, text: null, retry: retry ?? null });
}

/** A confirmation or a warning. */
export function notify(text: string, kind: "info" | "warning" = "info"): number {
  return showToast({ kind, error: null, text, retry: null });
}
