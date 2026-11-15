<script lang="ts">
  // "Backups" tab: the realm's database backups. Manual ones hold the
  // player data (characters, accounts, bots) and stay until deleted;
  // automatic ones are made before each update, repair or restore, and only
  // the newest few are kept. A restore puts the player data back as a
  // backup holds it, after saving the current state.
  import { api, asCommandError, type Backup, type CommandError, type EmulatorOverview } from "../api";
  import { i18n, t, type MessageKey } from "../i18n/i18n.svelte";
  import { launcher, runOperation } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import ProgressPanel from "./ProgressPanel.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived(launcher.overview?.instances.find((i) => i.record.emulator === realm.id) ?? null);
  const id = $derived(instance?.record.id ?? null);
  const running = $derived(id !== null && launcher.isRunning(id));
  const progress = $derived(id !== null && launcher.progress?.instance === id ? launcher.progress : null);
  const locked = $derived(
    running || launcher.progress !== null || !instance?.installed || !!instance?.unfinished,
  );

  let list = $state<Backup[]>([]);
  let working = $state(false);
  let error = $state<CommandError | null>(null);
  let notice = $state<string | null>(null);
  /** Backup waiting for the player's confirmation of a restore. */
  let confirmRestore = $state<Backup | null>(null);

  const recordKey = $derived(instance ? JSON.stringify(instance.record) : "");

  async function load() {
    if (!id) {
      list = [];
      return;
    }
    try {
      list = await api.backups(id);
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  $effect(() => {
    void recordKey;
    void load();
  });

  async function backupNow() {
    if (!id) return;
    working = true;
    error = notice = null;
    try {
      const made = await api.backupNow(id);
      notice = t("backups.made", { size: size(made.size) });
      await load();
    } catch (raw) {
      error = asCommandError(raw);
    } finally {
      working = false;
    }
  }

  async function restore(backup: Backup) {
    if (!id) return;
    const instanceId = id;
    confirmRestore = null;
    error = notice = null;
    try {
      await runOperation(instanceId, () => api.restoreBackup(instanceId, backup.name));
      notice = t("backups.restored", { date: date(backup.created) });
    } catch (raw) {
      error = asCommandError(raw);
    }
    await load();
  }

  async function remove(backup: Backup) {
    if (!id) return;
    error = notice = null;
    try {
      await api.deleteBackup(id, backup.name);
      await load();
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  async function openFolder() {
    if (!id) return;
    try {
      await api.openBackupsFolder(id);
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  function size(bytes: number): string {
    const number = (value: number) =>
      value.toLocaleString(i18n.locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
    return bytes >= 1e9
      ? t("backups.gb", { value: number(bytes / 1e9) })
      : t("backups.mb", { value: number(Math.max(0.1, bytes / 1e6)) });
  }

  function date(secs: number): string {
    return new Date(secs * 1000).toLocaleString(i18n.locale, { dateStyle: "medium", timeStyle: "short" });
  }

  const total = $derived(list.reduce((sum, b) => sum + b.size, 0));
</script>

{#if progress}
  <ProgressPanel {progress} title={t("backups.restoring")} />
{/if}
{#if error}
  <ErrorBox {error} />
{/if}
{#if notice}
  <p class="banner">{notice}</p>
{/if}

<section class="panel">
  <h3 class="panel-title">{t("backups.title")}</h3>
  {#if !instance}
    <p class="muted">{t("settings.installFirst")}</p>
  {:else}
    <p class="muted">{t("backups.body")}</p>
    {#if running}
      <p class="muted">{t("backups.stopFirst")}</p>
    {/if}
    <div class="actions">
      <button class="btn btn-play" disabled={locked || working} onclick={backupNow}>
        {working ? t("backups.working") : t("backups.now")}
      </button>
      <button class="btn" onclick={openFolder}>{t("backups.openFolder")}</button>
      {#if list.length > 0}
        <span class="muted total">{t("backups.total", { count: String(list.length), size: size(total) })}</span>
      {/if}
    </div>

    {#if list.length === 0}
      <p class="hint muted">{t("backups.empty")}</p>
    {:else}
      <ul class="backups">
        {#each list as backup (backup.name)}
          <li>
            <div class="what">
              <strong>{date(backup.created)}</strong>
              <small class="muted">
                {t(`backups.kind.${backup.kind}` as MessageKey)} · {size(backup.size)}
                {#if backup.automatic}· {t("backups.automatic")}{/if}
              </small>
            </div>
            {#if confirmRestore?.name === backup.name}
              <div class="confirm">
                <p>{t("backups.confirm", { date: date(backup.created) })}</p>
                <button class="btn" onclick={() => (confirmRestore = null)}>{t("common.cancel")}</button>
                <button class="btn btn-play" disabled={locked} onclick={() => restore(backup)}>
                  {t("backups.restore")}
                </button>
              </div>
            {:else}
              <button class="btn" disabled={locked} onclick={() => (confirmRestore = backup)}>
                {t("backups.restore")}
              </button>
              <button class="btn" disabled={locked} onclick={() => remove(backup)}>{t("backups.delete")}</button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }

  .total {
    margin-left: auto;
    font-size: 0.85rem;
  }

  .hint {
    margin: var(--space-4) 0 0;
    font-size: 0.85rem;
  }

  .banner {
    margin: 0;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--frame-light);
    background: var(--shade-soft);
  }

  .backups {
    list-style: none;
    margin: var(--space-4) 0 0;
    padding: 0;
  }

  .backups li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--border);
  }

  .backups li:last-child {
    border-bottom: none;
  }

  .what {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 12rem;
  }

  strong {
    color: var(--title);
  }

  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    flex-basis: 100%;
  }

  .confirm p {
    flex: 1;
    margin: 0;
    color: var(--danger);
  }
</style>
