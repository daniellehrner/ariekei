<!--
  The prompt box for asking the agent about code in the review (see CodeMenu): the lines it is
  about, a prompt, Send. Enter sends, Shift+Enter starts a new line; Esc, or a click elsewhere
  while it is empty, closes it. While the agent waits for permission it can't be told
  anything: the paste would answer the prompt.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Button from "./controls/Button.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import { place } from "./lib/popover";
  import { refLines, versionOf, type CodeRef } from "./lib/reviewComments";

  let {
    x,
    y,
    code,
    status,
    onsend,
    onclose,
  }: {
    x: number;
    y: number;
    code: CodeRef;
    // The agent's status (lib/agent.svelte.ts).
    status: string | undefined;
    onsend: (prompt: string) => Promise<void>;
    onclose: () => void;
  } = $props();

  let box: HTMLElement | undefined = $state();
  let input: HTMLTextAreaElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });
  let prompt = $state("");
  let sending = $state(false);
  const waiting = $derived(status === "waitingForPermission");

  onMount(() => {
    if (box) pos = place(box, x, y);
    input?.focus();
  });

  async function send(e?: SubmitEvent) {
    e?.preventDefault();
    if (!prompt.trim() || waiting || sending) return;
    sending = true;
    try {
      await onsend(prompt);
    } finally {
      sending = false;
    }
  }

  // A click elsewhere closes it, unless a prompt is typed (that would be lost).
  function onWindowDown(e: MouseEvent) {
    if (box && !box.contains(e.target as Node) && !prompt.trim()) onclose();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    } else if (e.key === "Enter" && !e.shiftKey && e.target === input) {
      e.preventDefault();
      send();
    }
  }
</script>

<svelte:window onmousedown={onWindowDown} />

<div class="ask" bind:this={box} onkeydown={onKey} style:left="{pos.left}px" style:top="{pos.top}px"
  role="dialog" tabindex="-1" aria-label="Ask the agent about {code.path}" data-ask>
  <form onsubmit={send}>
    <div class="head">
      <span class="dot"><svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><circle cx="5" cy="5" r="4.5" /></svg></span>
      <span class="where" title={code.path}><code>{code.path.slice(code.path.lastIndexOf("/") + 1)}:{refLines(code)}</code> · {versionOf(code)}</span>
      <IconButton icon="x" label="Close" size={22} type="button" onclick={onclose} />
    </div>
    <textarea bind:this={input} bind:value={prompt} rows="3" placeholder="What do you want to know or change?"></textarea>
    {#if waiting}
      <p class="blocked" role="alert" data-ask-blocked>The agent is waiting for your permission — answer it in the Terminal first.</p>
    {:else if status === "working"}
      <p class="hint">The agent is working; it reads this when its turn ends.</p>
    {/if}
    <div class="foot">
      <span class="hint">Enter sends · Shift+Enter new line</span>
      <Button type="submit" variant="agent" size="sm" icon="send-to-agent" disabled={!prompt.trim() || waiting} loading={sending} data-ask-send>Send to agent</Button>
    </div>
  </form>
</div>

<style>
  .ask {
    position: fixed; z-index: 40; width: min(420px, calc(100vw - 16px)); box-sizing: border-box; padding: var(--sp-4);
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop);
    font-size: var(--fs-12); line-height: var(--lh-12);
  }
  .ask:focus { outline: none; }
  form { display: flex; flex-direction: column; gap: var(--sp-3); }
  .head { display: flex; align-items: center; gap: var(--sp-3); min-width: 0; }
  .dot { display: flex; flex: none; }
  .dot svg { fill: var(--agent); }
  .where { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-muted); }
  .where code { font-family: var(--font-mono); color: var(--text); }
  textarea {
    resize: vertical; min-height: 60px; padding: var(--sp-3) var(--sp-4); box-sizing: border-box;
    border: 1px solid var(--border-strong); border-radius: var(--r-md); background: var(--surface-0); color: var(--text);
    font: inherit; font-size: var(--fs-13); line-height: var(--lh-13);
  }
  textarea:focus { outline: none; border-color: var(--focus); box-shadow: 0 0 0 3px var(--focus-soft); }
  .blocked { margin: 0; padding: var(--sp-3) var(--sp-4); border-radius: var(--r-md); background: var(--agent-bg); color: var(--agent-strong); }
  .foot { display: flex; align-items: center; gap: var(--sp-4); }
  .hint { flex: 1; margin: 0; color: var(--text-muted); }
</style>
