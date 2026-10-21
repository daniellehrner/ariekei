<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Browse from "./Browse.svelte";
  import Connect from "./Connect.svelte";
  import Inbox from "./Inbox.svelte";
  import Notices from "./Notices.svelte";
  import QuitConfirm from "./QuitConfirm.svelte";
  import Shortcuts from "./Shortcuts.svelte";
  import WorkItem from "./WorkItem.svelte";
  import AttentionPip, { URGENCY } from "./controls/AttentionPip.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import { homeDir } from "@tauri-apps/api/path";
  import { listenToAgents } from "./lib/agent.svelte";
  import { api, shortName, type Repo, type Row } from "./lib/api";
  import { ATTENTION_LABEL, attentionKind } from "./lib/attention";
  import { isTyping } from "./lib/keys";
  import { browser, layout, shortcuts } from "./lib/layout.svelte";
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
  // 1200 px it also steps aside once the review's side panel opens, and stays aside (whether
  // the panel is shown or not) until the user opens it again, so the layout doesn't jump.
  let inboxCollapsed = $state(false);
  let width = $state(window.innerWidth);
  let autoCollapsed = $state(false);
  let keepInboxOpen = $state(false);
  const crowded = $derived(width < 1200 && layout.reviewPanel);
  const collapsed = $derived(inboxCollapsed || autoCollapsed);
  $effect(() => {
    if (crowded && !keepInboxOpen) autoCollapsed = true;
  });
  $effect(() => {
    if (width >= 1200) autoCollapsed = keepInboxOpen = false;
  });

  function toggleInbox() {
    if (autoCollapsed && !inboxCollapsed) {
      autoCollapsed = false;
      keepInboxOpen = true;
      return;
    }
    inboxCollapsed = !inboxCollapsed;
    if (!inboxCollapsed && crowded) keepInboxOpen = true;
    api.setUiSetting("inboxCollapsed", String(inboxCollapsed));
  }

  // A refresh the user asked for is running: it ends with the next poll's result (the
  // inbox-changed or inbox-error event), or after two minutes at the latest.
  let refreshing = $state(false);
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  function refresh() {
    refreshing = true;
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => (refreshing = false), 120_000);
    api.refresh().catch((e) => {
      pollError = String(e);
      refreshing = false;
    });
  }

  // The Inbox's New task form.
  let creatingTask = $state(false);

  function newTask() {
    if (collapsed) toggleInbox();
    if (creatingTask) document.querySelector<HTMLInputElement>("[data-task-form] input")?.focus();
    creatingTask = true;
  }

  // Ctrl+B (Cmd+B) toggles the Inbox, Ctrl+N starts a new task, Ctrl+K browses the repo's open
  // PRs and issues and Ctrl+Shift+N opens a new window, except in the agent terminal, where the
  // keys belong to Claude Code. ? shows the shortcuts (also the review's), except while typing.
  function onKeydown(e: KeyboardEvent) {
    if (!e.ctrlKey && !e.metaKey && !e.altKey && !isTyping(e.target)) {
      if (e.key === "?") {
        e.preventDefault();
        shortcuts.open = !shortcuts.open;
      } else if (e.key === "Escape" && shortcuts.open) {
        e.preventDefault();
        shortcuts.open = false;
      }
      return;
    }
    if (!(e.ctrlKey || e.metaKey) || e.altKey || (e.target as Element | null)?.closest?.(".xterm")) return;
    const key = e.key.toLowerCase();
    if (!e.shiftKey && key === "b" && repo) {
      e.preventDefault();
      toggleInbox();
    } else if (!e.shiftKey && key === "n" && repo && !connecting) {
      e.preventDefault();
      newTask();
    } else if (!e.shiftKey && key === "k" && repo && !connecting) {
      e.preventDefault();
      // A new search each time; the kind (PRs or issues) stays.
      if (!browser.open) browser.text = "";
      browser.open = !browser.open;
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
    browser.open = false;
    selected = null;
    pollError = null;
    refreshing = false;
    selected = await api.selectedItem();
    reload();
  }

  // This window's repo was removed from Ariekei: back to the Connect screen.
  function removed() {
    repo = null;
    document.title = "Ariekei";
    rows = [];
    selected = null;
    pollError = null;
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
        refreshing = false;
        reload();
      }),
      listen<{ repo: number; message: string }>("inbox-error", (e) => {
        if (e.payload.repo !== repo?.id) return;
        pollError = e.payload.message;
        refreshing = false;
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

<!-- Coming back from another tool (e.g. a branch switched in an IDE) reads the rows again. -->
<svelte:window onkeydown={onKeydown} onfocus={reload} bind:innerWidth={width} />

{#if repo === null || connecting}
  <Connect onconnected={show} oncancel={repo ? () => (connecting = false) : undefined} />
{:else if repo}
  {#key repo.id}
  <div class="app">
    <Inbox {repo} {rows} {loaded} {selected} error={pollError} {collapsed} ontoggle={toggleInbox} onselect={select} onrefresh={refresh} {refreshing} onadded={reload}
      onswitch={show} onconnect={() => (connecting = true)} onremoved={removed} bind:creating={creatingTask} />
    {#if current}
      <WorkItem row={current} repoId={repo.id} viewer={repo.viewer} onchanged={reload} />
    {:else}
      <!-- What wants the user, most urgent first: the first five, to open from here. -->
      {@const wanting = rows
        .map((row) => ({ row, kind: attentionKind(row.attention) }))
        .filter((w) => w.kind !== "none" && w.kind !== "working")
        .sort((a, b) => URGENCY[a.kind] - URGENCY[b.kind])}
      <div class="empty">
        <div class="column">
          {#if loaded && !rows.length}
            <strong>Your Inbox is empty</strong>
            <span class="hint">What is assigned to you, your open pull requests, review requests and PRs you reviewed in {repo.owner}/{repo.name} appear here.
              To add any other open PR or issue, press <Kbd key="Ctrl+K" /> (Browse). To start work that has no PR or issue yet, press <Kbd key="Ctrl+N" /> (New task).</span>
          {:else if loaded}
            <strong>{wanting.length ? `${wanting.length} item${wanting.length === 1 ? " wants" : "s want"} your attention` : "You're all caught up"}</strong>
            {#if wanting.length}
              <ul class="wanting" data-wanting>
                {#each wanting.slice(0, 5) as { row, kind } (row.id)}
                  <li>
                    <button onclick={() => select(row.id)} data-want={row.number}>
                      <AttentionPip {kind} label="" />
                      <span class="num">{shortName(row)}</span>
                      <span class="title">{row.title}</span>
                      <span class="reason {kind}">{ATTENTION_LABEL[kind]}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
            <span class="hint">{wanting.length ? "Or pick an item in the Inbox." : "Pick an item in the Inbox."} <Kbd key="Ctrl+B" /> shows or hides the Inbox,
              <Kbd key="Ctrl+N" /> starts a new task.</span>
          {:else}
            <span class="hint">Loading your pull requests…</span>
          {/if}
        </div>
      </div>
    {/if}
  </div>
  {#if browser.open}
    <Browse onselect={select} onchanged={reload} />
  {/if}
  {/key}
{/if}
<Notices {selected} />
<Shortcuts />
<QuitConfirm />

<style>
  .app { display: flex; height: 100vh; min-height: 0; overflow: hidden; }
  .empty { flex: 1; display: flex; align-items: center; justify-content: center; padding: var(--sp-9); }
  .column { width: 100%; max-width: 440px; display: flex; flex-direction: column; align-items: center; gap: var(--sp-3); text-align: center; }
  .column strong { color: var(--text); font-size: var(--fs-13); line-height: var(--lh-13); font-weight: var(--fw-semibold); }
  .hint { color: var(--text-muted); font-size: var(--fs-12); line-height: var(--lh-12); }
  .wanting { list-style: none; margin: var(--sp-2) 0; padding: 0; width: 100%; display: flex; flex-direction: column; gap: 1px; text-align: left; }
  .wanting button {
    width: 100%; display: flex; align-items: center; gap: var(--sp-4); height: 32px; padding: 0 var(--sp-4);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); font-size: var(--fs-13);
  }
  .wanting button:hover { background: var(--surface-hover); }
  .wanting button:focus-visible { outline-offset: -2px; }
  .num { flex: none; font-family: var(--font-mono); font-size: var(--fs-code-inline); color: var(--text-muted); }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .reason { flex: none; font-size: var(--fs-12); font-weight: var(--fw-medium); color: var(--text-muted); }
  .reason.needs-you { color: var(--agent-strong); }
  .reason.blocked, .reason.ci-failing { color: var(--bad); }
  .reason.activity { color: var(--gh-strong); }
  .reason.review { color: var(--you-strong); }
</style>
