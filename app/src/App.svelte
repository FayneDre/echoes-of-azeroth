<script lang="ts">
  import "@fontsource-variable/cinzel";
  import { catalogOverview, inDesktopApp, type EmulatorOverview } from "./lib/api";
  import Home from "./lib/components/Home.svelte";
  import Rail from "./lib/components/Rail.svelte";
  import ServerSettings from "./lib/components/ServerSettings.svelte";
  import { i18n, LOCALES, t, type MessageKey } from "./lib/i18n/i18n.svelte";
  import { processLabel } from "./lib/labels";

  type CatalogState =
    | { status: "loading" }
    | { status: "unavailable" }
    | { status: "error"; message: string }
    | { status: "ready"; realms: EmulatorOverview[] };

  type TabId = "home" | "account" | "settings" | "bots" | "backups";

  const TABS: { id: TabId; label: MessageKey; ready: boolean }[] = [
    { id: "home", label: "nav.home", ready: true },
    { id: "account", label: "nav.account", ready: false },
    { id: "settings", label: "nav.settings", ready: true },
    { id: "bots", label: "nav.bots", ready: false },
    { id: "backups", label: "nav.backups", ready: false },
  ];

  let catalog = $state<CatalogState>({ status: "loading" });
  let selectedId = $state<string | null>(null);
  let tab = $state<TabId>("home");

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

      <nav class="tabs" aria-label={t("nav.sections")}>
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
      {:else if !realm}
        <p class="muted">{t("modules.empty")}</p>
      {:else if tab === "settings"}
        <ServerSettings {realm} />
      {:else}
        <Home {realm} />
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
      <small class="notice" title={t("footer.disclaimer")}>{t("footer.short")}</small>
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
    padding: var(--space-3) var(--space-6);
    background: linear-gradient(180deg, rgb(0 0 0 / 0.35), transparent);
    border-bottom: 2px solid var(--frame);
    box-shadow: 0 1px 0 color-mix(in srgb, var(--frame-light) 50%, transparent);
  }

  .brand {
    flex: none;
  }

  .brand h1 {
    margin: 0;
    font-size: 1.55rem;
    color: var(--title);
    text-shadow:
      0 0 12px rgb(224 176 79 / 0.25),
      0 2px 3px rgb(0 0 0 / 0.8);
  }

  .brand p {
    margin: 0;
    font-size: 0.82rem;
    font-style: italic;
  }

  .tabs,
  .lang {
    display: flex;
    gap: var(--space-1);
  }

  .tabs {
    margin: 0 auto;
  }

  .tabs button,
  .lang button {
    padding: var(--space-2) var(--space-4);
    background: var(--button-bg);
    border: 1px solid var(--frame);
    border-radius: var(--radius) var(--radius) 0 0;
    font-family: var(--font-display);
    font-size: 0.82rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    cursor: pointer;
  }

  .lang button {
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
  }

  .tabs button:hover:not(:disabled),
  .lang button:hover {
    color: var(--title);
  }

  .tabs button:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }

  .tabs button.active,
  .lang button.active {
    color: var(--title);
    border-color: var(--frame-light);
    box-shadow:
      inset 0 -2px 0 var(--accent-strong),
      0 0 10px rgb(224 176 79 / 0.2);
    text-shadow: 0 1px 2px rgb(0 0 0 / 0.8);
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

  .error {
    color: var(--danger);
  }

  .terminals {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-6);
    border-top: 2px solid var(--frame);
    background: linear-gradient(0deg, rgb(0 0 0 / 0.35), transparent);
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
