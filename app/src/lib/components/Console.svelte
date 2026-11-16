<script lang="ts">
  // Output of one server process (last lines; the full log stays in the
  // instance's logs folder) and the server console input.
  import { tick } from "svelte";
  import { api, asCommandError, type CommandError } from "../api";
  import { t } from "../i18n/i18n.svelte";
  import { launcher } from "../launcher.svelte";
  import { processLabel } from "../labels";
  import ErrorBox from "./ErrorBox.svelte";

  let {
    instance,
    process,
    console: takesInput,
    onclose,
  }: {
    instance: string;
    process: string;
    /** This process reads the console commands. */
    console: boolean;
    onclose: () => void;
  } = $props();

  const session = $derived(launcher.session(instance));
  const lines = $derived(session?.logs[process] ?? []);
  const running = $derived(launcher.isRunning(instance));

  let output = $state<HTMLElement | null>(null);
  let follow = $state(true);
  let command = $state("");
  let error = $state<CommandError | null>(null);

  $effect(() => {
    void lines.length;
    if (!follow) return;
    void tick().then(() => output?.scrollTo({ top: output.scrollHeight }));
  });

  function scrolled() {
    if (!output) return;
    follow = output.scrollTop + output.clientHeight >= output.scrollHeight - 8;
  }

  async function send(event: SubmitEvent) {
    event.preventDefault();
    const line = command.trim();
    if (!line) return;
    error = null;
    try {
      await api.console(instance, line);
      command = "";
    } catch (raw) {
      error = asCommandError(raw);
    }
  }
</script>

<section class="console" aria-label={t("terminals.of", { process: processLabel(process) })}>
  <header>
    <span class="display">{processLabel(process)}</span>
    <button class="btn" onclick={onclose}>{t("common.close")}</button>
  </header>
  <div class="output" bind:this={output} onscroll={scrolled}>
    {#each lines as line (line.id)}
      <div class:stderr={line.stderr}>{line.text}</div>
    {:else}
      <p class="muted">{t("terminals.empty")}</p>
    {/each}
  </div>
  {#if running && takesInput}
    <form onsubmit={send}>
      <input type="text" spellcheck="false" placeholder={t("terminals.placeholder")} bind:value={command} />
      <button class="btn" type="submit">{t("terminals.send")}</button>
    </form>
  {/if}
  {#if error}
    <ErrorBox {error} />
  {/if}
</section>

<style>
  .console {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-6);
    border-top: 2px solid var(--frame);
    background: var(--bg-elevated);
    min-height: 0;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  header span {
    color: var(--title);
  }

  .output {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-2);
    border: 1px solid var(--border);
    background: var(--shade-deep);
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 0.8rem;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .stderr {
    color: var(--danger);
  }

  form {
    display: flex;
    gap: var(--space-2);
  }

  form input {
    flex: 1;
    font-family: Consolas, "Cascadia Mono", monospace;
  }
</style>
