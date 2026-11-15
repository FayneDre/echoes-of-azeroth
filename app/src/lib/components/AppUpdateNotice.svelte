<script lang="ts">
  // A newer launcher from the signed update feed (ADR 0015). Checked once at
  // start; a failed check stays silent (offline is normal), a failed install
  // is shown.
  import { api, asCommandError, type AppUpdate, type CommandError } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";

  let update = $state<AppUpdate | null>(null);
  let installing = $state(false);
  let error = $state<CommandError | null>(null);

  $effect(() => {
    api
      .checkAppUpdate()
      .then((found) => (update = found))
      .catch(() => (update = null));
  });

  const blocked = $derived(
    launcher.progress !== null || (launcher.overview?.instances.some((i) => launcher.isRunning(i.record.id)) ?? false),
  );

  async function install() {
    installing = true;
    error = null;
    try {
      await api.installAppUpdate();
    } catch (raw) {
      error = asCommandError(raw);
      installing = false;
    }
  }
</script>

{#if update}
  <div class="update" role="status">
    <p>
      {t("appUpdate.available", { version: update.version })}
      {#if blocked}<small class="muted">{t("appUpdate.stopFirst")}</small>{/if}
    </p>
    <button class="btn" disabled={installing || blocked} onclick={install}>
      {installing ? t("appUpdate.installing") : t("appUpdate.install")}
    </button>
  </div>
  {#if error}
    <ErrorBox {error} />
  {/if}
{/if}

<style>
  .update {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--frame-light);
    background: var(--shade-soft);
    box-shadow: 0 0 8px var(--glow);
  }

  .update p {
    flex: 1;
    margin: 0;
    display: flex;
    flex-direction: column;
  }
</style>
