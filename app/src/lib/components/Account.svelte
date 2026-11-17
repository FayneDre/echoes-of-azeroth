<script lang="ts">
  // "Account" tab, in two parts: creating game accounts (works with the
  // servers stopped: the database is started for it) and the game master
  // panel (servers running).
  import { api, asCommandError, type AccountView, type CommandError, type EmulatorOverview } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import Notify from "./Notify.svelte";
  import GameMaster from "./GameMaster.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived(launcher.instanceFor(realm.id));

  type Part = "create" | "gm";
  let part = $state<Part>("create");

  let username = $state("");
  let password = $state("");
  let confirm = $state("");
  let admin = $state(false);
  let busy = $state(false);
  let error = $state<CommandError | null>(null);
  let created = $state<string | null>(null);

  /** GM levels offered (AzerothCore: 0 player ... 3 administrator). */
  const GM_LEVELS = [0, 1, 2, 3] as const;
  const busyElsewhere = $derived(launcher.progress !== null);

  let accounts = $state<AccountView[] | null>(null);
  let listError = $state<CommandError | null>(null);
  let saving = $state<string | null>(null);

  async function loadAccounts() {
    if (!instance) return;
    listError = null;
    try {
      accounts = await api.listAccounts(instance.record.id);
    } catch (raw) {
      listError = asCommandError(raw);
    }
  }

  // Listed once per realm (the database starts for it when stopped).
  $effect(() => {
    if (instance?.installed && !busyElsewhere) void loadAccounts();
  });

  async function setLevel(account: AccountView, level: number) {
    if (!instance) return;
    saving = account.username;
    listError = null;
    try {
      await api.setGmLevel(instance.record.id, account.username, level);
      account.gmLevel = level;
    } catch (raw) {
      listError = asCommandError(raw);
    } finally {
      saving = null;
    }
  }

  const mismatch = $derived(confirm.length > 0 && password !== confirm);
  const valid = $derived(username.trim().length > 0 && password.length > 0 && password === confirm);

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (!instance || !valid) return;
    busy = true;
    error = null;
    created = null;
    try {
      const name = username.trim();
      await api.createAccount(instance.record.id, name, password, admin ? 3 : 0);
      created = name.toUpperCase();
      username = password = confirm = "";
      admin = false;
      void loadAccounts();
    } catch (raw) {
      error = asCommandError(raw);
    } finally {
      busy = false;
    }
  }
</script>

<div class="segmented parts" role="tablist" aria-label={t("nav.account")}>
  <button role="tab" class:active={part === "create"} aria-selected={part === "create"} onclick={() => (part = "create")}>
    {t("account.partCreate")}
  </button>
  <button role="tab" class:active={part === "gm"} aria-selected={part === "gm"} onclick={() => (part = "gm")}>
    {t("nav.gm")}
  </button>
</div>

{#if part === "gm"}
  <GameMaster {realm} />
{:else}
<section class="panel">
  <h3 class="panel-title">{t("account.title")}</h3>
  {#if !instance}
    <p class="muted">{t("settings.installFirst")}</p>
  {:else}
    <p class="muted">{t("account.body")}</p>
    <form onsubmit={create}>
      <label>
        <span>{t("account.username")}</span>
        <input type="text" maxlength="17" autocomplete="off" spellcheck="false" bind:value={username} />
        <small class="muted">{t("account.usernameHint")}</small>
      </label>
      <label>
        <span>{t("account.password")}</span>
        <input type="password" maxlength="16" autocomplete="new-password" bind:value={password} />
        <small class="muted">{t("account.passwordHint")}</small>
      </label>
      <label>
        <span>{t("account.confirm")}</span>
        <input type="password" maxlength="16" autocomplete="new-password" bind:value={confirm} />
        {#if mismatch}<small class="bad">{t("account.mismatch")}</small>{/if}
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={admin} />
        <span>{t("account.admin")}</span>
      </label>
      <div class="actions">
        <button class="btn btn-play" type="submit" disabled={!valid || busy}>{t("account.create")}</button>
      </div>
    </form>
    <Notify text={created ? t("account.created", { name: created }) : null} />
    <Notify {error} />
  {/if}
</section>

{#if instance}
  <section class="panel">
    <h3 class="panel-title">{t("account.list")}</h3>
    <p class="muted">{t("account.listBody")}</p>
    {#if accounts === null && !listError}
      <p class="muted">{t("gm.loading")}</p>
    {:else if accounts && accounts.length === 0}
      <p class="muted">{t("account.none")}</p>
    {:else if accounts}
      <table>
        <thead>
          <tr>
            <th scope="col">{t("account.username")}</th>
            <th scope="col">{t("account.gmLevel")}</th>
          </tr>
        </thead>
        <tbody>
          {#each accounts as account (account.id)}
            <tr>
              <td>{account.username}</td>
              <td>
                <select
                  aria-label={t("account.gmLevelOf", { name: account.username })}
                  value={account.gmLevel}
                  disabled={saving !== null || busyElsewhere}
                  onchange={(e) => setLevel(account, Number(e.currentTarget.value))}
                >
                  {#each GM_LEVELS as level (level)}
                    <option value={level}>{t(`account.gm.${level}`)}</option>
                  {/each}
                </select>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
    <Notify error={listError} onretry={loadAccounts} />
  </section>
{/if}
{/if}

<style>
  .parts {
    align-self: flex-start;
  }

  form {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-4);
    margin-top: var(--space-4);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: var(--space-2);
    grid-column: 1 / -1;
    cursor: pointer;
  }

  .actions {
    grid-column: 1 / -1;
    display: flex;
    justify-content: flex-end;
  }


  section + section {
    margin-top: var(--space-4);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    margin-top: var(--space-3);
  }

  th,
  td {
    text-align: left;
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--border);
  }

  .bad {
    color: var(--danger);
  }
</style>
