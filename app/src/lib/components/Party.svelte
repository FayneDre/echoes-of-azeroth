<script lang="ts">
  // "Party" page of the Bots tab (ADR 0025): the character the player plays,
  // the bots already in its party, and new bots by role, class and spec at
  // the level chosen (the character's by default). Works while the servers
  // run and the player is in the world.
  import { untrack } from "svelte";
  import {
    api,
    asCommandError,
    type CommandError,
    type EmulatorOverview,
    type OnlineCharacter,
    type Party,
    type PartyClass,
    type PartyMember,
    type PartyRole,
  } from "../api";
  import { localized, t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import Notify from "./Notify.svelte";

  let { realm, party }: { realm: EmulatorOverview; party: Party } = $props();

  const instance = $derived(launcher.instanceFor(realm.id));
  const id = $derived(instance?.record.id ?? null);
  const session = $derived(id ? launcher.session(id) : null);
  const online = $derived(!!id && launcher.isRunning(id) && session?.phase === "ready");

  /** Whether the realm's server build has the party module. */
  let offered = $state<boolean | null>(null);
  const buildKey = $derived(instance ? `${instance.record.id}:${instance.record.build ?? ""}` : "");
  $effect(() => {
    void buildKey;
    const realmId = id;
    offered = null;
    if (!realmId) return;
    api
      .partyOffered(realmId)
      .then((value) => (offered = value))
      .catch(() => (offered = false));
  });

  /** The player's characters in the world, refreshed every 20 s. */
  let players = $state<OnlineCharacter[] | null>(null);
  $effect(() => {
    if (!online || !id) {
      players = null;
      return;
    }
    const realmId = id;
    const load = () =>
      api
        .onlineCharacters(realmId)
        .then((list) => (players = (list ?? []).filter((c) => !c.bot)))
        .catch(() => {});
    load();
    const timer = setInterval(load, 20_000);
    return () => clearInterval(timer);
  });

  let picked = $state<string>("");
  /** The character played: the one picked, or the only one in the world. */
  const player = $derived(
    players?.find((c) => c.name === picked) ?? (players?.length === 1 ? players[0] : undefined) ?? null,
  );

  /** Bots level: the character's until the player changes it. */
  let level = $state(1);
  let levelFor = "";
  $effect(() => {
    const p = player;
    if (p && p.name !== levelFor) {
      levelFor = p.name;
      level = p.level;
    }
  });

  let members = $state<PartyMember[]>([]);
  let error = $state<CommandError | null>(null);
  let done = $state<string | null>(null);
  let busy = $state(false);

  async function loadMembers() {
    if (!id || !player) {
      members = [];
      return;
    }
    try {
      members = await api.partyMembers(id, player.name);
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  // Listed again when the character changes; every 3 s while bots join.
  $effect(() => {
    void player?.name;
    if (online && offered) untrack(() => void loadMembers());
  });
  $effect(() => {
    if (!members.some((m) => !m.ready)) return;
    const timer = setInterval(() => void loadMembers(), 3_000);
    return () => clearInterval(timer);
  });

  const ROLES: PartyRole[] = ["tank", "healer", "dps"];
  type Slot = { role: PartyRole | ""; cls: string; spec: number };
  const DEFAULT_ROLES: (PartyRole | "")[] = ["tank", "healer", "dps", "dps"];
  let slots = $state<Slot[]>([]);

  const free = $derived(Math.max(0, party.size - 1 - members.length));

  function classesFor(role: PartyRole | ""): PartyClass[] {
    if (!role) return [];
    return party.classes.filter((c) => (c.minLevel ?? 1) <= level && c.specs.some((s) => s.role === role));
  }

  /** A slot with a class and spec that fit its role (the first ones). */
  function fit(slot: Slot): Slot {
    const classes = classesFor(slot.role);
    const cls = classes.find((c) => c.id === slot.cls) ?? classes[0];
    if (!cls) return { role: slot.role, cls: "", spec: 0 };
    const specs = cls.specs.filter((s) => s.role === slot.role);
    const spec = specs.find((s) => s.number === slot.spec) ?? specs[0];
    return { role: slot.role, cls: cls.id, spec: spec?.number ?? 0 };
  }

  // As many slots as free places; a new slot takes the next default role.
  $effect(() => {
    const count = free;
    untrack(() => {
      const next = slots.slice(0, count);
      while (next.length < count) next.push(fit({ role: DEFAULT_ROLES[next.length] ?? "dps", cls: "", spec: 0 }));
      slots = next;
    });
  });

  // A level under a class's minimum (death knight) changes its class.
  $effect(() => {
    void level;
    untrack(() => (slots = slots.map(fit)));
  });

  function setRole(index: number, role: PartyRole | "") {
    slots[index] = fit({ ...slots[index]!, role });
  }

  function setClass(index: number, cls: string) {
    slots[index] = fit({ ...slots[index]!, cls, spec: -1 });
  }

  const wanted = $derived(slots.filter((s) => s.role && s.cls));
  const levelValid = $derived(Number.isInteger(level) && level >= 1 && level <= 255);

  async function form() {
    if (!id || !player) return;
    busy = true;
    error = null;
    done = null;
    let added = 0;
    try {
      for (const slot of wanted) {
        await api.partyAdd(id, player.name, slot.cls, slot.spec, level);
        added += 1;
      }
    } catch (raw) {
      error = asCommandError(raw);
    } finally {
      busy = false;
      if (added > 0) done = t("party.added", { count: added });
      await loadMembers();
    }
  }

  async function dismiss() {
    if (!id || !player) return;
    busy = true;
    error = null;
    done = null;
    try {
      const names = await api.partyRemove(id, player.name);
      done = t("party.dismissed", { count: names.length });
    } catch (raw) {
      error = asCommandError(raw);
    } finally {
      busy = false;
      await loadMembers();
    }
  }

  function className(classId: string): string {
    const cls = party.classes.find((c) => c.id === classId);
    return cls ? localized(cls.name) : classId;
  }
</script>

{#if !online}
  <section class="panel">
    <h3 class="panel-title">{t("party.title")}</h3>
    <p class="muted">{t("party.startFirst")}</p>
  </section>
{:else if offered === false}
  <section class="panel">
    <h3 class="panel-title">{t("party.title")}</h3>
    <p class="muted">{t("party.updateFirst")}</p>
  </section>
{:else if players !== null && players.length === 0}
  <section class="panel">
    <h3 class="panel-title">{t("party.title")}</h3>
    <p class="muted">{t("party.noPlayer")}</p>
  </section>
{:else}
  <div class="split">
    <section class="panel">
      <h3 class="panel-title">{t("party.title")}</h3>
      <p class="muted">{t("party.body")}</p>
      <div class="fields">
        <label>
          <span>{t("party.player")}</span>
          <select
            value={player?.name ?? ""}
            disabled={busy || !players}
            onchange={(e) => (picked = e.currentTarget.value)}
          >
            {#if !player}<option value="">—</option>{/if}
            {#each players ?? [] as c (c.name)}
              <option value={c.name}>{c.name} ({t("gm.level", { level: c.level })})</option>
            {/each}
          </select>
        </label>
        <label>
          <span>{t("party.level")}</span>
          <input type="number" min="1" max="255" step="1" disabled={busy} bind:value={level} />
          <small class="muted">{t("party.levelHint")}</small>
        </label>
      </div>

      <h4>{t("party.members")}</h4>
      {#if members.length === 0}
        <p class="muted">{t("party.none")}</p>
      {:else}
        <ul class="members">
          {#each members as m (m.name)}
            <li>
              <strong>{m.name}</strong>
              <span class="muted">{t("gm.level", { level: m.level })} · {className(m.class)}</span>
              {#if !m.ready}<span class="joining">{t("party.joining")}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      <div class="actions">
        <button class="btn" disabled={busy || !player || members.length === 0} onclick={dismiss}>
          {t("party.dismiss")}
        </button>
      </div>
    </section>

    <section class="panel">
      <h3 class="panel-title">{t("party.new")}</h3>
      {#if free === 0}
        <p class="muted">{t("party.full", { size: party.size })}</p>
      {:else}
        <p class="muted">{t("party.free", { count: free })}</p>
        <div class="slots">
          <span class="head">{t("party.role")}</span>
          <span class="head">{t("party.class")}</span>
          <span class="head">{t("party.spec")}</span>
          {#each slots as slot, index (index)}
            <select
              aria-label={`${t("party.role")} ${index + 1}`}
              value={slot.role}
              disabled={busy}
              onchange={(e) => setRole(index, e.currentTarget.value as PartyRole | "")}
            >
              <option value="">{t("party.role.none")}</option>
              {#each ROLES as role (role)}
                <option value={role}>{t(`party.role.${role}`)}</option>
              {/each}
            </select>
            <select
              aria-label={`${t("party.class")} ${index + 1}`}
              value={slot.cls}
              disabled={busy || !slot.role}
              onchange={(e) => setClass(index, e.currentTarget.value)}
            >
              {#each classesFor(slot.role) as cls (cls.id)}
                <option value={cls.id}>{localized(cls.name)}</option>
              {/each}
            </select>
            <select
              aria-label={`${t("party.spec")} ${index + 1}`}
              value={slot.spec}
              disabled={busy || !slot.cls}
              onchange={(e) => (slots[index] = { ...slot, spec: Number(e.currentTarget.value) })}
            >
              {#each party.classes.find((c) => c.id === slot.cls)?.specs.filter((s) => s.role === slot.role) ?? [] as spec (spec.number)}
                <option value={spec.number}>{localized(spec.name)}</option>
              {/each}
            </select>
          {/each}
        </div>
        <div class="actions">
          <button
            class="btn btn-play"
            disabled={busy || !player || wanted.length === 0 || !levelValid}
            onclick={form}
          >
            {busy ? t("party.joiningAll") : t("party.form")}
          </button>
        </div>
      {/if}
    </section>
  </div>
  <Notify text={done} />
  <Notify {error} />
{/if}

<style>
  .split {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr);
    gap: var(--space-4);
    align-items: start;
  }

  .fields {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-4);
    margin: var(--space-3) 0;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  h4 {
    margin: var(--space-4) 0 var(--space-2);
    color: var(--title);
    font-family: var(--font-display);
    font-weight: normal;
  }

  .members {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .members li {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
  }

  .joining {
    color: var(--accent);
    font-size: 0.85rem;
  }

  .slots {
    display: grid;
    grid-template-columns: minmax(0, 0.8fr) minmax(0, 1fr) minmax(0, 1.2fr);
    gap: var(--space-2) var(--space-3);
    margin-top: var(--space-3);
  }

  .head {
    font-size: 0.8rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: var(--space-4);
  }
</style>
