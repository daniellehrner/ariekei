<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount, untrack } from "svelte";
  import Comments from "./Comments.svelte";
  import Overview from "./Overview.svelte";
  import ReviewView, { type ReviewViewState } from "./ReviewView.svelte";
  import TerminalPanel from "./TerminalPanel.svelte";
  import { agents } from "./lib/agent.svelte";
  import { api, type Conversation, type Row } from "./lib/api";

  let { row, repoId, onchanged }: { row: Row; repoId: number; onchanged: () => void } = $props();

  type Tab = "overview" | "comments" | "review" | "terminal";
  // Per-item view state, stored with the work item so switching back restores it.
  interface ItemUi {
    tab: Tab;
    review: ReviewViewState;
  }
  const fresh = (): ItemUi => ({ tab: "overview", review: { scope: "", file: null, comments: [] } });

  let ui = $state<ItemUi | null>(null);
  let conv = $state<Conversation | null>(null);
  let convError: string | null = $state(null);
  // Without a worktree the review reads the PR head fetched into the clone.
  let fetched = $state(false);
  let preparing = $state(false);
  let error: string | null = $state(null);
  let notice: string | null = $state(null);

  const id = $derived(row.id);
  const agent = $derived(agents[String(row.id)]);
  const tab = $derived(ui && ui.tab === "terminal" && !row.worktree ? "overview" : ui?.tab);

  async function loadConversation(current: number) {
    try {
      const c = await api.conversation(current);
      if (current === id) {
        conv = c;
        convError = null;
      }
    } catch (e) {
      if (current === id) convError = String(e);
    }
  }

  async function fetchHead(current: number) {
    try {
      await api.fetchPr(current);
      if (current === id) fetched = true;
    } catch (e) {
      if (current === id) error = `Could not fetch the PR head: ${e}`;
    }
  }

  // Load everything for the item whenever another item is shown. Only `id` is tracked:
  // `row` is a new object after every Inbox reload, which must not reset the view.
  $effect(() => {
    const current = id;
    untrack(() => load(current));
  });

  function load(current: number) {
    ui = null;
    conv = null;
    convError = error = notice = null;
    fetched = false;
    api.uiState<ItemUi>(current).then((saved) => {
      if (current === id) ui = { ...fresh(), ...saved, review: { ...fresh().review, ...saved?.review } };
    });
    api.markSeen(current).then(onchanged);
    api.setActiveItem(current);
    loadConversation(current);
    if (!row.worktree && row.kind === "pr") fetchHead(current);
  }

  // GitHub changes (including the user's own comments) arrive with the next poll.
  onMount(() => {
    // Every window hears every repo's polls; only this repo's matter here.
    const unlisten = listen<number>("inbox-changed", (e) => {
      if (e.payload === repoId) loadConversation(id);
    });
    return () => unlisten.then((f) => f());
  });

  // Persist view state (debounced) as it changes.
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    if (!ui) return;
    const snapshot = JSON.parse(JSON.stringify(ui));
    const target = id;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => api.setUiState(target, snapshot), 300);
  });

  async function start() {
    preparing = true;
    error = null;
    try {
      const p = await api.prepareItem(id);
      notice = p.adopted ? `Using the existing worktree ${p.path}` : `Created ${p.path} on ${p.branch}`;
      if (p.note) error = p.note;
      if (ui) ui.tab = "terminal";
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      preparing = false;
    }
  }

  async function sendPrFeedback() {
    try {
      await api.sendPrFeedback(id);
      notice = "Sent the PR's review threads and CI state to the agent.";
    } catch (e) {
      error = String(e);
    }
  }

  // Done: clean up now (same safety checks as the automatic cleanup).
  let blockers = $state<string[] | null>(null);
  let confirmDone = $state(false);
  async function done() {
    if (!confirmDone) {
      confirmDone = true;
      return;
    }
    confirmDone = false;
    try {
      const result = await api.markDone(id);
      if (result.outcome === "blocked") {
        blockers = result.blockers;
      } else {
        blockers = null;
        notice = result.removed.length ? `Cleaned up: ${result.removed.join(", ")}.` : "Marked done.";
        onchanged();
      }
    } catch (e) {
      error = String(e);
    }
  }

  function changed() {
    loadConversation(id);
    onchanged();
  }

  const CI_LABEL: Record<string, string> = { SUCCESS: "CI passing", FAILURE: "CI failing", ERROR: "CI error", PENDING: "CI running", EXPECTED: "CI expected" };
  const TABS: [Tab, string][] = [["overview", "Overview"], ["comments", "Comments"], ["review", "Review"], ["terminal", "Terminal"]];
</script>

