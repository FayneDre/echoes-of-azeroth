<script lang="ts">
  // First install of a realm: the player's client, the optional modules, solo
  // or LAN, then one resumable operation (download, databases, configs).
  import { api, asCommandError, type ClientView, type CommandError, type EmulatorOverview } from "../api";
  import Emblem from "../emblems/Emblem.svelte";
  import { localized, t } from "../i18n/i18n.svelte";
  import { launcher, runOperation } from "../launcher.svelte";
  import ClientPicker from "./ClientPicker.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import ProgressPanel from "./ProgressPanel.svelte";

  let { realm, client, instanceId }: { realm: EmulatorOverview; client: ClientView | null; instanceId: string } =
    $props();

  const optional = $derived(realm.modules.filter((m) => !m.mandatory));
  const mandatory = $derived(realm.modules.filter((m) => m.mandatory));

  let chosen = $state<string[]>([]);
  let lan = $state(false);
  let error = $state<CommandError | null>(null);

  const progress = $derived(launcher.progress?.instance === instanceId ? launcher.progress : null);
  const otherBusy = $derived(launcher.progress !== null && launcher.progress.instance !== instanceId);

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
      <Emblem emblem={realm.client.emblem} accent={realm.client.accent} label={localized(realm.client.shortName)} size={68} />
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

{#if progress}
  <ProgressPanel {progress} title={t("wizard.installing")} />
{:else}
  <div class="steps">
    {#if realm.client}
      <section class="panel">
        <h3 class="panel-title"><span class="n display">1</span>{t("wizard.client")}</h3>
        <ClientPicker client={realm.client} registered={client} />
        {#if !client}
          <p class="hint muted">{t("wizard.clientLater")}</p>
        {/if}
      </section>
    {/if}

    <section class="panel">
      <h3 class="panel-title"><span class="n display">2</span>{t("wizard.modules")}</h3>
      <ul class="choices">
        {#each mandatory as module (module.id)}
          <li>
            <label>
              <input type="checkbox" checked disabled />
              <span><strong>{localized(module.name)}</strong> <small class="tag">{t("modules.mandatory")}</small></span>
            </label>
            <p class="muted">{localized(module.description)}</p>
          </li>
        {/each}
        {#each optional as module (module.id)}
          <li>
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

    <section class="panel">
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
    </section>
  </div>

  {#if error}
    <ErrorBox {error} onretry={install} />
  {/if}

  <div class="go">
    <p class="muted">{t("wizard.sizeHint")}</p>
    <button class="btn btn-play" disabled={otherBusy} onclick={install}>{t("home.action.install")}</button>
  </div>
{/if}

<style>
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
    text-shadow: var(--text-shadow);
  }

  .hero-title p.muted {
    margin: 0;
  }

  .steps {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-5);
    align-items: start;
  }

  @media (max-width: 1200px) {
    .steps {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .n {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    margin-right: var(--space-2);
    border: 1px solid var(--frame-light);
    border-radius: 50%;
    font-size: 0.8rem;
  }

  .choices {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .choices label {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    cursor: pointer;
  }

  .choices p {
    margin: 2px 0 0 calc(13px + var(--space-2) + 4px);
    font-size: 0.85rem;
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

  .network {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .network label {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    padding: var(--space-3);
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

  .hint {
    margin: var(--space-3) 0 0;
    font-size: 0.85rem;
  }

  .go {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-4);
  }

  .go p {
    margin: 0;
  }
</style>
