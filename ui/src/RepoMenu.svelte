<script lang="ts">
  import { api, type Repo, type RepoEntry } from "./lib/api";

  // The repo name in the Inbox header: switch this window to another connected repo, open
  // one in a new window, or connect another. A repo shown in another window is focused
  // there instead.
  let {
    repo,
    onswitch,
    onconnect,
  }: {
    repo: Repo;
    onswitch: (repo: Repo) => void;
    onconnect: () => void;
  } = $props();

  let open = $state(false);
  let entries: RepoEntry[] = $state([]);
  let error: string | null = $state(null);
  let wrap: HTMLElement | undefined = $state();

  async function toggle() {
    open = !open;
    if (!open) return;
    error = null;
    try {
      entries = await api.repos();
    } catch (e) {
      error = String(e);
    }
  }

  async function run(f: () => Promise<void>) {
    try {
      await f();
      open = false;
    } catch (e) {
      error = String(e);
    }
  }

  const choose = (e: RepoEntry) =>
    run(async () => {
      if (e.here) return;
      const shown = await api.switchRepo(e.id);
      if (shown.here) onswitch(shown.repo);
    });

  const newWindow = (id: number | null) => run(() => api.openWindow(id));

  function onWindowClick(e: MouseEvent) {
    if (open && wrap && !wrap.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => open && e.key === "Escape" && (open = false)} />

<div class="wrap" bind:this={wrap}>
  <button class="repo" onclick={toggle} title={repo.path} aria-haspopup="menu" aria-expanded={open} data-repo-menu>
    <span class="name">{repo.owner}/{repo.name}</span><span class="caret">▾</span>
  </button>
  {#if open}
    <div class="menu" role="menu">
      {#each entries as e (e.id)}
        <div class="entry">
          <button class="pick" role="menuitem" onclick={() => choose(e)} title={e.path} data-repo-id={e.id}>
            <span class="check">{e.here ? "✓" : ""}</span>
            <span class="text">
              <span class="name">{e.owner}/{e.name}</span>
              <span class="path">{e.path}{e.elsewhere ? " · open in another window" : ""}</span>
            </span>
          </button>
          {#if !e.here && !e.elsewhere}
            <button class="side" onclick={() => newWindow(e.id)} title="Open in a new window" aria-label="Open {e.owner}/{e.name} in a new window" data-new-window={e.id}>⧉</button>
          {/if}
        </div>
      {/each}
      <hr />
      <button class="action" role="menuitem" onclick={() => run(async () => onconnect())} data-connect-another>Connect another repository…</button>
      <button class="action" role="menuitem" onclick={() => newWindow(null)} data-empty-window>New window</button>
      {#if error}<p class="error">{error}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .wrap { position: relative; flex: 1; min-width: 0; }
  .repo { display: flex; align-items: center; gap: 6px; max-width: 100%; border: 0; background: none; padding: 2px 4px; font-weight: 600; text-align: left; }
  .repo .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .caret { color: var(--muted); font-size: 0.8em; }
  .menu { position: absolute; top: calc(100% + 4px); left: 0; z-index: 20; width: 320px; max-width: calc(100vw - 24px); background: var(--panel); border: 1px solid var(--border); border-radius: 6px; padding: 4px; box-shadow: 0 6px 24px rgb(0 0 0 / 0.35); }
  .entry { display: flex; align-items: stretch; }
  .pick, .action { display: flex; gap: 6px; width: 100%; border: 0; border-radius: 4px; background: none; text-align: left; padding: 6px 8px; }
  .pick:hover, .action:hover, .side:hover { background: var(--sidebar); }
  .check { width: 1em; flex-shrink: 0; color: var(--accent); }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .text .name { font-weight: 600; }
  .path { font-size: 0.8em; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .side { border: 0; background: none; padding: 0 8px; border-radius: 4px; color: var(--muted); }
  hr { border: 0; border-top: 1px solid var(--border); margin: 4px 0; }
  .error { color: var(--bad); font-size: 0.85em; margin: 4px 8px; }
</style>
