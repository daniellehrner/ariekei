<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Connect from "./Connect.svelte";
  import Inbox from "./Inbox.svelte";
  import Notices from "./Notices.svelte";
  import WorkItem from "./WorkItem.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import { homeDir } from "@tauri-apps/api/path";
  import { listenToAgents } from "./lib/agent.svelte";
  import { api, type Repo, type Row } from "./lib/api";
  import { layout } from "./lib/layout.svelte";
  import { setHome } from "./lib/paths";

  let repo: Repo | null | undefined = $state(undefined);
  let rows: Row[] = $state([]);
  // The first Inbox load of the repo finished.
  let loaded = $state(false);
  let selected: number | null = $state(null);
  let pollError: string | null = $state(null);
  // The Connect screen over a connected repo (to add another one).
  let connecting = $state(false);
  // The Inbox can collapse to a thin strip, leaving the screen to the work item. Below
  // 1200 px it also steps aside while the review's side panel is open, unless the user
  // opened it again.
  let inboxCollapsed = $state(false);
  let width = $state(window.innerWidth);
  let keepInboxOpen = $state(false);
  const crowded = $derived(width < 1200 && layout.reviewPanel);
  const collapsed = $derived(inboxCollapsed || (crowded && !keepInboxOpen));
  $effect(() => {
    if (!crowded) keepInboxOpen = false;
  });

  function toggleInbox() {
    if (collapsed && !inboxCollapsed) {
      keepInboxOpen = true;
      return;
    }
    inboxCollapsed = !inboxCollapsed;
    if (!inboxCollapsed && crowded) keepInboxOpen = true;
    api.setUiSetting("inboxCollapsed", String(inboxCollapsed));
  }

  // Ctrl+B (Cmd+B) toggles the Inbox and Ctrl+Shift+N opens a new window, except in the
  // agent terminal, where the keys belong to Claude Code.
  function onKeydown(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || (e.target as Element | null)?.closest?.(".xterm")) return;
    const key = e.key.toLowerCase();
    if (!e.shiftKey && key === "b" && repo) {
      e.preventDefault();
      toggleInbox();
    } else if (e.shiftKey && key === "n") {
      e.preventDefault();
      api.openWindow(null);
    }
  }

  const current = $derived(rows.find((r) => r.id === selected) ?? null);

  let reloadTimer: ReturnType<typeof setTimeout> | undefined;
  function reload() {
    clearTimeout(reloadTimer);
    reloadTimer = setTimeout(async () => {
      const shown = repo?.id;
      try {
        const fetched = await api.inbox();
        // The window may have switched repos meanwhile.
        if (repo?.id === shown) rows = fetched;
      } catch (e) {
        pollError = String(e);
      }
      if (repo?.id === shown) loaded = true;
    }, 150);
  }

  // This window now shows `r` (at launch, after connecting, or switched from the repo menu).
  async function show(r: Repo) {
    connecting = false;
    repo = r;
    // The backend sets the window's title; this is the page's (as WebDriver sees it).
    document.title = `${r.owner}/${r.name} — Ariekei`;
    rows = [];
    loaded = false;
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
    homeDir().then(setHome, () => {});
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
      // A notice was opened (open_item): show its item.
      listen<number>("select-item", (e) => select(e.payload)),
    ];
    api.repo().then(async (r) => {
      inboxCollapsed = (await api.uiSetting("inboxCollapsed")) === "true";
      if (r) show(r);
      else repo = null;
    });
    return () => unlisten.forEach((p) => p.then((f) => f()));
  });
</script>

<svelte:window onkeydown={onKeydown} bind:innerWidth={width} />

{#if repo === null || connecting}
  <Connect onconnected={show} oncancel={repo ? () => (connecting = false) : undefined} />
{:else if repo}
  {#key repo.id}
  <div class="app">
    <Inbox {repo} {rows} {loaded} {selected} error={pollError} {collapsed} ontoggle={toggleInbox} onselect={select} onrefresh={() => api.refresh()} onadded={reload}
      onswitch={show} onconnect={() => (connecting = true)} />
    {#if current}
      <WorkItem row={current} {rows} repoId={repo.id} viewer={repo.viewer} onchanged={reload} onselect={select} />
    {:else}
      {@const waiting = rows.filter((r) => r.attention).length}
      <div class="empty">
        {#if loaded && !rows.length}
          <strong>Your Inbox is empty</strong>
          <span>Your open pull requests, review requests and assigned issues in {repo.owner}/{repo.name} appear here.
            To review any other pull request, search for it. To start work that has no PR or issue yet, use + (New task).</span>
        {:else if loaded}
          <strong>{waiting ? `${waiting} item${waiting === 1 ? " wants" : "s want"} your attention` : "You're all caught up"}</strong>
          <span>Pick an item in the Inbox{waiting ? ", starting at the top" : ""}. <Kbd key="Ctrl+B" /> shows or hides the Inbox.</span>
        {:else}
          <span>Loading your pull requests…</span>
        {/if}
      </div>
    {/if}
  </div>
  {/key}
{/if}
<Notices {selected} />

<style>
  .app { display: flex; height: 100vh; min-height: 0; overflow: hidden; }
  .empty { flex: 1; display: flex; flex-direction: column; gap: var(--sp-3); align-items: center; justify-content: center; padding: var(--sp-9); color: var(--text-muted); font-size: var(--fs-12); text-align: center; }
  .empty strong { color: var(--text); font-size: var(--fs-13); font-weight: var(--fw-semibold); }
</style>
