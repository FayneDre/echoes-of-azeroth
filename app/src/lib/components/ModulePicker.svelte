<script lang="ts">
  // Choosing optional modules in a dialog: search, filter by category, read
  // each full description. Scales to a catalog of many modules; the caller
  // keeps only the summary of the choice.
  import { openExternal, type ModuleCategory, type ModuleSummary } from "../api";
  import { localized, t } from "../i18n/i18n.svelte";
  import Modal from "./Modal.svelte";

  let {
    modules,
    chosen,
    onapply,
    onclose,
  }: {
    /** Optional modules only. */
    modules: ModuleSummary[];
    chosen: string[];
    onapply: (chosen: string[]) => void;
    onclose: () => void;
  } = $props();

  /** Edited here; given back only on "Apply". */
  let selection = $state<string[]>([]);
  let query = $state("");
  let category = $state<ModuleCategory | null>(null);

  $effect.pre(() => {
    selection = [...chosen];
  });

  const categories = $derived([...new Set(modules.map((m) => m.category))]);

  const normalize = (text: string) =>
    text
      .normalize("NFD")
      .replace(/[̀-ͯ]/g, "")
      .toLowerCase();

  const shown = $derived.by(() => {
    const words = normalize(query).split(/\s+/).filter(Boolean);
    return modules.filter((m) => {
      if (category && m.category !== category) return false;
      const text = normalize(`${localized(m.name)} ${localized(m.description)} ${m.id}`);
      return words.every((w) => text.includes(w));
    });
  });

  function toggle(id: string, on: boolean) {
    selection = on ? [...selection, id] : selection.filter((m) => m !== id);
  }
</script>

<Modal title={t("modules.pickTitle")} width="720px" {onclose}>
  <div class="filters">
    {#if modules.length > 6}
      <input type="search" placeholder={t("modules.search")} aria-label={t("modules.search")} bind:value={query} />
    {/if}
    {#if categories.length > 1}
      <div class="segmented" role="group" aria-label={t("modules.category")}>
        <button type="button" class:active={category === null} onclick={() => (category = null)}>
          {t("modules.allCategories")}
        </button>
        {#each categories as c (c)}
          <button type="button" class:active={category === c} onclick={() => (category = c)}>
            {t(`category.${c}`)}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <ul class="list">
    {#each shown as module (module.id)}
      {@const on = selection.includes(module.id)}
      <li class:on>
        <label>
          <input type="checkbox" checked={on} onchange={(e) => toggle(module.id, e.currentTarget.checked)} />
          <span class="text">
            <span class="head">
              <strong>{localized(module.name)}</strong>
              <small class="tag">{t(`category.${module.category}`)}</small>
            </span>
            <span class="muted desc">{localized(module.description)}</span>
          </span>
        </label>
        {#if module.homepage}
          <button type="button" class="link" onclick={() => void openExternal(module.homepage)}>
            {t("modules.homepage")} ›
          </button>
        {/if}
      </li>
    {:else}
      <li class="empty muted">{t("modules.noMatch")}</li>
    {/each}
  </ul>

  {#snippet actions()}
    <span class="count muted">{t("modules.selected", { count: selection.length, total: modules.length })}</span>
    <button type="button" class="btn" onclick={onclose}>{t("common.cancel")}</button>
    <button type="button" class="btn btn-play" onclick={() => onapply(selection)}>{t("common.apply")}</button>
  {/snippet}
</Modal>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }

  .filters input {
    flex: 1;
    min-width: 12rem;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  li {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    background: var(--surface-muted);
    border: 1px solid var(--border);
    border-left: 3px solid var(--border);
  }

  li.on {
    border-left-color: var(--frame-light);
  }

  label {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    cursor: pointer;
  }

  label input {
    margin-top: 0.3em;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .head {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
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

  .desc {
    font-size: 0.85rem;
    line-height: 1.4;
  }

  .link {
    flex: none;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }

  .link:hover {
    text-decoration: underline;
  }

  .empty {
    justify-content: center;
    font-style: italic;
  }

  .count {
    margin-right: auto;
    align-self: center;
    font-size: 0.82rem;
  }
</style>
