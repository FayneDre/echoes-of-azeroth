<script lang="ts">
  // Progress of the running change: step count, current step, bytes for
  // downloads. Cancel stops at the next step boundary (resumable / rolled back).
  import { api } from "../api";
  import { hasMessage, t } from "../i18n/i18n.svelte";
  import type { OperationProgress } from "../launcher.svelte";

  let { progress, title }: { progress: OperationProgress; title: string } = $props();

  let cancelling = $state(false);

  const kindKey = $derived(`step.${progress.stepKind}`);
  const stepLabel = $derived(
    progress.stepKind === "" ? t("progress.preparing") : hasMessage(kindKey) ? t(kindKey) : t("step.other"),
  );
  const overall = $derived(progress.total > 0 ? Math.min(1, progress.index / progress.total) : 0);
  const stepFraction = $derived(
    progress.done !== null && progress.size ? Math.min(1, progress.done / progress.size) : null,
  );
  const mb = (bytes: number) => (bytes / 1e6).toFixed(1);
  /** Seconds left at the current speed (downloads). */
  const remaining = $derived(
    progress.rate && progress.rate > 0 && progress.size && progress.done !== null
      ? Math.max(0, (progress.size - progress.done) / progress.rate)
      : null,
  );
  function duration(seconds: number): string {
    if (seconds < 60) return t("progress.seconds", { count: Math.ceil(seconds) });
    const minutes = Math.ceil(seconds / 60);
    if (minutes < 60) return t("progress.minutes", { count: minutes });
    return t("progress.hours", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
  }

  async function cancel() {
    cancelling = true;
    await api.cancelOperation();
  }
</script>

<section class="panel" aria-live="polite">
  <h3 class="panel-title">{title}</h3>
  {#if progress.rollingBack}
    <p class="warn">{t("progress.rollingBack")}</p>
  {/if}
  <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(overall * 100)}>
    <span style:width="{overall * 100}%"></span>
  </div>
  <p class="step">
    {#if progress.total > 0}
      <strong>{t("progress.step", { index: Math.min(progress.index + 1, progress.total), total: progress.total })}</strong>
    {/if}
    {stepLabel}
    <span class="muted">{progress.step}</span>
  </p>
  {#if stepFraction !== null && progress.size}
    <div class="bar thin"><span style:width="{stepFraction * 100}%"></span></div>
    <p class="muted bytes">
      {t("progress.bytes", { done: mb(progress.done ?? 0), total: mb(progress.size) })}
      {#if progress.rate !== null && remaining !== null}
        · {t("progress.speed", { speed: mb(progress.rate) })} · {t("progress.remaining", { time: duration(remaining) })}
      {/if}
    </p>
  {/if}
  <div class="actions">
    <button class="btn" disabled={cancelling || progress.rollingBack} onclick={cancel}>
      {cancelling ? t("progress.cancelling") : t("common.cancel")}
    </button>
    <small class="muted">{t("progress.cancelHint")}</small>
  </div>
</section>

<style>
  .bar {
    height: 12px;
    border: 1px solid var(--frame);
    background: var(--shade-deep);
    overflow: hidden;
  }

  .bar.thin {
    height: 6px;
    margin-top: var(--space-2);
  }

  .bar span {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, var(--accent-strong), var(--accent));
    box-shadow: 0 0 8px var(--glow);
    transition: width 0.2s ease-out;
  }

  .step {
    margin: var(--space-2) 0 0;
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    flex-wrap: wrap;
  }

  .step strong {
    color: var(--title);
  }

  .bytes {
    margin: var(--space-1) 0 0;
    font-size: 0.85rem;
  }

  .warn {
    margin: 0 0 var(--space-2);
    color: var(--danger);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-3);
  }
</style>
