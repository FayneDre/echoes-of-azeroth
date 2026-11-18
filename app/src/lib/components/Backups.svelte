<script lang="ts">
  // "Backups" tab: the realm's database backups. Manual ones hold the
  // player data (characters, accounts, bots) and stay until deleted;
  // automatic ones are made before each update, repair or restore, and once
  // a day when the servers stop (ADR 0018); only the newest few are kept. A
  // restore puts the player data back as a backup holds it, after saving
  // the current state. A backup can be made while the servers run (the
  // players are saved first, ADR 0022); a restore needs them stopped.
  import { api, asCommandError, type Backup, type CommandError, type EmulatorOverview } from "../api";
  import { i18n, t, type MessageKey } from "../i18n/i18n.svelte";
  import { launcher, runOperation } from "../launcher.svelte";
  import { preferences } from "../preferences.svelte";
  import Notify from "./Notify.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived(launcher.instanceFor(realm.id));
  const id = $derived(instance?.record.id ?? null);
  const running = $derived(id !== null && launcher.isRunning(id));
  /** No backup can be made or deleted: not installed, interrupted, or busy. */
  const unavailable = $derived(launcher.progress !== null || !instance?.installed || !!instance?.unfinished);
  /** No restore: the servers must be stopped too. */
  const locked = $derived(running || unavailable);

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

  /** Rows per page: the list never makes the page scroll. */
  const PAGE = 6;
  let page = $state(0);
  const pages = $derived(Math.max(1, Math.ceil(list.length / PAGE)));
  const visible = $derived(list.slice(page * PAGE, page * PAGE + PAGE));
  $effect(() => {
    if (page >= pages) page = pages - 1;
  });
</script>

<Notify {error} />
<Notify text={notice} />

<section class="panel list-panel">
  <h3 class="panel-title">{t("backups.title")}</h3>
  {#if !instance}
    <p class="muted">{t("settings.installFirst")}</p>
  {:else}
    <p class="muted intro">{running ? t("backups.running") : t("backups.body")}</p>
    <div class="actions">
      <button class="btn" disabled={unavailable || working} onclick={backupNow}>
        {working ? t("backups.working") : t("backups.now")}
      </button>
      <button class="btn" onclick={openFolder}>{t("backups.openFolder")}</button>
      {#if preferences.value}
        <label class="daily" title={t("backups.dailyHint")}>
          <input
            type="checkbox"
            checked={preferences.value.dailyBackup}
            onchange={(e) => void preferences.set({ dailyBackup: e.currentTarget.checked })}
          />
          <span>{t("backups.daily")}</span>
        </label>
      {/if}
      {#if list.length > 0}
        <span class="muted total">{t("backups.total", { count: String(list.length), size: size(total) })}</span>
      {/if}
    </div>

    {#if list.length === 0}
      <p class="hint muted">{t("backups.empty")}</p>
    {:else}
      <ul class="backups">
        {#each visible as backup (backup.name)}
          <li>
            {#if confirmRestore?.name === backup.name}
              <p class="confirm">{t("backups.confirm", { date: date(backup.created) })}</p>
              <button class="btn small" onclick={() => (confirmRestore = null)}>{t("common.cancel")}</button>
              <button class="btn small danger" disabled={locked} onclick={() => restore(backup)}>
                {t("backups.restore")}
              </button>
            {:else}
              <strong>{date(backup.created)}</strong>
              <span class="muted what">
                {t(`backups.kind.${backup.kind}` as MessageKey)} · {size(backup.size)}{#if backup.automatic}
                  · {t("backups.automatic")}{/if}
              </span>
              <button class="btn small" disabled={locked} onclick={() => (confirmRestore = backup)}>
                {t("backups.restore")}
              </button>
              <button class="btn small" disabled={unavailable || working} onclick={() => remove(backup)}>{t("backups.delete")}</button>
            {/if}
          </li>
        {/each}
      </ul>
      {#if pages > 1}
        <div class="pager">
          <button class="btn small" disabled={page === 0} onclick={() => (page -= 1)}>‹ {t("backups.newer")}</button>
          <span class="muted">{t("backups.page", { page: String(page + 1), pages: String(pages) })}</span>
          <button class="btn small" disabled={page >= pages - 1} onclick={() => (page += 1)}>
            {t("backups.older")} ›
          </button>
        </div>
      {/if}
    {/if}
  {/if}
</section>

<style>
  .list-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .intro {
    margin: 0;
    font-size: 0.88rem;
  }

  .daily {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.88rem;
    cursor: pointer;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-3);
  }

  .total {
    margin-left: auto;
    font-size: 0.85rem;
  }

  .hint {
    margin: var(--space-4) 0 0;
    font-size: 0.85rem;
  }


  .backups {
    list-style: none;
    margin: var(--space-3) 0 0;
    padding: 0;
  }

  .backups li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 40px;
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--border);
  }

  strong {
    flex: none;
    min-width: 11rem;
    color: var(--title);
  }

  .what {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 0.88rem;
  }

  .confirm {
    flex: 1;
    margin: 0;
    color: var(--danger);
    font-size: 0.85rem;
  }

  .btn.small {
    padding: 2px var(--space-3);
    font-size: 0.82rem;
  }

  .btn.danger {
    border-color: var(--danger);
    color: var(--danger);
  }

  .pager {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    margin-top: auto;
    padding-top: var(--space-3);
  }
</style>
