<script lang="ts">
  import "@fontsource-variable/cinzel";
  // Only the two ornate capitals of the wordmark use it.
  import "@fontsource/cinzel-decorative/latin-700.css";
  import {
    api,
    asCommandError,
    copyText,
    inDesktopApp,
    openExternal,
    type CommandError,
    type EmulatorOverview,
  } from "./lib/api";
  import Bots from "./lib/components/Bots.svelte";
  import Backups from "./lib/components/Backups.svelte";
  import Account from "./lib/components/Account.svelte";
  import Console from "./lib/components/Console.svelte";
  import CrashNotice from "./lib/components/CrashNotice.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import AppUpdate from "./lib/components/AppUpdate.svelte";
  import ErrorBox from "./lib/components/ErrorBox.svelte";
  import ProgressPanel from "./lib/components/ProgressPanel.svelte";
  import { connect, launcher } from "./lib/launcher.svelte";
  import Logo from "./lib/Logo.svelte";
  import Home from "./lib/components/Home.svelte";
  import Rail from "./lib/components/Rail.svelte";
  import ServerSettings from "./lib/components/ServerSettings.svelte";
  import WindowControls from "./lib/components/WindowControls.svelte";
  import Options from "./lib/components/Options.svelte";
  import WhatsNew from "./lib/components/WhatsNew.svelte";
  import MissingLocation from "./lib/components/MissingLocation.svelte";
  import { loadPreferences } from "./lib/preferences.svelte";
  import { i18n, t, type MessageKey } from "./lib/i18n/i18n.svelte";
  import { processLabel } from "./lib/labels";

  type CatalogState =
    | { status: "loading" }
    | { status: "unavailable" }
    | { status: "error"; error: CommandError }
    | { status: "ready"; realms: EmulatorOverview[] };

  const ISSUES_URL = "https://github.com/FayneDre/realmkeeper/issues";

  type TabId = "home" | "account" | "settings" | "bots" | "backups";

  /** Every tab but Home acts on an installed server. */
  const TABS: { id: TabId; label: MessageKey; needsServer: boolean }[] = [
    { id: "home", label: "nav.home", needsServer: false },
    { id: "account", label: "nav.account", needsServer: true },
    { id: "settings", label: "nav.settings", needsServer: true },
    { id: "bots", label: "nav.bots", needsServer: true },
    { id: "backups", label: "nav.backups", needsServer: true },
  ];

  let catalog = $state<CatalogState>({ status: "loading" });
  let selectedId = $state<string | null>(null);
  let tab = $state<TabId>("home");
  /** Process whose output is shown in the console drawer. */
  let terminal = $state<string | null>(null);

  const realms = $derived(catalog.status === "ready" ? catalog.realms : []);
  const realm = $derived(realms.find((r) => r.id === selectedId) ?? realms[0] ?? null);
  const instance = $derived(launcher.instanceFor(realm?.id));
  const instanceId = $derived(instance?.record.id ?? null);
  const serverInstalled = $derived(instance?.installed ?? false);
  const tabEnabled = (item: (typeof TABS)[number]) => !item.needsServer || serverInstalled;

  // A realm without a server (another realm picked, or one just uninstalled) only has Home.
  $effect(() => {
    const current = TABS.find((item) => item.id === tab);
    if (current && !tabEnabled(current)) tab = "home";
  });
  const session = $derived(instanceId ? launcher.session(instanceId) : null);

  $effect(() => {
    document.documentElement.lang = i18n.locale;
  });

  // The notification area menu speaks the player's language.
  $effect(() => {
    if (!inDesktopApp()) return;
    void api
      .setTrayTexts({
        show: t("tray.show"),
        stop: t("tray.stop"),
        quit: t("tray.quit"),
        shutdown: t("os.shutdownReason"),
        awake: t("os.awakeReason"),
        ready: t("os.realmReady"),
      })
      .catch(() => {});
  });

  /** Diagnostic report copy: "copied" or "failed" shortly after a click. */
  let diagnostic = $state<"idle" | "working" | "copied" | "failed">("idle");
  async function copyDiagnostic() {
    diagnostic = "working";
    try {
      await copyText(await api.diagnosticReport());
      diagnostic = "copied";
    } catch {
      diagnostic = "failed";
    }
    setTimeout(() => (diagnostic = "idle"), 4000);
  }

  let optionsOpen = $state(false);

  $effect(() => {
    if (!inDesktopApp()) {
      catalog = { status: "unavailable" };
      return;
    }
    connect();
    loadPreferences();
    api
      .catalogOverview()
      .then((list) => (catalog = { status: "ready", realms: list }))
      .catch((error: unknown) => (catalog = { status: "error", error: asCommandError(error) }));
  });
</script>

