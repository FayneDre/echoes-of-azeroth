<script lang="ts">
  // "Server settings" tab: the realm's modules (mandatory ones always
  // installed, optional ones toggled at any time with the servers stopped)
  // and the options of the enabled modules.
  import {
    api,
    asCommandError,
    type CommandError,
    type EmulatorOverview,
    type InstanceView,
    type ModuleOption,
    type ModuleOptions,
    type OptionValue,
  } from "../api";
  import { untrack } from "svelte";
  import { localized, t, type MessageKey } from "../i18n/i18n.svelte";
  import { launcher, runOperation } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import ProgressPanel from "./ProgressPanel.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived<InstanceView | null>(
    launcher.overview?.instances.find((i) => i.record.emulator === realm.id) ?? null,
  );
  const id = $derived(instance?.record.id ?? null);
  const running = $derived(id !== null && launcher.isRunning(id));
  const busy = $derived(launcher.progress !== null);
  const progress = $derived(id !== null && launcher.progress?.instance === id ? launcher.progress : null);
  const locked = $derived(running || busy || !!instance?.unfinished || !instance?.installed);

  /** Module selection being edited (starts from the installed one). */
  let selection = $state<string[]>([]);
  let options = $state<ModuleOptions[]>([]);
  /** Edited option values, as text, per module. */
  let drafts = $state<Record<string, Record<string, string>>>({});
  let error = $state<CommandError | null>(null);
  let rebuilds = $state<string[]>([]);

  const installed = $derived(instance?.record.modules ?? []);
  const toEnable = $derived(selection.filter((m) => !installed.includes(m)));
  const toDisable = $derived(installed.filter((m) => !selection.includes(m)));
  const changed = $derived(toEnable.length + toDisable.length > 0);

  function asText(value: OptionValue): string {
    return String(value);
  }

  async function load() {
    if (!id) return;
    selection = [...installed];
    try {
      options = await api.options(id);
      drafts = Object.fromEntries(
        options.map((m) => [m.module, Object.fromEntries(m.options.map((o) => [o.option.id, asText(o.value)]))]),
      );
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

  function optionChanged(module: ModuleOptions): boolean {
    return module.options.some((o) => drafts[module.module]?.[o.option.id] !== asText(o.value));
  }

  async function applyOptions(module: ModuleOptions) {
    if (!id) return;
    const instanceId = id;
    const draft = drafts[module.module] ?? {};
    const values = module.options
      .filter((o) => o.option.id in draft && draft[o.option.id] !== asText(o.value))
      .map((o) => [o.option.id, draft[o.option.id]] as [string, string]);
    error = null;
    try {
      await runOperation(instanceId, () => api.setOptions(instanceId, module.module, values));
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  function moduleName(moduleId: string): string {
    const module = realm.modules.find((m) => m.id === moduleId);
    return module ? localized(module.name) : moduleId;
  }

  function setDraft(module: string, option: string, value: string) {
    drafts[module] = { ...drafts[module], [option]: value };
  }

  function help(option: ModuleOption): string | null {
    return option.help ? localized(option.help) : null;
  }
</script>

{#if progress}
  <ProgressPanel {progress} title={t("progress.title")} />
{/if}
{#if error}
  <ErrorBox {error} />
{/if}
{#if instance && running}
  <p class="banner">{t("settings.stopFirst")}</p>
{/if}

<section class="panel">
  <h3 class="panel-title">{t("modules.title")}</h3>
  <ul class="modules">
    {#each realm.modules as module (module.id)}
      <li class:on={module.mandatory || (instance ? selection.includes(module.id) : false)}>
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
        <small class="muted">{t(`category.${module.category}` as MessageKey)}</small>
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

{#each options as module (module.module)}
  <section class="panel">
    <h3 class="panel-title">{t("settings.options", { module: moduleName(module.module) })}</h3>
    <ul class="options">
      {#each module.options as { option } (option.id)}
        {@const value = drafts[module.module]?.[option.id] ?? ""}
        <li>
          <div class="label">
            <span>{localized(option.label)}</span>
            {#if help(option)}<small class="muted">{help(option)}</small>{/if}
          </div>
          {#if option.type === "boolean"}
            <input
              type="checkbox"
              disabled={locked}
              checked={value === "true"}
              onchange={(e) => setDraft(module.module, option.id, String(e.currentTarget.checked))}
            />
          {:else if option.type === "integer"}
            <input
              type="number"
              disabled={locked}
              min={option.min ?? undefined}
              max={option.max ?? undefined}
              {value}
              onchange={(e) => setDraft(module.module, option.id, e.currentTarget.value)}
            />
          {:else}
            <select
              disabled={locked}
              {value}
              onchange={(e) => setDraft(module.module, option.id, e.currentTarget.value)}
            >
              {#each option.choices as choice (choice.id)}
                <option value={choice.id}>{localized(choice.label)}</option>
              {/each}
            </select>
          {/if}
        </li>
      {/each}
    </ul>
    <div class="apply">
      <p class="muted">{t("settings.optionsHint")}</p>
      <button class="btn btn-play" disabled={!optionChanged(module) || locked} onclick={() => applyOptions(module)}>
        {t("common.apply")}
      </button>
    </div>
  </section>
{/each}

<style>
  .modules {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: var(--space-4);
  }

  .modules li {
    display: flex;
    flex-direction: column;
    padding: var(--space-3) var(--space-4);
    background: var(--surface-muted);
    border: 1px solid var(--border);
    border-left: 3px solid var(--frame);
  }

  .modules li.on {
    border-left-color: var(--frame-light);
  }

  .modules p {
    flex: 1;
    margin: var(--space-1) 0;
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

  .hint {
    margin: var(--space-4) 0 0;
    font-size: 0.85rem;
  }

  .warn {
    margin: var(--space-4) 0 0;
    color: var(--danger);
  }

  .banner {
    margin: 0;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--frame-light);
    background: var(--shade-soft);
  }

  .apply {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }

  .apply p {
    flex: 1;
    margin: 0;
    font-size: 0.85rem;
  }

  .options {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .options li {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--border);
  }

  .options li:last-child {
    border-bottom: none;
  }

  .label {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
</style>
