<script lang="ts">
  import { answerPermission, cancel, sendPrompt, sessions, type Session } from "./lib/agent.svelte";

  let { agentKey }: { agentKey: string } = $props();

  const session: Session | undefined = $derived(sessions[agentKey]);
  const busy = $derived(session?.status === "working" || session?.status === "waitingForPermission");
  let draft = $state("");
  let log: HTMLElement | undefined = $state();

  // Keep the newest output in view while the agent streams.
  $effect(() => {
    void session?.entries.length;
    void session?.entries.at(-1);
    log?.scrollTo({ top: log.scrollHeight });
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const text = draft.trim();
    if (!text || busy) return;
    draft = "";
    await sendPrompt(agentKey, text);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) (e.currentTarget as HTMLTextAreaElement).form?.requestSubmit();
  }
</script>

<section class="agent">
  <div class="log" bind:this={log}>
    {#each session?.entries ?? [] as entry, i (i)}
      {#if entry.kind === "tool"}
        <div class="tool {entry.status}">
          <span class="badge">{entry.toolKind}</span>
          <span class="title">{entry.title}</span>
          <span class="state">{entry.status}</span>
        </div>
      {:else}
        <div class="msg {entry.kind}">{entry.text}</div>
      {/if}
    {:else}
      <p class="empty">No conversation yet.</p>
    {/each}
  </div>

  {#if session?.plan.length}
    <ol class="plan">
      {#each session.plan as step}
        <li class={step.status}>{step.content}</li>
      {/each}
    </ol>
  {/if}

  {#if session?.permission}
    {@const p = session.permission}
    <div class="permission" role="alertdialog" aria-label="Permission request">
      <strong>Agent wants to:</strong> {p.title ?? "run a tool"}
      <div class="options">
        {#each p.options as option}
          <button class={option.kind} onclick={() => answerPermission(agentKey, p.requestId, option.id)}>
            {option.name}
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <form onsubmit={submit}>
    <textarea
      bind:value={draft}
      onkeydown={onKeydown}
      rows="3"
      placeholder="Message the agent (Ctrl+Enter to send)"
      disabled={!session || session.status === "stopped" || session.status === "starting"}
    ></textarea>
    <div class="actions">
      {#if busy}
        <button type="button" onclick={() => cancel(agentKey)}>Stop turn</button>
      {/if}
      <button type="submit" disabled={busy || !draft.trim()}>Send</button>
    </div>
  </form>
</section>

<style>
  .agent { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .log { flex: 1; overflow-y: auto; padding: 12px; display: flex; flex-direction: column; gap: 8px; }
  .msg { white-space: pre-wrap; line-height: 1.45; padding: 8px 10px; border-radius: 6px; }
  .msg.user { background: var(--user-bg); align-self: flex-end; max-width: 80%; }
  .msg.agent { background: var(--panel); }
  .msg.thought { color: var(--muted); font-style: italic; font-size: 0.92em; }
  .tool { display: flex; gap: 8px; align-items: baseline; font-family: var(--mono); font-size: 0.85em; padding: 4px 8px; border-left: 3px solid var(--border); }
  .tool.completed { border-color: var(--ok); }
  .tool.failed { border-color: var(--bad); }
  .tool .badge { color: var(--muted); min-width: 5em; }
  .tool .title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tool .state { color: var(--muted); }
  .plan { margin: 0; padding: 8px 12px 8px 32px; border-top: 1px solid var(--border); font-size: 0.9em; }
  .plan .completed { text-decoration: line-through; color: var(--muted); }
  .plan .in_progress { font-weight: 600; }
  .permission { margin: 8px 12px; padding: 10px 12px; border: 1px solid var(--warn); border-radius: 6px; background: var(--warn-bg); }
  .options { display: flex; gap: 8px; margin-top: 8px; flex-wrap: wrap; }
  .options .allow_once, .options .allow_always { border-color: var(--ok); }
  .options .reject_once, .options .reject_always { border-color: var(--bad); }
  form { border-top: 1px solid var(--border); padding: 8px 12px; display: flex; flex-direction: column; gap: 6px; }
  textarea { width: 100%; box-sizing: border-box; resize: vertical; font: inherit; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; }
  .empty { color: var(--muted); }
</style>
