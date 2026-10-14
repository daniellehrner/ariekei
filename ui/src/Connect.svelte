<script lang="ts">
  import { homeDir } from "@tauri-apps/api/path";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { api, type Repo, type RepoEntry, type Shown } from "./lib/api";

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
</script>

<section class="connect">
  <h1>Ariekei</h1>
  {#if recent.length}
    <h2>Open a connected repository</h2>
    <ul class="recent">
      {#each recent as r (r.id)}
        <li>
          <button onclick={() => run(() => api.switchRepo(r.id))} disabled={busy} title={r.path} data-recent={r.id}>
            <span class="name">{r.owner}/{r.name}</span>
            <span class="path">{r.path}{r.elsewhere ? " · open in another window" : ""}</span>
          </button>
        </li>
      {/each}
    </ul>
    <h2>Or connect another</h2>
  {/if}
  <p>Connect a local clone of a GitHub repository. Your open pull requests in it become work items.</p>
  <button class="primary choose" onclick={choose} disabled={busy}>{busy ? "Connecting…" : "Choose folder…"}</button>
  <form onsubmit={connect}>
    <input bind:value={path} placeholder="or type a path: /path/to/clone" size="48" disabled={busy} />
    <button type="submit" disabled={busy || !path.trim()}>Connect</button>
  </form>
  {#if error}<p class="error">{error}</p>{/if}
  {#if oncancel}<button onclick={oncancel} disabled={busy} data-cancel-connect>Cancel</button>{/if}
  <p class="hint">Uses your <code>git</code> remotes to find the repository and your <code>gh</code> login for GitHub.</p>
</section>

<style>
  .connect { max-width: 640px; margin: 15vh auto 0; padding: 0 16px; }
  h1 { font-size: 22px; margin: 0 0 8px; }
  h2 { font-size: 0.8em; text-transform: uppercase; letter-spacing: 0.05em; color: var(--muted); margin: 20px 0 6px; }
  .recent { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .recent button { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; width: 100%; text-align: left; padding: 8px 12px; }
  .recent .name { font-weight: 600; }
  .recent .path { font-size: 0.8em; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  form { display: flex; gap: 8px; flex-wrap: wrap; margin: 16px 0; }
  input { flex: 1; min-width: 0; }
  .error { color: var(--bad); }
  .primary { border-color: var(--accent); }
  .choose { margin-top: 8px; padding: 6px 16px; }
  .hint { color: var(--muted); font-size: 0.9em; }
</style>
