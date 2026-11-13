<script lang="ts">
  import { catalogOverview, inDesktopApp, type EmulatorOverview } from "./lib/api";
  import { i18n, LOCALES, localized, t, type MessageKey } from "./lib/i18n/i18n.svelte";

  type CatalogState =
    | { status: "loading" }
    | { status: "unavailable" }
    | { status: "error"; message: string }
    | { status: "ready"; emulators: EmulatorOverview[] };

  let catalog = $state<CatalogState>({ status: "loading" });

  $effect(() => {
    document.documentElement.lang = i18n.locale;
  });

  $effect(() => {
    if (!inDesktopApp()) {
      catalog = { status: "unavailable" };
      return;
    }
    catalogOverview()
      .then((emulators) => (catalog = { status: "ready", emulators }))
      .catch((error: unknown) => (catalog = { status: "error", message: String(error) }));
  });
</script>

<div class="shell">
  <header class="topbar">
    <div class="brand">
      <h1>{t("app.title")}</h1>
      <p>{t("app.tagline")}</p>
    </div>
    <label class="language">
      <span>{t("nav.language")}</span>
      <select bind:value={i18n.locale}>
        {#each LOCALES as locale (locale.id)}
          <option value={locale.id}>{locale.label}</option>
        {/each}
      </select>
    </label>
  </header>

  <main>
    <section class="card hero">
      <h2>{t("home.install.title")}</h2>
      <p>{t("home.install.body")}</p>
      <button class="primary" disabled title={t("home.install.soon")}>
        {t("home.install.cta")}
      </button>
      <small>{t("home.install.soon")}</small>
    </section>

    <section class="card">
      <h2>{t("modules.title")}</h2>
      {#if catalog.status === "loading"}
        <p class="muted">{t("modules.loading")}</p>
      {:else if catalog.status === "unavailable"}
        <p class="muted">{t("modules.unavailable")}</p>
      {:else if catalog.status === "error"}
        <p class="error">{t("modules.error", { message: catalog.message })}</p>
      {:else}
        {#each catalog.emulators as emulator (emulator.id)}
          <h3>{emulator.name}</h3>
          <p class="muted">{localized(emulator.description)}</p>
          <ul class="modules">
            {#each emulator.modules as module (module.id)}
              <li>
                <div class="module-head">
                  <strong>{localized(module.name)}</strong>
                  <span class="tag">{t(`category.${module.category}` as MessageKey)}</span>
                </div>
                <p>{localized(module.description)}</p>
                <small class:mandatory={module.mandatory}>
                  {module.mandatory ? t("modules.mandatory") : t("modules.optional")}
                </small>
              </li>
            {/each}
          </ul>
        {/each}
      {/if}
    </section>
  </main>

  <footer>
    <small>{t("footer.client")}</small>
  </footer>
</div>

<style>
  .shell {
    max-width: 960px;
    margin: 0 auto;
    padding: var(--space-6) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    min-height: 100%;
  }

  .topbar {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .brand h1 {
    margin: 0;
    font-size: 1.6rem;
  }

  .brand p,
  .muted {
    margin: var(--space-1) 0 0;
    color: var(--text-muted);
  }

  .language {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text-muted);
  }

  select {
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
  }

  main {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-6);
  }

  .card h2 {
    margin: 0 0 var(--space-2);
    font-size: 1.15rem;
  }

  .hero small {
    margin-left: var(--space-3);
    color: var(--text-muted);
  }

  .primary {
    background: var(--accent);
    color: var(--accent-text);
    border: none;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    font-weight: 600;
    cursor: pointer;
  }

  .primary:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .modules {
    list-style: none;
    margin: var(--space-4) 0 0;
    padding: 0;
    display: grid;
    gap: var(--space-3);
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  }

  .modules li {
    background: var(--surface-muted);
    border-radius: var(--radius);
    padding: var(--space-4);
  }

  .modules p {
    margin: var(--space-2) 0;
  }

  .module-head {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .tag {
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .mandatory {
    color: var(--accent);
    font-weight: 600;
  }

  .error {
    color: var(--danger);
  }

  footer {
    margin-top: auto;
    color: var(--text-muted);
    text-align: center;
  }
</style>
