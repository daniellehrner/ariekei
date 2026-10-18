<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  Asked before quitting (closing the last window, or Quit from the app menu) when that stops
  agents' work: agents in a turn, and what they run in the background (dev servers, watchers),
  which resuming a conversation does not start again. Names each agent, where it works and
  what it runs (quit.rs, `confirm-quit`).
-->
<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import Mark from "./controls/Mark.svelte";
  import { api, type Running } from "./lib/api";
  import { modal } from "./lib/modal";
  import { shortPath } from "./lib/paths";

  let running = $state<Running[] | null>(null);
  let quitting = $state(false);

  onMount(() => {
    const unlisten = listen<Running[]>("confirm-quit", (e) => (running = e.payload));
    return () => unlisten.then((f) => f());
  });

  function quit() {
    quitting = true;
    api.quitApp();
  }

  // A path as [parent, folder].
  function split(path: string): [string, string] {
    const trimmed = path.replace(/[\\/]+$/, "");
    const at = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
    return [trimmed.slice(0, Math.max(at, 0)) || trimmed, trimmed.slice(at + 1)];
  }

  const count = $derived(running?.reduce((n, r) => n + r.processes.length, 0) ?? 0);
</script>

{#if running}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <dialog use:modal aria-labelledby="quit-title" aria-modal="true" data-confirm-quit
    oncancel={(e) => { e.preventDefault(); running = null; }}
    onclick={(e) => e.target === e.currentTarget && (running = null)}>
    <div class="box">
      <strong id="quit-title">Quit Ariekei?</strong>
      <p>
        Quitting ends {running.length === 1 ? "this agent" : `these ${running.length} agents`}{count ? ` and the ${count === 1 ? "process" : `${count} processes`} they run` : ""}.
        Conversations can be resumed later; background processes are not started again.
      </p>
      <ul>
        {#each running as r (r.item)}
          <li data-running={r.item}>
            <div class="who"><Mark who="agent" /><strong>{r.name}</strong><span class="repo">{r.repo}</span></div>
            {#if r.worktree}
              {@const [parent, folder] = split(r.worktree)}
              <!-- The folder names the worktree; its parent gives way when space runs out. -->
              <div class="where" title={r.worktree}><Icon name="folder" size={12} /><code class="folder">{folder}</code><span class="parent">in {shortPath(parent)}</span></div>
            {/if}
            {#if r.busy}<div class="busy">In the middle of a turn</div>{/if}
            {#each r.processes as p (p.pid)}
              <div class="proc" title={p.command}><code>{p.command}</code><span class="pid">PID {p.pid}</span></div>
            {/each}
          </li>
        {/each}
      </ul>
      <div class="actions">
        <!-- svelte-ignore a11y_autofocus -->
        <Button variant="quiet" onclick={() => (running = null)} autofocus data-cancel-quit>Cancel</Button>
        <Button variant="danger" onclick={quit} loading={quitting} data-quit-anyway>Quit anyway</Button>
      </div>
    </div>
  </dialog>
{/if}

<style>
  /* The dialog covers the window and is the scrim; the box is centred. */
  dialog {
    position: fixed; inset: 0; width: 100%; height: 100%; max-width: none; max-height: none; margin: 0; padding: 0; border: 0;
    display: grid; place-items: center; background: var(--scrim); color: var(--text);
  }
  dialog::backdrop { background: transparent; }
  .box {
    width: 520px; max-width: calc(100% - 32px); max-height: calc(100% - 32px); overflow-y: auto; padding: var(--sp-7) 18px;
    box-sizing: border-box; border-radius: var(--r-xl); background: var(--surface-raised); box-shadow: var(--shadow-pop);
    display: flex; flex-direction: column; gap: var(--sp-5);
  }
  #quit-title { font-size: var(--fs-13); line-height: var(--lh-13); font-weight: var(--fw-semibold); }
  p { margin: 0; font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted); }
  ul { margin: 0; padding: 0; list-style: none; display: flex; flex-direction: column; gap: var(--sp-4); }
  li {
    display: flex; flex-direction: column; gap: var(--sp-2); padding: var(--sp-4) var(--sp-5);
    border-radius: var(--r-md); background: var(--surface-hover); font-size: var(--fs-12); line-height: var(--lh-12);
  }
  .who { display: flex; align-items: center; gap: var(--sp-3); min-width: 0; }
  .who strong { font-weight: var(--fw-semibold); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .repo { flex: none; margin-left: auto; color: var(--text-muted); }
  .where, .proc { display: flex; align-items: center; gap: var(--sp-3); min-width: 0; color: var(--text-muted); }
  .busy { color: var(--agent-strong); }
  code { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-code-inline); }
  .proc code, .where .folder { color: var(--text); }
  .where .folder { flex: none; max-width: 60%; }
  .parent { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pid { flex: none; margin-left: auto; font-family: var(--font-mono); font-size: var(--fs-11); }
  .actions { display: flex; justify-content: flex-end; gap: var(--sp-3); }
</style>
