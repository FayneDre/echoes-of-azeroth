<script lang="ts">
  // Join a realm hosted by another computer of the local network: the game
  // is pointed at the host's address and launched; no server runs here.
  // Rendered inside the "game client" panels (it needs the client folder).
  import { api, asCommandError, type ClientView, type CommandError, type EmulatorOverview } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import Modal from "./Modal.svelte";

  let { realm, client }: { realm: EmulatorOverview; client: ClientView | null } = $props();

  let open = $state(false);
  let address = $state("");
  let busy = $state(false);
  let launched = $state(false);
  let error = $state<CommandError | null>(null);

  const ready = $derived(!!client && !client.problem);
  const valid = $derived(/^[A-Za-z0-9.-]{1,253}$/.test(address.trim()));

  function close() {
    open = false;
    error = null;
    launched = false;
  }

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
  <button class="link" disabled={!ready} title={ready ? t("lan.body") : t("lan.needClient")} onclick={() => (open = true)}>
    {t("lan.title")} ›
  </button>
</div>

{#if open}
  <Modal title={t("lan.title")} onclose={close}>
    <p class="muted">{t("lan.body")}</p>
    <p class="muted small">{t("lan.patches")}</p>
    <form id="join-lan" onsubmit={join}>
      <label for="join-lan-address">{t("lan.address")}</label>
      <input
        id="join-lan-address"
        type="text"
        spellcheck="false"
        autocomplete="off"
        placeholder="192.168.1.20"
        bind:value={address}
      />
    </form>
    {#if launched}
      <p class="ok">{t("lan.launched", { address: address.trim() })}</p>
    {/if}
    {#if error}
      <ErrorBox {error} />
    {/if}
    {#snippet actions()}
      <button type="button" class="btn" onclick={close}>{t("common.close")}</button>
      <button type="submit" form="join-lan" class="btn btn-play" disabled={!valid || busy}>{t("lan.join")}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .join {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
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
    flex-direction: column;
    gap: var(--space-1);
  }

  .small {
    font-size: 0.82rem;
  }

  .ok {
    margin: var(--space-3) 0 0;
    color: var(--ok);
  }
</style>
