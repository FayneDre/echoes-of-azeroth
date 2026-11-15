<script lang="ts">
  import "@fontsource-variable/cinzel";
  import { api, asCommandError, inDesktopApp, openExternal, type CommandError, type EmulatorOverview } from "./lib/api";
  import Account from "./lib/components/Account.svelte";
  import Console from "./lib/components/Console.svelte";
  import CrashNotice from "./lib/components/CrashNotice.svelte";
  import AppUpdateNotice from "./lib/components/AppUpdateNotice.svelte";
  import ErrorBox from "./lib/components/ErrorBox.svelte";
  import { connect, launcher } from "./lib/launcher.svelte";
  import Emblem from "./lib/emblems/Emblem.svelte";
  import Home from "./lib/components/Home.svelte";
  import Rail from "./lib/components/Rail.svelte";
  import ServerSettings from "./lib/components/ServerSettings.svelte";
  import WindowControls from "./lib/components/WindowControls.svelte";
  import { i18n, LOCALES, t, type MessageKey } from "./lib/i18n/i18n.svelte";
  import { processLabel } from "./lib/labels";

  type CatalogState =
    | { status: "loading" }
    | { status: "unavailable" }
    | { status: "error"; error: CommandError }
    | { status: "ready"; realms: EmulatorOverview[] };

  const ISSUES_URL = "https://github.com/FayneDre/echoes-of-azeroth/issues";

  type TabId = "home" | "account" | "settings" | "bots" | "backups";

  const TABS: { id: TabId; label: MessageKey; ready: boolean }[] = [
    { id: "home", label: "nav.home", ready: true },
    { id: "account", label: "nav.account", ready: true },
    { id: "settings", label: "nav.settings", ready: true },
    { id: "bots", label: "nav.bots", ready: false },
    { id: "backups", label: "nav.backups", ready: false },
  ];

  let catalog = $state<CatalogState>({ status: "loading" });
  let selectedId = $state<string | null>(null);
  let tab = $state<TabId>("home");
  /** Process whose output is shown in the console drawer. */
  let terminal = $state<string | null>(null);

  const realms = $derived(catalog.status === "ready" ? catalog.realms : []);
  const realm = $derived(realms.find((r) => r.id === selectedId) ?? realms[0] ?? null);
  const instanceId = $derived(
    launcher.overview?.instances.find((i) => i.record.emulator === realm?.id)?.record.id ?? null,
  );
  const session = $derived(instanceId ? launcher.session(instanceId) : null);

  $effect(() => {
    document.documentElement.lang = i18n.locale;
  });

  $effect(() => {
    if (!inDesktopApp()) {
      catalog = { status: "unavailable" };
      return;
    }
    connect();
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
      <span class="logo"><Emblem emblem="generic" accent="var(--accent)" label="" size={40} /></span>
      <div class="brand-text">
        <h1 class="display">{t("app.title")}</h1>
        <p class="muted">{t("app.tagline")}</p>
      </div>
    </div>

    <nav class="segmented tabs" aria-label={t("nav.sections")}>
      {#each TABS as item (item.id)}
        <button
          class:active={tab === item.id}
          disabled={!item.ready}
          title={item.ready ? undefined : t("common.soon")}
          aria-current={tab === item.id ? "page" : undefined}
          onclick={() => (tab = item.id)}
        >
          {t(item.label)}
        </button>
      {/each}
    </nav>

    <div class="segmented" role="group" aria-label={t("nav.language")}>
      {#each LOCALES as locale (locale.id)}
        <button
          class:active={i18n.locale === locale.id}
          aria-pressed={i18n.locale === locale.id}
          title={locale.label}
          onclick={() => (i18n.locale = locale.id)}
        >
          {locale.id.toUpperCase()}
        </button>
      {/each}
    </div>

    <WindowControls />
  </header>

  <div class="body">
    <Rail {realms} selected={realm?.id ?? null} onselect={(id) => (selectedId = id)} />

    <div class="main">
      <main>
        {#if inDesktopApp()}
          <CrashNotice />
          <AppUpdateNotice />
        {/if}
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
        {:else}
          <Home {realm} />
        {/if}
      </main>

      {#if realm}
        <div class="terminals">
          <span class="display terminals-title">▸ {t("terminals.title")}</span>
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
        {#if terminal && instanceId && session}
          <Console
            instance={instanceId}
            process={terminal}
            console={realm.console === terminal}
            onclose={() => (terminal = null)}
          />
        {/if}
      {/if}

      <footer>
        <small class="notice" title={t("footer.disclaimer")}>{t("footer.short")}</small>
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
    font-size: 1.45rem;
    line-height: 1.2;
    color: var(--title);
    text-shadow:
      0 0 12px var(--glow),
      var(--text-shadow);
  }

  .brand-text {
    min-width: 0;
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

  /* Narrow windows (down to the 1024 px minimum): the logo stays, the title
     text goes, so the tabs never wrap. */
  @media (max-width: 1100px) {
    .brand-text {
      display: none;
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

  main {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-5) var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
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

  .terminals .btn.active {
    border-color: var(--frame-light);
    box-shadow: 0 0 8px var(--glow);
  }

  .terminals {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-6);
    border-top: 2px solid var(--frame);
    background: linear-gradient(0deg, var(--shade), transparent);
  }

  .terminals-title {
    margin-right: var(--space-3);
    color: var(--title);
    font-size: 0.85rem;
    text-transform: uppercase;
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

  footer a {
    flex: none;
    color: var(--accent);
    font-size: 0.82rem;
  }
</style>
