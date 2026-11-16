<script lang="ts">
  // First install of a realm: the player's client, the optional modules, solo
  // or LAN, then one resumable operation (download, databases, configs).
  import {
    api,
    asCommandError,
    pickFolder,
    type ClientView,
    type CommandError,
    type DataLocation,
    type EmulatorOverview,
    type SpaceEstimate,
  } from "../api";
  import Emblem from "../emblems/Emblem.svelte";
  import { gigabytes, localized, t } from "../i18n/i18n.svelte";
  import { launcher, runOperation } from "../launcher.svelte";
  import ClientPicker from "./ClientPicker.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import JoinLan from "./JoinLan.svelte";

  let { realm, client, instanceId }: { realm: EmulatorOverview; client: ClientView | null; instanceId: string } =
    $props();

  const optional = $derived(realm.modules.filter((m) => !m.mandatory));
  const mandatory = $derived(realm.modules.filter((m) => m.mandatory));

  let chosen = $state<string[]>([]);
  let lan = $state(false);
  let error = $state<CommandError | null>(null);
  let location = $state<DataLocation | null>(null);

  let space = $state<SpaceEstimate | null>(null);

  $effect(() => {
    api
      .dataLocation()
      .then((l) => (location = l))
      .catch(() => (location = null));
  });

  // Measured again when the install location changes.
  $effect(() => {
    void location?.root;
    api
      .installSpace(realm.id)
      .then((s) => (space = s))
      .catch(() => (space = null));
  });

  async function changeLocation(reset: boolean) {
    error = null;
    try {
      const folder = reset ? null : await pickFolder(t("location.title"));
      if (!reset && folder === null) return;
      location = await api.setDataLocation(folder);
    } catch (raw) {
      error = asCommandError(raw);
    }
  }


  function toggle(id: string, on: boolean) {
    chosen = on ? [...chosen, id] : chosen.filter((m) => m !== id);
  }

  async function install() {
    error = null;
    try {
      await runOperation(instanceId, () =>
        api.install({ id: instanceId, emulator: realm.id, modules: chosen, lan }),
      );
    } catch (raw) {
      error = asCommandError(raw);
    }
  }
</script>

