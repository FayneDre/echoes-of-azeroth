<script lang="ts">
  // Opt-in crash report (ADR 0015): shown after the launcher crashed. Nothing
  // is sent by the launcher; "Report" opens a pre-filled GitHub issue that
  // the player reads and submits themselves.
  import { api, openExternal, type CrashReport } from "../api";
  import { t } from "../i18n/i18n.svelte";

  const NEW_ISSUE = "https://github.com/FayneDre/realmkeeper/issues/new";
  /** Keeps the URL under browser limits. */
  const BODY_LIMIT = 6000;

  let reports = $state<CrashReport[]>([]);
  let open = $state(false);

  $effect(() => {
    api
      .crashReports()
      .then((list) => (reports = list))
      .catch(() => (reports = []));
  });

  const report = $derived(reports[0] ?? null);

  async function dismiss() {
    if (!report) return;
    await api.dismissCrashReport(report.id).catch(() => undefined);
    reports = reports.slice(1);
    open = false;
  }

  async function send() {
    if (!report) return;
    const text = report.text.length > BODY_LIMIT ? `${report.text.slice(0, BODY_LIMIT)}\n[…]` : report.text;
    const body = `${t("crash.issueIntro")}\n\n\`\`\`\n${text}\n\`\`\``;
    const url = `${NEW_ISSUE}?labels=crash&title=${encodeURIComponent(t("crash.issueTitle"))}&body=${encodeURIComponent(body)}`;
    await openExternal(url);
    await dismiss();
  }
</script>

{#if report}
  <div class="crash" role="alert">
    <p>{t("crash.notice")}</p>
    <button class="btn" onclick={() => (open = !open)}>{open ? t("crash.hide") : t("crash.show")}</button>
    <button class="btn" onclick={send}>{t("crash.report")}</button>
    <button class="btn" onclick={dismiss}>{t("crash.ignore")}</button>
  </div>
  {#if open}
    <pre class="report">{report.text}</pre>
  {/if}
{/if}

<style>
  /* A floating card (bottom right), never in the page flow. */
  .crash {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--danger);
    background: var(--bg-elevated);
    box-shadow: var(--panel-shadow);
  }

  .crash p {
    flex-basis: 100%;
    margin: 0 0 var(--space-1);
    font-size: 0.82rem;
    line-height: 1.35;
  }

  .crash .btn {
    padding: 2px var(--space-3);
    font-size: 0.78rem;
  }

  .report {
    margin: 0;
    background: var(--bg-elevated);
    max-height: 14rem;
    overflow: auto;
    padding: var(--space-3);
    border: 1px solid var(--border);
    font-size: 0.78rem;
    white-space: pre-wrap;
  }
</style>
