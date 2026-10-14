<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Connect from "./Connect.svelte";
  import Inbox from "./Inbox.svelte";
  import WorkItem from "./WorkItem.svelte";
  import { listenToAgents } from "./lib/agent.svelte";
  import { api, type Repo, type Row } from "./lib/api";

  let repo: Repo | null | undefined = $state(undefined);
  let rows: Row[] = $state([]);
  let selected: number | null = $state(null);
  let pollError: string | null = $state(null);
  // The Connect screen over a connected repo (to add another one).
  let connecting = $state(false);
  // The Inbox can collapse to a thin strip, leaving the screen to the work item.
  let inboxCollapsed = $state(false);

  function toggleInbox() {
    inboxCollapsed = !inboxCollapsed;
    api.setUiSetting("inboxCollapsed", String(inboxCollapsed));
  }

  // Ctrl+B (Cmd+B) toggles the Inbox, except in the agent terminal, where the key belongs
  // to Claude Code.
  function onKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "b") {
      if ((e.target as Element | null)?.closest?.(".xterm")) return;
      e.preventDefault();
      toggleInbox();
    }
  }

  const current = $derived(rows.find((r) => r.id === selected) ?? null);

  let reloadTimer: ReturnType<typeof setTimeout> | undefined;
  function reload() {
    clearTimeout(reloadTimer);
    reloadTimer = setTimeout(async () => {
      const shown = repo?.id;
      try {
        const loaded = await api.inbox();
        // The window may have switched repos meanwhile.
        if (repo?.id === shown) rows = loaded;
      } catch (e) {
        pollError = String(e);
      }
    }, 150);
  }

  // This window now shows `r` (at launch, after connecting, or switched from the repo menu).
  async function show(r: Repo) {
    connecting = false;
    repo = r;
    // The backend sets the window's title; this is the page's (as WebDriver sees it).
    document.title = `${r.owner}/${r.name} — Ariekei`;
    rows = [];
    selected = null;
    pollError = null;
    selected = await api.selectedItem();
    reload();
  }

  // The work item view tells the backend which item is shown (set_active_item), which
  // also remembers it for the next launch.
  function select(id: number) {
    selected = id;
  }

  onMount(() => {
    listenToAgents();
    const unlisten = [
      // Polls of every window's repo arrive in every window.
      listen<number>("inbox-changed", (e) => {
        if (e.payload !== repo?.id) return;
        pollError = null;
        reload();
      }),
      listen<{ repo: number; message: string }>("inbox-error", (e) => {
        if (e.payload.repo === repo?.id) pollError = e.payload.message;
      }),
      // Agent status feeds the Inbox's ordering.
      listen("agent-event", reload),
    ];
    api.repo().then(async (r) => {
      inboxCollapsed = (await api.uiSetting("inboxCollapsed")) === "true";
      if (r) show(r);
      else repo = null;
    });
    return () => unlisten.forEach((p) => p.then((f) => f()));
  });
</script>

<svelte:window onkeydown={onKeydown} />

{#if repo === null || connecting}
  <Connect onconnected={show} oncancel={repo ? () => (connecting = false) : undefined} />
{:else if repo}
  {#key repo.id}
  <div class="app">
    <Inbox {repo} {rows} {selected} error={pollError} collapsed={inboxCollapsed} ontoggle={toggleInbox} onselect={select} onrefresh={() => api.refresh()} onadded={reload}
      onswitch={show} onconnect={() => (connecting = true)} />
    {#if current}
      <WorkItem row={current} repoId={repo.id} onchanged={reload} />
    {:else}
      <p class="hint">{rows.length ? "Select a pull request." : "Loading your pull requests…"}</p>
    {/if}
  </div>
  {/key}
{/if}

<style>
  .app { display: flex; height: 100vh; }
  .hint { color: var(--muted); margin: 24px; }
</style>
