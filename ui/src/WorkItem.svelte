<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { onMount, untrack } from "svelte";
  import Comments from "./Comments.svelte";
  import Overview from "./Overview.svelte";
  import TaskOverview from "./TaskOverview.svelte";
  import ReviewView, { type ReviewViewState } from "./ReviewView.svelte";
  import TerminalPanel from "./TerminalPanel.svelte";
  import Banner from "./controls/Banner.svelte";
  import Button from "./controls/Button.svelte";
  import CiMark, { ciState } from "./controls/CiMark.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import InlineConfirm from "./controls/InlineConfirm.svelte";
  import StatusDot, { AGENT_LABEL } from "./controls/StatusDot.svelte";
  import Spinner from "./controls/Spinner.svelte";
  import StarToggle from "./controls/StarToggle.svelte";
  import { agents } from "./lib/agent.svelte";
  import { api, shortName, type Conversation, type Row } from "./lib/api";
  import { openingTab, type Tab } from "./lib/attention";
  import { newestLoad } from "./lib/latest";
  import { openOn } from "./lib/layout.svelte";

  let {
    row,
    repoId,
    viewer,
    onchanged,
  }: {
    row: Row;
    repoId: number;
    /** The user's GitHub login. */
    viewer: string;
    onchanged: () => void;
  } = $props();

  // Per-item view state, stored with the work item so switching back restores it.
  interface ItemUi {
    tab: Tab;
    review: ReviewViewState;
  }
  const fresh = (): ItemUi => ({ tab: "overview", review: { scope: "", file: null } });

  let ui = $state<ItemUi | null>(null);
  // The item `ui` is the view of.
  let uiOf = 0;
  let conv = $state<Conversation | null>(null);
  let convError: string | null = $state(null);
  // One that took longer than a later one must not replace what that showed.
  const convLoads = newestLoad();
  // How far the user had seen the item's activity when they opened it: Comments marks what is
  // newer as new.
  let since: string | null = $state(null);
  let preparing = $state(false);
  // Polls of this repo since the item opened: the review checks GitHub for newer after each.
  let polls = $state(0);
  // Times the user started the agent again after it exited (a new terminal view each time).
  let restarts = $state(0);
  let error: string | null = $state(null);
  // Lifecycle news ("Created the worktree …"); `code` spans are written `like this`. It goes by
  // itself after a while: it is news, nothing to act on.
  let notice: string | null = $state(null);
  const NOTICE_MS = 6_000;
  $effect(() => {
    if (!notice) return;
    const timer = setTimeout(() => (notice = null), NOTICE_MS);
    return () => clearTimeout(timer);
  });
  // The last part of a path: a worktree is named by its folder (the full path is its title).
  const folderName = (path: string) => path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? path;

  // Clicking the worktree chip copies its full path, and says so for a moment.
  let copied = $state(false);
  async function copyPath(path: string) {
    try {
      await writeText(path);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      error = `Could not copy the path: ${e}`;
    }
  }

  const id = $derived(row.id);
  const agent = $derived(agents[String(row.id)]);
  // A terminal shows the agent of a running item; the status comes from its hooks.
  const status = $derived(row.worktree ? (agent?.status ?? row.agent) : null);
  const hasReview = $derived(row.kind === "pr" || !!row.worktree);
  const isTask = $derived(row.kind === "task");
  const tab = $derived(ui && ui.tab === "terminal" && !row.worktree ? "overview" : ui?.tab);
  // The agent's worktree was switched to another branch (it is often the user's own
  // checkout): Ariekei sends the agent nothing until it is back, or its work would land
  // there. Its terminal still takes what the user types; the item can follow the branch.
  const moved = $derived.by(() => {
    if (row.movedTo == null || !row.worktree || !row.branch) return null;
    return { id: row.id, folder: folderName(row.worktree), now: row.movedTo, branch: row.branch, follow: row.canFollow };
  });
  const agentAway = $derived(moved && `${moved.folder} is on ${moved.now || "a detached HEAD"}, not on ${moved.branch}`);

  async function loadConversation(current: number) {
    if (isTask) return;
    const n = convLoads.start();
    try {
      const c = await api.conversation(current);
      if (current !== id || !convLoads.succeeded(n)) return;
      conv = c;
      convError = null;
      // What the user sees now is seen: the Inbox no longer says it has new activity (a poll
      // with new comments says so while the item is open).
      api.markSeen(current).then((s) => {
        if (s.before !== s.after) onchanged();
      }, () => {});
    } catch (e) {
      if (current === id && convLoads.failed(n)) convError = String(e);
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
    since = null;
    blockers = null;
    confirmDone = false;
    losses = null;
    // The previous item's view, if its save is still waiting.
    saveUi();
    const attention = row.attention;
    const terminal = !!row.worktree;
    api.uiState<ItemUi>(current).then((saved) => {
      if (current !== id) return;
      uiOf = current;
      ui = { ...fresh(), ...saved, review: { ...fresh().review, ...saved?.review } };
      // A new task opens on its agent (shown once the row has the worktree: a reload that
      // started while the task was being created can still lack it).
      if (!saved && isTask) ui.tab = "terminal";
      // Where the user left it; opened for the first time, where its attention reason is dealt with.
      const opening = openingTab(saved?.tab, attention, { terminal });
      if (opening) ui.tab = opening;
      // Asked for (a notice from the agent, asking it about code); a Terminal only where the
      // agent runs.
      if (openOn.item === current && openOn.tab && (openOn.tab !== "terminal" || row.worktree)) ui.tab = openOn.tab;
      const starting = openOn.item === current && openOn.start && !row.worktree;
      openOn.item = openOn.tab = null;
      openOn.start = false;
      if (starting) start();
    });
    api.markSeen(current).then((s) => {
      if (current === id) since = s.before;
      onchanged();
    });
    api.setActiveItem(current);
    // As it was last loaded, at once; the current one replaces it when GitHub answers.
    if (!isTask) {
      api.cachedConversation(current).then((c) => {
        if (current === id && c && !conv) conv = c;
      }, () => {});
    }
    loadConversation(current);
  }

  // GitHub changes (including the user's own comments) arrive with the next poll.
  onMount(() => {
    // Every window hears every repo's polls; only this repo's matter here.
    const unlisten = listen<number>("inbox-changed", (e) => {
      if (e.payload !== repoId) return;
      loadConversation(id);
      polls++;
    });
    return () => unlisten.then((f) => f());
  });

  // Persist view state (debounced) as it changes.
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let unsaved: (() => void) | null = null;
  $effect(() => {
    if (!ui) return;
    const snapshot = JSON.parse(JSON.stringify(ui));
    // Not `id`: when another item is shown, this may run before `load` cleared the view.
    const target = uiOf;
    clearTimeout(saveTimer);
    unsaved = () => api.setUiState(target, snapshot);
    saveTimer = setTimeout(saveUi, 300);
  });
  // Saves the view now if a save is waiting: switching to another item must not drop it.
  function saveUi() {
    clearTimeout(saveTimer);
    unsaved?.();
    unsaved = null;
  }
  onMount(() => saveUi);

  // Putting the agent's worktree right (Switch back, Move here, Update): one at a time.
  let fixing: "switch" | "move" | "follow" | "update" | null = $state(null);
  async function fix(what: "switch" | "move" | "follow" | "update", f: () => Promise<unknown>) {
    fixing = what;
    error = null;
    try {
      await f();
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      fixing = null;
    }
  }

  async function start() {
    preparing = true;
    error = null;
    try {
      const p = await api.prepareItem(id);
      notice = p.adopted ? `Using the existing worktree \`${folderName(p.path)}\`.` : `Created the worktree \`${folderName(p.path)}\` on \`${p.branch}\`.`;
      if (p.note) error = p.note;
      if (ui) ui.tab = "terminal";
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      preparing = false;
    }
  }

  // The terminal of the agent that gets what is sent.
  function showAgent() {
    if (ui) ui.tab = "terminal";
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

  // Finished while open: the user keeps it until Done instead of the automatic cleanup.
  let keeping = $state(false);
  async function keep() {
    keeping = true;
    try {
      await api.keepItem(id);
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      keeping = false;
    }
  }

  // Discard (tasks only): delete the task with its worktree and branch, after the user saw
  // what that loses.
  let losses = $state<string[] | null>(null);
  async function askDiscard() {
    try {
      losses = await api.discardLosses(id);
    } catch (e) {
      error = String(e);
    }
  }
  async function discard() {
    const what = losses;
    losses = null;
    try {
      const result = await api.discardTask(id);
      if (result.outcome === "blocked") error = `Not discarded: ${result.blockers.join("; ")}.`;
      else onchanged();
    } catch (e) {
      error = String(e);
      losses = what;
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
  const ci = $derived(ciState(row.ci));
  const ciLabel = $derived.by(() => {
    if (ci === "pass") return "CI passing";
    if (ci === "fail") return `CI failing${row.failingChecks.length ? `: ${row.failingChecks.join(", ")}` : ""}`;
    if (ci === "pending") return "CI running";
    return null;
  });
  const DECISION: Record<string, [string, string]> = {
    APPROVED: ["approved", "ok"],
    CHANGES_REQUESTED: ["changes requested", "warn"],
    REVIEW_REQUIRED: ["review required", ""],
  };
  const unresolved = $derived(conv ? conv.threads.filter((t) => !t.resolved).length : 0);
  const TABS: [Tab, string][] = [["overview", "Overview"], ["comments", "Comments"], ["review", "Review"], ["terminal", "Terminal"]];
  const KIND_LABEL = { pr: "Pull request", issue: "Issue", task: "Task" } as const;
  const KIND_ICON = { pr: "pull-request", issue: "issue", task: "task" } as const;
</script>

<main class="item">
  <header>
    <div class="title-row">
      <span class="kind"><Icon name={KIND_ICON[row.kind]} label={KIND_LABEL[row.kind]} /></span>
      <h1 class="title"><span class="num">{shortName(row)}</span>{row.title}</h1>
      <StarToggle starred={row.starred} name={shortName(row)} size={26} onclick={() => api.setStarred(row.id, !row.starred).then(onchanged)} data-item-star />
      <div class="actions">
        {#if preparing}
          <span class="preparing" role="status"><Spinner />Preparing the worktree…</span>
        {:else if !row.worktree}
          <Button variant="primary" icon="play" onclick={start}
            title={isTask
              ? "Create the task's worktree again and open Claude Code in it"
              : "Create (or reuse) a worktree for the PR's branch and open Claude Code in it with the PR as context"} data-start>Start working</Button>
        {/if}
        {#if isTask && !confirmDone}
          {#if losses}
            <InlineConfirm text="Discard this task{losses.length ? ' and lose its work' : ''}?" confirm="Discard"
              onconfirm={discard} oncancel={() => (losses = null)} data-confirm-discard />
          {:else}
            <!-- Rarer and destructive: it does not compete with Done. -->
            <Button variant="quiet" icon="trash" onclick={askDiscard}
              title={row.adopted
                ? "Delete the task and its agent session; the worktree it used and its branch are kept"
                : "Delete the task with its worktree, branch and agent session, even if that loses work (you see what first)"} data-discard>Discard</Button>
          {/if}
        {/if}
        {#if confirmDone}
          <!-- Neutral: Done only cleans up when nothing would be lost. -->
          <InlineConfirm text="Clean up {row.worktree ? (row.adopted ? 'the agent and ' : 'the worktree, agent and ') : ''}this {isTask ? 'task' : 'item'}?" confirm="Clean up"
            tone="neutral" onconfirm={done} oncancel={() => (confirmDone = false)} data-confirm-done />
        {:else if !losses}
          <Button icon="done" onclick={() => (confirmDone = true)}
            title={row.adopted
              ? "Remove this item's agent session and refs, and hide it; the worktree it used is kept"
              : "Remove this item's worktree, agent session and refs, and hide it — only if nothing would be lost"} data-done>Done</Button>
        {/if}
      </div>
    </div>
    <div class="meta" data-meta>
      <span>{KIND_LABEL[row.kind]}</span>
      {#if row.issueNumber}<span>from issue #{row.issueNumber}</span>{/if}
      {#if !isTask}<span>by {row.author}</span>{/if}
      {#if ciLabel}
        <span class={ci === "pass" ? "ok" : ci === "fail" ? "bad" : "warn"}><CiMark state={ci} label="" />{ciLabel}</span>
      {/if}
      {#if row.reviewDecision}
        {@const d = DECISION[row.reviewDecision] ?? [row.reviewDecision.toLowerCase(), ""]}
        <span class={d[1]}>{d[0]}</span>
      {/if}
      {#if row.unresolvedThreads}<span class="gh"><Icon name="comment" size={12} />{row.unresolvedThreads} unresolved</span>{/if}
      {#if isTask && row.branch}<span class="branch" title="Branch"><Icon name="branch" size={12} /><code>{row.branch}</code></span>{/if}
      {#if row.worktree}
        {@const path = row.worktree}
        <button class="path" title="{path} — click to copy" onclick={() => copyPath(path)} data-worktree>
          <Icon name="folder" size={12} /><code>{copied ? "Copied" : folderName(path)}</code>
        </button>
      {/if}
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
        <strong>Waiting for your permission</strong>{#if agent?.detail}<code>{agent.detail}</code>{/if}
      </span>
      {#if tab !== "terminal"}
        <!-- Only switches tab, so not a filled agent button. -->
        <Button variant="agent-soft" size="sm" icon="terminal" onclick={() => ui && (ui.tab = "terminal")} data-answer>Answer in Terminal</Button>
      {/if}
    </div>
  {/if}
  {#if moved}
    <div class="banner">
      <Banner tone="bad" icon="branch" data-moved>
        <span><code>{moved.folder}</code> is on {#if moved.now}<code>{moved.now}</code>{:else}a detached HEAD{/if} now, not on
          <code>{moved.branch}</code>. Ariekei sends the agent nothing until you switch it back{#if moved.follow} or continue on
          <code>{moved.now}</code>{/if}; what you type in its terminal still reaches it.</span>
        {#snippet action()}
          {@const m = moved!}
          {#if m.follow}
            <Button size="sm" loading={fixing === "follow"} disabled={!!fixing} onclick={() => fix("follow", () => api.followBranch(m.id))}
              title={row.kind === "pr" ? `Work on ${m.now} from now on; the PR on GitHub changes when you push it there` : `Make ${m.now} the ${row.kind}'s branch`}
              data-follow-branch>Continue on {m.now}</Button>
          {/if}
          {#if m.now === ""}
            <Button size="sm" loading={fixing === "move"} disabled={!!fixing} onclick={() => fix("move", () => api.moveBranchHere(m.id))}
              title="Put {m.branch} on the checked out commit, e.g. a newer version of the PR" data-move-branch>Move {m.branch} here</Button>
          {/if}
          <Button size="sm" variant="primary" loading={fixing === "switch"} disabled={!!fixing} onclick={() => fix("switch", () => api.switchBack(m.id))}
            data-switch-back>Switch back to {m.branch}</Button>
        {/snippet}
      </Banner>
    </div>
  {/if}
  {#if row.update && !moved && row.branch}
    <div class="banner">
      <Banner tone="gh" data-update={row.update}>
        <span>The PR on GitHub has commits that <code>{row.branch}</code> lacks{#if row.update === "rewritten"}, and its history
          was rewritten (e.g. rebased): updating replaces <code>{row.branch}</code> with GitHub's version{/if}.</span>
        {#snippet action()}
          <Button size="sm" variant="gh" mark="gh" loading={fixing === "update"} disabled={!!fixing} onclick={() => fix("update", () => api.updateBranch(row.id, row.update === "rewritten"))}
            data-update-branch>Update {row.branch}</Button>
        {/snippet}
      </Banner>
    </div>
  {/if}
  {#if row.finished && !row.kept && !shownBlockers && !isTask}
    <div class="banner">
      <Banner tone="gh" icon="done" data-finished={row.finished}>
        <span>{row.kind === "pr" ? "This PR" : "This issue"} was {row.finished} on GitHub. {row.worktree
          ? `Clean up ${row.adopted ? "its agent" : "its worktree and agent"}`
          : "Clean it up"} now, or keep it until you press Done? If you just open another item, it is cleaned up then.</span>
        {#snippet action()}
          <Button size="sm" disabled={keeping} onclick={keep} data-keep>Keep until Done</Button>
          <Button size="sm" variant="primary" disabled={keeping} onclick={done} data-clean-up>Clean up</Button>
        {/snippet}
      </Banner>
    </div>
  {/if}
  {#if losses?.length}
    <div class="banner">
      <Banner tone="bad" icon="trash" compact={false} data-losses>
        <strong>Discarding the task deletes its worktree and branch, and with them:</strong>
        <ul>{#each losses as l}<li>{l}</li>{/each}</ul>
      </Banner>
    </div>
  {/if}
  {#if shownBlockers}
    <div class="banner">
      <Banner tone="bad" icon="blocked" compact={false} data-blocked>
        <strong>{row.open ? "Not cleaned up" : "Finished, but not cleaned up"}, so no work is lost:</strong>
        <ul>{#each shownBlockers as b}<li>{b}</li>{/each}</ul>
        <span class="muted">Commit and push these, or discard them, then press Done again.</span>
        {#snippet action()}<IconButton icon="x" label="Dismiss" size={22} onclick={() => (blockersDismissed = true)} />{/snippet}
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
        <span class="note">{#each notice.split("`") as part, i}{#if i % 2}<code>{part}</code>{:else}{part}{/if}{/each}</span>
        {#snippet action()}<IconButton icon="x" label="Dismiss" size={22} onclick={() => (notice = null)} />{/snippet}
      </Banner>
    </div>
  {/if}

  {#if ui}
    <div class="tabs" role="tablist" aria-label="Work item">
      {#each TABS as [value, label]}
        {#if (value !== "terminal" || row.worktree) && (value !== "review" || hasReview) && (value !== "comments" || !isTask)}
          <button role="tab" aria-selected={tab === value} class:on={tab === value} onclick={() => ui && (ui.tab = value)} data-tab={value}>
            {#if value === "terminal"}<StatusDot {status} size={10} />{/if}
            <span>{label}</span>
            {#if value === "comments" && unresolved}<span class="badge" title="Unresolved review threads">{unresolved}</span>{/if}
          </button>
        {/if}
      {/each}
      <span class="spacer"></span>
      {#if row.worktree && row.kind === "pr"}
        <Button variant="quiet" mark="agent" onclick={sendPrFeedback} disabled={!!agentAway}
          title={agentAway ?? "Send the PR's current review threads, review summaries and failing checks to the agent"}>Send PR feedback to agent</Button>
      {/if}
    </div>
    <div class="pane">
      {#key id}
        {#if row.worktree}
          <!-- Stays mounted under the other tabs so the terminal keeps running in view. -->
          <!-- Opened again to start an agent that exited. -->
          {#key restarts}
            <TerminalPanel {id} visible={tab === "terminal"} {status} path={row.worktree}
              onrestart={() => restarts++} />
          {/key}
        {/if}
        {#if isTask && (tab === "overview" || tab === "comments")}
          <TaskOverview task={row} />
        {:else if tab === "overview" || tab === "comments"}
          {#if conv}
            {#if convError}
              <div class="banner">
                <Banner tone="warn" data-stale>Shown as GitHub last gave it: could not update it ({convError}). Ariekei tries again with the next poll.</Banner>
              </div>
            {/if}
            {#if tab === "overview"}<Overview {conv} />{:else}<Comments {id} {conv} {since} {viewer} onchanged={changed} onshow={hasReview ? showInReview : undefined} />{/if}
          {:else}
            <p class="hint">{convError ? `Could not load the ${row.kind === "pr" ? "pull request" : "issue"} from GitHub: ${convError}` : "Loading…"}</p>
          {/if}
        {:else if tab === "review"}
          <ReviewView {id} bind:view={ui.review} onchanged={changed} canSendToAgent={!!row.worktree} {agentAway}
            agentStatus={row.worktree ? (agent?.status ?? row.agent ?? undefined) : undefined}
            onterminal={showAgent} {polls} />
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
    flex: 0 1 auto; min-width: 0; margin: 0; font-size: var(--fs-15); line-height: var(--lh-15); font-weight: var(--fw-semibold);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .num { font-family: var(--font-mono); font-weight: var(--fw-medium); color: var(--text-muted); margin-right: var(--sp-3); }
  /* The star follows the title; the actions keep to the right. */
  .actions { flex: none; margin-left: auto; display: flex; align-items: center; gap: var(--sp-3); }
  .preparing { display: inline-flex; align-items: center; gap: var(--sp-3); font-size: var(--fs-12); color: var(--text-muted); }
  .meta {
    display: flex; align-items: center; gap: var(--sp-6); min-width: 0; padding-left: 26px;
    font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted); white-space: nowrap; overflow: hidden;
  }
  .meta > span, .meta > .path { display: inline-flex; align-items: center; gap: var(--sp-2); flex: none; }
  /* The branch and worktree give way first; the agent's status sits at the right end. */
  .meta > .branch, .meta > .path { flex: 0 100 auto; min-width: 4em; overflow: hidden; }
  .meta > .path {
    padding: 0; border: 0; border-radius: var(--r-sm); background: none; color: inherit; font: inherit; cursor: copy;
  }
  .meta > .path:hover { color: var(--text); }
  .meta > span.agent { flex: none; max-width: 45%; margin-left: auto; color: var(--agent-strong); }
  .meta .t, .meta code { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .meta code { font-size: var(--fs-code-inline); }
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
  .waiting .what {
    flex: 1; min-width: 0; display: flex; align-items: baseline; gap: var(--sp-4);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: var(--fs-12); line-height: var(--lh-12);
  }
  .waiting strong { flex: none; font-weight: var(--fw-semibold); color: var(--agent-strong); }
  .waiting code { min-width: 0; overflow: hidden; text-overflow: ellipsis; font-size: var(--fs-code-inline); }
  /* In a narrow window the command wraps to a second line instead of being cut. */
  @media (max-width: 1199px) {
    .waiting { align-items: flex-start; }
    .waiting .what { flex-wrap: wrap; white-space: normal; }
    .waiting code { white-space: normal; overflow-wrap: anywhere; }
  }
  /* The lifecycle note is one line. */
  .note { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
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
    min-width: 16px; height: 16px; padding: 0 var(--sp-2); box-sizing: border-box; border-radius: var(--r-pill);
    box-shadow: inset 0 0 0 1px var(--gh-line); color: var(--gh-strong); font-size: var(--fs-11); line-height: 16px; font-weight: var(--fw-semibold); text-align: center;
  }
  .tabs > :global(.btn) { align-self: center; }
  .spacer { flex: 1; }
  .pane { flex: 1; min-height: 0; display: flex; flex-direction: column; position: relative; }
  .hint { display: flex; align-items: center; gap: var(--sp-3); color: var(--text-muted); margin: var(--sp-9); font-size: var(--fs-12); }
</style>
