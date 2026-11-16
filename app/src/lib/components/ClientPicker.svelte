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

  /** The folder's own name, shown first; its parent path below it. */
  function split(path: string): { name: string; parent: string } {
    const trimmed = path.replace(/[\\/]+$/, "");
    const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
    return cut < 0 ? { name: trimmed, parent: "" } : { name: trimmed.slice(cut + 1), parent: trimmed.slice(0, cut + 1) };
  }

  async function choose() {
    const path = await pickFolder(t("client.pickTitle", { version: client.version }));
    if (path) await register(path, null);
  }
</script>

{#snippet folder(path: string, meta: string, ok: boolean | null)}
  {@const parts = split(path)}
  <div class="folder" title={path}>
    <span class="dot" class:ok={ok === true} class:bad={ok === false} aria-hidden="true"></span>
    <div class="what">
      <strong>{parts.name}</strong>
      {#if parts.parent}<small class="path">{parts.parent}</small>{/if}
      <small class="muted">{meta}</small>
    </div>
  </div>
{/snippet}

<div class="client">
  {#if registered}
    <div class="card">
      {@render folder(
        registered.path,
        t("client.registered", { locale: registered.locale, build: client.build }),
        !registered.problem,
      )}
      <button class="btn" disabled={busy} onclick={choose}>{t("client.change")}</button>
    </div>
    {#if registered.problem}
      <ErrorBox error={registered.problem} />
    {/if}
  {:else}
    <p class="muted explain">{t("client.explain", { version: client.version, build: client.build })}</p>
    {#if found === null}
      <p class="muted searching">{t("client.searching")}</p>
    {:else if found.length > 0}
      <ul class="found">
        {#each found as candidate (candidate.path)}
          <li class="card">
            {@render folder(
              candidate.path,
              t("client.foundMeta", { build: client.build, locales: candidate.locales.join(", ") || "?" }),
              true,
            )}
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
    {:else}
      <p class="muted searching">{t("client.notFound")}</p>
    {/if}
    <button class="btn other" class:link={!!found && found.length > 0} disabled={busy} onclick={choose}>
      {found && found.length > 0 ? t("client.chooseOther") : t("client.choose")}
    </button>
  {/if}

  {#if pending}
    <div class="locales">
      <p>{t("client.whichLocale")}</p>
      <div class="locale-buttons">
        {#each pending.locales as locale (locale)}
          <button class="btn" disabled={busy} onclick={() => pending && register(pending.path, locale)}>{locale}</button>
        {/each}
      </div>
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
  }

  .explain {
    margin: 0;
    font-size: 0.88rem;
    line-height: 1.45;
  }

  .searching {
    margin: 0;
    font-size: 0.85rem;
    font-style: italic;
  }

  .found {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  /* One game folder: its name, where it is, what it holds, and its action. */
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3);
    background: var(--surface-muted);
    border: 1px solid var(--border);
    border-left: 3px solid var(--frame-light);
  }

  .card .btn {
    align-self: stretch;
  }

  .folder {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    min-width: 0;
  }

  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .what strong {
    color: var(--title);
    font-family: var(--font-display);
    overflow-wrap: anywhere;
  }

  .path {
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 0.78rem;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }

  .what small.muted {
    font-size: 0.8rem;
  }

  .dot {
    flex: none;
    width: 9px;
    height: 9px;
    margin-top: 0.45em;
    border-radius: 50%;
    background: var(--frame);
  }

  .dot.ok {
    background: var(--ok);
  }

  .dot.bad {
    background: var(--danger);
  }

  .other {
    align-self: stretch;
  }

  /* Secondary when a folder was found: the found one is the obvious choice. */
  .other.link {
    align-self: center;
    padding: 0;
    border: none;
    background: none;
    box-shadow: none;
    color: var(--accent);
    font-family: inherit;
    font-size: 0.88rem;
    letter-spacing: normal;
    text-transform: none;
  }

  .other.link:hover:not(:disabled) {
    text-decoration: underline;
  }

  .locales p {
    margin: 0 0 var(--space-2);
  }

  .locale-buttons {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
</style>
