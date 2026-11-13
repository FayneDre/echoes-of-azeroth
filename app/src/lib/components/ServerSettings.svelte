<script lang="ts">
  // "Server settings" tab. First section: the realm's modules (mandatory ones
  // are always installed; optional ones can be added or removed at any time).
  import type { EmulatorOverview } from "../api";
  import { localized, t, type MessageKey } from "../i18n/i18n.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();
</script>

<section class="panel">
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
    border-left: 3px solid var(--frame-light);
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
</style>
