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
  import Notify from "./Notify.svelte";
  import JoinLan from "./JoinLan.svelte";
  import Modal from "./Modal.svelte";
  import ModulePicker from "./ModulePicker.svelte";

  let { realm, client, instanceId }: { realm: EmulatorOverview; client: ClientView | null; instanceId: string } =
    $props();

  const optional = $derived(realm.modules.filter((m) => !m.mandatory));
  const mandatory = $derived(realm.modules.filter((m) => m.mandatory));

  let chosen = $state<string[]>([]);
  let picking = $state(false);
  /** Chosen optional modules, in catalog order. */
  const chosenModules = $derived(optional.filter((m) => chosen.includes(m.id)));
  let lan = $state(false);
  let error = $state<CommandError | null>(null);
  let location = $state<DataLocation | null>(null);
  /** The install button first asks where the server goes. */
  let choosingPlace = $state(false);

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

  /** Shown in the location dialog: its retry is choosing again, not installing. */
  let placeError = $state<CommandError | null>(null);

  async function changeLocation(reset: boolean) {
    placeError = null;
    try {
      const folder = reset ? null : await pickFolder(t("location.title"));
      if (!reset && folder === null) return;
      location = await api.setDataLocation(folder);
    } catch (raw) {
      placeError = asCommandError(raw);
    }
  }

  function remove(id: string) {
    chosen = chosen.filter((m) => m !== id);
  }

  /** Where to install comes first; without a known location, as before. */
  function askPlace() {
    error = null;
    placeError = null;
    if (location) choosingPlace = true;
    else void install();
  }

  async function install() {
    choosingPlace = false;
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
      </div>
      {#if !client}
        <p class="hint muted later">{t("wizard.clientLater")}</p>
      {/if}
      <div class="foot">
        <JoinLan {realm} {client} />
      </div>
    </section>
  {/if}

  <section class="panel step">
    <h3 class="panel-title"><span class="n display">2</span>{t("wizard.modules")}</h3>
    <div class="modules">
      <p class="group">{t("modules.included")}</p>
      <ul class="choices">
        {#each mandatory as module (module.id)}
          <li class="included" title={localized(module.description)}>
            <span class="check" aria-hidden="true">✓</span>
            <span class="what">
              <strong>{localized(module.name)}</strong>
              <small class="muted">{localized(module.description)}</small>
            </span>
          </li>
        {/each}
      </ul>

      <p class="group">{t("modules.optionalGroup")}</p>
      {#if chosenModules.length > 0}
        <ul class="choices chosen">
          {#each chosenModules as module (module.id)}
            <li title={localized(module.description)}>
              <span class="what">
                <strong>{localized(module.name)}</strong>
                <small class="tag">{t(`category.${module.category}`)}</small>
              </span>
              <button
                class="remove"
                aria-label={t("modules.remove", { name: localized(module.name) })}
                title={t("modules.remove", { name: localized(module.name) })}
                onclick={() => remove(module.id)}
              >
                ×
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="none muted">{t("modules.noneChosen", { count: optional.length })}</p>
      {/if}
      {#if optional.length > 0}
        <button class="btn" onclick={() => (picking = true)}>
          {t("modules.choose")}
        </button>
      {/if}
    </div>
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
    <div class="go foot">
      <button class="btn btn-play" disabled={launcher.progress !== null} onclick={askPlace}>
        {t("home.action.install")}
      </button>
      {#if space}
        <small class="muted">{t("wizard.sizeHint", { download: gigabytes(space.download) })}</small>
      {:else}
        <small class="muted">{t("wizard.resumeHint")}</small>
      {/if}
    </div>
  </section>
</div>

<Notify {error} onretry={install} />

{#if choosingPlace && location}
  <Modal title={t("location.question")} onclose={() => (choosingPlace = false)}>
    <div class="place">
      <code title={location.root}>{location.root}</code>
      {#if space && space.available !== null}
        <p class:short={space.tooSmall}>
          {t("wizard.space", { needed: gigabytes(space.needed), free: gigabytes(space.available) })}
        </p>
      {/if}
      {#if space?.tooSmall}
        <p class="short">{t("wizard.spaceShort")}</p>
      {/if}
      {#if location.changeable}
        <p class="muted">{t("location.hint")}</p>
      {/if}
      {#if location.relocatable}
        <div class="place-actions">
          <button class="btn" onclick={() => changeLocation(false)}>{t("location.pick")}</button>
          {#if location.changeable && location.root !== location.default}
            <button class="link" onclick={() => changeLocation(true)}>{t("location.reset")}</button>
          {/if}
        </div>
      {/if}
      {#if placeError}
        <ErrorBox error={placeError} compact />
      {/if}
    </div>
    {#snippet actions()}
      <button class="btn" onclick={() => (choosingPlace = false)}>{t("common.cancel")}</button>
      <button class="btn btn-play" disabled={!!space?.tooSmall} onclick={install}>
        {t("location.installHere")}
      </button>
    {/snippet}
  </Modal>
{/if}

{#if picking}
  <ModulePicker
    modules={optional}
    {chosen}
    onapply={(list) => {
      chosen = list;
      picking = false;
    }}
    onclose={() => (picking = false)}
  />
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

  /* Step 1: the game folder, from the top of the column. */
  .pick {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    margin-bottom: var(--space-3);
  }

  .later {
    margin-bottom: var(--space-3);
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

  /* Step 2: what is included and what was chosen; the full list (any
     size) is in the module dialog. */
  .modules {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }

  .group {
    margin: 0;
    font-size: 0.72rem;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .group:not(:first-child) {
    margin-top: var(--space-2);
  }

  .choices {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .choices li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--surface-muted);
    border: 1px solid var(--border);
    border-left: 3px solid var(--frame-light);
  }

  .choices li.included {
    align-items: flex-start;
  }

  .check {
    color: var(--ok);
    line-height: 1.5;
  }

  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .what small.muted {
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
    color: var(--text-muted);
  }

  .remove {
    flex: none;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 1px solid transparent;
    background: none;
    color: var(--text-muted);
    font-size: 1.1rem;
    line-height: 1;
    cursor: pointer;
  }

  .remove:hover {
    color: var(--danger);
    border-color: var(--border);
  }

  .none {
    margin: 0;
    font-size: 0.85rem;
    font-style: italic;
  }

  .modules > .btn {
    align-self: stretch;
    margin-top: var(--space-1);
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

  /* The install location dialog. */
  .place {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .place code {
    display: block;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-muted);
    border: 1px solid var(--border);
    overflow-wrap: anywhere;
  }

  .place p {
    margin: 0;
    font-size: 0.88rem;
  }

  .short {
    color: var(--danger);
  }

  .place-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-1);
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
