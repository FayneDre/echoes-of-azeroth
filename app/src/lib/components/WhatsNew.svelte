<script lang="ts">
  // After a launcher update: the release notes of the version now running,
  // saved by AppUpdateNotice before the installer restarted the launcher.
  import { appVersion } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { readWhatsNew, clearWhatsNew, type WhatsNewNotes } from "../whatsNew";

  let shown = $state<WhatsNewNotes | null>(null);

  $effect(() => {
    const saved = readWhatsNew();
    if (!saved) return;
    appVersion()
      .then((version) => {
        // Saved for another version: the update did not go through (yet).
        if (saved.version === version) shown = saved;
      })
      .catch(() => {});
  });

  function dismiss() {
    clearWhatsNew();
    shown = null;
  }
</script>

{#if shown}
  <div class="whats-new" role="status">
    <p class="title">{t("whatsNew.title", { version: shown.version })}</p>
    {#if shown.notes}
      <pre>{shown.notes}</pre>
    {/if}
    <button class="btn" onclick={dismiss}>{t("whatsNew.dismiss")}</button>
  </div>
{/if}

<style>
  .whats-new {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--frame-light);
    background: var(--bg-elevated);
    box-shadow:
      0 0 8px var(--glow),
      var(--panel-shadow);
  }

  .title {
    margin: 0;
    color: var(--title);
  }

  pre {
    margin: 0;
    max-height: 220px;
    width: 100%;
    overflow: auto;
    white-space: pre-wrap;
    font-family: inherit;
    font-size: 0.85rem;
    color: var(--text-muted);
  }
</style>
