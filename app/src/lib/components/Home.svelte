<script lang="ts">
  // Home view of one realm. Until the install engine is wired in, every state
  // shown is the real one ("not installed"); nothing is faked.
  import type { EmulatorOverview } from "../api";
  import Emblem from "../emblems/Emblem.svelte";
  import { localized, t, type MessageKey } from "../i18n/i18n.svelte";
  import { processLabel } from "../labels";

  let { realm }: { realm: EmulatorOverview } = $props();

  const mandatory = $derived(realm.modules.filter((m) => m.mandatory).length);
  const optional = $derived(realm.modules.length - mandatory);
  const servers = $derived(["database", ...realm.processes]);
</script>

<section class="panel hero">
  <div class="hero-head">
    {#if realm.client}
      <Emblem emblem={realm.client.emblem} accent={realm.client.accent} label={localized(realm.client.shortName)} size={72} />
    {/if}
    <div class="hero-title">
      {#if realm.client}
        <p class="realm-name">{localized(realm.client.shortName)} · {realm.client.version}</p>
      {/if}
      <h2 class="display">{t("home.status.notInstalled")}</h2>
      <p class="muted">{t("home.status.notInstalledBody", { emulator: realm.name })}</p>
    </div>
    <!-- Start/stop appear once a realm is installed (runtime state, phase 3). -->
    <div class="actions">
      <button class="btn btn-play" disabled title={t("common.soon")}>{t("home.action.install")}</button>
    </div>
  </div>

  <dl class="stats">
    <div>
      <dt>{t("home.stat.emulator")}</dt>
      <dd>{realm.name}</dd>
    </div>
    <div>
      <dt>{t("home.stat.client")}</dt>
      <dd>
        {#if realm.client}
          {t("realm.version", { version: realm.client.version, build: realm.client.build })}
          <span class="muted"> · {t("home.stat.clientNone")}</span>
        {:else}
          —
        {/if}
      </dd>
    </div>
    <div>
      <dt>{t("home.stat.modules")}</dt>
      <dd>{t("home.stat.modulesCount", { mandatory, optional })}</dd>
    </div>
    <div>
      <dt>{t("home.stat.backup")}</dt>
      <dd class="muted">{t("home.stat.none")}</dd>
    </div>
  </dl>
</section>

<div class="columns">
  <div class="column">
    <section class="panel">
      <h3 class="panel-title">{t("servers.title")}</h3>
      <ul class="rows">
        {#each servers as id (id)}
          <li>
            <span class="dot" aria-hidden="true"></span>
            <span class="row-name">{processLabel(id)}</span>
            <span class="muted">{t("servers.notInstalled")}</span>
          </li>
        {/each}
      </ul>
    </section>

    <section class="panel">
      <h3 class="panel-title">{t("install.title")}</h3>
      <ul class="rows">
        <li><span class="row-name">{t("install.serverDir")}</span><span class="muted">{t("install.notSet")}</span></li>
        <li><span class="row-name">{t("install.clientDir")}</span><span class="muted">{t("install.notSet")}</span></li>
        <li><span class="row-name">{t("install.serverData")}</span><span class="muted">{t("install.toDownload")}</span></li>
      </ul>
    </section>
  </div>

  <section class="panel column">
    <h3 class="panel-title">{t("modules.title")}</h3>
    <ul class="modules">
      {#each realm.modules as module (module.id)}
        <li>
          <div class="module-head">
            <strong>{localized(module.name)}</strong>
            <span class="tag" class:included={module.mandatory}>
              {module.mandatory ? t("modules.mandatory") : t("modules.optional")}
            </span>
          </div>
          <p class="muted">{localized(module.description)}</p>
          <small class="muted">{t(`category.${module.category}` as MessageKey)}</small>
        </li>
      {/each}
    </ul>
    <p class="hint muted">{t("modules.optionalHint")}</p>
  </section>
</div>

<style>
  .hero-head {
    display: flex;
    align-items: center;
    gap: var(--space-5);
    flex-wrap: wrap;
  }

  .hero-title {
    flex: 1;
    min-width: 260px;
  }

  .realm-name {
    margin: 0;
    font-size: 0.85rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--realm-accent);
  }

  h2 {
    margin: var(--space-1) 0;
    font-size: 1.8rem;
  }

  .hero-title p.muted {
    margin: 0;
    max-width: 52ch;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    margin: var(--space-5) 0 0;
    padding-top: var(--space-4);
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
    font-size: 0.75rem;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  dd {
    margin: var(--space-1) 0 0;
  }

  .columns {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
    gap: var(--space-5);
  }

  .column {
    display: flex;
    flex-direction: column;
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
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--border);
  }

  .rows li:last-child {
    border-bottom: none;
  }

  .row-name {
    flex: 1;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--border-strong);
  }

  .modules {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .modules li {
    padding: var(--space-3) var(--space-4);
    background: var(--surface-muted);
    border-left: 2px solid var(--border-strong);
  }

  .modules p {
    margin: var(--space-1) 0;
  }

  .module-head {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .tag {
    font-size: 0.75rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .tag.included {
    color: var(--accent);
  }

  .hint {
    margin: var(--space-4) 0 0;
    font-size: 0.85rem;
  }
</style>
