<script lang="ts">
  // Home of one realm: the install wizard until an instance exists, then
  // the instance itself. Every state shown comes from the backend.
  import type { EmulatorOverview } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import InstallWizard from "./InstallWizard.svelte";
  import InstancePanel from "./InstancePanel.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const overview = $derived(launcher.overview);
  const instance = $derived(overview?.instances.find((i) => i.record.emulator === realm.id) ?? null);
  const client = $derived(overview?.clients.find((c) => c.id === realm.client?.id) ?? null);

  /** Id of a new instance: the emulator id, made unique. */
  const newId = $derived.by(() => {
    const taken = new Set(overview?.instances.map((i) => i.record.id) ?? []);
    let id = realm.id;
    for (let n = 2; taken.has(id); n++) id = `${realm.id}-${n}`;
    return id;
  });
</script>

{#if launcher.error}
  <ErrorBox error={launcher.error} />
{:else if !overview}
  <p class="muted">{t("common.loading")}</p>
{:else if instance}
  <InstancePanel {realm} {instance} {client} />
{:else}
  <InstallWizard {realm} {client} instanceId={newId} />
{/if}
