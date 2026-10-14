<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount, untrack } from "svelte";
  import Comments from "./Comments.svelte";
  import Overview from "./Overview.svelte";
  import ReviewView, { type ReviewViewState } from "./ReviewView.svelte";
  import TerminalPanel from "./TerminalPanel.svelte";
  import Banner from "./controls/Banner.svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import InlineConfirm from "./controls/InlineConfirm.svelte";
  import StatusDot, { AGENT_LABEL } from "./controls/StatusDot.svelte";
  import Spinner from "./controls/Spinner.svelte";
  import { agents } from "./lib/agent.svelte";
  import { api, type Conversation, type Row } from "./lib/api";
  import { landingTab, type Tab } from "./lib/attention";
  import { shortPath } from "./lib/paths";
  import { progress, rangeOf, scopeOf } from "./lib/review";

  let { row, repoId, viewer, onchanged }: { row: Row; repoId: number; viewer: string; onchanged: () => void } = $props();

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
  // Files of the review's scope marked reviewed, for the Review tab's label.
  let reviewProgress = $state<{ reviewed: number; total: number } | null>(null);

  const id = $derived(row.id);
  const agent = $derived(agents[String(row.id)]);
  // A terminal shows the agent of a running item; the status comes from its hooks.
  const status = $derived(row.worktree ? (agent?.status ?? row.agent) : null);
  const hasReview = $derived(row.kind === "pr" || !!row.worktree);
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

  // The Review tab's "7/12": the files of the scope the review view would show.
  async function loadProgress(current: number) {
    try {
      const state = await api.reviewState(current);
      const view = ui?.review;
      const [from, to] = rangeOf(state, scopeOf(state, view?.scope ?? ""));
      const files = await api.reviewFiles(current, from, to, !!view?.ignoreWs);
      if (current === id) reviewProgress = files.length ? progress(files) : null;
    } catch {
      if (current === id) reviewProgress = null;
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
    blockers = null;
    reviewProgress = null;
    confirmDone = false;
    // Open on the tab that resolves why the item wants attention, else where the user was.
    const landing = landingTab(row.attention, { terminal: !!row.worktree, review: hasReview });
    api.uiState<ItemUi>(current).then((saved) => {
      if (current !== id) return;
      ui = { ...fresh(), ...saved, review: { ...fresh().review, ...saved?.review } };
      if (landing) ui.tab = landing;
      if (hasReview && (row.worktree || row.kind === "pr")) loadProgress(current);
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
      notice = p.adopted ? `Using the existing worktree ${shortPath(p.path)}.` : `Created the worktree ${shortPath(p.path)} on ${p.branch}.`;
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
  let blockersDismissed = $state(false);
  let confirmDone = $state(false);
  async function done() {
    confirmDone = false;
    try {
      const result = await api.markDone(id);
      if (result.outcome === "blocked") {
        blockers = result.blockers;
        blockersDismissed = false;
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

  // From a review thread to its file in the Review tab.
  function showInReview(path: string) {
    if (!ui) return;
    ui.review.file = path;
    ui.tab = "review";
  }

  const shownBlockers = $derived(blockersDismissed ? null : (blockers ?? row.cleanupBlockers));
  const ciFailing = $derived(row.ci === "FAILURE" || row.ci === "ERROR");
  const ciLabel = $derived.by(() => {
    if (!row.ci) return null;
    if (row.ci === "SUCCESS") return "CI passing";
    if (ciFailing) return `CI failing${row.failingChecks.length ? `: ${row.failingChecks.join(", ")}` : ""}`;
    return "CI running";
  });
  const DECISION: Record<string, [string, string]> = {
    APPROVED: ["approved", "ok"],
    CHANGES_REQUESTED: ["changes requested", "warn"],
    REVIEW_REQUIRED: ["review required", ""],
  };
  const unresolved = $derived(conv ? conv.threads.filter((t) => !t.resolved).length : 0);
  const TABS: [Tab, string][] = [["overview", "Overview"], ["comments", "Comments"], ["review", "Review"], ["terminal", "Terminal"]];
</script>

<main class="item">
  <header>
    <div class="title-row">
      <span class="kind"><Icon name={row.kind === "pr" ? "pull-request" : "issue"} label={row.kind === "pr" ? "Pull request" : "Issue"} /></span>
      <h1 class="title"><span class="num">#{row.number}</span>{row.title}</h1>
      <div class="actions">
        {#if preparing}
          <span class="preparing" role="status"><Spinner />Preparing the worktree…</span>
        {:else if !row.worktree}
          <Button variant="primary" icon="play" onclick={start}
            title="Create (or reuse) a worktree for the PR's branch and open Claude Code in it with the PR as context" data-start>Start working</Button>
        {/if}
        {#if confirmDone}
          <InlineConfirm text="Clean up {row.worktree ? 'the worktree, agent and ' : ''}this item?" confirm="Clean up"
            onconfirm={done} oncancel={() => (confirmDone = false)} data-confirm-done />
        {:else}
          <Button icon="done" onclick={() => (confirmDone = true)}
            title="Remove this item's worktree, agent session and refs, and hide it — only if nothing would be lost" data-done>Done</Button>
        {/if}
      </div>
    </div>
    <div class="meta" data-meta>
      <span>{row.kind === "pr" ? "Pull request" : "Issue"}</span>
      {#if row.issueNumber}<span>from issue #{row.issueNumber}</span>{/if}
      <span>by {row.author}</span>
      {#if ciLabel}
        <span class={row.ci === "SUCCESS" ? "ok" : ciFailing ? "bad" : "warn"}>
          <Icon name={row.ci === "SUCCESS" ? "check" : ciFailing ? "x" : "pending"} size={12} />{ciLabel}
        </span>
      {/if}
      {#if row.reviewDecision}
        {@const d = DECISION[row.reviewDecision] ?? [row.reviewDecision.toLowerCase(), ""]}
        <span class={d[1]}>{d[0]}</span>
      {/if}
      {#if row.unresolvedThreads}<span class="gh"><Icon name="comment" size={12} />{row.unresolvedThreads} unresolved</span>{/if}
      {#if row.worktree}<span class="path" title={row.worktree}><Icon name="folder" size={12} /><code>{shortPath(row.worktree)}</code></span>{/if}
      {#if status}
        <span class="agent" data-agent-status={status}>
          <StatusDot {status} size={12} /><span class="t">{AGENT_LABEL[status].replace(/^Agent /, "").replace(/^./, (c) => c.toUpperCase())}{agent?.detail && status !== "waitingForPermission" ? ` — ${agent.detail}` : ""}</span>
        </span>
      {/if}
    </div>
  </header>

  {#if status === "waitingForPermission"}
    <div class="waiting" role="status" data-waiting>
      <StatusDot status="waitingForPermission" />
      <span class="what">
        <strong>The agent is waiting for your permission</strong>{#if agent?.detail}<span class="muted">{" · "}</span><code>{agent.detail}</code>{/if}
      </span>
      {#if tab !== "terminal"}
        <Button variant="agent" size="sm" icon="terminal" onclick={() => ui && (ui.tab = "terminal")} data-answer>Answer in Terminal</Button>
      {/if}
    </div>
  {/if}
  {#if shownBlockers}
    <div class="banner">
      <Banner tone="bad" icon="blocked" compact={false} data-blocked>
        <strong>{row.open ? "Not cleaned up" : "Finished, but not cleaned up"}, so no work is lost:</strong>
        <ul>{#each shownBlockers as b}<li>{b}</li>{/each}</ul>
        <span class="muted">Commit and push these, or discard them, then press Done again.</span>
        {#snippet action()}<IconButton icon="x" label="Dismiss" size={24} onclick={() => (blockersDismissed = true)} />{/snippet}
      </Banner>
    </div>
  {/if}
  {#if error}
    <div class="banner">
      <Banner tone="bad" data-error>
        {error}
        {#snippet action()}<IconButton icon="x" label="Dismiss" size={22} onclick={() => (error = null)} />{/snippet}
      </Banner>
    </div>
  {/if}
  {#if notice}
    <div class="banner">
      <Banner tone="info" data-notice>
        {notice}
        {#snippet action()}<IconButton icon="x" label="Dismiss" size={22} onclick={() => (notice = null)} />{/snippet}
      </Banner>
    </div>
  {/if}

  {#if ui}
    <div class="tabs" role="tablist" aria-label="Work item">
      {#each TABS as [value, label]}
        {#if (value !== "terminal" || row.worktree) && (value !== "review" || hasReview)}
          <button role="tab" aria-selected={tab === value} class:on={tab === value} onclick={() => ui && (ui.tab = value)} data-tab={value}>
            {#if value === "terminal"}<StatusDot {status} size={10} />{/if}
            <span>{label}</span>
            {#if value === "comments" && unresolved}<span class="badge" title="Unresolved review threads">{unresolved}</span>{/if}
            {#if value === "review" && reviewProgress}
              <span class="sub" class:complete={reviewProgress.reviewed === reviewProgress.total} title="Files marked reviewed">{reviewProgress.reviewed}/{reviewProgress.total}</span>
            {/if}
          </button>
        {/if}
      {/each}
      <span class="spacer"></span>
      {#if row.worktree && row.kind === "pr"}
        <Button variant="quiet" icon="send-to-agent" onclick={sendPrFeedback}
          title="Send the PR's current review threads, review summaries and failing checks to the agent">Send PR feedback to agent</Button>
      {/if}
    </div>
    <div class="pane">
      {#key id}
        {#if row.worktree}
          <!-- Stays mounted under the other tabs so the terminal keeps running in view. -->
          <TerminalPanel {id} visible={tab === "terminal"} {status} path={row.worktree} />
        {/if}
        {#if tab === "overview" || tab === "comments"}
          {#if conv}
            {#if tab === "overview"}<Overview {conv} />{:else}<Comments {id} {conv} {viewer} onchanged={changed} onshow={hasReview ? showInReview : undefined} />{/if}
          {:else}
            <p class="hint">{convError ? `Could not load the ${row.kind === "pr" ? "pull request" : "issue"} from GitHub: ${convError}` : "Loading…"}</p>
          {/if}
        {:else if tab === "review"}
          {#if row.worktree || fetched}
            <ReviewView {id} bind:view={ui.review} onchanged={changed} canSendToAgent={!!row.worktree}
              onprogress={(p) => (reviewProgress = p.total ? p : null)} />
          {:else}
            <p class="hint"><Icon name="lock" size={14} />Read-only · Fetching the PR head…</p>
          {/if}
        {/if}
      {/key}
    </div>
  {/if}
</main>

<style>
  .item { flex: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; position: relative; }
  header { flex: none; display: flex; flex-direction: column; gap: var(--sp-1); padding: var(--sp-5) var(--sp-7) 0; }
  .title-row { display: flex; align-items: center; gap: var(--sp-5); min-width: 0; min-height: var(--h-control); }
  .kind { display: flex; color: var(--text-muted); }
  .title {
    flex: 1; min-width: 0; margin: 0; font-size: var(--fs-15); line-height: var(--lh-15); font-weight: var(--fw-semibold);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .num { font-family: var(--font-mono); font-weight: var(--fw-medium); color: var(--text-muted); margin-right: var(--sp-3); }
  .actions { flex: none; display: flex; align-items: center; gap: var(--sp-3); }
  .preparing { display: inline-flex; align-items: center; gap: var(--sp-3); font-size: var(--fs-12); color: var(--text-muted); }
  .meta {
    display: flex; align-items: center; gap: var(--sp-6); min-width: 0; padding-left: 26px;
    font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted); white-space: nowrap; overflow: hidden;
  }
  .meta > span { display: inline-flex; align-items: center; gap: var(--sp-2); flex: none; }
  /* The worktree path gives way first, so the agent's status (the last chip) stays visible. */
  .meta > span.path { flex: 0 100 auto; min-width: 6em; overflow: hidden; }
  .meta > span.agent { flex: none; max-width: 45%; color: var(--agent-strong); }
  .meta .t, .meta > span.path code { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .meta code { font-size: 11.5px; }
  .ok { color: var(--ok); }
  .bad { color: var(--bad); }
  .warn { color: var(--warn); }
  .gh { color: var(--gh-strong); }
  .muted { color: var(--text-muted); }

  .waiting {
    flex: none; margin: var(--sp-4) var(--sp-7) 0; display: flex; align-items: center; gap: var(--sp-5);
    padding: var(--sp-3) var(--sp-3) var(--sp-3) var(--sp-5); min-height: 24px;
    border-radius: var(--r-lg); background: var(--agent-bg); box-shadow: inset 0 0 0 1px var(--agent-line);
  }
  .waiting .what { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: var(--fs-12); }
  .waiting strong { font-weight: var(--fw-semibold); color: var(--agent-strong); }
  .waiting code { font-size: 11.5px; }
  .banner { flex: none; margin: var(--sp-4) var(--sp-7) 0; }

  .tabs {
    flex: none; display: flex; align-items: flex-end; gap: var(--sp-1); height: var(--h-tabbar); box-sizing: border-box;
    padding: 0 var(--sp-6); border-bottom: 1px solid var(--border);
  }
  [role="tab"] {
    position: relative; display: inline-flex; align-items: center; gap: var(--sp-3); height: 34px; padding: 0 var(--sp-5);
    border: 0; border-radius: 0; background: transparent; color: var(--text-muted); font-weight: var(--fw-medium);
  }
  [role="tab"]:hover { color: var(--text); }
  [role="tab"].on { color: var(--text); font-weight: var(--fw-semibold); box-shadow: inset 0 -2px 0 var(--text); }
  [role="tab"]:focus-visible { outline-offset: -2px; }
  .badge {
    min-width: 16px; height: 16px; padding: 0 var(--sp-2); box-sizing: border-box; border-radius: 8px;
    background: var(--gh-bg); color: var(--gh-strong); font-size: var(--fs-11); line-height: 16px; font-weight: var(--fw-semibold); text-align: center;
  }
  .sub { font-size: 11.5px; font-weight: var(--fw-medium); color: var(--text-faint); }
  .sub.complete { color: var(--you-strong); }
  .tabs > :global(.btn) { align-self: center; }
  .spacer { flex: 1; }
  .pane { flex: 1; min-height: 0; display: flex; flex-direction: column; position: relative; }
  .hint { display: flex; align-items: center; gap: var(--sp-3); color: var(--text-muted); margin: var(--sp-9); font-size: var(--fs-12); }
</style>
