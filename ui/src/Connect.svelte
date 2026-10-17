<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { homeDir } from "@tauri-apps/api/path";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import Logo from "./controls/Logo.svelte";
  import Banner from "./controls/Banner.svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import { api, type Repo, type RepoEntry, type Shown } from "./lib/api";
  import { shortPath } from "./lib/paths";

  // `oncancel` is set when the window already shows a repo (connecting another one).
  let { onconnected, oncancel }: { onconnected: (repo: Repo) => void; oncancel?: () => void } = $props();
  let path = $state("");
  let busy = $state(false);
  let error: string | null = $state(null);
  // Repos connected before, other than the one this window shows.
  let recent: RepoEntry[] = $state([]);

  onMount(async () => {
    recent = (await api.repos().catch(() => [])).filter((r) => !r.here);
  });

  // A repo another window shows is focused there; this window stays as it was (an empty
  // one is closed by the backend).
  async function run(f: () => Promise<Shown>) {
    busy = true;
    error = null;
    try {
      const shown = await f();
      if (shown.here) onconnected(shown.repo);
      else oncancel?.();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  const connectTo = (dir: string) => run(() => api.connectRepo(dir));

  function connect(e: SubmitEvent) {
    e.preventDefault();
    connectTo(path.trim());
  }

  // Any folder inside the clone works (also one of its worktrees): the main checkout is found.
  async function choose() {
    const picked = await open({ directory: true, title: "Choose a local clone", defaultPath: await homeDir() });
    if (typeof picked === "string") {
      path = picked;
      connectTo(picked);
    }
  }

  async function newWindow(id: number) {
    try {
      await api.openWindow(id);
    } catch (e) {
      error = String(e);
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && oncancel && !busy) oncancel();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<main class="connect">
  {#if oncancel}
    <span class="cancel"><Button onclick={oncancel} disabled={busy} data-cancel-connect>Cancel <Kbd key="Esc" /></Button></span>
  {/if}
  <div class="column">
    <div class="brand"><Logo size={28} /><span class="wordmark">ariekei</span></div>
    <div class="intro">
      <h1>Connect a repository</h1>
      <p>Pick a local clone with a GitHub remote. Each window shows one repository.</p>
    </div>
    <div class="pick">
      <Button variant="primary" size="lg" icon="folder" loading={busy} onclick={choose}>{busy ? "Connecting…" : "Choose folder…"}</Button>
      <div class="or"><span></span>or enter a path<span></span></div>
      <form onsubmit={connect}>
        <input bind:value={path} placeholder="~/src/project" class:invalid={!!error} disabled={busy} aria-label="Repository path" spellcheck="false" />
        <Button type="submit" size="lg" disabled={busy || !path.trim()}>Connect</Button>
      </form>
      {#if error}
        <Banner tone="bad" compact={false}><strong>Could not connect</strong><div class="muted">{error}</div></Banner>
      {/if}
    </div>
    {#if recent.length}
      <section>
        <h2>Connected before</h2>
        <ul class="recent">
          {#each recent as r (r.id)}
            <li>
              <button class="open" onclick={() => run(() => api.switchRepo(r.id))} disabled={busy} title={r.path} data-recent={r.id}>
                <Icon name="folder" />
                <span class="text">
                  <span class="name">{r.owner}/{r.name}</span>
                  <span class="path">{shortPath(r.path)}</span>
                </span>
              </button>
              {#if r.elsewhere}
                <span class="state">open in another window</span>
                <Button variant="quiet" icon="external" onclick={() => run(() => api.switchRepo(r.id))} disabled={busy}>Switch to window</Button>
              {:else if oncancel}
                <Button variant="quiet" icon="window-new" onclick={() => newWindow(r.id)} disabled={busy}>Open in new window</Button>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    <p class="hint">Uses your <code>git</code> remotes to find the repository and your <code>gh</code> login for GitHub.</p>
  </div>
</main>

<style>
  .connect { flex: 1; height: 100vh; box-sizing: border-box; display: flex; justify-content: center; overflow: auto; padding: 96px var(--sp-9) 48px; position: relative; }
  .cancel { position: absolute; top: var(--sp-7); right: var(--sp-7); }
  .column { width: 100%; max-width: 560px; display: flex; flex-direction: column; gap: 28px; }
  .brand { display: flex; align-items: center; gap: var(--sp-5); }
  .wordmark { font-family: var(--font-mono); font-weight: var(--fw-semibold); font-size: 20px; letter-spacing: -0.03em; }
  .intro { display: flex; flex-direction: column; gap: var(--sp-3); }
  h1 { margin: 0; font-size: var(--fs-20); line-height: var(--lh-20); font-weight: var(--fw-semibold); }
  .intro p { margin: 0; color: var(--text-muted); }
  .pick { display: flex; flex-direction: column; gap: var(--sp-6); }
  .pick > :global(button) { align-self: flex-start; }
  .or { display: flex; align-items: center; gap: var(--sp-4); color: var(--text-faint); font-size: var(--fs-12); }
  .or span { flex: 1; height: 1px; background: var(--border); }
  form { display: flex; gap: var(--sp-4); }
  form input { flex: 1; min-width: 0; height: 30px; padding: 0 var(--sp-5); font-family: var(--font-mono); }
  form input.invalid { border-color: var(--bad); }
  .muted { color: var(--text-muted); overflow-wrap: anywhere; }
  section { display: flex; flex-direction: column; gap: var(--sp-4); }
  h2 {
    margin: 0; font-size: var(--fs-11); line-height: var(--lh-11); font-weight: var(--fw-semibold);
    letter-spacing: 0.04em; text-transform: uppercase; color: var(--text-muted);
  }
  .recent {
    list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column;
    border: 1px solid var(--border); border-radius: var(--r-lg); overflow: hidden; background: var(--surface-raised);
  }
  .recent li { display: flex; align-items: center; gap: var(--sp-6); padding: 0 var(--sp-6) 0 0; }
  .recent li + li { border-top: 1px solid var(--border); }
  .open {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: var(--sp-6); padding: var(--sp-5) 0 var(--sp-5) var(--sp-6);
    border: 0; border-radius: 0; background: transparent; color: var(--text-muted); text-align: left;
  }
  .open:hover:not(:disabled) .name { text-decoration: underline; }
  .open:focus-visible { outline-offset: -2px; }
  .text { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .name { color: var(--text); font-weight: var(--fw-medium); }
  .path { font-family: var(--font-mono); font-size: 11.5px; color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .state { font-size: var(--fs-11); color: var(--text-faint); white-space: nowrap; }
  .hint { margin: 0; color: var(--text-faint); font-size: var(--fs-12); }
  code { font-size: 11.5px; }
</style>
