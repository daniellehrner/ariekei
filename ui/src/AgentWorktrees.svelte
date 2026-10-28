<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  The worktrees the item's agent made itself (a `git worktree add` of its own, Claude Code's
  worktrees): where each is, its branch, and what removing it would lose. Each can be removed
  (after a confirmation that names what is lost) or made a task of its own. Cleaning up the item
  removes them too, unless that would lose work. Nothing shows while there are none.
-->
<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { onMount, untrack } from "svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import InlineConfirm from "./controls/InlineConfirm.svelte";
  import { api, type AgentWorktree } from "./lib/api";

  let { id, kind, status, onopen }: {
    id: number;
    /** "PR", "issue" or "task": what is cleaned up with them. */
    kind: string;
    /** The agent's status: what is in its worktrees may change when it does. */
    status: string | null;
    /** Opens the task made of one. */
    onopen: (task: number) => void;
  } = $props();

  let worktrees = $state<AgentWorktree[]>([]);
  let error = $state<string | null>(null);
  // The one whose removal waits for a yes, and the one being worked on.
  let confirming = $state<number | null>(null);
  let busy = $state<number | null>(null);
  let copied = $state<number | null>(null);

  async function load(current: number) {
    try {
      const found = await api.agentWorktrees(current);
      if (current === id) worktrees = found;
    } catch (e) {
      if (current === id) error = String(e);
    }
  }

  $effect(() => {
    const current = id;
    void status;
    untrack(() => load(current));
  });
  $effect(() => {
    void id;
    untrack(() => {
      worktrees = [];
      error = null;
      confirming = null;
    });
  });

  onMount(() => {
    const unlisten = listen<number>("agent-worktrees-changed", (e) => {
      if (e.payload === id) load(id);
    });
    return () => unlisten.then((f) => f());
  });

  const folder = (path: string) => path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? path;

  async function act(w: AgentWorktree, f: () => Promise<unknown>) {
    busy = w.id;
    error = null;
    try {
      await f();
    } catch (e) {
      error = String(e);
    } finally {
      busy = null;
      load(id);
    }
  }

  const remove = (w: AgentWorktree) => {
    confirming = null;
    // Forced only where the user saw what is lost; otherwise git refuses if that changed.
    act(w, () => api.removeAgentWorktree(id, w.id, w.losses.length > 0));
  };
  const makeTask = (w: AgentWorktree) => act(w, async () => onopen(await api.agentWorktreeTask(id, w.id)));

  async function copy(w: AgentWorktree) {
    try {
      await writeText(w.path);
      copied = w.id;
      setTimeout(() => (copied = null), 1500);
    } catch (e) {
      error = `Could not copy the path: ${e}`;
    }
  }
</script>

{#if worktrees.length}
  <section class="worktrees" aria-label="Worktrees the agent made" data-agent-worktrees>
    <div class="head">
      <Icon name="folder" size={14} />
      <strong>The agent made {worktrees.length === 1 ? "a worktree" : `${worktrees.length} worktrees`}</strong>
      <span class="muted">· cleaned up with the {kind} if that loses nothing</span>
    </div>
    <ul>
      {#each worktrees as w (w.id)}
        <li data-agent-worktree={folder(w.path)}>
          <button class="path" title="{w.path} — click to copy" onclick={() => copy(w)}><code>{copied === w.id ? "Copied" : folder(w.path)}</code></button>
          <span class="branch" title={w.branch ? (w.branchCreated ? "A branch the agent made: it goes with the worktree" : "A branch that was there before: it stays") : ""}>
            <Icon name="branch" size={12} />{#if w.branch}<code>{w.branch}</code>{:else}detached HEAD{/if}
          </span>
          <span class="state" class:warn={w.losses.length > 0} title={w.losses.join("\n")} data-losses={w.losses.length}>
            {w.losses.length ? w.losses.join(" · ") : "nothing unsaved"}
          </span>
          <span class="actions">
            {#if confirming === w.id}
              <InlineConfirm text={w.losses.length ? `Remove ${folder(w.path)} and lose that?` : `Remove ${folder(w.path)}${w.branchCreated ? ` and its branch ${w.branch}` : ""}?`}
                confirm="Remove" tone={w.losses.length ? "danger" : "neutral"} onconfirm={() => remove(w)} oncancel={() => (confirming = null)} data-confirm-remove />
            {:else}
              <Button size="sm" variant="quiet" icon="task" disabled={!w.branch || busy !== null} loading={busy === w.id}
                title={w.branch ? `Make ${folder(w.path)} a task of its own, on ${w.branch}` : "Its HEAD is detached: check out a branch there first"}
                onclick={() => makeTask(w)} data-make-task>Make it a task</Button>
              <Button size="sm" variant="quiet" icon="trash" disabled={busy !== null} onclick={() => (confirming = w.id)}
                title="Remove the worktree{w.branchCreated ? ' and its branch' : ''}" data-remove-worktree>Remove</Button>
            {/if}
          </span>
        </li>
      {/each}
    </ul>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </section>
{/if}

<style>
  .worktrees {
    flex: none; margin: var(--sp-4) var(--sp-7) 0; padding: var(--sp-3) var(--sp-3) var(--sp-3) var(--sp-6);
    border-radius: var(--r-lg); background: var(--agent-bg); box-shadow: inset 0 0 0 1px var(--agent-line);
    font-size: var(--fs-12); line-height: var(--lh-12);
  }
  .head { display: flex; align-items: center; gap: var(--sp-3); min-height: 24px; color: var(--agent-strong); white-space: nowrap; overflow: hidden; }
  .head strong { font-weight: var(--fw-semibold); }
  .muted { color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; }
  ul { margin: 0; padding: 0; list-style: none; }
  li { display: flex; align-items: center; gap: var(--sp-5); min-height: 28px; padding-left: 22px; min-width: 0; }
  code { font-size: var(--fs-code-inline); }
  .path { flex: 0 1 auto; min-width: 4em; padding: 0; border: 0; background: none; color: var(--text); font: inherit; cursor: copy; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .branch { flex: 0 2 auto; min-width: 3em; display: inline-flex; align-items: center; gap: var(--sp-2); color: var(--text-muted); overflow: hidden; white-space: nowrap; }
  .branch code { overflow: hidden; text-overflow: ellipsis; }
  .state { flex: 1 5 auto; min-width: 0; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .state.warn { color: var(--warn); }
  .actions { flex: none; margin-left: auto; display: flex; align-items: center; gap: var(--sp-2); }
  .error { margin: var(--sp-2) 0 0; padding-left: 22px; color: var(--bad); }
</style>
