<script lang="ts">
  // The realm strip of Home (ADR 0020): which realm is shown (a game version
  // can have several), and what can be done to a whole realm: name it, copy
  // it to try other modules, write it to a file for another computer,
  // install one from such a file, or remove it.
  import {
    api,
    asCommandError,
    pickRealmFile,
    pickRealmTarget,
    type CommandError,
    type EmulatorOverview,
    type ExportSummary,
    type InstanceView,
  } from "../api";
  import { localized, t } from "../i18n/i18n.svelte";
  import { chooseRealm, launcher, realmLabel, refresh, runOperation } from "../launcher.svelte";
  import ErrorBox from "./ErrorBox.svelte";
  import Modal from "./Modal.svelte";

  let { realm, instance }: { realm: EmulatorOverview; instance: InstanceView | null } = $props();

  /** Longest realm name (the backend checks it too). */
  const NAME_LIMIT = 48;

  const realms = $derived(launcher.realmsOf(realm.id));
  const busy = $derived(launcher.progress !== null);
  const running = $derived(instance ? launcher.isRunning(instance.record.id) : false);
  const ready = $derived(!!instance && instance.installed && !instance.unfinished);

  let error = $state<CommandError | null>(null);
  let note = $state<string | null>(null);

  type Dialog =
    | { kind: "uninstall"; keepBackup: boolean }
    | { kind: "rename"; name: string }
    | { kind: "duplicate"; name: string }
    | { kind: "import"; path: string; summary: ExportSummary; name: string };
  let dialog = $state<Dialog | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);
  // The name field takes the focus when a dialog opens (keyboard players).
  $effect(() => {
    if (dialog && nameInput) {
      nameInput.focus();
      nameInput.select();
    }
  });

  /** A new realm id from `base` (lowercase letters, digits, dashes), unique. */
  function newId(base: string): string {
    const slug =
      base
        .normalize("NFD")
        .replace(/[̀-ͯ]/g, "")
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 40) || realm.id;
    const taken = new Set(launcher.overview?.instances.map((i) => i.record.id) ?? []);
    let id = slug;
    for (let n = 2; taken.has(id); n++) id = `${slug}-${n}`;
    return id;
  }

  async function act(run: () => Promise<unknown>) {
    error = null;
    note = null;
    try {
      await run();
    } catch (raw) {
      error = asCommandError(raw);
    }
  }

  const clean = (name: string) => name.trim() || null;

  function confirm() {
    const current = dialog;
    if (!current) return;
    dialog = null;
    if (current.kind === "uninstall" && instance) {
      const id = instance.record.id;
      const keepBackup = current.keepBackup;
      void act(() => runOperation(id, () => api.uninstall(id, keepBackup)));
    } else if (current.kind === "rename" && instance) {
      const id = instance.record.id;
      void act(async () => {
        await api.rename(id, clean(current.name));
        await refresh();
      });
    } else if (current.kind === "duplicate" && instance) {
      const source = instance.record.id;
      const name = clean(current.name);
      const id = newId(name ?? `${source}-copy`);
      void act(async () => {
        await runOperation(id, () => api.duplicate(source, id, name), "realms.duplicating");
        chooseRealm(realm.id, id);
        note = t("realms.duplicated", { name: name ?? id });
      });
    } else if (current.kind === "import") {
      const name = clean(current.name);
      const id = newId(name ?? realm.id);
      const path = current.path;
      void act(async () => {
        await runOperation(id, () => api.importRealm(path, id, name), "realms.importing");
        chooseRealm(realm.id, id);
        note = t("realms.imported", { name: name ?? id });
      });
    }
  }

  function exportRealm() {
    if (!instance) return;
    const id = instance.record.id;
    void act(async () => {
      const path = await pickRealmTarget(t("realms.exportTitle"), t("realms.fileType"), newId(realmLabel(instance)));
      if (!path) return;
      const summary = await runOperation(id, () => api.exportRealm(id, path), "realms.exporting");
      note = t("realms.exported", { size: (summary.dumpSize / 1e6).toFixed(1) });
    });
  }

  function importRealm() {
    void act(async () => {
      const path = await pickRealmFile(t("realms.importTitle"), t("realms.fileType"));
      if (!path) return;
      const summary = await api.readExport(path);
      if (summary.realm.emulator !== realm.id) {
        error = { code: "realm.otherGame", params: {}, detail: summary.realm.emulator };
        return;
      }
      dialog = { kind: "import", path, summary, name: summary.realm.name ?? "" };
    });
  }

  function moduleNames(ids: string[]): string {
    if (ids.length === 0) return t("realms.noModules");
    return ids
      .map((id) => {
        const module = realm.modules.find((m) => m.id === id);
        return module ? localized(module.name) : id;
      })
      .join(", ");
  }

  const date = (secs: number) => new Date(secs * 1000).toLocaleDateString();
</script>

