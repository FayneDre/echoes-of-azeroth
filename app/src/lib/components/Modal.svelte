<script lang="ts">
  // One modal dialog for the whole launcher: a framed panel over a dimmed
  // backdrop. Escape, a click outside or the close button call `onclose`.
  // Anything the player unfolds to read (details, reports, short forms) opens
  // here rather than growing the page.
  import type { Snippet } from "svelte";
  import { t } from "../i18n/i18n.svelte";

  let {
    title,
    onclose,
    width = "520px",
    children,
    actions,
  }: {
    title: string;
    onclose: () => void;
    /** Largest width of the panel (CSS length). */
    width?: string;
    children: Snippet;
    /** Buttons at the bottom right; a single "Close" button when absent. */
    actions?: Snippet;
  } = $props();

  /** Moves the dialog to <body>: no parent stacking context or clipping. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  const id = `modal-${Math.random().toString(36).slice(2)}`;
  let panel = $state<HTMLElement | null>(null);

  // Focus moves into the dialog (keyboard players) and back when it closes.
  $effect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const first = panel?.querySelector<HTMLElement>(
      "input:not([disabled]), select, textarea, button:not([disabled]):not(.close)",
    );
    (first ?? panel)?.focus();
    return () => previous?.focus?.();
  });
</script>

<div
  class="modal"
  role="presentation"
  use:portal
  onclick={(e) => {
    if (e.target === e.currentTarget) onclose();
  }}
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    }
  }}
>
  <div
    class="panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby={id}
    tabindex="-1"
    style:width={`min(${width}, 100%)`}
    bind:this={panel}
  >
    <header>
      <h3 class="panel-title" {id}>{title}</h3>
      <button type="button" class="close" aria-label={t("common.close")} title={t("common.close")} onclick={onclose}>
        ×
      </button>
    </header>
    <div class="body">
      {@render children()}
    </div>
    <div class="modal-actions">
      {#if actions}
        {@render actions()}
      {:else}
        <button type="button" class="btn" onclick={onclose}>{t("common.close")}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .modal {
    position: fixed;
    inset: 0;
    z-index: 9;
    display: grid;
    place-items: center;
    padding: var(--space-6);
    background: var(--shade-deep);
    backdrop-filter: blur(2px);
  }

  /* Capped by the window, not by the grid row (which grows with the
     content): the body scrolls and the actions stay in view. */
  .panel {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 2 * var(--space-6));
    background: var(--bg-elevated);
    outline: none;
  }

  header {
    position: relative;
  }

  .close {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 1.3rem;
    line-height: 1;
    cursor: pointer;
  }

  .close:hover {
    color: var(--title);
  }

  .body {
    min-height: 0;
    overflow-y: auto;
  }

  .body :global(p) {
    margin: 0 0 var(--space-3);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }

  /* The main action of a dialog: the red button, at dialog scale. */
  .modal-actions :global(.btn-play) {
    min-width: 8rem;
    padding: var(--space-2) var(--space-5);
    font-size: 0.95rem;
  }
</style>