<section class="panel hero">
  <div class="hero-head">
    {#if realm.client}
      <Emblem emblem={realm.client.emblem} accent={realm.client.accent} label={localized(realm.client.shortName)} size={52} />
    {/if}
    <div class="hero-title">
      {#if realm.client}
        <p class="realm-name">{localized(realm.client.shortName)} · {realm.client.version}</p>
      {/if}
      <h2 class="display">{t("home.status.notInstalled")}</h2>
      <p class="muted">{t("home.status.notInstalledBody", { emulator: realm.name })}</p>
    </div>
  </div>
</section>

<div class="steps">
  {#if realm.client}
    <section class="panel step">
      <h3 class="panel-title"><span class="n display">1</span>{t("wizard.client")}</h3>
      <div class="pick">
        <ClientPicker client={realm.client} registered={client} />
        {#if !client}
          <p class="hint muted">{t("wizard.clientLater")}</p>
        {/if}
      </div>
      <div class="foot">
        <JoinLan {realm} {client} />
      </div>
    </section>
  {/if}

  <section class="panel step">
    <h3 class="panel-title"><span class="n display">2</span>{t("wizard.modules")}</h3>
    <ul class="choices">
      {#each mandatory as module (module.id)}
        <li title={localized(module.description)}>
          <label>
            <input type="checkbox" checked disabled />
            <span><strong>{localized(module.name)}</strong> <small class="tag">{t("modules.mandatory")}</small></span>
          </label>
          <p class="muted">{localized(module.description)}</p>
        </li>
      {/each}
      {#each optional as module (module.id)}
        <li title={localized(module.description)}>
          <label>
            <input
              type="checkbox"
              checked={chosen.includes(module.id)}
              onchange={(e) => toggle(module.id, e.currentTarget.checked)}
            />
            <span><strong>{localized(module.name)}</strong></span>
          </label>
          <p class="muted">{localized(module.description)}</p>
        </li>
      {/each}
    </ul>
    <p class="hint muted">{t("modules.optionalHint")}</p>
  </section>

  <section class="panel step">
    <h3 class="panel-title"><span class="n display">3</span>{t("wizard.network")}</h3>
    <div class="network">
      <label class:active={!lan}>
        <input type="radio" name="network" checked={!lan} onchange={() => (lan = false)} />
        <span><strong>{t("wizard.solo")}</strong><small class="muted">{t("wizard.soloBody")}</small></span>
      </label>
      <label class:active={lan}>
        <input type="radio" name="network" checked={lan} onchange={() => (lan = true)} />
        <span><strong>{t("wizard.lan")}</strong><small class="muted">{t("wizard.lanBody")}</small></span>
      </label>
    </div>
    {#if location}
      <div class="location">
        <span class="muted">{t("location.title")}</span>
        <code title={location.root}>{location.root}</code>
        {#if space && space.available !== null}
          <span class="space" class:short={space.tooSmall}>
            {t("wizard.space", { needed: gigabytes(space.needed), free: gigabytes(space.available) })}
          </span>
        {/if}
        {#if location.changeable}
          <span class="space">{t("location.hint")}</span>
        {/if}
        {#if location.relocatable}
          <div class="location-actions">
            {#if location.changeable && location.root !== location.default}
              <button class="link" disabled={launcher.progress !== null} onclick={() => changeLocation(true)}>
                {t("location.reset")}
              </button>
            {/if}
            <button class="link" disabled={launcher.progress !== null} onclick={() => changeLocation(false)}>
              {t("location.change")} ›
            </button>
          </div>
        {/if}
      </div>
    {/if}
    <div class="go foot">
      <button class="btn btn-play" disabled={launcher.progress !== null || !!space?.tooSmall} onclick={install}>
        {t("home.action.install")}
      </button>
      {#if space?.tooSmall}
        <small class="short">{t("wizard.spaceShort")}</small>
      {:else if space}
        <small class="muted">{t("wizard.sizeHint", { download: gigabytes(space.download) })}</small>
      {:else}
        <small class="muted">{t("wizard.resumeHint")}</small>
      {/if}
    </div>
  </section>
</div>

{#if error}
  <ErrorBox {error} onretry={install} />
{/if}

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
    font-size: 0.75rem;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--realm-accent);
  }

  h2 {
    margin: 0;
    font-size: 1.4rem;
    line-height: 1.25;
    color: var(--title);
    text-shadow: var(--text-shadow);
  }

  .hero-title p.muted {
    margin: 0;
    font-size: 0.9rem;
  }

  /* Three columns at every window size (the minimum is 1024 px wide), of
     equal height: each step ends with its own footer, no empty band. */
  .steps {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-4);
  }

  .step {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .foot {
    margin-top: auto;
  }

  .step .panel-title {
    font-size: 0.92rem;
    letter-spacing: 0.08em;
  }

  /* Step 1: the folder choice is a framed zone filling the column. */
  .pick {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
    padding: var(--space-4);
    border: 1px dashed var(--frame-light);
    background: var(--surface-muted);
  }

  .pick .hint {
    margin: 0;
  }

  .n {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    margin-right: var(--space-2);
    border: 1px solid var(--frame-light);
    border-radius: 50%;
    font-size: 0.75rem;
  }

  /* Step 2: one card per module, sharing the column's height. */
  .choices {
    flex: 1;
    list-style: none;
    margin: 0 0 var(--space-3);
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .choices li {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-muted);
    border: 1px solid var(--border);
    border-left: 3px solid var(--frame-light);
  }

  .choices label {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    cursor: pointer;
  }

  /* Two lines at most; the whole text is the item's tooltip. */
  .choices p {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    margin: 2px 0 0 calc(13px + var(--space-2) + 4px);
    font-size: 0.82rem;
    line-height: 1.35;
  }

  strong {
    color: var(--title);
    font-family: var(--font-display);
  }

  .tag {
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--ok);
  }

  /* Step 3: the two choices share the column above the button. */
  .network {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }

  .network label {
    flex: 1;
    display: flex;
    gap: var(--space-3);
    align-items: center;
    background: var(--surface-muted);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    cursor: pointer;
  }

  .network label.active {
    border-color: var(--frame-light);
    box-shadow: 0 0 8px var(--glow);
  }

  .network span {
    display: flex;
    flex-direction: column;
  }

  .network small {
    font-size: 0.8rem;
    line-height: 1.35;
  }

  .hint {
    margin: 0;
    font-size: 0.82rem;
  }


  .location {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: baseline;
    column-gap: var(--space-2);
    margin-bottom: var(--space-3);
    font-size: 0.82rem;
  }

  .location code {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .space {
    grid-column: 1 / -1;
    color: var(--text-muted);
  }

  .short {
    color: var(--danger);
  }

  .location-actions {
    grid-column: 1 / -1;
    display: flex;
    justify-content: flex-end;
    gap: var(--space-3);
  }

  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }

  .link:hover:not(:disabled) {
    text-decoration: underline;
  }

  .go {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--space-2);
    text-align: center;
  }

  .go small {
    font-size: 0.8rem;
  }
</style>
