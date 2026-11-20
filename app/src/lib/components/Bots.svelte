<script lang="ts">
  // "Bots" tab, two pages: the options of the realm's bot modules (catalog
  // category "bots", e.g. Playerbots), applied with the servers stopped, and
  // the party of bots formed for the character played (ADR 0025).
  import { untrack } from "svelte";
  import { api, asCommandError, type CommandError, type EmulatorOverview, type ModuleOptions } from "../api";
  import { localized, t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import Notify from "./Notify.svelte";
  import OptionsPanel from "./OptionsPanel.svelte";
  import Party from "./Party.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived(launcher.instanceFor(realm.id));
  const id = $derived(instance?.record.id ?? null);
  const running = $derived(id !== null && launcher.isRunning(id));
  const locked = $derived(
    running || launcher.progress !== null || !!instance?.unfinished || !instance?.installed,
  );
  const botModules = $derived(realm.modules.filter((m) => m.category === "bots"));

  let page = $state<"settings" | "party">("settings");

  let options = $state<ModuleOptions[]>([]);
  let error = $state<CommandError | null>(null);

  const recordKey = $derived(instance ? JSON.stringify(instance.record) : "");

  async function load() {
    if (!id) {
      options = [];
      return;
    }
    try {
      const all = await api.options(id);
      options = all.filter((m) => botModules.some((b) => b.id === m.module));
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  $effect(() => {
    void recordKey;
    untrack(() => void load());
  });

  function name(moduleId: string): string {
    const module = realm.modules.find((m) => m.id === moduleId);
    return module ? localized(module.name) : moduleId;
  }
</script>

<Notify {error} />

{#if instance && realm.party}
  <nav class="segmented pages" aria-label={t("bots.pages")}>
    <button class:active={page === "settings"} onclick={() => (page = "settings")}>{t("bots.settings")}</button>
    <button class:active={page === "party"} onclick={() => (page = "party")}>{t("party.tab")}</button>
  </nav>
{/if}

{#if instance && realm.party && page === "party"}
  <Party {realm} party={realm.party} />
{:else if !instance}
  <section class="panel">
    <h3 class="panel-title">{t("bots.title")}</h3>
    <p class="muted">{t("settings.installFirst")}</p>
  </section>
{:else if botModules.length === 0}
  <section class="panel">
    <h3 class="panel-title">{t("bots.title")}</h3>
    <p class="muted">{t("bots.none")}</p>
  </section>
{:else}
  <p class="muted intro">{running ? t("settings.stopFirst") : t("bots.body")}</p>
  {#each options as module (module.module)}
    {#if id}
      <OptionsPanel
        instanceId={id}
        {module}
        title={name(module.module)}
        columns={2}
        {locked}
        onerror={(e) => (error = e)}
      />
    {/if}
  {:else}
    <section class="panel">
      <h3 class="panel-title">{t("bots.title")}</h3>
      <p class="muted">{t("bots.noOptions")}</p>
    </section>
  {/each}
{/if}

<style>
  .pages {
    align-self: flex-start;
  }

  .intro {
    margin: 0;
  }
</style>
