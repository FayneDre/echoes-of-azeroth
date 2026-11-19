<script lang="ts">
  // The changes of one launcher version, in the player's language.
  import { t } from "../i18n/i18n.svelte";
  import { noteLines, type ReleaseNotes } from "../whatsNew";

  let { notes }: { notes: ReleaseNotes | null } = $props();

  const lines = $derived(noteLines(notes));
</script>

{#if lines.length > 0}
  <ul class="notes">
    {#each lines as line, i (i)}
      <li>{line}</li>
    {/each}
  </ul>
{:else}
  <p class="muted">{t("appUpdate.noNotes")}</p>
{/if}

<style>
  .notes {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-height: 50vh;
    overflow: auto;
  }

  .notes li {
    position: relative;
    padding-left: var(--space-4);
    line-height: 1.45;
  }

  /* A small gilded diamond, like the panel corners. */
  .notes li::before {
    content: "";
    position: absolute;
    left: 2px;
    top: 0.55em;
    width: 6px;
    height: 6px;
    background: var(--accent);
    transform: rotate(45deg);
    box-shadow: 0 0 4px var(--glow);
  }
</style>
