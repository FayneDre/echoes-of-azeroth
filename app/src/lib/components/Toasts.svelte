<script lang="ts">
  // The stack of toasts (lib/toasts.svelte.ts), in the floating notices
  // column at the bottom right of the window.
  import { t } from "../i18n/i18n.svelte";
  import { removeToast, toasts } from "../toasts.svelte";
  import ErrorBox from "./ErrorBox.svelte";
</script>

{#each toasts.list as toast (toast.id)}
  <div class="toast {toast.kind}" role={toast.kind === "info" ? "status" : "alert"}>
    {#if toast.error}
      <ErrorBox error={toast.error} onretry={toast.retry ?? undefined} compact />
    {:else}
      <p>{toast.text}</p>
    {/if}
    <button class="close" aria-label={t("common.close")} title={t("common.close")} onclick={() => removeToast(toast.id)}>
      ×
    </button>
  </div>
{/each}

<style>
  .toast {
    position: relative;
    padding: var(--space-3) var(--space-6) var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-left: 3px solid var(--frame-light);
    background: var(--bg-elevated);
    box-shadow: var(--panel-shadow);
    font-size: 0.85rem;
    line-height: 1.4;
    animation: rise 0.18s ease-out;
  }

  .toast.error {
    border-color: var(--danger);
    border-left-color: var(--danger);
  }

  .toast.warning {
    border-left-color: var(--accent);
  }

  p {
    margin: 0;
  }

  .close {
    position: absolute;
    top: 2px;
    right: 4px;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 1.1rem;
    line-height: 1;
    cursor: pointer;
  }

  .close:hover {
    color: var(--title);
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .toast {
      animation: none;
    }
  }
</style>
