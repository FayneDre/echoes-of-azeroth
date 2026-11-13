<script lang="ts">
  import "@fontsource-variable/cinzel";
  import { catalogOverview, inDesktopApp, type EmulatorOverview } from "./lib/api";
  import Home from "./lib/components/Home.svelte";
  import Rail from "./lib/components/Rail.svelte";
  import { i18n, LOCALES, t, type MessageKey } from "./lib/i18n/i18n.svelte";
  import { processLabel } from "./lib/labels";

  type CatalogState =
    | { status: "loading" }
    | { status: "unavailable" }
    | { status: "error"; message: string }
    | { status: "ready"; realms: EmulatorOverview[] };

  const TABS: { id: string; label: MessageKey; ready: boolean }[] = [
    { id: "home", label: "nav.home", ready: true },
    { id: "account", label: "nav.account", ready: false },
    { id: "settings", label: "nav.settings", ready: false },
    { id: "bots", label: "nav.bots", ready: false },
    { id: "backups", label: "nav.backups", ready: false },
  ];

  let catalog = $state<CatalogState>({ status: "loading" });
  let selectedId = $state<string | null>(null);

  const realms = $derived(catalog.status === "ready" ? catalog.realms : []);
  const realm = $derived(realms.find((r) => r.id === selectedId) ?? realms[0] ?? null);

  $effect(() => {
    document.documentElement.lang = i18n.locale;
  });

  $effect(() => {
    if (!inDesktopApp()) {
      catalog = { status: "unavailable" };
      return;
    }
    catalogOverview()
      .then((list) => (catalog = { status: "ready", realms: list }))
      .catch((error: unknown) => (catalog = { status: "error", message: String(error) }));
  });
</script>

<div class="app" style:--realm-accent={realm?.client?.accent}>
  <Rail {realms} selected={realm?.id ?? null} onselect={(id) => (selectedId = id)} />

  <div class="main">
    <header class="topbar">
      <div class="brand">
        <h1 class="display">{t("app.title")}</h1>
        <p class="muted">{t("app.tagline")}</p>
      </div>

      <nav class="tabs" aria-label={t("nav.home")}>
        {#each TABS as tab (tab.id)}
          <button
            class="tab"
            class:active={tab.id === "home"}
            disabled={!tab.ready}
            title={tab.ready ? undefined : t("common.soon")}
            aria-current={tab.id === "home" ? "page" : undefined}
          >
            {t(tab.label)}
          </button>
        {/each}
      </nav>

      <div class="lang" role="group" aria-label={t("nav.language")}>
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
    </header>

    <main>
      {#if catalog.status === "loading"}
        <p class="muted">{t("modules.loading")}</p>
      {:else if catalog.status === "unavailable"}
        <p class="muted">{t("modules.unavailable")}</p>
      {:else if catalog.status === "error"}
        <p class="error">{t("modules.error", { message: catalog.message })}</p>
      {:else if realm}
        <Home {realm} />
      {:else}
        <p class="muted">{t("modules.empty")}</p>
      {/if}
    </main>

    {#if realm}
      <div class="terminals">
        <span class="display terminals-title">▸ {t("terminals.title")}</span>
        {#each ["database", ...realm.processes] as id (id)}
          <button class="btn" disabled title={t("common.soon")}>{processLabel(id)}</button>
        {/each}
      </div>
    {/if}

    <footer>
      <small>{t("footer.disclaimer")}</small>
      <a href="https://github.com/FayneDre/echoes-of-azeroth/issues" target="_blank" rel="noreferrer">
        {t("footer.report")}
      </a>
    </footer>
  </div>
</div>

<style>
  .app {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .topbar {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
  }

  .brand h1 {
    margin: 0;
    font-size: 1.5rem;
    color: var(--accent);
  }

  .brand p {
    margin: 0;
    font-size: 0.85rem;
    font-style: italic;
  }

  .tabs,
  .lang {
    display: flex;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .tabs {
    margin: 0 auto;
  }

  .tabs button,
  .lang button {
    padding: var(--space-2) var(--space-4);
    background: var(--surface);
    border: none;
    border-right: 1px solid var(--border);
    font-family: var(--font-display);
    font-size: 0.85rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    cursor: pointer;
  }

  .tabs button:last-child,
  .lang button:last-child {
    border-right: none;
  }

  .tabs button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .tabs button.active,
  .lang button.active {
    color: var(--accent);
    background: var(--surface-muted);
    box-shadow: inset 0 -2px 0 var(--accent);
  }

  main {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .error {
    color: var(--danger);
  }

  .terminals {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-6);
    border-top: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-wrap: wrap;
  }

  .terminals-title {
    margin-right: var(--space-3);
    color: var(--accent);
    font-size: 0.9rem;
    text-transform: uppercase;
  }

  footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-6);
    border-top: 1px solid var(--border);
    color: var(--text-muted);
  }

  footer a {
    flex: none;
    color: var(--realm-accent);
    font-size: 0.85rem;
  }
</style>
