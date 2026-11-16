<script lang="ts">
  // The install folder the player chose no longer exists (renamed, moved,
  // drive unplugged, ADR 0017): offer to point the launcher at its new place.
  // The launcher never creates it again by itself.
  import { api, asCommandError, pickFolder, type CommandError } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { refresh } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";

  let missing = $state<string | null>(null);
  let error = $state<CommandError | null>(null);
  let busy = $state(false);

  $effect(() => {
    api
      .dataLocation()
      .then((l) => (missing = l.missing))
      .catch(() => (missing = null));
  });

  async function choose(reset: boolean) {
    error = null;
    busy = true;
    try {
      const folder = reset ? null : await pickFolder(t("location.title"));
      if (!reset && folder === null) return;
      const location = await api.setDataLocation(folder);
      missing = location.missing;
      await refresh();
    } catch (raw) {
      error = asCommandError(raw);
    } finally {
      busy = false;
    }
  }
</script>

{#if missing}
  <div class="missing" role="alert">
    <p>{t("location.missing", { path: missing })}</p>
    <button class="btn" disabled={busy} onclick={() => choose(false)}>{t("location.locate")}</button>
    <button class="btn" disabled={busy} onclick={() => choose(true)}>{t("location.useDefault")}</button>
    {#if error}
      <ErrorBox {error} />
    {/if}
  </div>
{/if}

<style>
  /* A floating card (bottom right), like the other notices. */
  .missing {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--danger);
    background: var(--bg-elevated);
    box-shadow: var(--panel-shadow);
  }

  .missing p {
    flex-basis: 100%;
    margin: 0 0 var(--space-1);
    font-size: 0.82rem;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .missing .btn {
    padding: 2px var(--space-3);
    font-size: 0.78rem;
  }
</style>
