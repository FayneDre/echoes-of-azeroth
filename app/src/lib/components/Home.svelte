<script lang="ts">
  // Home view of one realm: designed to fit the window without scrolling.
  // Until the install engine is wired in, every state shown is the real one
  // ("not installed"); nothing is faked.
  import type { EmulatorOverview } from "../api";
  import Emblem from "../emblems/Emblem.svelte";
  import { localized, t } from "../i18n/i18n.svelte";
  import { processLabel } from "../labels";

  let { realm }: { realm: EmulatorOverview } = $props();

  const mandatory = $derived(realm.modules.filter((m) => m.mandatory).length);
  const optional = $derived(realm.modules.length - mandatory);
  const servers = $derived(["database", ...realm.processes]);
  const steps = $derived([
    {
      title: t("guide.client.title"),
      body: t("guide.client.body", { version: realm.client?.version ?? "", build: realm.client?.build ?? "" }),
    },
    { title: t("guide.install.title"), body: t("guide.install.body") },
    { title: t("guide.play.title"), body: t("guide.play.body") },
  ]);
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
      <h2 class="display">{t("home.status.notInstalled")}</h2>
      <p class="muted">{t("home.status.notInstalledBody", { emulator: realm.name })}</p>
    </div>
    <!-- Start/stop appear once a realm is installed (runtime state, phase 3). -->
    <button class="btn btn-play" disabled title={t("common.soon")}>{t("home.action.install")}</button>
  </div>

  <dl class="stats">
    <div>
      <dt>{t("home.stat.emulator")}</dt>
      <dd>{realm.name}</dd>
    </div>
    <div>
      <dt>{t("home.stat.client")}</dt>
      <dd>{realm.client ? t("realm.version", { version: realm.client.version, build: realm.client.build }) : "—"}</dd>
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

<section class="panel">
  <h3 class="panel-title">{t("guide.title")}</h3>
  <ol class="steps">
    {#each steps as step, index (index)}
      <li>
        <span class="step-number display" aria-hidden="true">{index + 1}</span>
        <div>
          <strong class="display">{step.title}</strong>
          <p class="muted">{step.body}</p>
        </div>
      </li>
    {/each}
  </ol>
</section>

<style>
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-5);
  }

  .steps li {
    display: flex;
    gap: var(--space-3);
  }

  .step-number {
    flex: none;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border: 2px solid var(--frame-light);
    border-radius: 50%;
    background: radial-gradient(circle at 35% 30%, var(--surface-muted), rgb(0 0 0 / 0.6));
    color: var(--title);
    box-shadow: 0 0 8px rgb(224 176 79 / 0.25);
  }

  .steps strong {
    color: var(--title);
  }

  .steps p {
    margin: 2px 0 0;
    font-size: 0.9rem;
  }

  .hero {
    padding-top: var(--space-4);
    padding-bottom: var(--space-4);
  }

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
    text-shadow: 0 2px 3px rgb(0 0 0 / 0.7);
  }

  .hero-title p.muted {
    margin: 0;
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
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.5);
  }
</style>