<section class="realm-bar" aria-labelledby="realm-bar-label">
  <div class="who" title={t("realms.explain")}>
    <span class="label" id="realm-bar-label">{t("realms.label")}</span>
    {#if realms.length > 1}
      <div class="segmented" role="group" aria-label={t("realms.choose")}>
        {#each realms as item (item.record.id)}
          <button
            class:active={item.record.id === instance?.record.id}
            aria-pressed={item.record.id === instance?.record.id}
            disabled={busy}
            onclick={() => chooseRealm(realm.id, item.record.id)}
          >
            {realmLabel(item)}
          </button>
        {/each}
      </div>
    {:else if instance}
      <span class="name display">{realmLabel(instance)}</span>
    {:else}
      <span class="muted">{t("realms.none")}</span>
    {/if}
  </div>
  <div class="realm-actions">
    {#if instance}
      <button
        class="btn small"
        disabled={busy || running}
        title={running ? t("realms.stopFirst") : undefined}
        onclick={() => (dialog = { kind: "rename", name: instance.record.name ?? "" })}
      >
        {t("realms.rename")}
      </button>
      <button
        class="btn small"
        disabled={busy || running || !ready}
        title={running ? t("realms.stopFirst") : t("realms.duplicateHint")}
        onclick={() => (dialog = { kind: "duplicate", name: `${realmLabel(instance)} (2)` })}
      >
        {t("realms.duplicate")}
      </button>
      <button
        class="btn small"
        disabled={busy || running || !ready}
        title={running ? t("realms.stopFirst") : t("realms.exportHint")}
        onclick={exportRealm}
      >
        {t("realms.export")}
      </button>
    {/if}
    <button class="btn small" disabled={busy} title={t("realms.importHint")} onclick={importRealm}>
      {t("realms.import")}
    </button>
    {#if instance}
      <button
        class="btn small danger"
        disabled={busy || running}
        title={running ? t("uninstall.stopFirst") : t("uninstall.open")}
        onclick={() => (dialog = { kind: "uninstall", keepBackup: true })}
      >
        {t("realms.uninstall")}
      </button>
    {/if}
  </div>
</section>

{#if note}
  <p class="note muted" role="status">{note}</p>
{/if}
{#if error}
  <ErrorBox {error} />
{/if}

{#if dialog}
  <Modal
    title={dialog.kind === "uninstall"
      ? t("uninstall.title")
      : dialog.kind === "rename"
        ? t("realms.renameTitle")
        : dialog.kind === "duplicate"
          ? t("realms.duplicateTitle")
          : t("realms.importConfirmTitle")}
    onclose={() => (dialog = null)}
  >
    <form
      id="realm-dialog"
      onsubmit={(e) => {
        e.preventDefault();
        confirm();
      }}
    >
      {#if dialog.kind === "uninstall"}
        <p>{t("uninstall.body")}</p>
        <label class="check">
          <input type="checkbox" bind:checked={dialog.keepBackup} />
          <span>{t("uninstall.keepBackup")}</span>
        </label>
        {#if !dialog.keepBackup}
          <p class="warn">{t("uninstall.noBackupWarning")}</p>
        {/if}
      {:else if dialog.kind === "duplicate"}
        <p>{t("realms.duplicateBody")}</p>
      {:else if dialog.kind === "import"}
        <p>{t("realms.importBody")}</p>
        <dl class="summary">
          <div>
            <dt>{t("home.stat.modules")}</dt>
            <dd>{moduleNames(dialog.summary.realm.modules)}</dd>
          </div>
          <div>
            <dt>{t("realms.exportedOn")}</dt>
            <dd>{date(dialog.summary.realm.exported)}</dd>
          </div>
          <div>
            <dt>{t("realms.playerData")}</dt>
            <dd>{(dialog.summary.dumpSize / 1e6).toFixed(1)} MB</dd>
          </div>
        </dl>
      {/if}
      {#if dialog.kind !== "uninstall"}
        <label class="name">
          <span>{t("realms.name")}</span>
          <input
            type="text"
            maxlength={NAME_LIMIT}
            bind:this={nameInput}
            bind:value={dialog.name}
            placeholder={t("realms.namePlaceholder")}
          />
        </label>
      {/if}
    </form>
    {#snippet actions()}
      <button type="button" class="btn" onclick={() => (dialog = null)}>{t("common.cancel")}</button>
      <button type="submit" form="realm-dialog" class={dialog?.kind === "uninstall" ? "btn danger" : "btn btn-play"}>
        {dialog?.kind === "uninstall"
          ? t("uninstall.confirm")
          : dialog?.kind === "rename"
            ? t("realms.renameConfirm")
            : dialog?.kind === "duplicate"
              ? t("realms.duplicateConfirm")
              : t("realms.importConfirm")}
      </button>
    {/snippet}
  </Modal>
{/if}

<style>
  .realm-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2) var(--space-4);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    background: var(--shade-soft);
  }

  .who {
    display: flex;
    /* Label and name share a baseline (the name is in the display font). */
    align-items: baseline;
    gap: var(--space-3);
    min-width: 0;
  }

  .label {
    color: var(--text-muted);
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .name {
    color: var(--title);
    font-size: 1.1rem;
    line-height: 1.2;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .realm-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .btn.small {
    padding: var(--space-1) var(--space-3);
    font-size: 0.8rem;
  }

  .btn.danger {
    border-color: var(--danger);
    color: var(--danger);
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
    cursor: pointer;
  }

  .warn {
    color: var(--danger);
    font-size: 0.88rem;
  }

  .note {
    margin: 0 0 var(--space-3);
  }

  .summary {
    display: grid;
    gap: var(--space-2);
    margin: 0 0 var(--space-3);
  }

  .summary div {
    display: grid;
    grid-template-columns: 10rem 1fr;
    gap: var(--space-2);
  }

  .summary dt {
    color: var(--text-muted);
  }

  .summary dd {
    margin: 0;
  }

  .name {
    display: grid;
    gap: var(--space-1);
    margin-bottom: var(--space-4);
  }
</style>
