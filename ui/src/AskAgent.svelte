<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

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
  import Kbd from "./controls/Kbd.svelte";
  import Mark from "./controls/Mark.svelte";
  import StatusDot from "./controls/StatusDot.svelte";
  import { nearLines, type Area } from "./lib/popover";
  import { refLines, type CodeRef } from "./lib/reviewComments";

  let {
    x,
    lines,
    area,
    code,
    status,
    onsend,
    onterminal,
    onclose,
  }: {
    // Where the lines are (viewport px): it goes below them, else above, else docked.
    x: number;
    lines: { top: number; bottom: number };
    // The pane the lines are in, down to the review's dock.
    area: Area;
    code: CodeRef;
    // The agent's status (lib/agent.svelte.ts).
    status: string | undefined;
    onsend: (prompt: string) => Promise<void>;
    // Shows the agent's Terminal (to answer it while it waits).
    onterminal?: () => void;
    onclose: () => void;
  } = $props();

  let box: HTMLElement | undefined = $state();
  let input: HTMLTextAreaElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });
  let prompt = $state("");
  let sending = $state(false);
  const waiting = $derived(status === "waitingForPermission");
  // As wide as the popover token allows, but within the pane.
  const width = $derived(Math.min(440, area.right - area.left - 16));

  function position() {
    if (!box) return;
    const r = box.getBoundingClientRect();
    pos = nearLines(r.width, r.height, x, lines, area);
  }

  onMount(() => {
    position();
    input?.focus();
  });
  // The waiting note comes and goes: the box's height changes.
  $effect(() => {
    void waiting;
    queueMicrotask(position);
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

  const fileName = $derived(code.path.slice(code.path.lastIndexOf("/") + 1));
  const where = $derived(`${code.side}${code.rev ? ` · ${code.rev.slice(0, 8)}` : code.unsaved ? " · unsaved" : " · working tree"}`);
</script>

<svelte:window onmousedown={onWindowDown} />

<div class="ask" bind:this={box} onkeydown={onKey} style:left="{pos.left}px" style:top="{pos.top}px" style:width="{width}px"
  role="dialog" tabindex="-1" aria-label="Ask the agent about {code.path}" data-ask>
  <form onsubmit={send}>
    <div class="head">
      <Mark who="agent" />
      <strong>Ask the agent</strong>
      <span class="where" title={code.path}><code>{fileName}:{refLines(code)}</code> · {where}</span>
      <IconButton icon="x" label="Close (Esc)" size={22} type="button" onclick={onclose} />
    </div>
    <div class="body">
      <textarea bind:this={input} bind:value={prompt} rows="3" placeholder="What do you want to know or change?"></textarea>
      {#if waiting}
        <div class="waiting" role="alert" data-ask-blocked>
          <StatusDot status="waitingForPermission" size={12} />
          <span class="grow">Waiting for your permission first.</span>
          {#if onterminal}<Button type="button" size="sm" variant="agent-soft" icon="terminal" onclick={onterminal}>Answer in Terminal</Button>{/if}
        </div>
      {:else if status === "working"}
        <p class="hint">The agent is working; it reads this when its turn ends.</p>
      {/if}
    </div>
    <div class="foot">
      <span class="keys"><Kbd key="Enter" /> sends</span>
      <span class="keys"><Kbd key="Shift Enter" /> new line</span>
      <span class="grow"></span>
      <Button type="submit" variant="agent" size="sm" mark="agent" disabled={!prompt.trim() || waiting} loading={sending} data-ask-send>Send to agent</Button>
    </div>
  </form>
</div>

<style>
  .ask {
    position: fixed; z-index: 40; box-sizing: border-box;
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop);
    font-size: var(--fs-12); line-height: var(--lh-12);
  }
  .ask:focus { outline: none; }
  form { display: flex; flex-direction: column; }
  .head {
    display: flex; align-items: center; gap: var(--sp-4); min-width: 0; height: 34px; padding: 0 var(--sp-2) 0 10px;
    border-bottom: 1px solid var(--border);
  }
  .head strong { flex: none; font-weight: var(--fw-semibold); }
  .where { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-muted); }
  .where code { font-family: var(--font-mono); font-size: var(--fs-code-inline); }
  .body { display: flex; flex-direction: column; gap: var(--sp-4); padding: var(--sp-4) 10px; }
  textarea { resize: vertical; min-height: 64px; font-size: var(--fs-13); line-height: var(--lh-13); }
  .waiting {
    display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-3) var(--sp-3) var(--sp-3) 10px;
    border-radius: var(--r-md); background: var(--agent-bg); box-shadow: inset 0 0 0 1px var(--agent-line); color: var(--agent-strong);
  }
  .waiting :global(.btn) { background: var(--surface-raised); }
  .grow { flex: 1; min-width: 0; }
  .hint { margin: 0; color: var(--text-muted); }
  .foot { display: flex; align-items: center; gap: var(--sp-4); padding: 0 10px 10px; color: var(--text-muted); }
  .keys { display: inline-flex; align-items: center; gap: var(--sp-2); white-space: nowrap; }
</style>
