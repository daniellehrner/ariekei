<script lang="ts">
  import ReviewView, { type ReviewViewState } from "./ReviewView.svelte";
  import TerminalPanel from "./TerminalPanel.svelte";
  import { agents } from "./lib/agent.svelte";
  import { api, type Row } from "./lib/api";

  let { row, onchanged }: { row: Row; onchanged: () => void } = $props();

  // Per-item view state, stored with the work item so switching back restores it.
  interface ItemUi {
    tab: "terminal" | "review";
    review: ReviewViewState;
  }
  const fresh = (): ItemUi => ({ tab: "terminal", review: { scope: "review", file: null, comments: [] } });

  let ui: ItemUi | null = $state(null);
  let preparing = $state(false);
  let error: string | null = $state(null);
  let notice: string | null = $state(null);

  const id = $derived(row.id);
  const agent = $derived(agents[String(row.id)]);

  // Load the item's saved view state whenever another item is shown.
  $effect(() => {
    const current = id;
    ui = null;
    error = notice = null;
    api.uiState<ItemUi>(current).then((saved) => {
      if (current === id) ui = { ...fresh(), ...saved, review: { ...fresh().review, ...saved?.review } };
    });
    api.markSeen(current).then(onchanged);
    api.setActiveItem(current);
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

  const CI_LABEL: Record<string, string> = { SUCCESS: "CI passing", FAILURE: "CI failing", ERROR: "CI error", PENDING: "CI running", EXPECTED: "CI expected" };
</script>

<section class="item">
  <header>
    <div class="title">
      <span class="num">#{row.number}</span>
      <span>{row.title}</span>
    </div>
    <div class="meta">
      {#if row.ci}<span class="ci {row.ci}">{CI_LABEL[row.ci] ?? row.ci}{row.failingChecks.length ? `: ${row.failingChecks.join(", ")}` : ""}</span>{/if}
      {#if row.reviewDecision}<span>{row.reviewDecision.toLowerCase().replaceAll("_", " ")}</span>{/if}
      {#if row.unresolvedThreads}<span>{row.unresolvedThreads} unresolved thread{row.unresolvedThreads > 1 ? "s" : ""}</span>{/if}
      <span class="url">{row.url}</span>
      {#if row.worktree}<span class="path" title={row.worktree}>{row.worktree}</span>{/if}
      {#if agent?.detail}<span class="detail">{agent.detail}</span>{/if}
    </div>
  </header>

  {#if error}<p class="error">{error}</p>{/if}
  {#if notice}<p class="notice">{notice}</p>{/if}

  {#if !row.worktree}
    <div class="start">
      <p>This pull request has no working copy yet. Starting creates (or reuses) a worktree for its branch and opens Claude Code in it with the PR's review threads and CI failures as context.</p>
      <button class="primary" onclick={start} disabled={preparing}>{preparing ? "Preparing the worktree…" : "Start working"}</button>
    </div>
  {:else if ui}
    <nav>
      <button class:on={ui.tab === "terminal"} onclick={() => ui && (ui.tab = "terminal")}>Terminal</button>
      <button class:on={ui.tab === "review"} onclick={() => ui && (ui.tab = "review")}>Review</button>
      <span class="spacer"></span>
      <button onclick={sendPrFeedback} title="Send the PR's current review threads, review summaries and failing checks to the agent">Send PR feedback to agent</button>
    </nav>
    <div class="pane">
      {#key id}
        <!-- Stays mounted under the Review tab so the terminal keeps running in view. -->
        <TerminalPanel {id} visible={ui.tab === "terminal"} />
        {#if ui.tab === "review"}
          <ReviewView {id} bind:view={ui.review} {onchanged} />
        {/if}
      {/key}
    </div>
  {/if}
</section>

<style>
  .item { flex: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  header { padding: 10px 14px 8px; border-bottom: 1px solid var(--border); }
  .title { font-size: 15px; font-weight: 600; display: flex; gap: 8px; }
  .num { color: var(--muted); }
  .meta { display: flex; gap: 12px; flex-wrap: wrap; color: var(--muted); font-size: 0.85em; margin-top: 4px; }
  .ci.FAILURE, .ci.ERROR { color: var(--bad); }
  .ci.SUCCESS { color: var(--ok); }
  .url, .path { font-family: var(--mono); font-size: 0.95em; }
  .detail { margin-left: auto; }
  .start { max-width: 560px; margin: 40px auto; padding: 0 16px; }
  .primary { border-color: var(--accent); }
  nav { display: flex; gap: 4px; align-items: center; padding: 6px 10px; border-bottom: 1px solid var(--border); }
  nav button { border: 1px solid transparent; background: none; padding: 4px 12px; border-radius: 4px; }
  nav button.on { background: var(--panel); border-color: var(--border); }
  .spacer { flex: 1; }
  .pane { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .error { color: var(--bad); margin: 6px 14px; }
  .notice { color: var(--ok); margin: 6px 14px; font-size: 0.9em; }
</style>
