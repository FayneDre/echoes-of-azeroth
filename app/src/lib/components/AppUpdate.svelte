<script lang="ts">
  // A newer launcher from the signed update feed (ADR 0015): a button in the
  // title bar opens its release notes and installs it, with the download
  // progress. Checked at start and every few hours; a failed check stays
  // silent (offline is normal), a failed install is shown in the dialog.
  import { api, asCommandError, events, type AppUpdate, type AppUpdateProgress, type CommandError } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import Modal from "./Modal.svelte";
  import ReleaseNotesList from "./ReleaseNotesList.svelte";

  const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;

  let update = $state<AppUpdate | null>(null);
  let open = $state(false);
  let installing = $state(false);
  let progress = $state<AppUpdateProgress | null>(null);
  let error = $state<CommandError | null>(null);

  $effect(() => {
    const check = () =>
      api
        .checkAppUpdate()
        .then((found) => {
          if (!installing) update = found;
        })
        .catch(() => {});
    check();
    const timer = setInterval(check, CHECK_EVERY_MS);
    return () => clearInterval(timer);
  });

  $effect(() => {
    const off = events.appUpdate((p) => (progress = p));
    return () => void off.then((unlisten) => unlisten());
  });

  const blocked = $derived(
    launcher.progress !== null || (launcher.overview?.instances.some((i) => launcher.isRunning(i.record.id)) ?? false),
  );
  const percent = $derived(
    progress?.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null,
  );
  const done = $derived(percent === 100);

  async function install() {
    installing = true;
    progress = null;
    error = null;
    try {
      // The launcher restarts on the new version: this call does not return.
      await api.installAppUpdate();
    } catch (raw) {
      error = asCommandError(raw);
      installing = false;
    }
  }

  function close() {
    // The installer closes the launcher itself; the dialog stays meanwhile.
    if (!installing) open = false;
  }
</script>

{#if update}
  <button
    class="update-button"
    title={t("appUpdate.available", { version: update.version })}
    onclick={() => (open = true)}
  >
    <span class="dot" aria-hidden="true"></span>
    {t("appUpdate.button", { version: update.version })}
  </button>

  {#if open}
    <Modal title={t("appUpdate.title", { version: update.version })} onclose={close} width="560px">
      <ReleaseNotesList notes={update.notes} />
      {#if installing}
        <div class="install" aria-live="polite">
          <div
            class="bar"
            role="progressbar"
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={percent ?? undefined}
          >
            <span class:indeterminate={percent === null} style:width={percent === null ? undefined : `${percent}%`}
            ></span>
          </div>
          <p class="muted">
            {done || !progress
              ? t("appUpdate.installing")
              : percent === null
                ? t("appUpdate.downloading")
                : t("appUpdate.downloadingPercent", { percent })}
          </p>
        </div>
      {:else if blocked}
        <p class="muted hint">{t("appUpdate.stopFirst")}</p>
      {/if}
      {#if error}
        <ErrorBox {error} />
      {/if}
      {#snippet actions()}
        <button class="btn" disabled={installing} onclick={close}>{t("appUpdate.later")}</button>
        <button class="btn btn-play" disabled={installing || blocked} onclick={install}>
          {t("appUpdate.install")}
        </button>
      {/snippet}
    </Modal>
  {/if}
{/if}

<style>
  .update-button {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-3);
    border: 1px solid var(--frame-light);
    background: var(--bg-elevated);
    color: var(--title);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    box-shadow: 0 0 8px var(--glow);
  }

  .update-button:hover,
  .update-button:focus-visible {
    border-color: var(--accent);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px var(--accent);
    animation: pulse 2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }

  .install {
    margin-top: var(--space-4);
  }

  .install p,
  .hint {
    margin: var(--space-2) 0 0;
  }

  .hint {
    margin-top: var(--space-4);
  }

  .bar {
    height: 10px;
    border: 1px solid var(--frame);
    background: var(--shade-deep);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, var(--accent-strong), var(--accent));
    box-shadow: 0 0 8px var(--glow);
    transition: width 0.2s ease-out;
  }

  .bar span.indeterminate {
    width: 30%;
    animation: slide 1.4s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
</style>
