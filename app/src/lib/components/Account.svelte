<script lang="ts">
  // "Account" tab: game accounts and their GM level (works with the servers
  // stopped: the database is started for it), then the game master tools
  // while the servers are online.
  import { untrack } from "svelte";
  import { api, asCommandError, type AccountView, type CommandError, type EmulatorOverview } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import Notify from "./Notify.svelte";
  import GameMaster from "./GameMaster.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived(launcher.instanceFor(realm.id));

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
  /** The realm `accounts` belong to (a GM level is set there). */
  let listedFor = $state<string | null>(null);
  let listError = $state<CommandError | null>(null);
  let saving = $state<string | null>(null);
  /** Only the latest listing is shown (an older one may answer last). */
  let request = 0;

  const realmId = $derived(instance?.record.id ?? null);

  // Another realm: its list replaces the previous one, never shown under it.
  $effect(() => {
    const id = realmId;
    untrack(() => {
      if (listedFor !== id) {
        accounts = null;
        listError = null;
        listedFor = id;
      }
    });
  });

  async function loadAccounts() {
    const id = realmId;
    if (!id) return;
    const mine = ++request;
    listError = null;
    try {
      const list = await api.listAccounts(id);
      if (mine === request && id === realmId) accounts = list;
    } catch (raw) {
      if (mine === request && id === realmId) listError = asCommandError(raw);
    }
  }

  const session = $derived(instance ? launcher.session(instance.record.id) : null);
  /** Servers starting or stopping: their database may go away, nothing is read. */
  const settling = $derived(session?.phase === "starting" || session?.phase === "stopping");
  const online = $derived(!!instance && launcher.isRunning(instance.record.id) && !settling);
  /** Listed again only when this changes (realm, servers up or down). */
  const listKey = $derived(
    instance?.installed && !busyElsewhere && !settling ? `${instance.record.id}:${online}` : null,
  );

  $effect(() => {
    if (listKey) untrack(() => void loadAccounts());
  });

  async function setLevel(account: AccountView, level: number) {
    if (!listedFor || listedFor !== realmId) return;
    saving = account.username;
    listError = null;
    try {
      await api.setGmLevel(listedFor, account.username, level);
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

{#if online}
  <div class="gm">
    <GameMaster {realm} />
  </div>
{/if}

<style>
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


  section + section,
  .gm {
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
