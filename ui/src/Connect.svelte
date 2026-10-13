<script lang="ts">
  import { api, type Repo } from "./lib/api";

  let { onconnected }: { onconnected: (repo: Repo) => void } = $props();
  let path = $state("");
  let busy = $state(false);
  let error: string | null = $state(null);

  async function connect(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = null;
    try {
      onconnected(await api.connectRepo(path.trim()));
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<section class="connect">
  <h1>Ariekei</h1>
  <p>Connect a local clone of a GitHub repository. Your open pull requests in it become work items.</p>
  <form onsubmit={connect}>
    <input bind:value={path} placeholder="/path/to/clone" size="48" disabled={busy} />
    <button type="submit" disabled={busy || !path.trim()}>{busy ? "Connecting…" : "Connect"}</button>
  </form>
  {#if error}<p class="error">{error}</p>{/if}
  <p class="hint">Uses your <code>git</code> remotes to find the repository and your <code>gh</code> login for GitHub.</p>
</section>

<style>
  .connect { max-width: 640px; margin: 15vh auto 0; padding: 0 16px; }
  h1 { font-size: 22px; margin: 0 0 8px; }
  form { display: flex; gap: 8px; flex-wrap: wrap; margin: 16px 0; }
  input { flex: 1; min-width: 0; }
  .error { color: var(--bad); }
  .hint { color: var(--muted); font-size: 0.9em; }
</style>
