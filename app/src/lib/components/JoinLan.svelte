<script lang="ts">
  // Join a realm hosted by another computer of the local network: the game
  // is pointed at the host's address and launched; no server runs here.
  // Rendered inside the "game client" panels (it needs the client folder).
  import { api, asCommandError, type ClientView, type CommandError, type EmulatorOverview } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import ErrorBox from "./ErrorBox.svelte";

  let { realm, client }: { realm: EmulatorOverview; client: ClientView | null } = $props();

  let open = $state(false);
  let address = $state("");
  let busy = $state(false);
  let launched = $state(false);
  let error = $state<CommandError | null>(null);

  const ready = $derived(!!client && !client.problem);
  const valid = $derived(/^[A-Za-z0-9.-]{1,253}$/.test(address.trim()));

  async function join(event: SubmitEvent) {
    event.preventDefault();
    if (!valid) return;
    busy = true;
    error = null;
    launched = false;
    try {
      await api.joinLan(realm.id, address.trim());
      launched = true;
    } catch (raw) {
      error = asCommandError(raw);
    } finally {
      busy = false;
    }
  }
</script>

<div class="join">
  {#if !open}
    <button class="link" disabled={!ready} title={ready ? t("lan.body") : t("lan.needClient")} onclick={() => (open = true)}>
      {t("lan.title")} ›
    </button>
  {:else}
    <p class="muted" title={t("lan.patches")}>{t("lan.body")}</p>
    <form onsubmit={join}>
      <input
        type="text"
        spellcheck="false"
        autocomplete="off"
        placeholder="192.168.1.20"
        aria-label={t("lan.address")}
        bind:value={address}
      />
      <button class="btn" type="submit" disabled={!valid || busy}>{t("lan.join")}</button>
      <button class="btn" type="button" onclick={() => (open = false)}>{t("common.close")}</button>
    </form>
    {#if launched}
      <small>{t("lan.launched", { address: address.trim() })}</small>
    {/if}
  {/if}
  {#if error}
    <ErrorBox {error} />
  {/if}
</div>

<style>
  .join {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .join p {
    margin: 0;
    font-size: 0.85rem;
  }

  .link {
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 0.9rem;
    cursor: pointer;
  }

  .link:disabled {
    color: var(--text-muted);
    cursor: default;
  }

  .link:not(:disabled):hover {
    text-decoration: underline;
  }

  form {
    display: flex;
    gap: var(--space-2);
  }

  input {
    flex: 1;
    min-width: 0;
  }
</style>
