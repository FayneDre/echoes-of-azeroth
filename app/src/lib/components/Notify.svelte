<script lang="ts">
  // Shows a component's error or message as a toast over the page when it
  // is set. Renders nothing in place: the page layout never moves. The
  // toast outlives the component (an error stays until the player closes
  // it, even if the page changed meanwhile); a new value replaces it and
  // clearing the value (a retry) removes it.
  import { untrack } from "svelte";
  import type { CommandError } from "../api";
  import { removeToast, showToast, type ToastKind } from "../toasts.svelte";

  let {
    error = null,
    text = null,
    kind = "info",
    onretry,
  }: {
    error?: CommandError | null;
    text?: string | null;
    kind?: Exclude<ToastKind, "error">;
    onretry?: () => void;
  } = $props();

  let shown: number | null = null;

  $effect(() => {
    const currentError = error;
    const currentText = text;
    untrack(() => {
      if (shown !== null) removeToast(shown);
      shown = null;
      if (!currentError && !currentText) return;
      shown = showToast({
        kind: currentError ? "error" : kind,
        error: currentError,
        text: currentError ? null : currentText,
        retry: onretry ?? null,
      });
    });
  });
</script>
