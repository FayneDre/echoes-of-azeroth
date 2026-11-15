<script lang="ts">
  // "Account" tab: create a game account on the realm (works with the
  // servers stopped: the database is started for it).
  import { api, asCommandError, type CommandError, type EmulatorOverview } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived(launcher.overview?.instances.find((i) => i.record.emulator === realm.id) ?? null);

  let username = $state("");
  let password = $state("");
  let confirm = $state("");
  let admin = $state(false);
  let busy = $state(false);
  let error = $state<CommandError | null>(null);
  let created = $state<string | null>(null);

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
    {#if created}
      <p class="ok" role="status">{t("account.created", { name: created })}</p>
    {/if}
    {#if error}
      <ErrorBox {error} />
    {/if}
  {/if}
</section>

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

  .ok {
    color: var(--ok);
  }

  .bad {
    color: var(--danger);
  }
</style>
