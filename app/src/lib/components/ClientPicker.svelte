<script lang="ts">
  // The player's own game folder (never distributed, principle 7): found in
  // the usual places or picked with the system dialog, checked by the
  // backend (build, language).
  import {
    api,
    asCommandError,
    pickFolder,
    type ClientSummary,
    type ClientView,
    type CommandError,
    type FoundClient,
  } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { refresh } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";

  let { client, registered }: { client: ClientSummary; registered: ClientView | null } = $props();

  let error = $state<CommandError | null>(null);
  let busy = $state(false);
  /** Folder waiting for a language choice (several installed). */
  let pending = $state<{ path: string; locales: string[] } | null>(null);
  /** Clients found on this computer; `null` while looking. */
  let found = $state<FoundClient[] | null>(null);
  let searched = false;

  // Looked for once, while no folder is chosen.
  $effect(() => {
    if (registered || searched) return;
    searched = true;
    api
      .findClients(client.id)
      .then((list) => (found = list))
      .catch(() => (found = []));
  });

  async function register(path: string, locale: string | null) {
    busy = true;
    error = null;
    try {
      await api.registerClient(client.id, path, locale);
      pending = null;
      await refresh();
    } catch (raw) {
      const failure = asCommandError(raw);
      if (failure.code === "client.ambiguousLocale" && failure.params.found) {
        pending = { path, locales: failure.params.found.split(",").map((l) => l.trim()).filter(Boolean) };
      } else {
        error = failure;
      }
    } finally {
      busy = false;
    }
  }

  async function choose() {
    const path = await pickFolder(t("client.pickTitle", { version: client.version }));
    if (path) await register(path, null);
  }
</script>

<div class="client">
  {#if registered}
    <div class="row">
      <span class="dot" class:ok={!registered.problem} class:bad={!!registered.problem} aria-hidden="true"></span>
      <div class="what">
        <code title={registered.path}>{registered.path}</code>
        <small class="muted">{t("client.registered", { locale: registered.locale, build: client.build })}</small>
      </div>
      <button class="btn" disabled={busy} onclick={choose}>{t("client.change")}</button>
    </div>
    {#if registered.problem}
      <ErrorBox error={registered.problem} />
    {/if}
  {:else}
    <p class="muted">{t("client.explain", { version: client.version, build: client.build })}</p>
    {#if found === null}
      <p class="muted searching">{t("client.searching")}</p>
    {:else if found.length > 0}
      <ul class="found">
        {#each found as candidate (candidate.path)}
          <li>
            <code title={candidate.path}>{candidate.path}</code>
            <button
              class="btn"
              disabled={busy}
              onclick={() => register(candidate.path, candidate.locales.length === 1 ? (candidate.locales[0] ?? null) : null)}
            >
              {t("client.useFound")}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    <button class="btn" disabled={busy} onclick={choose}>
      {found && found.length > 0 ? t("client.chooseOther") : t("client.choose")}
    </button>
  {/if}

  {#if pending}
    <div class="locales">
      <p>{t("client.whichLocale")}</p>
      {#each pending.locales as locale (locale)}
        <button class="btn" disabled={busy} onclick={() => pending && register(pending.path, locale)}>{locale}</button>
      {/each}
    </div>
  {/if}

  {#if error}
    <ErrorBox {error} />
  {/if}
</div>

<style>
  .client {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    align-items: flex-start;
  }

  .client > p {
    margin: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
  }

  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  code {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .dot {
    flex: none;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--frame);
  }

  .dot.ok {
    background: var(--ok);
  }

  .dot.bad {
    background: var(--danger);
  }

  .searching {
    font-size: 0.85rem;
    font-style: italic;
  }

  .found {
    list-style: none;
    margin: 0;
    padding: 0;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .found li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .found code {
    flex: 1;
    min-width: 0;
  }

  .locales {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .locales p {
    margin: 0;
  }
</style>
