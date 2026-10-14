<script lang="ts">
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import { api, type Repo, type RepoEntry } from "./lib/api";
  import { shortPath } from "./lib/paths";

  // The repo name in the Inbox header: switch this window to another connected repo, open
  // one in a new window, or connect another. A repo shown in another window is focused
  // there instead. Arrow keys move, Enter picks, Esc closes.
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
  let menu: HTMLElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();

  async function toggle(e: MouseEvent) {
    // Opened from the keyboard (Enter/Space: no pointer), the first entry gets the focus.
    const fromKeyboard = e.detail === 0;
    open = !open;
    if (!open) return;
    error = null;
    try {
      entries = await api.repos();
    } catch (e) {
      error = String(e);
    }
    if (fromKeyboard) requestAnimationFrame(() => items()[0]?.focus());
  }

  function close() {
    open = false;
    trigger?.focus();
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

  const items = () => [...(menu?.querySelectorAll<HTMLElement>("[role^='menuitem']") ?? [])];

  function onMenuKey(e: KeyboardEvent) {
    const list = items();
    const i = list.indexOf(document.activeElement as HTMLElement);
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (step) {
      e.preventDefault();
      list[(i + step + list.length) % list.length]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  function onWindowClick(e: MouseEvent) {
    if (open && wrap && !wrap.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} />

<div class="wrap" bind:this={wrap}>
  <button class="repo" class:open bind:this={trigger} onclick={toggle} title={repo.path} aria-haspopup="menu" aria-expanded={open} data-repo-menu>
    <span class="name">{repo.owner}/{repo.name}</span>
    <Icon name="chevron-down" size={14} />
  </button>
  {#if open}
    <div class="menu" role="menu" aria-label="Repositories" tabindex="-1" bind:this={menu} onkeydown={onMenuKey}>
      <div class="heading">Connected repositories</div>
      {#each entries as e (e.id)}
        <div class="entry" class:here={e.here}>
          <span class="check">{#if e.here}<Icon name="check" size={16} />{/if}</span>
          <button class="pick" role="menuitemradio" aria-checked={e.here} onclick={() => choose(e)} title={e.path} data-repo-id={e.id}>
            <span class="name">{e.owner}/{e.name}</span>
            <span class="path">{shortPath(e.path)}{e.here ? " · this window" : e.elsewhere ? " · open in another window" : ""}</span>
          </button>
          {#if e.elsewhere}
            <IconButton icon="external" label="Switch to the window of {e.owner}/{e.name}" title="Switch to its window" tabindex={-1} onclick={() => choose(e)} />
          {:else if !e.here}
            <IconButton icon="window-new" label="Open {e.owner}/{e.name} in a new window" title="Open in new window" tabindex={-1}
              onclick={() => newWindow(e.id)} data-new-window={e.id} />
          {/if}
        </div>
      {/each}
      <hr />
      <button class="action" role="menuitem" onclick={() => run(async () => onconnect())} data-connect-another>
        <Icon name="plus" />Connect another repository…
      </button>
      <button class="action" role="menuitem" onclick={() => newWindow(null)} data-empty-window>
        <Icon name="window-new" /><span class="grow">New window</span><Kbd key="Ctrl+Shift+N" />
      </button>
      {#if error}<p class="error">{error}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .wrap { position: relative; flex: 1; min-width: 0; }
  .repo {
    display: inline-flex; align-items: center; gap: var(--sp-3); max-width: 100%; height: 28px; padding: 0 var(--sp-4);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text);
    font-size: var(--fs-13); font-weight: var(--fw-semibold); text-align: left;
  }
  .repo:hover, .repo.open { background: var(--surface-hover); }
  .repo :global(svg) { color: var(--text-muted); }
  .repo .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .menu {
    position: absolute; top: calc(100% + 4px); left: 0; z-index: 20; width: 330px; max-width: calc(100vw - 24px);
    padding: var(--sp-2); box-sizing: border-box; display: flex; flex-direction: column;
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop);
  }
  .menu:focus { outline: none; }
  .heading {
    padding: var(--sp-3) var(--sp-4) var(--sp-2); font-size: var(--fs-11); line-height: var(--lh-11);
    font-weight: var(--fw-semibold); letter-spacing: 0.04em; text-transform: uppercase; color: var(--text-muted);
  }
  .entry { display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-3) var(--sp-4); border-radius: var(--r-md); }
  .entry.here { background: var(--surface-hover); }
  .entry:has(.pick:focus-visible) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .check { width: 16px; flex: none; display: flex; color: var(--you); }
  .pick {
    flex: 1; min-width: 0; display: flex; flex-direction: column; align-items: flex-start;
    padding: 0; border: 0; background: transparent; color: var(--text); text-align: left;
  }
  .pick:focus-visible { outline: none; }
  .pick .name { font-weight: var(--fw-medium); }
  .here .pick .name { font-weight: var(--fw-semibold); }
  .path {
    max-width: 100%; font-family: var(--font-mono); font-size: var(--fs-11); color: var(--text-muted);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  hr { border: 0; height: 1px; margin: var(--sp-2) 0; background: var(--border); }
  .action {
    display: flex; align-items: center; gap: var(--sp-4); height: 30px; padding: 0 var(--sp-4);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); text-align: left;
  }
  .action:hover { background: var(--surface-hover); }
  .action:focus-visible { outline-offset: -2px; }
  .grow { flex: 1; }
  .error { color: var(--bad); font-size: var(--fs-12); margin: var(--sp-2) var(--sp-4); }
</style>
