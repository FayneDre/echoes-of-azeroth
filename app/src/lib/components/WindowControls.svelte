<script lang="ts">
  // Minimise / maximise / close, drawn in the launcher's own style: the native
  // title bar is off (tauri.conf.json "decorations": false).
  import { appWindow, inDesktopApp } from "../api";
  import { t } from "../i18n/i18n.svelte";

  let maximized = $state(false);

  $effect(() => {
    if (!inDesktopApp()) return;
    const refresh = () => appWindow.isMaximized().then((value) => (maximized = value));
    void refresh();
    const unlisten = appWindow.onResized(() => void refresh());
    return () => void unlisten.then((stop) => stop());
  });
</script>

<div class="controls">
  <button title={t("window.minimize")} aria-label={t("window.minimize")} onclick={() => void appWindow.minimize()}>
    <svg viewBox="0 0 12 12" aria-hidden="true"><line x1="2" y1="6.5" x2="10" y2="6.5" /></svg>
  </button>
  <button
    title={t(maximized ? "window.restore" : "window.maximize")}
    aria-label={t(maximized ? "window.restore" : "window.maximize")}
    onclick={() => void appWindow.toggleMaximize()}
  >
    <svg viewBox="0 0 12 12" aria-hidden="true">
      {#if maximized}
        <rect x="2" y="4" width="6" height="6" /><polyline points="4,4 4,2 10,2 10,8 8,8" />
      {:else}
        <rect x="2" y="2" width="8" height="8" />
      {/if}
    </svg>
  </button>
  <button class="close" title={t("window.close")} aria-label={t("window.close")} onclick={() => void appWindow.close()}>
    <svg viewBox="0 0 12 12" aria-hidden="true"><line x1="2.5" y1="2.5" x2="9.5" y2="9.5" /><line x1="9.5" y1="2.5" x2="2.5" y2="9.5" /></svg>
  </button>
</div>

<style>
  .controls {
    flex: none;
    display: flex;
    align-self: stretch;
    align-items: flex-start;
  }

  button {
    display: grid;
    place-items: center;
    width: 42px;
    height: 32px;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
  }

  button:hover {
    background: var(--shade);
    color: var(--title);
  }

  button.close:hover {
    background: var(--danger-bg);
    color: var(--play-text);
  }

  svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.2;
  }
</style>
