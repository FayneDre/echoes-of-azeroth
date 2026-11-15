<script lang="ts">
  // Join a realm hosted by another computer of the local network: the game
  // is pointed at the host's address and launched; no server runs here.
  import { api, asCommandError, type ClientView, type CommandError, type EmulatorOverview } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import ErrorBox from "./ErrorBox.svelte";

  let { realm, client }: { realm: EmulatorOverview; client: ClientView | null } = $props();

  let address = $state("");
  let busy = $state(false);
  let launched = $state(false);
  let error = $state<CommandError | null>(null);

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

<section class="panel">
  <h3 class="panel-title">{t("lan.title")}</h3>
  <p class="muted">{t("lan.body")}</p>
  {#if !client || client.problem}
    <p class="muted">{t("lan.needClient")}</p>
  {:else}
    <form onsubmit={join}>
      <input
        type="text"
        spellcheck="false"
        autocomplete="off"
        placeholder="192.168.1.20"
        aria-label={t("lan.address")}
        bind:value={address}
      />
      <button class="btn btn-play" type="submit" disabled={!valid || busy}>{t("lan.join")}</button>
    </form>
    <small class="muted">{t("lan.patches")}</small>
  {/if}
  {#if launched}
    <p>{t("lan.launched", { address: address.trim() })}</p>
  {/if}
  {#if error}
    <ErrorBox {error} />
  {/if}
</section>

<style>
  form {
    display: flex;
    gap: var(--space-3);
    margin: var(--space-3) 0 var(--space-2);
  }

  input {
    flex: 1;
    max-width: 18rem;
  }
</style>
