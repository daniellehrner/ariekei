<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  A draft comment in the diff, below its last line (lib/draftCards.ts): whose it is, the
  thread it answers in if it is a reply (who started it, with what), its text, and what can be
  done with it: Edit it in place, Ask the agent about it (it changes the draft
  with its tools), Delete it. In the textareas Ctrl+Enter saves or sends, Esc cancels.
-->
<script lang="ts">
  import Button from "./controls/Button.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Mark from "./controls/Mark.svelte";
  import type { CardActions } from "./lib/draftCards";
  import { type ReviewDraft } from "./lib/reviewComments";

  let { draft, elsewhere, actions }: { draft: ReviewDraft; elsewhere: boolean; actions: CardActions } = $props();

  let mode = $state<"show" | "edit" | "ask">("show");
  let text = $state("");
  let prompt = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  const agent = $derived(actions.agent);
  const waits = $derived(agent.status === "waitingForPermission");
  const why = $derived(agent.away ?? (waits ? "The agent is waiting for your permission: answer it first" : null));

  function open(next: "edit" | "ask") {
    error = null;
    if (next === "edit") text = draft.text;
    mode = next;
  }

  async function run(action: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await action();
      mode = "show";
      prompt = "";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const save = () => text.trim() && run(() => actions.edit(draft, text));
  const send = () => prompt.trim() && !why && run(() => actions.ask(draft, prompt));

  function onKey(e: KeyboardEvent, submit: () => unknown) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      mode = "show";
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      submit();
    }
  }

  // Focused when it opens.
  const focus = (el: HTMLTextAreaElement) => el.focus();
</script>

<div class="card" class:agent={draft.author === "agent"} data-draft={draft.id} data-author={draft.author}>
  <div class="head">
    <Mark who={draft.author === "agent" ? "agent" : "you"} />
    <strong>{draft.author === "agent" ? "Agent" : "You"}</strong>
    {#if draft.replyTo}<span class="muted" data-reply-to={draft.replyTo.thread}>· reply in {draft.replyTo.author}'s thread</span>{/if}
    {#if draft.edited}<span class="muted" data-edited>· edited by you</span>{/if}
    {#if elsewhere && draft.rev}<span class="muted" title="Drafted on commit {draft.rev}; the lines may have moved since">· on {draft.rev.slice(0, 8)}</span>{/if}
    <span class="grow"></span>
    {#if mode === "show"}
      <Button size="sm" variant="quiet" onclick={() => open("edit")} data-draft-edit>Edit</Button>
      {#if agent.present}
        <Button size="sm" variant="quiet" mark="agent" onclick={() => open("ask")} title={why ?? "Ask the agent to change this comment"} data-draft-ask>Ask agent</Button>
      {/if}
      <IconButton icon="trash" label="Delete draft" size={22} disabled={busy} onclick={() => run(() => actions.remove(draft))} data-draft-delete />
    {/if}
  </div>
  {#if draft.replyTo}
    <div class="thread" title="{draft.replyTo.author}: {draft.replyTo.text}"><strong>{draft.replyTo.author}:</strong> {draft.replyTo.text}</div>
  {/if}
  {#if mode === "edit"}
    <textarea use:focus bind:value={text} rows={Math.min(8, Math.max(2, text.split("\n").length))} onkeydown={(e) => onKey(e, save)} data-draft-text></textarea>
    <div class="buttons">
      <span class="grow keys">Ctrl+Enter saves · Esc cancels</span>
      <Button size="sm" variant="quiet" onclick={() => (mode = "show")}>Cancel</Button>
      <Button size="sm" variant="primary" disabled={!text.trim()} loading={busy} onclick={save} data-draft-save>Save</Button>
    </div>
  {:else}
    <div class="text" data-draft-body>{draft.text}</div>
  {/if}
  {#if mode === "ask"}
    <textarea use:focus bind:value={prompt} rows="2" placeholder="What should the agent change about this comment?" onkeydown={(e) => onKey(e, send)} data-draft-prompt></textarea>
    <div class="buttons">
      <span class="grow keys">{why ?? "Ctrl+Enter sends · Esc cancels"}</span>
      <Button size="sm" variant="quiet" onclick={() => (mode = "show")}>Cancel</Button>
      <Button size="sm" variant="agent" mark="agent" disabled={!prompt.trim() || !!why} loading={busy} onclick={send} data-draft-send>Send to agent</Button>
    </div>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .card {
    max-width: 720px; border: 1px solid var(--border); border-left: 3px solid var(--you); border-radius: var(--r-md);
    background: var(--surface-raised); font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text);
  }
  .card.agent { border-left-color: var(--agent); }
  .head { display: flex; align-items: center; gap: var(--sp-3); min-height: 28px; padding: 0 var(--sp-2) 0 var(--sp-4); }
  .head strong { font-weight: var(--fw-semibold); }
  .grow { flex: 1; min-width: 0; }
  .muted { color: var(--text-muted); white-space: nowrap; }
  .thread {
    margin: 0 var(--sp-4) var(--sp-3); padding-left: var(--sp-3); border-left: 2px solid var(--border); color: var(--text-muted);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .thread strong { font-weight: var(--fw-semibold); }
  .text { padding: 0 var(--sp-4) var(--sp-3); white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--fs-13); line-height: var(--lh-13); }
  textarea {
    display: block; box-sizing: border-box; width: calc(100% - 2 * var(--sp-4)); margin: 0 var(--sp-4) var(--sp-3); resize: vertical;
    font-family: var(--font-ui); font-size: var(--fs-13); line-height: var(--lh-13);
  }
  .buttons { display: flex; align-items: center; gap: var(--sp-3); padding: 0 var(--sp-4) var(--sp-3); }
  .keys { color: var(--text-faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .error { margin: 0; padding: 0 var(--sp-4) var(--sp-3); color: var(--bad); }
</style>
