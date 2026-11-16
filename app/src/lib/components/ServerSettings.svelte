<script lang="ts">
  // "Server settings" tab. A sub-navigation picks one page:
  // - a section of realm settings (rates, realm type, characters), from the
  //   emulator manifest (ADR 0017);
  // - the modules: the list on the left (mandatory ones always on, optional
  //   ones toggled with the servers stopped), the options of the selected
  //   module on the right. Bot modules have their own tab.
  import { untrack } from "svelte";
  import {
    api,
    asCommandError,
    type CommandError,
    type EmulatorOverview,
    type InstanceView,
    type ModuleOptions,
    type SettingsSection,
    REALM_OPTIONS,
  } from "../api";
  import { localized, t } from "../i18n/i18n.svelte";
  import { launcher, runOperation } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import OptionsPanel from "./OptionsPanel.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived<InstanceView | null>(
    launcher.instanceFor(realm.id),
  );
  const id = $derived(instance?.record.id ?? null);
  const running = $derived(id !== null && launcher.isRunning(id));
  const busy = $derived(launcher.progress !== null);
  const locked = $derived(running || busy || !!instance?.unfinished || !instance?.installed);
  const modules = $derived(realm.modules.filter((m) => m.category !== "bots"));

  /** Module selection being edited (starts from the installed one). */
  let selection = $state<string[]>([]);
  let options = $state<ModuleOptions[]>([]);
  let error = $state<CommandError | null>(null);
  let rebuilds = $state<string[]>([]);
  /** Module whose options are shown on the right. */
  let selected = $state<string | null>(null);
  let sections = $state<SettingsSection[]>([]);
  /** Page picked: a realm settings section id, or "modules"; the first
   *  section until one is picked. */
  let picked = $state<string | null>(null);
  const page = $derived(picked ?? sections[0]?.id ?? "modules");
  const section = $derived(sections.find((s) => s.id === page) ?? null);

  const installed = $derived(instance?.record.modules ?? []);
  const toEnable = $derived(selection.filter((m) => !installed.includes(m)));
  const toDisable = $derived(installed.filter((m) => !selection.includes(m)));
  const changed = $derived(toEnable.length + toDisable.length > 0);
  const shown = $derived(options.find((m) => m.module === selected) ?? null);

  async function load() {
    if (!id) return;
    selection = [...installed];
    try {
      sections = await api.realmSettings(id);
      if (picked !== "modules" && !sections.some((s) => s.id === picked)) picked = null;
      options = (await api.options(id)).filter((m) => modules.some((x) => x.id === m.module));
      if (!options.some((m) => m.module === selected)) selected = options[0]?.module ?? null;
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  /** Content of the installed state: the overview is refreshed often (new
   *  objects each time), the edits are reset only when it really changes. */
  const recordKey = $derived(instance ? JSON.stringify(instance.record) : "");

  $effect(() => {
    void recordKey;
    // load() reads the instance: only the key above is a dependency.
    untrack(() => void load());
  });

  $effect(() => {
    const instanceId = id;
    const disable = toDisable;
    if (!instanceId || disable.length === 0) {
      rebuilds = [];
      return;
    }
    api
      .rebuildsFor(instanceId, disable)
      .then((list) => (rebuilds = list))
      .catch(() => (rebuilds = []));
  });

  function toggle(module: string, on: boolean) {
    selection = on ? [...selection, module] : selection.filter((m) => m !== module);
  }

  async function applyModules() {
    if (!id) return;
    const instanceId = id;
    error = null;
    try {
      await runOperation(instanceId, () => api.setModules(instanceId, toEnable, toDisable));
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  function hasOptions(moduleId: string): boolean {
    return options.some((m) => m.module === moduleId);
  }

  function moduleName(moduleId: string): string {
    const module = realm.modules.find((m) => m.id === moduleId);
    return module ? localized(module.name) : moduleId;
  }
</script>

{#if error}
  <ErrorBox {error} />
{/if}
{#if instance && running}
  <p class="banner">{t("settings.stopFirst")}</p>
{/if}

{#if sections.length > 0}
  <nav class="segmented pages" aria-label={t("settings.pages")}>
    {#each sections as item (item.id)}
      <button class:active={page === item.id} onclick={() => (picked = item.id)}>{localized(item.label)}</button>
    {/each}
    <button class:active={page === "modules"} onclick={() => (picked = "modules")}>{t("modules.title")}</button>
  </nav>
{/if}

{#if section && id}
  <OptionsPanel
    instanceId={id}
    module={{ module: REALM_OPTIONS, options: section.options }}
    title={localized(section.label)}
    columns={2}
    {locked}
    onerror={(e) => (error = e)}
  />
{:else}
<div class="split">
  <section class="panel side">
    <h3 class="panel-title">{t("modules.title")}</h3>
    <ul class="modules">
      {#each modules as module (module.id)}
        {@const on = module.mandatory || (instance ? selection.includes(module.id) : false)}
        <li class:on class:selected={selected === module.id} title={localized(module.description)}>
          <div class="module-head">
            {#if instance && !module.mandatory}
              <label>
                <input
                  type="checkbox"
                  disabled={locked}
                  checked={selection.includes(module.id)}
                  onchange={(e) => toggle(module.id, e.currentTarget.checked)}
                />
                <strong>{localized(module.name)}</strong>
              </label>
            {:else}
              <strong>{localized(module.name)}</strong>
            {/if}
            <span class="tag" class:included={module.mandatory}>
              {module.mandatory ? t("modules.mandatory") : t("modules.optional")}
            </span>
          </div>
          <p class="muted">{localized(module.description)}</p>
          {#if hasOptions(module.id)}
            <button class="link" onclick={() => (selected = module.id)}>{t("settings.showOptions")} ›</button>
          {/if}
        </li>
      {/each}
    </ul>
    {#if instance}
      {#if rebuilds.length > 0}
        <p class="warn">{t("settings.rebuild", { databases: rebuilds.join(", ") })}</p>
      {/if}
      <div class="apply">
        <p class="muted">{t("modules.optionalHint")}</p>
        <button class="btn" disabled={!changed || locked} onclick={() => (selection = [...installed])}>
          {t("common.reset")}
        </button>
        <button class="btn btn-play" disabled={!changed || locked} onclick={applyModules}>{t("common.apply")}</button>
      </div>
    {:else}
      <p class="hint muted">{t("settings.installFirst")}</p>
    {/if}
  </section>

  <div class="detail">
    {#if shown && id}
      <OptionsPanel
        instanceId={id}
        module={shown}
        title={t("settings.options", { module: moduleName(shown.module) })}
        {locked}
        onerror={(e) => (error = e)}
      />
    {:else}
      <section class="panel">
        <h3 class="panel-title">{t("settings.optionsTitle")}</h3>
        <p class="muted">{instance ? t("settings.noOptions") : t("settings.installFirst")}</p>
      </section>
    {/if}
  </div>
</div>
{/if}

<style>
  .pages {
    align-self: flex-start;
  }

  /* Modules | options of the selected module, filling the page. */
  .split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr);
    gap: var(--space-4);
  }

  .side,
  .detail {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .modules {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .modules li {
    display: flex;
    flex-direction: column;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-muted);
    border: 1px solid var(--border);
    border-left: 3px solid var(--frame);
  }

  .modules li.on {
    border-left-color: var(--frame-light);
  }

  .modules li.selected {
    box-shadow: 0 0 8px var(--glow);
    border-color: var(--frame-light);
  }

  /* Two lines at most; the whole text is the item's tooltip. */
  .modules p {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    margin: 2px 0 0;
    font-size: 0.82rem;
    line-height: 1.35;
  }

  .module-head {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .module-head label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
  }

  strong {
    color: var(--title);
    font-family: var(--font-display);
  }

  .tag {
    font-size: 0.72rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .tag.included {
    color: var(--ok);
  }

  .link {
    align-self: flex-end;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .link:hover {
    text-decoration: underline;
  }

  .hint {
    margin: var(--space-3) 0 0;
    font-size: 0.85rem;
  }

  .warn {
    margin: var(--space-3) 0 0;
    color: var(--danger);
    font-size: 0.85rem;
  }

  .banner {
    margin: 0;
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--frame-light);
    background: var(--shade-soft);
  }

  /* Hint on its own line, buttons under it on the right: never squeezed. */
  .apply {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-2) var(--space-3);
    margin-top: auto;
    padding-top: var(--space-3);
  }

  .apply p {
    flex: 1 1 100%;
    margin: 0;
    font-size: 0.82rem;
  }

  .apply .btn {
    padding: var(--space-1) var(--space-3);
    font-size: 0.85rem;
  }

  .apply .btn-play {
    min-width: 8rem;
    font-size: 0.95rem;
  }
</style>