<section class="item">
  <header>
    <div class="title">
      <span class="num">#{row.number}</span>
      <span>{row.title}</span>
      <span class="spacer"></span>
      {#if confirmDone}
        <span class="confirm">Clean up {row.worktree ? "the worktree, agent and " : ""}this item?</span>
        <button onclick={done}>Yes, clean up</button>
        <button onclick={() => (confirmDone = false)}>Cancel</button>
      {:else}
        <button onclick={done} title="Remove this item's worktree, agent session and refs, and hide it — only if nothing would be lost">Done</button>
      {/if}
      {#if !row.worktree}
        <button class="primary" onclick={start} disabled={preparing}
          title="Create (or reuse) a worktree for the PR's branch and open Claude Code in it with the PR as context">
          {preparing ? "Preparing the worktree…" : "Start working"}
        </button>
      {/if}
    </div>
    <div class="meta">
      {#if row.kind === "issue"}<span>issue</span>{/if}
      {#if row.issueNumber}<span>from issue #{row.issueNumber}</span>{/if}
      {#if row.role !== "author"}<span>by {row.author}</span>{/if}
      {#if row.ci}<span class="ci {row.ci}">{CI_LABEL[row.ci] ?? row.ci}{row.failingChecks.length ? `: ${row.failingChecks.join(", ")}` : ""}</span>{/if}
      {#if row.reviewDecision}<span>{row.reviewDecision.toLowerCase().replaceAll("_", " ")}</span>{/if}
      {#if row.unresolvedThreads}<span>{row.unresolvedThreads} unresolved thread{row.unresolvedThreads > 1 ? "s" : ""}</span>{/if}
      {#if row.worktree}<span class="path" title={row.worktree}>{row.worktree}</span>{/if}
      {#if agent?.detail}<span class="detail">{agent.detail}</span>{/if}
    </div>
  </header>

  {#if error}<p class="error">{error}</p>{/if}
  {#if notice}<p class="notice">{notice}</p>{/if}
  {#if blockers ?? row.cleanupBlockers}
    <div class="blocked">
      <strong>{row.open ? "Not cleaned up" : "Finished, but not cleaned up"}</strong> — this would lose work:
      <ul>{#each blockers ?? row.cleanupBlockers ?? [] as b}<li>{b}</li>{/each}</ul>
      Resolve these (commit and push, or discard), then press Done.
    </div>
  {/if}

  {#if ui}
    <nav>
      {#each TABS as [value, label]}
        {#if (value !== "terminal" || row.worktree) && (value !== "review" || row.kind === "pr" || row.worktree)}
          <button class:on={tab === value} onclick={() => ui && (ui.tab = value)}>
            {label}{#if value === "comments" && conv}<span class="badge">{conv.threads.filter((t) => !t.resolved).length || ""}</span>{/if}
          </button>
        {/if}
      {/each}
      <span class="spacer"></span>
      {#if row.worktree && row.kind === "pr"}
        <button onclick={sendPrFeedback} title="Send the PR's current review threads, review summaries and failing checks to the agent">Send PR feedback to agent</button>
      {/if}
    </nav>
    <div class="pane">
      {#key id}
        {#if row.worktree}
          <!-- Stays mounted under the other tabs so the terminal keeps running in view. -->
          <TerminalPanel {id} visible={tab === "terminal"} />
        {/if}
        {#if tab === "overview" || tab === "comments"}
          {#if conv}
            {#if tab === "overview"}<Overview {conv} />{:else}<Comments {id} {conv} onchanged={changed} />{/if}
          {:else}
            <p class="hint">{convError ? `Could not load the PR from GitHub: ${convError}` : "Loading the pull request…"}</p>
          {/if}
        {:else if tab === "review"}
          {#if row.worktree || fetched}
            <ReviewView {id} bind:view={ui.review} onchanged={changed} canSendToAgent={!!row.worktree} />
          {:else}
            <p class="hint">Fetching the PR head…</p>
          {/if}
        {/if}
      {/key}
    </div>
  {/if}
</section>

<style>
  .item { flex: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  header { padding: 10px 14px 8px; border-bottom: 1px solid var(--border); }
  .title { font-size: 15px; font-weight: 600; display: flex; gap: 8px; align-items: center; }
  .title button { font-weight: 400; font-size: 13px; }
  .num { color: var(--muted); }
  .meta { display: flex; gap: 12px; flex-wrap: wrap; color: var(--muted); font-size: 0.85em; margin-top: 4px; }
  .ci.FAILURE, .ci.ERROR { color: var(--bad); }
  .ci.SUCCESS { color: var(--ok); }
  .path { font-family: var(--mono); font-size: 0.95em; }
  .detail { margin-left: auto; }
  .primary { border-color: var(--accent); }
  nav { display: flex; gap: 4px; align-items: center; padding: 6px 10px; border-bottom: 1px solid var(--border); }
  nav button { border: 1px solid transparent; background: none; padding: 4px 12px; border-radius: 4px; }
  nav button.on { background: var(--panel); border-color: var(--border); }
  .badge { color: var(--warn); margin-left: 4px; font-size: 0.85em; }
  .spacer { flex: 1; }
  .pane { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .hint { color: var(--muted); margin: 24px; }
  .error { color: var(--bad); margin: 6px 14px; }
  .confirm { font-weight: 400; font-size: 13px; color: var(--warn); }
  .blocked { margin: 6px 14px; padding: 8px 12px; border: 1px solid var(--warn); border-radius: 6px; background: var(--warn-bg); font-size: 0.9em; }
  .blocked ul { margin: 4px 0; padding-left: 20px; }
  .notice { color: var(--ok); margin: 6px 14px; font-size: 0.9em; }
</style>
