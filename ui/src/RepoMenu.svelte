<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import InlineConfirm from "./controls/InlineConfirm.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import Button from "./controls/Button.svelte";
  import { api, type Repo, type RepoEntry, type RepoView } from "./lib/api";
  import { shortPath } from "./lib/paths";

  // The repo name in the Inbox header: switch this window to another connected repo, open
  // one in a new window, remove one from Ariekei, or connect another. A repo shown in another
  // window is focused there instead. Arrow keys move, Enter picks, Esc closes. This window's
  // repo also gets its private mirrors here (added, listed, removed).
  let {
    repo,
    onswitch,
    onconnect,
    onremoved,
    onmirrors,
  }: {
    repo: RepoView;
    onswitch: (repo: Repo) => void;
    onconnect: () => void;
    /** This window's repo was removed: it shows the Connect screen. */
    onremoved: () => void;
    /** A private mirror of this window's repo was added or removed. */
    onmirrors: () => void;
  } = $props();

  let open = $state(false);
  let entries: RepoEntry[] = $state([]);
  let error: string | null = $state(null);
  let wrap: HTMLElement | undefined = $state();
  let menu: HTMLElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();

  // Removing a repo asks first, naming the worktrees it leaves on disk.
  let removing = $state<{ id: number; leftovers: string[] } | null>(null);

  async function askRemove(e: RepoEntry) {
    error = null;
    try {
      removing = { id: e.id, leftovers: await api.repoLeftovers(e.id) };
    } catch (err) {
      error = String(err);
    }
  }

  async function remove(e: RepoEntry) {
    removing = null;
    try {
      await api.removeRepo(e.id);
      if (e.here) {
        open = false;
        onremoved();
      } else entries = await api.repos();
    } catch (err) {
      error = `Could not remove ${e.owner}/${e.name}: ${err}`;
    }
  }

  // --- private mirrors of this window's repo ---
  let addingMirror = $state(false);
  let mirrorName = $state("");
  let mirrorBusy = $state(false);
  let mirrorNote: string | null = $state(null);
  let removingMirror: number | null = $state(null);

  async function addMirror(e: SubmitEvent) {
    e.preventDefault();
    if (!mirrorName.trim() || mirrorBusy) return;
    mirrorBusy = true;
    error = mirrorNote = null;
    try {
      const added = await api.addMirror(mirrorName.trim());
      mirrorName = "";
      addingMirror = false;
      mirrorNote = added.note;
      entries = await api.repos();
      onmirrors();
    } catch (err) {
      error = String(err);
    } finally {
      mirrorBusy = false;
    }
  }

  async function removeMirror(m: Repo) {
    removingMirror = null;
    try {
      await api.removeMirror(m.id);
      entries = await api.repos();
      onmirrors();
    } catch (err) {
      error = `Could not remove ${m.owner}/${m.name}: ${err}`;
    }
  }

  async function toggle(e: MouseEvent) {
    // Opened from the keyboard (Enter/Space: no pointer), the first entry gets the focus.
    const fromKeyboard = e.detail === 0;
    open = !open;
    removing = null;
    removingMirror = null;
    addingMirror = false;
    if (!open) return;
    error = mirrorNote = null;
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
    // → from a repo reaches its buttons (window, Remove), ← goes back.
    const entry = (document.activeElement as HTMLElement | null)?.closest(".entry");
    if (step) {
      e.preventDefault();
      list[(i + step + list.length) % list.length]?.focus();
    } else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && entry) {
      e.preventDefault();
      const buttons = [...entry.querySelectorAll<HTMLElement>("button")];
      const at = buttons.indexOf(document.activeElement as HTMLElement);
      buttons[Math.max(0, Math.min(buttons.length - 1, at + (e.key === "ArrowRight" ? 1 : -1)))]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (removing) removing = null;
      else if (removingMirror !== null) removingMirror = null;
      else if (addingMirror) addingMirror = false;
      else close();
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
          {#if !e.elsewhere}
            <span class="remove">
              <IconButton icon="inbox-remove" label="Remove {e.owner}/{e.name} from Ariekei" title="Remove from Ariekei" tabindex={-1}
                onclick={() => askRemove(e)} data-remove-repo={e.id} />
            </span>
          {/if}
        </div>
        {#if removing?.id === e.id}
          {@const left = removing.leftovers}
          <div class="removing" data-removing={e.id}>
            <p>
              Its Inbox, stars and review drafts go; the clone{left.length ? " and these worktrees stay" : " stays"} on disk{left.length ? ":" : "."}
            </p>
            {#if left.length}<ul>{#each left as path}<li title={path}>{shortPath(path)}</li>{/each}</ul>{/if}
            <InlineConfirm text="Remove {e.owner}/{e.name}?" confirm="Remove" onconfirm={() => remove(e)} oncancel={() => (removing = null)} data-confirm-remove />
          </div>
        {/if}
        {#each e.mirrors as m (m.id)}
          <div class="mirror" data-mirror={m.id}>
            <Icon name="lock" size={14} />
            <span class="name" title="A private mirror of {e.owner}/{e.name}: its issues and PRs are in {e.owner}/{e.name}'s Inbox, and their work is pushed only to it ({m.baseRemote})">{m.owner}/{m.name}</span>
            <span class="path">private mirror</span>
            {#if e.here}
              <span class="remove">
                <IconButton icon="inbox-remove" label="Remove the mirror {m.owner}/{m.name}" title="Remove the mirror" tabindex={-1}
                  onclick={() => (removingMirror = m.id)} data-remove-mirror={m.id} />
              </span>
            {/if}
          </div>
          {#if removingMirror === m.id}
            <div class="removing">
              <p>Its items leave the Inbox. The remote, the pre-push hook and the worktrees stay.</p>
              <InlineConfirm text="Remove {m.owner}/{m.name}?" confirm="Remove" onconfirm={() => removeMirror(m)} oncancel={() => (removingMirror = null)} />
            </div>
          {/if}
        {/each}
      {/each}
      {#if mirrorNote}<p class="note" data-mirror-note>{mirrorNote}</p>{/if}
      <hr />
      {#if addingMirror}
        <form class="add-mirror" onsubmit={addMirror} data-mirror-form>
          <p>
            Another GitHub repository in this clone, e.g. a private copy for security fixes. Its issues and PRs are listed in
            this Inbox, tagged, and their branches push only to it.
          </p>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="mono" bind:value={mirrorName} placeholder="owner/name" autofocus disabled={mirrorBusy}
            onkeydown={(ev) => ev.key !== "Escape" && ev.stopPropagation()} data-mirror-name />
          <div class="form-actions">
            <Button size="sm" variant="quiet" type="button" onclick={() => (addingMirror = false)} disabled={mirrorBusy}>Cancel</Button>
            <Button size="sm" variant="primary" type="submit" loading={mirrorBusy} disabled={!mirrorName.trim()}>Add mirror</Button>
          </div>
        </form>
      {:else}
        <button class="action" role="menuitem" onclick={() => ((addingMirror = true), (error = null))} data-add-mirror>
          <Icon name="lock" />Add a private mirror of {repo.owner}/{repo.name}…
        </button>
      {/if}
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
  /* Remove is a hover/focus action, as in the Inbox's rows. */
  .remove { display: flex; opacity: 0; }
  .entry:hover .remove, .entry:focus-within .remove { opacity: 1; }
  .removing {
    display: flex; flex-direction: column; gap: var(--sp-3); margin: 0 var(--sp-2) var(--sp-2); padding: var(--sp-4);
    border-radius: var(--r-md); background: var(--surface-hover); font-size: var(--fs-12); color: var(--text-muted);
  }
  .removing p, .removing ul { margin: 0; }
  .removing ul { padding-left: var(--sp-6); font-family: var(--font-mono); font-size: var(--fs-11); }
  .removing li { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  hr { border: 0; height: 1px; margin: var(--sp-2) 0; background: var(--border); }
  .action {
    display: flex; align-items: center; gap: var(--sp-4); height: 30px; padding: 0 var(--sp-4);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); text-align: left;
  }
  .action:hover { background: var(--surface-hover); }
  .action:focus-visible { outline-offset: -2px; }
  .grow { flex: 1; }
  .error { color: var(--bad); font-size: var(--fs-12); margin: var(--sp-2) var(--sp-4); }
  .mirror {
    display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) var(--sp-4) var(--sp-2) calc(16px + var(--sp-4) * 2);
    color: var(--text-muted); font-size: var(--fs-12);
  }
  .mirror .name { color: var(--text); font-weight: var(--fw-medium); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mirror .path { flex: 1; }
  .mirror:hover .remove, .mirror:focus-within .remove { opacity: 1; }
  .note { margin: var(--sp-2) var(--sp-4); font-size: var(--fs-12); color: var(--warn); }
  .add-mirror { display: flex; flex-direction: column; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4); }
  .add-mirror p { margin: 0; font-size: var(--fs-12); color: var(--text-muted); }
  .add-mirror input {
    height: 28px; padding: 0 var(--sp-4); border: 1px solid var(--border); border-radius: var(--r-md);
    background: var(--surface-0); color: var(--text); font-family: var(--font-mono); font-size: var(--fs-12);
  }
  .add-mirror .form-actions { display: flex; justify-content: flex-end; gap: var(--sp-3); }
</style>