{#if launcher.closing}
  <div class="closing" role="alert" aria-live="assertive">
    <p class="display">{t("app.closing")}</p>
  </div>
{/if}

<div class="app" style:--realm-accent={realm?.client?.accent}>
  <!-- Own title bar (native decorations are off): drag anywhere that is not a control. -->
  <header class="titlebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <span class="logo"><Logo size={42} /></span>
      <div class="brand-text">
        <!-- The wordmark (ADR 0021): ornate R and K, the rest in Cinzel. -->
        <h1 class="display wordmark" aria-label={t("app.title")}>
          <span class="cap">R</span>ealm<span class="cap">K</span>eeper
        </h1>
        <p class="muted">{t("app.tagline")}</p>
      </div>
    </div>

    <nav class="segmented tabs" aria-label={t("nav.sections")}>
      {#each TABS as item (item.id)}
        <button
          class:active={tab === item.id}
          disabled={!tabEnabled(item)}
          title={tabEnabled(item) ? undefined : t("nav.needsServer")}
          aria-current={tab === item.id ? "page" : undefined}
          onclick={() => (tab = item.id)}
        >
          {t(item.label)}
        </button>
      {/each}
    </nav>

    {#if inDesktopApp()}
      <AppUpdate />
      <WhatsNew />
    {/if}

    {#if inDesktopApp()}
      <button
        class="options-button"
        title={t("options.open")}
        aria-label={t("options.open")}
        onclick={() => (optionsOpen = true)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="3.2" />
          <path
            d="M12 2.5l1.6 2.6 3-.7.7 3 2.6 1.6-1.4 2.7 1.4 2.7-2.6 1.6-.7 3-3-.7L12 21.5l-1.6-2.6-3 .7-.7-3-2.6-1.6 1.4-2.7-1.4-2.7 2.6-1.6.7-3 3 .7z"
          />
        </svg>
      </button>
    {/if}

    <WindowControls />
  </header>

  {#if optionsOpen}
    <Options onclose={() => (optionsOpen = false)} />
  {/if}

  <div class="body">
    <Rail {realms} selected={realm?.id ?? null} onselect={(id) => (selectedId = id)} />

    <div class="main">
      <div class="stage">
      <main>
        {#if catalog.status === "loading"}
          <p class="muted">{t("modules.loading")}</p>
        {:else if catalog.status === "unavailable"}
          <p class="muted">{t("modules.unavailable")}</p>
        {:else if catalog.status === "error"}
          <ErrorBox error={catalog.error} />
        {:else if !realm}
          <p class="muted">{t("modules.empty")}</p>
        {:else if tab === "settings"}
          <ServerSettings {realm} />
        {:else if tab === "account"}
          <Account {realm} />
        {:else if tab === "bots"}
          <Bots {realm} />
        {:else if tab === "backups"}
          <Backups {realm} />
        {:else}
          <Home {realm} />
        {/if}
      </main>

      <!-- Over the page, never in its flow: the pages are laid out to fit the
           window without a scrollbar, whatever is in progress. -->
      {#if launcher.progress}
        <div class="overlay">
          <ProgressPanel
            progress={launcher.progress}
            title={launcher.progress.title
              ? t(launcher.progress.title)
              : launcher.overview?.instances.find((i) => i.record.id === launcher.progress?.instance)?.installed
                ? t("progress.title")
                : t("wizard.installing")}
          />
        </div>
      {/if}
      <!-- The console drawer opens over the page (bottom), never moving it. -->
      {#if realm && terminal && instanceId && session}
        <div class="drawer">
          <Console
            instance={instanceId}
            process={terminal}
            console={realm.console === terminal}
            onclose={() => (terminal = null)}
          />
        </div>
      {/if}
      <!-- Notices and toasts float over the page (bottom right): something
           happening never moves the layout. -->
      <div class="notices">
        {#if inDesktopApp()}
          <MissingLocation />
          <CrashNotice />
        {/if}
        <Toasts />
      </div>
      </div>

      {#if realm}
        <div class="terminals">
          <button
            class="display terminals-title"
            disabled={!session}
            aria-expanded={terminal !== null}
            title={session ? undefined : t("terminals.notStarted")}
            onclick={() => (terminal = terminal ? null : (realm.console ?? realm.processes[0] ?? null))}
          >
            <span class="caret" class:open={terminal !== null} aria-hidden="true">▸</span>
            {t("terminals.title")}
          </button>
          {#each realm.processes as id (id)}
            <button
              class="btn"
              class:active={terminal === id}
              disabled={!session}
              title={session ? undefined : t("terminals.notStarted")}
              onclick={() => (terminal = terminal === id ? null : id)}
            >
              {processLabel(id)}
            </button>
          {/each}
        </div>
      {/if}

      <footer>
        <small class="notice" title={t("footer.disclaimer")}>{t("footer.short")}</small>
        {#if inDesktopApp()}
          <button
            class="footer-link"
            disabled={diagnostic === "working"}
            title={t("diagnostic.hint")}
            onclick={copyDiagnostic}
          >
            {diagnostic === "copied"
              ? t("diagnostic.copied")
              : diagnostic === "failed"
                ? t("diagnostic.failed")
                : t("diagnostic.copy")}
          </button>
        {/if}
        <a
          href={ISSUES_URL}
          onclick={(event) => {
            event.preventDefault();
            void openExternal(ISSUES_URL);
          }}
        >
          {t("footer.report")}
        </a>
      </footer>
    </div>
  </div>
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    border: 1px solid var(--frame);
  }

  .options-button {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    padding: 0;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius, 4px);
    color: var(--text-muted);
    cursor: pointer;
  }

  .options-button:hover {
    color: var(--title);
    border-color: var(--accent);
  }

  .options-button svg {
    width: 22px;
    height: 22px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linejoin: round;
  }

  .titlebar {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    background: linear-gradient(180deg, var(--shade), transparent);
    border-bottom: 2px solid var(--frame);
    box-shadow: 0 1px 0 color-mix(in srgb, var(--frame-light) 50%, transparent);
    user-select: none;
  }

  .brand {
    /* The title gives way first (tagline, then text): the window controls
       must always stay in view. */
    flex: 0 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) 0;
  }

  /* Clicks fall through to the drag region. */
  .brand > * {
    pointer-events: none;
  }

  .logo {
    display: grid;
    place-items: center;
    width: var(--rail-width);
  }

  .brand h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
    letter-spacing: 0.05em;

    line-height: 1.2;
    color: var(--title);
    text-shadow:
      0 0 12px var(--glow),
      var(--text-shadow);
  }

  .brand-text {
    min-width: 0;
    overflow: hidden;
  }

  .brand h1 {
    white-space: nowrap;
  }

  .wordmark .cap {
    font-family: "Cinzel Decorative", var(--font-display);
    font-weight: 700;
    font-size: 1.3em;
    line-height: 1;
    color: var(--wordmark-cap);
  }

  .brand p {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    margin: 0;
    font-size: 0.8rem;
    font-style: italic;
  }

  .tabs {
    flex: none;
    margin-left: auto;
  }

  /* Narrower windows (down to the 1024 px minimum): the tagline goes, then
     the title text and the tabs get tighter; the logo, tabs, options and
     window controls always fit (the language is in the Options window). */
  @media (max-width: 1700px) {
    .brand p {
      display: none;
    }
  }

  @media (max-width: 1220px) {
    .brand-text {
      display: none;
    }

    .titlebar {
      gap: var(--space-3);
    }
    .titlebar :global(.segmented button) {
      padding-inline: var(--space-2);
      letter-spacing: 0.04em;
    }
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }

  /* Every page is laid out to fit the minimum window (1024 x 760) without a
     scrollbar; `auto` stays only as a safety net. */
  main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: var(--space-4) var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .overlay {
    position: absolute;
    inset: 0;
    z-index: 5;
    display: grid;
    place-items: center;
    padding: var(--space-6);
    background: var(--shade-deep);
    backdrop-filter: blur(2px);
  }

  .overlay > :global(*) {
    width: min(640px, 100%);
  }

  .notices {
    position: absolute;
    right: var(--space-5);
    bottom: var(--space-4);
    /* Over the modals (z-index 9): an error raised from a dialog shows. */
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    width: min(360px, calc(100% - 2 * var(--space-5)));
    pointer-events: none;
  }

  .notices > :global(*) {
    pointer-events: auto;
  }

  .closing {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    background: var(--shade-deep);
    backdrop-filter: blur(2px);
  }

  .closing p {
    padding: var(--space-5) var(--space-8);
    border: 2px solid var(--frame-light);
    background: var(--bg-elevated);
    color: var(--title);
  }

  .terminals .btn {
    padding: 2px var(--space-3);
    font-size: 0.85rem;
  }

  .terminals .btn.active {
    border-color: var(--frame-light);
    box-shadow: 0 0 8px var(--glow);
  }

  .terminals {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-5);
    border-top: 2px solid var(--frame);
    background: linear-gradient(0deg, var(--shade), transparent);
  }

  .terminals-title {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    margin-right: var(--space-3);
    padding: 0;
    border: none;
    background: none;
    color: var(--title);
    font-size: 0.85rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .terminals-title:disabled {
    cursor: default;
    opacity: 0.6;
  }

  .caret {
    display: inline-block;
    transition: transform 0.15s ease;
  }

  .caret.open {
    transform: rotate(90deg);
  }

  .drawer {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 4;
    height: min(55%, 420px);
    display: flex;
    box-shadow: 0 -8px 24px rgb(0 0 0 / 0.45);
  }

  .drawer > :global(*) {
    flex: 1;
    min-width: 0;
  }

  footer {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-1) var(--space-6);
    border-top: 1px solid var(--border);
    color: var(--text-muted);
  }

  .notice {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  footer a,
  .footer-link {
    flex: none;
    color: var(--accent);
    font-size: 0.82rem;
  }

  .footer-link {
    padding: 0;
    border: none;
    background: none;
    font-family: inherit;
    text-decoration: underline;
    cursor: pointer;
  }

  .footer-link:disabled {
    cursor: progress;
    opacity: 0.6;
  }
</style>
