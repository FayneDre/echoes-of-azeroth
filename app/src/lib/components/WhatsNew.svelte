<script lang="ts">
  // After a launcher update: once, a dialog with the changes of the version
  // now running, from the notes shipped in this build.
  import { appVersion } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { bundledNotes, justUpdated, type ReleaseNotes } from "../whatsNew";
  import Modal from "./Modal.svelte";
  import ReleaseNotesList from "./ReleaseNotesList.svelte";

  let shown = $state<{ version: string; notes: ReleaseNotes } | null>(null);

  $effect(() => {
    appVersion()
      .then((version) => {
        const notes = bundledNotes(version);
        if (justUpdated(version) && notes) shown = { version, notes };
      })
      .catch(() => {});
  });
</script>

{#if shown}
  <Modal title={t("whatsNew.title", { version: shown.version })} onclose={() => (shown = null)} width="560px">
    <ReleaseNotesList notes={shown.notes} />
    {#snippet actions()}
      <button class="btn btn-play" onclick={() => (shown = null)}>{t("whatsNew.dismiss")}</button>
    {/snippet}
  </Modal>
{/if}
