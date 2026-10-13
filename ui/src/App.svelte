<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Connect from "./Connect.svelte";
  import Inbox from "./Inbox.svelte";
  import WorkItem from "./WorkItem.svelte";
  import { listenToAgents } from "./lib/agent.svelte";
  import { api, type Repo, type Row } from "./lib/api";

  const SELECTED = "ariekei.selected";
  const remembered = () => {
    try {
      return Number(localStorage.getItem(SELECTED)) || null;
    } catch {
      return null;
    }
  };

  let repo: Repo | null | undefined = $state(undefined);
  let rows: Row[] = $state([]);
  let selected: number | null = $state(remembered());
  let pollError: string | null = $state(null);

  const current = $derived(rows.find((r) => r.id === selected) ?? null);

  let reloadTimer: ReturnType<typeof setTimeout> | undefined;
  function reload() {
    clearTimeout(reloadTimer);
    reloadTimer = setTimeout(async () => {
      try {
        rows = await api.inbox();
      } catch (e) {
        pollError = String(e);
      }
    }, 150);
  }

  function select(id: number) {
    selected = id;
    try {
      localStorage.setItem(SELECTED, String(id));
    } catch {
      // Only a convenience: the next launch starts without a selection.
    }
  }

  onMount(() => {
    listenToAgents();
    const unlisten = [
      listen("inbox-changed", () => {
        pollError = null;
        reload();
      }),
      listen<string>("inbox-error", (e) => (pollError = e.payload)),
      // Agent status feeds the Inbox's ordering.
      listen("agent-event", reload),
    ];
    api.repo().then((r) => {
      repo = r;
      if (r) reload();
    });
    return () => unlisten.forEach((p) => p.then((f) => f()));
  });
</script>

{#if repo === null}
  <Connect onconnected={(r) => { repo = r; reload(); }} />
{:else if repo}
  <div class="app">
    <Inbox {repo} {rows} {selected} error={pollError} onselect={select} onrefresh={() => api.refresh()} />
    {#if current}
      <WorkItem row={current} onchanged={reload} />
    {:else}
      <p class="hint">{rows.length ? "Select a pull request." : "Loading your pull requests…"}</p>
    {/if}
  </div>
{/if}

<style>
  .app { display: flex; height: 100vh; }
  .hint { color: var(--muted); margin: 24px; }
</style>
