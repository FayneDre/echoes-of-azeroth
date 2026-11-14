<script lang="ts">
  // Left rail: one emblem per game version (realm) from the catalog.
  // Adding an emulator/client to the catalog adds an entry here; no UI change.
  import type { EmulatorOverview } from "../api";
  import Emblem from "../emblems/Emblem.svelte";
  import { localized, t } from "../i18n/i18n.svelte";

  let {
    realms,
    selected,
    onselect,
  }: { realms: EmulatorOverview[]; selected: string | null; onselect: (id: string) => void } = $props();

  const label = (realm: EmulatorOverview) =>
    realm.client ? `${localized(realm.client.shortName)} · ${realm.name}` : realm.name;
</script>

<nav class="rail" aria-label={t("nav.realms")}>
  <ul>
    {#each realms as realm (realm.id)}
      <li>
        <button
          class="realm"
          class:active={realm.id === selected}
          style:--item-accent={realm.client?.accent ?? "var(--accent)"}
          aria-current={realm.id === selected ? "page" : undefined}
          title={label(realm)}
          onclick={() => onselect(realm.id)}
        >
          <Emblem
            emblem={realm.client?.emblem ?? "generic"}
            accent={realm.client?.accent ?? "var(--accent)"}
            label={label(realm)}
            size={40}
          />
          <span class="version">{realm.client?.version ?? realm.name}</span>
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  .rail {
    width: var(--rail-width);
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-4) 0;
    background: linear-gradient(90deg, var(--shade), var(--shade-soft));
    border-right: 2px solid var(--frame);
    box-shadow: 1px 0 0 color-mix(in srgb, var(--frame-light) 50%, transparent);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .realm {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    width: 68px;
    aspect-ratio: 1;
    padding: 0;
    background: var(--shade);
    border: 2px solid var(--frame);
    border-radius: var(--radius);
    box-shadow: inset 0 0 12px var(--shade-deep);
    cursor: pointer;
    opacity: 0.7;
    transition:
      opacity 0.15s,
      border-color 0.15s;
  }

  .realm:hover {
    opacity: 1;
  }

  .realm.active {
    opacity: 1;
    border-color: var(--frame-light);
    background: color-mix(in srgb, var(--item-accent) 12%, var(--shade));
    box-shadow:
      inset 0 0 12px var(--shade-deep),
      0 0 12px color-mix(in srgb, var(--item-accent) 45%, transparent);
  }

  .realm.active::before {
    content: "";
    position: absolute;
    left: -9px;
    top: 18%;
    bottom: 18%;
    width: 3px;
    border-radius: 2px;
    background: var(--item-accent);
  }

  .version {
    font-size: 0.75rem;
    letter-spacing: 0.05em;
    color: var(--text-muted);
  }

  .realm.active .version {
    color: var(--item-accent);
  }
</style>
