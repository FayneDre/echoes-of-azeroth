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
    columns = 1,
  }: {
    instanceId: string;
    module: ModuleOptions;
    title: string;
    locked: boolean;
    onerror: (error: CommandError | null) => void;
    /** Options side by side (long lists). */
    columns?: number;
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
  <ul class="options" style:--columns={columns}>
    {#each module.options as { option } (option.id)}
      {@const value = draft[option.id] ?? ""}
      <li title={help(option) ?? undefined}>
        <span class="label">
          {localized(option.label)}
          {#if help(option)}<span class="help" aria-hidden="true">?</span>{/if}
        </span>
        {#if option.type === "boolean"}
          <input
            type="checkbox"
            disabled={locked}
            checked={value === "true"}
            onchange={(e) => set(option.id, String(e.currentTarget.checked))}
          />
        {:else if option.type === "integer" || option.type === "number"}
          <input
            type="number"
            disabled={locked}
            min={option.min ?? undefined}
            max={option.max ?? undefined}
            step={option.type === "number" ? "0.1" : "1"}
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
  /* Fills its column; the buttons stay at the bottom. */
  section {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .options {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(var(--columns), minmax(0, 1fr));
    column-gap: var(--space-6);
  }

  .options li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 38px;
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--border);
  }

  .label {
    flex: 1;
    min-width: 0;
    font-size: 0.92rem;
    line-height: 1.3;
  }

  /* The help text is the row's tooltip. */
  .help {
    display: inline-grid;
    place-items: center;
    width: 15px;
    height: 15px;
    margin-left: var(--space-1);
    border: 1px solid var(--frame-light);
    border-radius: 50%;
    font-size: 0.65rem;
    color: var(--accent);
    vertical-align: 1px;
    cursor: help;
  }

  input[type="number"] {
    width: 6rem;
  }

  select {
    max-width: 55%;
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
