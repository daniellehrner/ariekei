<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import AgentPanel from "./AgentPanel.svelte";
  import DiffView from "./DiffView.svelte";
  import { listenToAgents, sessions, startAgent, stop } from "./lib/agent.svelte";

  // Spike: the open worktrees and their view state live in localStorage.
  // In M1 these become work items in the SQLite store.
  interface ItemView { tab: "agent" | "review"; base: string; selected: string | null }
  const STORAGE = "ariekei.spike.v1";

  function load(): { items: string[]; active: string | null; views: Record<string, ItemView> } {
    try {
      return JSON.parse(localStorage.getItem(STORAGE) ?? "");
    } catch {
      return { items: [], active: null, views: {} };
    }
  }

  const saved = load();
  let items: string[] = $state(saved.items ?? []);
  let active: string | null = $state(saved.active ?? null);
  let views: Record<string, ItemView> = $state(saved.views ?? {});
  let newPath = $state("");
  let error: string | null = $state(null);
  let rssMib: number | null = $state(null);

  $effect(() => {
    const snapshot = JSON.stringify({ items, active, views });
    try {
      localStorage.setItem(STORAGE, snapshot);
    } catch {
      // Not fatal: the next launch just starts without restored state.
    }
  });

  const view = $derived(active ? views[active] : null);

  function activate(key: string) {
    views[key] ??= { tab: "agent", base: "HEAD", selected: null };
    active = key;
  }

  async function ensureAgent(key: string) {
    const status = sessions[key]?.status;
    if (status && status !== "stopped" && status !== "error") return;
    try {
      error = null;
      await startAgent(key);
    } catch (e) {
      error = String(e);
    }
  }

  async function add(e: SubmitEvent) {
    e.preventDefault();
    const path = newPath.trim();
    if (!path) return;
    try {
      error = null;
      const { key } = await startAgent(path);
      if (!items.includes(key)) items.push(key);
      activate(key);
      newPath = "";
    } catch (err) {
      error = String(err);
    }
  }

  function select(key: string) {
    activate(key);
    ensureAgent(key);
  }

  async function remove(key: string) {
    await stop(key);
    items = items.filter((k) => k !== key);
    delete views[key];
    if (active === key) active = items[0] ?? null;
  }

  const label = (key: string) => key.split(/[\\/]/).filter(Boolean).slice(-2).join("/");

  const STATUS_LABEL: Record<string, string> = {
    starting: "starting",
    ready: "ready",
    working: "working",
    waitingForPermission: "needs you",
    finished: "finished",
    error: "error",
    stopped: "stopped",
  };

  onMount(() => {
    listenToAgents();
    // Coming back to the app resumes the work item that was open.
    if (active) {
      activate(active);
      ensureAgent(active);
    }
    const poll = () => invoke<number | null>("self_rss_kib").then((k) => (rssMib = k ? Math.round(k / 1024) : null));
    poll();
    const timer = setInterval(poll, 5000);
    return () => clearInterval(timer);
  });
</script>

<div class="app">
  <aside>
    <h1>Ariekei <small>spike</small></h1>
    <ul>
      {#each items as key (key)}
        {@const status = sessions[key]?.status ?? "stopped"}
        <li class:active={key === active}>
          <button class="item" onclick={() => select(key)} title={key}>
            <span class="dot {status}"></span>
            <span class="name">{label(key)}</span>
            <span class="status">{STATUS_LABEL[status]}</span>
          </button>
          <button class="close" onclick={() => remove(key)} aria-label="Close {label(key)}">×</button>
        </li>
      {/each}
    </ul>
    <form onsubmit={add}>
      <input bind:value={newPath} placeholder="/path/to/worktree" />
      <button type="submit">Open</button>
    </form>
    {#if error}<p class="error">{error}</p>{/if}
    {#if rssMib !== null}<p class="metric">App RSS: {rssMib} MiB</p>{/if}
  </aside>

  <main>
    {#if active && view}
      <nav>
        <button class:on={view.tab === "agent"} onclick={() => (view.tab = "agent")}>Agent</button>
        <button class:on={view.tab === "review"} onclick={() => (view.tab = "review")}>Review</button>
        <span class="path">{active}</span>
        {#if sessions[active]?.detail}<span class="detail">{sessions[active].detail}</span>{/if}
      </nav>
      <div class="pane">
        {#key active}
          {#if view.tab === "agent"}
            <AgentPanel agentKey={active} />
          {:else}
            <DiffView cwd={active} bind:base={view.base} bind:selected={view.selected} />
          {/if}
        {/key}
      </div>
    {:else}
      <p class="hint">Open a git worktree to start an agent session in it.</p>
    {/if}
  </main>
</div>

<style>
  .app { display: flex; height: 100vh; }
  aside { width: 270px; flex-shrink: 0; border-right: 1px solid var(--border); display: flex; flex-direction: column; background: var(--sidebar); }
  h1 { font-size: 15px; margin: 12px; }
  h1 small { color: var(--muted); font-weight: 400; }
  ul { list-style: none; margin: 0; padding: 0; flex: 1; overflow-y: auto; }
  li { display: flex; align-items: center; }
  li.active { background: var(--panel); }
  .item { flex: 1; display: flex; align-items: center; gap: 8px; border: 0; background: none; text-align: left; padding: 7px 12px; min-width: 0; }
  .name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { color: var(--muted); font-size: 0.8em; }
  .close { border: 0; background: none; color: var(--muted); padding: 4px 10px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--muted); flex-shrink: 0; }
  .dot.working, .dot.starting { background: var(--accent); }
  .dot.waitingForPermission { background: var(--warn); }
  .dot.finished, .dot.ready { background: var(--ok); }
  .dot.error { background: var(--bad); }
  form { display: flex; gap: 6px; padding: 10px 12px; border-top: 1px solid var(--border); }
  form input { flex: 1; min-width: 0; }
  .error { color: var(--bad); margin: 0 12px 8px; font-size: 0.85em; word-break: break-word; }
  .metric { color: var(--muted); margin: 0 12px 10px; font-size: 0.8em; }
  main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  nav { display: flex; gap: 4px; align-items: center; padding: 6px 10px; border-bottom: 1px solid var(--border); }
  nav button { border: 1px solid transparent; background: none; padding: 4px 12px; border-radius: 4px; }
  nav button.on { background: var(--panel); border-color: var(--border); }
  .path { color: var(--muted); font-family: var(--mono); font-size: 0.8em; margin-left: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .detail { color: var(--muted); font-size: 0.8em; margin-left: auto; }
  .pane { flex: 1; min-height: 0; }
  .hint { color: var(--muted); margin: 24px; }
</style>
