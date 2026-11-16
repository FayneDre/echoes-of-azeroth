<script lang="ts">
  // A failure as the player sees it: the translated, actionable message,
  // with the technical chain folded for bug reports (principle 8).
  import { openExternal, type CommandError } from "../api";
  import { hasMessage, i18n, t } from "../i18n/i18n.svelte";
  import { processLabel } from "../labels";

  let { error, onretry }: { error: CommandError; onretry?: () => void } = $props();

  const key = $derived(`error.${error.code}`);
  /** Catalog ids among the parameters, shown by their translated name. */
  const params = $derived(
    error.params?.process ? { ...error.params, process: processLabel(error.params.process) } : error.params,
  );
  const message = $derived(hasMessage(key) ? t(key, params) : t("error.unknown"));

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
    <details>
      <summary class="muted">{t("error.details")}</summary>
      <pre>{error.code}: {error.detail}</pre>
    </details>
  </div>
</div>

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

  details {
    flex: 1;
    min-width: 0;
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

  summary {
    cursor: pointer;
    font-size: 0.85rem;
  }

  pre {
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 0.78rem;
    color: var(--text-muted);
    max-height: 12rem;
    overflow-y: auto;
  }
</style>
