<script lang="ts">
  // Home of an installed realm: play / start / stop, the real state of each
  // server, the client folder, resuming an interrupted change and updates.
  import {
    api,
    asCommandError,
    type ClientView,
    type CommandError,
    type EmulatorOverview,
    type InstanceView,
  } from "../api";
  import Emblem from "../emblems/Emblem.svelte";
  import { localized, t, type MessageKey } from "../i18n/i18n.svelte";
  import { launcher, runOperation, startSession, type ProcessState } from "../launcher.svelte";
  import { processLabel } from "../labels";
  import ClientPicker from "./ClientPicker.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import ProgressPanel from "./ProgressPanel.svelte";

  let { realm, instance, client }: { realm: EmulatorOverview; instance: InstanceView; client: ClientView | null } =
    $props();

  const id = $derived(instance.record.id);
  const session = $derived(launcher.session(id));
  const running = $derived(launcher.isRunning(id));
  const progress = $derived(launcher.progress?.instance === id ? launcher.progress : null);
  const busy = $derived(launcher.progress !== null);
  const servers = $derived(["database", ...realm.processes]);
  const clientReady = $derived(client !== null && client.problem === null);
  /** Interrupted change, or an installation that never completed. */
  const incomplete = $derived(!!instance.unfinished || !instance.installed);

  let keepRunning = $state(false);
  let error = $state<CommandError | null>(null);
  let updateNote = $state<string | null>(null);

  type Status = "stopped" | "starting" | "online" | "stopping" | "failed";
  const status = $derived.by((): Status => {
    if (!session) return running ? "online" : "stopped";
    if (session.phase === "ended") return session.failed ? "failed" : "stopped";
    if (session.phase === "ready") return "online";
    return session.phase;
  });

  function processState(process: string): ProcessState | "idle" {
    if (status === "online" && !session) return "ready";
    return session?.processes[process] ?? "idle";
  }

  async function act(run: () => Promise<unknown>) {
    error = null;
    try {
      await run();
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  const play = () => act(() => startSession(id, true, keepRunning));
  const startOnly = () => act(() => startSession(id, false, true));
  const stop = () => act(() => api.stop(id));

  /** Finishes the interrupted change with the plan it started with. */
  const resume = () => act(() => runOperation(id, () => api.resume(id)));

  let repairNote = $state<string | null>(null);
  function repair() {
    repairNote = null;
    void act(async () => {
      await runOperation(id, () => api.repair(id));
      repairNote = t("repair.done");
    });
  }

  function update() {
    updateNote = null;
    void act(async () => {
      const outcome = await runOperation(id, () => api.update(id));
      updateNote =
        outcome.status === "upToDate"
          ? t("update.upToDate")
          : t("update.done", { build: outcome.to.slice(0, 12) });
    });
  }

  const STATUS_KEYS: Record<Status, MessageKey> = {
    stopped: "status.stopped",
    starting: "status.starting",
    online: "status.online",
    stopping: "status.stopping",
    failed: "status.failed",
  };
  const PROCESS_KEYS: Record<ProcessState | "idle", MessageKey> = {
    idle: "process.idle",
    starting: "process.starting",
    ready: "process.ready",
    stopping: "process.stopping",
    stopped: "process.stopped",
    exited: "process.exited",
  };
</script>

<section class="panel hero">
  <div class="hero-head">
    {#if realm.client}
      <Emblem emblem={realm.client.emblem} accent={realm.client.accent} label={localized(realm.client.shortName)} size={68} />
    {/if}
    <div class="hero-title">
      {#if realm.client}
        <p class="realm-name">{localized(realm.client.shortName)} · {realm.client.version}</p>
      {/if}
      <h2 class="display" class:online={status === "online"} class:failed={status === "failed"}>
        {t(STATUS_KEYS[status])}
      </h2>
      <p class="muted">
        {#if status === "online" && session?.game === "running"}
          {t("home.gameRunning")}
        {:else if status === "online" && instance.record.network === "lan" && session?.address}
          {t("home.lanAddress", { address: session.address })}
        {:else if status === "online"}
          {t("home.onlineBody")}
        {:else if !clientReady}
          {t("home.needClient")}
        {:else}
          {t("home.readyBody")}
        {/if}
      </p>
    </div>
    <div class="actions">
      {#if running}
        <button class="btn" disabled={status === "stopping"} onclick={stop}>{t("home.action.stop")}</button>
      {:else}
        <button class="btn" disabled={busy || incomplete} onclick={startOnly}>
          {t("home.action.startServers")}
        </button>
        <button class="btn btn-play" disabled={busy || !clientReady || incomplete} onclick={play}>
          {t("home.action.play")}
        </button>
      {/if}
    </div>
  </div>
  {#if !running}
    <label class="keep">
      <input type="checkbox" bind:checked={keepRunning} />
      <small>{t("home.keepRunning")}</small>
    </label>
  {/if}

  <dl class="stats">
    <div>
      <dt>{t("home.stat.emulator")}</dt>
      <dd>{realm.name}</dd>
    </div>
    <div>
      <dt>{t("home.stat.modules")}</dt>
      <dd>{t("home.stat.modulesChosen", { count: instance.record.modules.length })}</dd>
    </div>
    <div>
      <dt>{t("home.stat.network")}</dt>
      <dd>{instance.record.network === "lan" ? t("wizard.lan") : t("wizard.solo")}</dd>
    </div>
    <div>
      <dt>{t("home.stat.build")}</dt>
      <dd><code>{instance.record.build?.slice(0, 12) ?? "—"}</code></dd>
    </div>
  </dl>
</section>

{#if incomplete && !progress}
  <div class="banner" role="status">
    <p>{t("home.unfinished")}</p>
    <button class="btn" disabled={busy || running} onclick={resume}>{t("home.action.resume")}</button>
  </div>
{/if}

{#if progress}
  <ProgressPanel {progress} title={t("progress.title")} />
{/if}

{#if session?.error}
  <ErrorBox error={session.error} />
{:else if session?.phase === "ended" && session.failed}
  <div class="banner bad" role="alert"><p>{t("home.crashed")}</p></div>
{/if}
{#if error}
  <ErrorBox {error} />
{/if}
{#each session?.warnings ?? [] as warning, index (index)}
  <p class="warning muted">⚠ {warning}</p>
{/each}

<div class="columns">
  <section class="panel">
    <h3 class="panel-title">{t("servers.title")}</h3>
    <ul class="rows">
      {#each servers as process (process)}
        {@const state = processState(process)}
        <li>
          <span
            class="dot"
            class:ok={state === "ready"}
            class:wait={state === "starting" || state === "stopping"}
            class:bad={state === "exited"}
            aria-hidden="true"
          ></span>
          <span class="row-name">{processLabel(process)}</span>
          <span class="muted">{t(PROCESS_KEYS[state])}</span>
        </li>
      {/each}
    </ul>
    {#if status === "online" && session?.authPort}
      <p class="hint muted">
        {t("home.realmlist", { address: session.address ?? "127.0.0.1", port: session.authPort })}
      </p>
    {/if}
  </section>

  <section class="panel">
    <h3 class="panel-title">{t("home.client")}</h3>
    {#if realm.client}
      <ClientPicker client={realm.client} registered={client} />
    {/if}
  </section>
</div>

<section class="panel">
  <h3 class="panel-title">{t("update.title")}</h3>
  <div class="update">
    <p class="muted">{updateNote ?? t("update.body")}</p>
    <button class="btn" disabled={busy || running || incomplete} onclick={update}>
      {t("update.check")}
    </button>
  </div>
  <div class="update">
    <p class="muted">{repairNote ?? t("repair.body")}</p>
    <button class="btn" disabled={busy || running || incomplete} onclick={repair}>
      {t("repair.action")}
    </button>
  </div>
</section>

<style>
  .hero-head {
    display: flex;
    align-items: center;
    gap: var(--space-5);
  }

  .hero-title {
    flex: 1;
    min-width: 0;
  }

  .realm-name {
    margin: 0;
    font-size: 0.8rem;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--realm-accent);
  }

  h2 {
    margin: 0;
    font-size: 1.7rem;
    color: var(--title);
    text-shadow: var(--text-shadow);
  }

  h2.online {
    color: var(--ok);
  }

  h2.failed {
    color: var(--danger);
  }

  .hero-title p.muted {
    margin: 0;
  }

  .actions {
    display: flex;
    gap: var(--space-3);
    align-items: center;
  }

  .keep {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
    cursor: pointer;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    margin: var(--space-4) 0 0;
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .stats div {
    padding: 0 var(--space-4);
    border-left: 1px solid var(--border);
  }

  .stats div:first-child {
    border-left: 2px solid var(--realm-accent);
  }

  dt {
    font-size: 0.72rem;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  dd {
    margin: 2px 0 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--frame-light);
    background: var(--shade-soft);
  }

  .banner.bad {
    border-color: var(--danger);
    color: var(--danger);
  }

  .banner p {
    flex: 1;
    margin: 0;
  }

  .warning {
    margin: 0;
    font-size: 0.85rem;
  }

  .columns {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-5);
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .rows li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--border);
  }

  .rows li:last-child {
    border-bottom: none;
  }

  .row-name {
    flex: 1;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--frame);
    box-shadow: inset 0 0 0 1px var(--shade-deep);
  }

  .dot.ok {
    background: var(--ok);
    box-shadow: 0 0 6px var(--ok);
  }

  .dot.wait {
    background: var(--accent);
  }

  .dot.bad {
    background: var(--danger);
  }

  .hint {
    margin: var(--space-3) 0 0;
    font-size: 0.85rem;
  }

  .update {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .update + .update {
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .update p {
    flex: 1;
    margin: 0;
  }
</style>
