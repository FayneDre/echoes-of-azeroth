<script lang="ts">
  // The launcher's own options (not a realm's), opened from the title bar:
  // what happens when the game starts and closes, automatic backups. Each
  // change is saved at once in preferences.json.
  import { asCommandError, type CommandError, type Preferences } from "../api";
  import { i18n, LOCALES, t, type MessageKey } from "../i18n/i18n.svelte";
  import { preferences } from "../preferences.svelte";
  import Modal from "./Modal.svelte";
  import Notify from "./Notify.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let error = $state<CommandError | null>(null);
  let saved = $state<string | null>(null);

  type Key = keyof Preferences;
  const PLAY: { key: Key; label: MessageKey; hint: MessageKey }[] = [
    { key: "keepServersRunning", label: "options.keepRunning", hint: "options.keepRunningHint" },
    { key: "hideWhilePlaying", label: "options.hideWhilePlaying", hint: "options.hideWhilePlayingHint" },
  ];
  const BACKUPS: { key: Key; label: MessageKey; hint: MessageKey }[] = [
    { key: "dailyBackup", label: "backups.daily", hint: "backups.dailyHint" },
  ];

  const SECTIONS: { title: MessageKey; items: typeof PLAY }[] = [
    { title: "options.play", items: PLAY },
    { title: "options.backups", items: BACKUPS },
  ];

  async function change(key: Key, value: boolean) {
    error = null;
    saved = null;
    try {
      await preferences.set({ [key]: value });
      saved = t("options.saved");
    } catch (raw) {
      error = asCommandError(raw);
    }
  }
</script>

<Modal title={t("options.title")} {onclose} width="560px">
  <h4>{t("options.language")}</h4>
  <div class="segmented languages" role="group" aria-label={t("nav.language")}>
    {#each LOCALES as locale (locale.id)}
      <button
        class:active={i18n.locale === locale.id}
        aria-pressed={i18n.locale === locale.id}
        onclick={() => (i18n.locale = locale.id)}
      >
        {locale.label}
      </button>
    {/each}
  </div>
  {#if preferences.value}
    {@const value = preferences.value}
    {#each SECTIONS as section (section.title)}
      <h4>{t(section.title)}</h4>
      {#each section.items as item (item.key)}
        <label class="option">
          <input
            type="checkbox"
            checked={value[item.key]}
            onchange={(e) => void change(item.key, e.currentTarget.checked)}
          />
          <span>
            {t(item.label)}
            <small class="muted">{t(item.hint)}</small>
          </span>
        </label>
      {/each}
    {/each}
  {:else}
    <p class="muted">{t("modules.loading")}</p>
  {/if}
</Modal>
<Notify text={saved} />
<Notify {error} />

<style>
  h4 {
    margin: var(--space-4) 0 var(--space-2);
    color: var(--title);
    font-family: var(--font-display);
    font-weight: normal;
  }

  h4:first-child {
    margin-top: 0;
  }

  .languages {
    width: fit-content;
    margin-bottom: var(--space-2);
  }

  .option {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    padding: var(--space-2) 0;
    cursor: pointer;
  }

  .option input {
    margin-top: 0.2em;
  }

  .option span {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
</style>
