<script lang="ts">
  // "Party" page of the Bots tab (ADR 0025): the party as a row of places,
  // the character played first, then the bots already in it, then the free
  // places, each with the role, class, spec and level the player wants.
  // Works while the servers run and the player is in the world.
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
  /** One free place: what the player asks for it (no role: left empty). */
  type Slot = { role: PartyRole | ""; cls: string; spec: number; level: number };
  const DEFAULT_ROLES: (PartyRole | "")[] = ["tank", "healer", "dps", "dps"];
  let slots = $state<Slot[]>([]);

  const free = $derived(Math.max(0, party.size - 1 - members.length));
  const playerLevel = $derived(player?.level ?? 1);

  function classesFor(slot: Slot): PartyClass[] {
    if (!slot.role) return [];
    return party.classes.filter((c) => (c.minLevel ?? 1) <= slot.level && c.specs.some((s) => s.role === slot.role));
  }

  /** A slot whose class and spec fit its role and level (the first ones). */
  function fit(slot: Slot): Slot {
    const classes = classesFor(slot);
    const cls = classes.find((c) => c.id === slot.cls) ?? classes[0];
    if (!cls) return { ...slot, cls: "", spec: 0 };
    const specs = cls.specs.filter((s) => s.role === slot.role);
    const spec = specs.find((s) => s.number === slot.spec) ?? specs[0];
    return { ...slot, cls: cls.id, spec: spec?.number ?? 0 };
  }

  // As many slots as free places; a new slot takes the next default role
  // and the character's level.
  $effect(() => {
    const count = free;
    const level = playerLevel;
    untrack(() => {
      const next = slots.slice(0, count);
      while (next.length < count) next.push(fit({ role: DEFAULT_ROLES[next.length] ?? "dps", cls: "", spec: 0, level }));
      slots = next;
    });
  });

  // Another character: every slot starts again at its level.
  let levelFor = "";
  $effect(() => {
    const p = player;
    if (p && p.name !== levelFor) {
      levelFor = p.name;
      untrack(() => (slots = slots.map((slot) => fit({ ...slot, level: p.level }))));
    }
  });

  function update(index: number, change: Partial<Slot>) {
    const slot = { ...slots[index]!, ...change };
    if (change.cls !== undefined) slot.spec = -1;
    slots[index] = fit(slot);
  }

  function setLevel(index: number, raw: string) {
    const level = Math.round(Number(raw));
    if (Number.isFinite(level) && raw !== "") update(index, { level: Math.min(Math.max(level, 1), 255) });
  }

  const wanted = $derived(slots.filter((s) => s.role && s.cls));

  async function form() {
    if (!id || !player) return;
    busy = true;
    error = null;
    done = null;
    let added = 0;
    try {
      for (const slot of wanted) {
        await api.partyAdd(id, player.name, slot.cls, slot.spec, slot.level);
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
  <section class="panel">
    <h3 class="panel-title">{t("party.title")}</h3>
    <div class="head">
      <p class="muted">{t("party.body")}</p>
      <label class="player">
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
    </div>

    <ol class="roster" style:--size={party.size}>
      <li class="card you">
        <span class="tag">{t("party.you")}</span>
        <strong class="name">{player?.name ?? "—"}</strong>
        {#if player}
          <span class="muted">{t("gm.level", { level: player.level })} · {className(player.class)}</span>
        {/if}
      </li>

      {#each members as m (m.name)}
        <li class="card member">
          <span class="tag">{m.ready ? t("party.inParty") : t("party.joining")}</span>
          <strong class="name">{m.name}</strong>
          <span class="muted">{t("gm.level", { level: m.level })} · {className(m.class)}</span>
        </li>
      {/each}

      {#each slots as slot, index (index)}
        <li class="card slot" class:empty={!slot.role}>
          <span class="tag">{t("party.freeSlot")}</span>
          <div class="roles" role="group" aria-label={`${t("party.role")} ${index + 1}`}>
            {#each ROLES as role (role)}
              <button
                class:active={slot.role === role}
                aria-pressed={slot.role === role}
                disabled={busy}
                onclick={() => update(index, { role })}
              >
                {t(`party.role.${role}`)}
              </button>
            {/each}
            <button
              class:active={!slot.role}
              aria-pressed={!slot.role}
              disabled={busy}
              onclick={() => update(index, { role: "" })}
            >
              {t("party.role.none")}
            </button>
          </div>
          {#if slot.role}
            <label>
              <span>{t("party.class")}</span>
              <select value={slot.cls} disabled={busy} onchange={(e) => update(index, { cls: e.currentTarget.value })}>
                {#each classesFor(slot) as cls (cls.id)}
                  <option value={cls.id}>{localized(cls.name)}</option>
                {/each}
              </select>
            </label>
            <label>
              <span>{t("party.spec")}</span>
              <select
                value={slot.spec}
                disabled={busy || !slot.cls}
                onchange={(e) => update(index, { spec: Number(e.currentTarget.value) })}
              >
                {#each party.classes
                  .find((c) => c.id === slot.cls)
                  ?.specs.filter((s) => s.role === slot.role) ?? [] as spec (spec.number)}
                  <option value={spec.number}>{localized(spec.name)}</option>
                {/each}
              </select>
            </label>
            <label>
              <span>{t("party.level")}</span>
              <input
                type="number"
                min="1"
                max="255"
                step="1"
                value={slot.level}
                disabled={busy}
                onchange={(e) => setLevel(index, e.currentTarget.value)}
              />
            </label>
          {:else}
            <p class="muted leave">{t("party.leftEmpty")}</p>
          {/if}
        </li>
      {/each}
    </ol>

    <div class="actions">
      <button class="btn" disabled={busy || !player || members.length === 0} onclick={dismiss}>
        {t("party.dismiss")}
      </button>
      {#if free > 0}
        <button class="btn btn-play" disabled={busy || !player || wanted.length === 0} onclick={form}>
          {busy ? t("party.joiningAll") : t("party.form", { count: wanted.length })}
        </button>
      {/if}
    </div>
  </section>
  <Notify text={done} />
  <Notify {error} />
{/if}

<style>
  .head {
    display: flex;
    gap: var(--space-6);
    align-items: flex-end;
    justify-content: space-between;
    margin-bottom: var(--space-4);
  }

  .head p {
    margin: 0;
    max-width: 60ch;
  }

  .player {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 260px;
  }

  .roster {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(var(--size), minmax(0, 1fr));
    gap: var(--space-3);
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 270px;
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-muted);
  }

  .card.you {
    border-color: var(--accent);
  }

  .card.slot.empty {
    border-style: dashed;
    background: transparent;
  }

  .tag {
    font-size: 0.75rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .you .tag,
  .member .tag {
    color: var(--accent);
  }

  .name {
    color: var(--title);
    font-family: var(--font-display);
    font-size: 1.15rem;
    font-weight: normal;
  }

  /* Two by two: the labels never squeeze each other in a narrow card. */
  .roles {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-1);
  }

  .roles button {
    min-width: 0;
    padding: var(--space-1) var(--space-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    background: var(--button-bg);
    border: 1px solid var(--frame);
    border-radius: var(--radius);
    color: var(--text-muted);
    font-family: var(--font-display);
    font-size: 0.8rem;
    font-weight: 700;
    letter-spacing: 0.03em;
    cursor: pointer;
  }

  .roles button:hover:not(:disabled) {
    color: var(--title);
  }

  .roles button.active {
    color: var(--title);
    border-color: var(--accent);
    box-shadow: inset 0 -2px 0 var(--accent);
  }

  .roles button:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }

  .card label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: 0.85rem;
  }

  .card label span {
    color: var(--text-muted);
  }

  .card input[type="number"] {
    width: 6em;
  }

  .leave {
    margin: auto 0;
    text-align: center;
  }

  .actions {
    display: flex;
    gap: var(--space-3);
    justify-content: flex-end;
    margin-top: var(--space-4);
  }
</style>
