<script lang="ts">
  // The options of one enabled module, edited as a draft and applied like a
  // module change (servers stopped). Used by "Server settings" and "Bots".
  import { untrack } from "svelte";
  import { api, asCommandError, type CommandError, type ModuleOption, type ModuleOptions, type OptionValue } from "../api";
  import { localized, t } from "../i18n/i18n.svelte";
  import { runOperation } from "../launcher.svelte";

  let {
    instanceId,
    module,
    title,
    locked,
    onerror,
  }: {
    instanceId: string;
    module: ModuleOptions;
    title: string;
    locked: boolean;
    onerror: (error: CommandError | null) => void;
  } = $props();

  function asText(value: OptionValue): string {
    return String(value);
  }

  function saved(): Record<string, string> {
    return Object.fromEntries(module.options.map((o) => [o.option.id, asText(o.value)]));
  }

  /** Edited values, as text, reset whenever the saved values change. */
  let draft = $state<Record<string, string>>({});
  const savedKey = $derived(JSON.stringify(module.options.map((o) => o.value)));
  $effect(() => {
    void savedKey;
    untrack(() => (draft = saved()));
  });

  const changed = $derived(module.options.some((o) => draft[o.option.id] !== asText(o.value)));

  function set(option: string, value: string) {
    draft = { ...draft, [option]: value };
  }

  function help(option: ModuleOption): string | null {
    return option.help ? localized(option.help) : null;
  }

  async function apply() {
    const values = module.options
      .filter((o) => o.option.id in draft && draft[o.option.id] !== asText(o.value))
      .map((o) => [o.option.id, draft[o.option.id]] as [string, string]);
    onerror(null);
    try {
      await runOperation(instanceId, () => api.setOptions(instanceId, module.module, values));
    } catch (raw) {
      onerror(asCommandError(raw));
    }
  }
</script>

<section class="panel">
  <h3 class="panel-title">{title}</h3>
  <ul class="options">
    {#each module.options as { option } (option.id)}
      {@const value = draft[option.id] ?? ""}
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
            onchange={(e) => set(option.id, String(e.currentTarget.checked))}
          />
        {:else if option.type === "integer"}
          <input
            type="number"
            disabled={locked}
            min={option.min ?? undefined}
            max={option.max ?? undefined}
            {value}
            onchange={(e) => set(option.id, e.currentTarget.value)}
          />
        {:else}
          <select disabled={locked} {value} onchange={(e) => set(option.id, e.currentTarget.value)}>
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
    <button class="btn" disabled={!changed || locked} onclick={() => (draft = saved())}>
      {t("common.reset")}
    </button>
    <button class="btn btn-play" disabled={!changed || locked} onclick={apply}>{t("common.apply")}</button>
  </div>
</section>

<style>
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
</style>
