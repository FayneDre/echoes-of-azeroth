<script lang="ts">
  // A failure as the player sees it: the translated, actionable message,
  // with the technical chain in a dialog for bug reports (principle 8).
  import { copyText, openExternal, type CommandError } from "../api";
  import { hasMessage, i18n, t } from "../i18n/i18n.svelte";
  import { processLabel } from "../labels";
  import Modal from "./Modal.svelte";

  let { error, onretry }: { error: CommandError; onretry?: () => void } = $props();

  const key = $derived(`error.${error.code}`);
  /** Catalog ids among the parameters, shown by their translated name. */
  const params = $derived(
    error.params?.process ? { ...error.params, process: processLabel(error.params.process) } : error.params,
  );
  const message = $derived(hasMessage(key) ? t(key, params) : t("error.unknown"));

  let showDetails = $state(false);
  let copied = $state<"copied" | "failed" | null>(null);
  const details = $derived(`${error.code}: ${error.detail}`);

  async function copyDetails() {
    try {
      await copyText(details);
      copied = "copied";
    } catch {
      copied = "failed";
    }
    setTimeout(() => (copied = null), 2500);
  }

  /** FAQ section for an error code (docs/faq.md anchors). */
  const FAQ_SECTIONS: [RegExp, string][] = [
    [/^client\./, "client"],
    [/^(location|disk)\./, "location"],
    [/^(security\.|server\.filesMissing|instance\.damaged|assets\.)/, "antivirus"],
    [/^port\./, "ports"],
    [/^(network|index|appUpdate)\./, "network"],
    [/^instance\.moved$/, "servers"],
    [/^(operation|instance)\./, "operations"],
    [/^(server|database)\./, "servers"],
    [/^backup\./, "backups"],
    [/^realm\./, "realms"],
    [/^lan\./, "lan"],
    [/^(gm|account)\./, "gm"],
  ];
  const FAQ_URL = "https://github.com/FayneDre/realmkeeper/blob/main/docs/";
  const section = $derived(FAQ_SECTIONS.find(([pattern]) => pattern.test(error.code))?.[1] ?? "other");
  const helpUrl = $derived(`${FAQ_URL}${i18n.locale === "fr" ? "faq.fr.md" : "faq.md"}#${section}`);
</script>

<div class="error-box" role="alert">
  <p>{message}</p>
  <div class="actions">
    {#if onretry}
      <button class="btn" onclick={onretry}>{t("common.retry")}</button>
    {/if}
    <button class="help" onclick={() => void openExternal(helpUrl)}>{t("error.help")} ›</button>
    <button class="help muted" onclick={() => (showDetails = true)}>{t("error.details")} ›</button>
  </div>
</div>

{#if showDetails}
  <Modal title={t("error.detailsTitle")} width="760px" onclose={() => (showDetails = false)}>
    <p class="intro">{message}</p>
    <pre>{details}</pre>
    {#snippet actions()}
      {#if copied}
        <small class="copied" class:failed={copied === "failed"}>{t(copied === "copied" ? "error.copyDone" : "error.copyFailed")}</small>
      {/if}
      <button type="button" class="btn" onclick={copyDetails}>{t("error.copy")}</button>
      <button type="button" class="btn" onclick={() => (showDetails = false)}>{t("common.close")}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .error-box {
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--danger);
    border-left: 3px solid var(--danger);
    background: var(--shade-soft);
  }

  p {
    margin: 0 0 var(--space-2);
    color: var(--danger);
  }

  .actions {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .help {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    white-space: nowrap;
  }

  .help:hover {
    text-decoration: underline;
  }

  .help.muted {
    color: var(--text-muted);
  }

  .intro {
    color: var(--danger);
  }

  pre {
    margin: 0;
    padding: var(--space-3);
    border: 1px solid var(--border);
    background: var(--shade-soft);
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 0.78rem;
    color: var(--text-muted);
    user-select: text;
  }

  .copied {
    align-self: center;
    color: var(--ok);
    font-size: 0.8rem;
  }

  .copied.failed {
    color: var(--danger);
  }
</style>
