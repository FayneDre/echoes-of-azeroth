<script lang="ts">
  // "Game master" tab (ADR 0020): characters in the world and ready-made
  // console commands from the emulator manifest. Each command is checked by
  // the launcher, typed in the server console, and answered in its log.
  import {
    api,
    asCommandError,
    type CommandError,
    type ConsoleAction,
    type EmulatorOverview,
    type OnlineCharacter,
  } from "../api";
  import { hasMessage, localized, t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import Notify from "./Notify.svelte";

  let { realm }: { realm: EmulatorOverview } = $props();

  const instance = $derived(launcher.instanceFor(realm.id));
  const id = $derived(instance?.record.id ?? null);
  const session = $derived(id ? launcher.session(id) : null);
  const online = $derived(!!id && launcher.isRunning(id) && (!session || session.phase === "ready"));

  /** Refreshed every 20 s while the tab is open and the realm online. */
  const REFRESH_MS = 20_000;
  let characters = $state<OnlineCharacter[] | null>(null);
  let showBots = $state(false);
  let locations = $state<string[]>([]);

  $effect(() => {
    if (!online || !id) {
      characters = null;
      return;
    }
    const realmId = id;
    const load = () =>
      api
        .onlineCharacters(realmId)
        .then((list) => (characters = list))
        .catch(() => {});
    load();
    const timer = setInterval(load, REFRESH_MS);
    return () => clearInterval(timer);
  });

  $effect(() => {
    if (!online || !id || !realm.consoleActions.some((a) => a.params.some((p) => p.type === "location"))) return;
    api
      .teleportLocations(id)
      .then((list) => (locations = list))
      .catch(() => (locations = []));
  });

  const players = $derived(characters?.filter((c) => !c.bot) ?? []);
  const bots = $derived(characters?.filter((c) => c.bot) ?? []);

  let selected = $state<string>("");
  const action = $derived<ConsoleAction | null>(
    realm.consoleActions.find((a) => a.id === selected) ?? realm.consoleActions[0] ?? null,
  );
  // The picker shows the action in use (the first one until one is picked).
  $effect(() => {
    if (action && selected !== action.id) selected = action.id;
  });
  let values = $state<Record<string, string>>({});
  let error = $state<CommandError | null>(null);
  let sent = $state<string | null>(null);

  // Defaults of the action just picked; the character picked stays.
  $effect(() => {
    if (!action) return;
    const next: Record<string, string> = {};
    for (const param of action.params) {
      if (param.type === "character") next[param.id] = values[param.id] ?? "";
      else if (param.type === "number") next[param.id] = param.default !== undefined ? String(param.default) : "";
      else if (param.type === "choice") next[param.id] = param.choices[0]?.value ?? "";
      else next[param.id] = "";
    }
    values = next;
    error = null;
    sent = null;
  });

  function pick(name: string) {
    if (!action) return;
    const param = action.params.find((p) => p.type === "character");
    if (param) values = { ...values, [param.id]: name };
  }

  async function run(event: SubmitEvent) {
    event.preventDefault();
    if (!id || !action) return;
    error = null;
    sent = null;
    try {
      sent = await api.runConsoleAction(id, action.id, values);
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  /** A character's race and class in the player's language, when known. */
  function describe(c: OnlineCharacter): string {
    const race = `race.${c.race}`;
    const cls = `class.${c.class}`;
    return `${hasMessage(race) ? t(race) : c.race} ${hasMessage(cls) ? t(cls) : c.class}`;
  }

  /** The parameter an error names, shown by its label. */
  function withLabels(error: CommandError, action: ConsoleAction): CommandError {
    const paramId = error.params?.param;
    const param = paramId ? action.params.find((p) => p.id === paramId) : undefined;
    return param ? { ...error, params: { ...error.params, param: localized(param.label) } } : error;
  }
</script>

{#if !instance}
  <section class="panel"><p class="muted">{t("settings.installFirst")}</p></section>
{:else if !online}
  <section class="panel">
    <h3 class="panel-title">{t("gm.title")}</h3>
    <p class="muted">{t("gm.startFirst")}</p>
  </section>
{:else}
  <div class="columns">
    <section class="panel column">
      <h3 class="panel-title">{t("gm.online")}</h3>
      {#if characters === null}
        <p class="muted">{t("gm.loading")}</p>
      {:else}
        <p class="muted">{t("gm.counts", { players: players.length, bots: bots.length })}</p>
        {#if players.length === 0}
          <p class="muted">{t("gm.noPlayers")}</p>
        {/if}
        <ul class="characters">
          {#each players as c (c.name)}
            <li>
              <button class="link" onclick={() => pick(c.name)} title={t("gm.pick")}>{c.name}</button>
              <span class="muted">{t("gm.level", { level: c.level })} · {describe(c)}</span>
            </li>
          {/each}
        </ul>
        {#if bots.length > 0}
          <label class="check">
            <input type="checkbox" bind:checked={showBots} />
            <span>{t("gm.showBots")}</span>
          </label>
          {#if showBots}
            <ul class="characters bots">
              {#each bots as c (c.name)}
                <li>
                  <button class="link" onclick={() => pick(c.name)} title={t("gm.pick")}>{c.name}</button>
                  <span class="muted">{t("gm.level", { level: c.level })} · {describe(c)}</span>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
      {/if}
    </section>

    <section class="panel column">
      <h3 class="panel-title">{t("gm.actions")}</h3>
      {#if action}
        <form onsubmit={run}>
          <label>
            <span>{t("gm.action")}</span>
            <select bind:value={selected}>
              {#each realm.consoleActions as a (a.id)}
                <option value={a.id}>{localized(a.label)}</option>
              {/each}
            </select>
          </label>
          {#if action.description}
            <p class="muted small">{localized(action.description)}</p>
          {/if}
          {#each action.params as param (param.id)}
            <label>
              <span>{localized(param.label)}</span>
              {#if param.type === "character"}
                <input
                  type="text"
                  list="gm-characters"
                  autocomplete="off"
                  spellcheck="false"
                  maxlength="12"
                  bind:value={values[param.id]}
                />
              {:else if param.type === "number"}
                <input type="number" min={param.min} max={param.max} step="1" bind:value={values[param.id]} />
              {:else if param.type === "location"}
                <input type="text" list="gm-locations" autocomplete="off" spellcheck="false" bind:value={values[param.id]} />
              {:else if param.type === "choice"}
                <select bind:value={values[param.id]}>
                  {#each param.choices as choice (choice.value)}
                    <option value={choice.value}>{localized(choice.label)}</option>
                  {/each}
                </select>
              {:else}
                <input type="text" maxlength={param.maxLength ?? 255} bind:value={values[param.id]} />
              {/if}
            </label>
          {/each}
          <div class="actions">
            <button class="btn btn-play" type="submit">{t("gm.run")}</button>
          </div>
        </form>
        <datalist id="gm-characters">
          {#each characters ?? [] as c (c.name)}
            <option value={c.name}></option>
          {/each}
        </datalist>
        <datalist id="gm-locations">
          {#each locations as place (place)}
            <option value={place}></option>
          {/each}
        </datalist>
        <Notify text={sent ? `${t("gm.sent")} ${sent}` : null} />
        <Notify error={error ? withLabels(error, action) : null} />
      {:else}
        <p class="muted">{t("gm.none")}</p>
      {/if}
    </section>
  </div>
{/if}

<style>
  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: var(--space-4);
  }

  @media (max-width: 900px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .characters {
    list-style: none;
    margin: var(--space-2) 0;
    padding: 0;
    max-height: 320px;
    overflow: auto;
  }

  .characters li {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    padding: 2px 0;
  }

  .bots {
    max-height: 220px;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
  }

  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }

  .link:hover,
  .link:focus-visible {
    text-decoration: underline;
  }

  .small {
    font-size: 0.85rem;
    margin: 0;
  }

</style>
