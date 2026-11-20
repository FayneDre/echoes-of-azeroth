<script lang="ts">
  // Home of an installed realm: play / start / stop, the real state of each
  // server, the client folder, resuming an interrupted change and updates.
  import {
    api,
    asCommandError,
    type ClientView,
    type CommandError,
    type EmulatorOverview,
    type FirewallStatus,
    type InstanceView,
    type ServerUpdate,
  } from "../api";
  import Emblem from "../emblems/Emblem.svelte";
  import { gigabytes, localized, t, type MessageKey } from "../i18n/i18n.svelte";
  import { launcher, runOperation, startSession, type ProcessState } from "../launcher.svelte";
  import { preferences } from "../preferences.svelte";
  import { processLabel } from "../labels";
  import ClientPicker from "./ClientPicker.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import Notify from "./Notify.svelte";
  import JoinLan from "./JoinLan.svelte";

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
  /** The realm's folder was renamed or moved: its paths are written again. */
  const moved = $derived(instance.moved && !incomplete);

  let keepRunning = $state(false);
  // The default of each realm (`lan` below: a LAN realm stays up for the
  // others when the host closes the game); then the player's choice stays.
  $effect(() => {
    void id;
    keepRunning = lan;
  });
  let error = $state<CommandError | null>(null);
  let updateNote = $state<string | null>(null);

  type Status = "stopped" | "starting" | "online" | "stopping" | "failed" | "unfinished" | "moved";
  const status = $derived.by((): Status => {
    // A state to fix first is the realm's status, in place (never a banner).
    if (!running && !progress && incomplete) return "unfinished";
    if (!running && !progress && moved) return "moved";
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

  // A failed start is kept in the session and shown in the hero: not twice.
  const startOnly = () => act(() => startSession(id).catch(() => {}));
  const stop = () => act(() => api.stop(id));
  /** Play needs the servers online; they stop when the game exits unless kept running. */
  let launching = $state(false);
  function launchGame() {
    launching = true;
    void act(() => api.launchGame(id, keepRunning)).finally(() => setTimeout(() => (launching = false), 3000));
  }

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

  /** What the signed index offers this realm; `failed` when it cannot be read (offline). */
  let serverUpdate = $state<ServerUpdate | "checking" | "failed">("checking");
  // Checked when the realm is shown and again after each change (an update,
  // a repair) has finished: only the small signed index is downloaded.
  $effect(() => {
    const realm = id;
    void instance.record.build;
    if (busy || incomplete) return;
    serverUpdate = "checking";
    api
      .serverUpdate(realm)
      .then((offer) => {
        if (realm === id) serverUpdate = offer;
      })
      .catch(() => {
        if (realm === id) serverUpdate = "failed";
      });
  });
  const updateAvailable = $derived(typeof serverUpdate === "object" && serverUpdate.status === "available");
  const updateText = $derived.by(() => {
    if (updateNote) return updateNote;
    if (serverUpdate === "checking") return t("update.checking");
    if (serverUpdate === "failed") return t("update.checkFailed");
    if (serverUpdate.status === "available")
      return t("update.available", { download: gigabytes(serverUpdate.download) });
    if (serverUpdate.status === "notPublished") return t("update.notPublished");
    return t("update.current");
  });

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

  /** LAN realms: whether the Windows firewall lets other computers in (ADR 0022). */
  const lan = $derived(instance.record.network === "lan");
  let firewall = $state<FirewallStatus | null>(null);
  let firewallBusy = $state(false);
  $effect(() => {
    firewall = null;
    if (!lan) return;
    const realm = id;
    api
      .firewallStatus(realm)
      .then((status) => {
        if (realm === id) firewall = status;
      })
      // Unreadable: offered as not allowed, so the player can still act.
      .catch(() => {
        if (realm === id) firewall = { supported: true, allowed: false, publicNetwork: false };
      });
  });
  function changeFirewall(allow: boolean) {
    const realm = id;
    firewallBusy = true;
    void act(async () => {
      const status = await (allow ? api.allowFirewall(realm) : api.removeFirewall(realm));
      if (realm === id) firewall = status;
    }).finally(() => (firewallBusy = false));
  }
  const firewallText = $derived.by(() => {
    if (firewall?.publicNetwork) return t("firewall.public");
    return firewall?.allowed ? t("firewall.allowed") : t("firewall.short");
  });

  /** Ticks every 30 s while online, for the uptime. */
  let now = $state(Date.now());
  $effect(() => {
    if (status !== "online") return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  const uptime = $derived.by(() => {
    if (!session?.readyAt) return null;
    const minutes = Math.floor(Math.max(0, now - session.readyAt) / 60_000);
    if (minutes < 1) return t("live.justNow");
    if (minutes < 60) return t("progress.minutes", { count: minutes });
    return t("progress.hours", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
  });

  const STATUS_KEYS: Record<Status, MessageKey> = {
    stopped: "status.stopped",
    starting: "status.starting",
    online: "status.online",
    stopping: "status.stopping",
    failed: "status.failed",
    unfinished: "status.unfinished",
    moved: "status.moved",
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
      <Emblem emblem={realm.client.emblem} accent={realm.client.accent} label={localized(realm.client.shortName)} size={56} />
    {/if}
    <div class="hero-title">
      {#if realm.client}
        <p class="realm-name">{localized(realm.client.shortName)} · {realm.client.version}</p>
      {/if}
      <h2
        class="display"
        class:online={status === "online"}
        class:failed={status === "failed"}
        class:attention={status === "unfinished" || status === "moved"}
      >
        {t(STATUS_KEYS[status])}
      </h2>
      <div class="muted subtitle" role={session?.error ? "alert" : undefined}>
        {#if status === "unfinished"}
          {t("home.unfinished")}
        {:else if status === "moved"}
          {t("home.moved")}
        {:else if session?.error}
          <!-- Whatever the phase (a game that cannot launch while online too). -->
          <ErrorBox error={session.error} compact />
        {:else if status === "failed"}
          {t("home.crashed")}
        {:else if status === "online" && session?.game === "running"}
          {session.keepRunning ? t("home.gameRunningKeep") : t("home.gameRunning")}
        {:else if status === "online" && instance.record.network === "lan" && session?.address}
          {t("home.lanAddress", { address: session.address })}
        {:else if status === "online"}
          {t("home.onlineBody")}
        {:else if !clientReady}
          {t("home.needClient")}
        {:else}
          {t("home.readyBody")}
        {/if}
      </div>
    </div>
    <div class="actions">
      <div class="buttons">
        {#if running}
          <button class="btn" disabled={status === "stopping"} onclick={stop}>{t("home.action.stop")}</button>
          {#if session?.game !== "running"}
            <button
              class="btn btn-play"
              disabled={status !== "online" || !clientReady || launching}
              title={status === "online" ? undefined : t("home.playWhenOnline")}
              onclick={launchGame}
            >
              {t("home.action.play")}
            </button>
          {/if}
        {:else if status === "unfinished" || status === "moved"}
          <button class="btn btn-play" disabled={busy} onclick={resume}>
            {status === "unfinished" ? t("home.action.resume") : t("home.action.relocate")}
          </button>
        {:else}
          <button class="btn" disabled={busy || incomplete} onclick={startOnly}>
            {t("home.action.startServers")}
          </button>
          <button class="btn btn-play" disabled title={t("home.playWhenOnline")}>
            {t("home.action.play")}
          </button>
        {/if}
      </div>
      {#if status !== "unfinished" && status !== "moved" && session?.game !== "running"}
        <label class="keep">
          <input type="checkbox" bind:checked={keepRunning} />
          <small>{t("home.keepRunning")}</small>
        </label>
      {/if}
      {#if preferences.value}
        <label class="keep" title={t("home.hideWhilePlayingHint")}>
          <input
            type="checkbox"
            checked={preferences.value.hideWhilePlaying}
            onchange={(e) => void preferences.set({ hideWhilePlaying: e.currentTarget.checked })}
          />
          <small>{t("home.hideWhilePlaying")}</small>
        </label>
      {/if}
    </div>
  </div>

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
      <!-- Always one line (never moves the hero): checked, up to date or not. -->
      <dd
        class="build-tag"
        class:available={updateAvailable}
        class:current={typeof serverUpdate === "object" && !updateAvailable}
      >
        {#if serverUpdate === "checking"}
          {t("home.stat.buildChecking")}
        {:else if serverUpdate === "failed"}
          {t("home.stat.buildUnchecked")}
        {:else if updateAvailable}
          {t("home.stat.buildAvailable")}
        {:else}
          {t("home.stat.buildCurrent")}
        {/if}
      </dd>
    </div>
  </dl>
</section>

<!-- Action errors and warnings float over the page (toasts). -->
<Notify {error} />

<div class="columns">
  <section class="panel column">
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
          {#if state === "starting" && session?.startup?.process === process}
            <!-- Right under the server it belongs to. -->
            <div class="startup" role="status">
              <div class="bar" aria-hidden="true">
                <span style:width={`${Math.round(session.startup.progress * 100)}%`}></span>
              </div>
              <p class="muted">
                {localized(session.startup.label)} · {Math.round(session.startup.progress * 100)} %
              </p>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if status === "online" && session}
      <h4 class="sub-title">{t("live.title")}</h4>
      <ul class="rows live">
        {#if uptime}
          <li>
            <span class="row-name muted">{t("live.uptime")}</span>
            <span class="value">{uptime}</span>
          </li>
        {/if}
        <li>
          <span class="row-name muted">{t("live.players")}</span>
          <span class="value">{session.population?.players ?? "…"}</span>
        </li>
        <li>
          <span class="row-name muted">{t("live.bots")}</span>
          <span class="value">{session.population?.bots ?? "…"}</span>
        </li>
        {#if session.authPort}
          <li title={t("live.addressHint")}>
            <span class="row-name muted">{t("live.address")}</span>
            <code class="value">{session.address ?? "127.0.0.1"}:{session.authPort}</code>
          </li>
        {/if}
      </ul>
    {/if}
  </section>

  <section class="panel column">
    <h3 class="panel-title">{t("home.client")}</h3>
    {#if realm.client}
      <ClientPicker client={realm.client} registered={client} />
    {/if}
    <div class="foot">
      <JoinLan {realm} {client} />
    </div>
  </section>

  <section class="panel column">
    <h3 class="panel-title">{t("maintenance.title")}</h3>
    <div class="task">
      <p class="muted" class:offer={updateAvailable} title={t("update.body")}>{updateText}</p>
      <button
        class="btn"
        class:offer={updateAvailable}
        disabled={busy || running || incomplete || (typeof serverUpdate === "object" && !updateAvailable)}
        title={running && updateAvailable ? t("update.stopFirst") : undefined}
        onclick={update}
      >
        {t("update.action")}
      </button>
    </div>
    <div class="task">
      <p class="muted" title={t("repair.body")}>{repairNote ?? t("repair.short")}</p>
      <button class="btn" disabled={busy || running || incomplete} onclick={repair}>
        {t("repair.action")}
      </button>
    </div>
    {#if lan && firewall?.supported !== false}
      <div class="task">
        <p class="muted" title={t("firewall.body")}>{firewallText}</p>
        <button
          class="btn"
          disabled={firewall === null || firewallBusy}
          title={t("firewall.body")}
          onclick={() => changeFirewall(!firewall?.allowed)}
        >
          {firewall?.allowed ? t("firewall.remove") : t("firewall.allow")}
        </button>
      </div>
    {/if}
  </section>
</div>

<style>
  .hero {
    padding-top: var(--space-3);
    padding-bottom: var(--space-3);
  }

  .hero-head {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .hero-title {
    flex: 1;
    min-width: 0;
  }

  .realm-name {
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 0.8rem;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--realm-accent);
  }

  h2 {
    margin: 0;
    font-size: 1.45rem;
    line-height: 1.25;
    color: var(--title);
    text-shadow: var(--text-shadow);
  }

  h2.online {
    color: var(--ok);
  }

  h2.failed {
    color: var(--danger);
  }

  h2.attention {
    color: var(--accent);
  }

  .subtitle {
    margin: 0;
  }

  .actions {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: var(--space-2);
  }

  .buttons {
    display: flex;
    gap: var(--space-3);
    align-items: center;
  }

  .keep {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    margin: var(--space-3) 0 0;
    padding-top: var(--space-2);
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

  /* Whether the installed server is the published one. */
  .build-tag {
    margin-top: 0;
    font-size: 0.72rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .build-tag.current {
    color: var(--ok);
  }

  .build-tag.available {
    color: var(--accent);
    font-weight: 700;
  }

  /* A server update is waiting: its line and button stand out. */
  p.offer {
    color: var(--title);
  }

  .btn.offer:not(:disabled) {
    border-color: var(--accent-strong);
    color: var(--title);
    box-shadow: 0 0 10px var(--glow);
  }

  /* Fills the rest of the page: three columns of equal height. */
  .columns {
    flex: 1;
    /* Never clipped: on a very small window the page scrolls instead. */
    min-height: auto;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-4);
  }

  .column {
    display: flex;
    flex-direction: column;
  }

  .foot {
    margin-top: auto;
  }

  .task {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
  }

  .task + .task {
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .task p {
    margin: 0;
    font-size: 0.88rem;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .rows li {
    display: flex;
    flex-wrap: wrap;
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

  .sub-title {
    margin: var(--space-4) 0 var(--space-1);
    color: var(--text-muted);
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .live .value {
    color: var(--title);
    font-variant-numeric: tabular-nums;
  }

  /* Full width under the starting server's row, aligned with its name. */
  .startup {
    flex-basis: 100%;
    padding-left: calc(9px + var(--space-3));
  }

  .startup p {
    margin: var(--space-1) 0 0;
    font-size: 0.85rem;
  }

  .bar {
    height: 6px;
    background: var(--border);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.6s ease;
  }

  @media (prefers-reduced-motion: reduce) {
    .bar span {
      transition: none;
    }
  }
</style>
