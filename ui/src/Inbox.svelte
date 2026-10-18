<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import RepoMenu from "./RepoMenu.svelte";
  import AttentionPip, { URGENCY } from "./controls/AttentionPip.svelte";
  import Banner from "./controls/Banner.svelte";
  import CiMark, { ciState } from "./controls/CiMark.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Button from "./controls/Button.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import StarToggle from "./controls/StarToggle.svelte";
  import StatusDot from "./controls/StatusDot.svelte";
  import { ATTENTION_LABEL, attentionKind } from "./lib/attention";
  import { api, shortName, type Repo, type Row } from "./lib/api";
  import { SECTIONS, bySection } from "./lib/inboxSections";
  import { browser } from "./lib/layout.svelte";

  let {
    repo,
    rows,
    loaded,
    selected,
    error,
    collapsed,
    ontoggle,
    onselect,
    onrefresh,
    refreshing = false,
    onadded,
    onswitch,
    onconnect,
    onremoved,
    creating = $bindable(false),
  }: {
    repo: Repo;
    rows: Row[];
    /// The first Inbox load finished (until then: a skeleton).
    loaded: boolean;
    selected: number | null;
    error: string | null;
    collapsed: boolean;
    ontoggle: () => void;
    onselect: (id: number) => void;
    onrefresh: () => void;
    /// A refresh the user asked for is running (the button spins).
    refreshing?: boolean;
    /// The Inbox changed here: a task was added, an item starred or removed.
    onadded: () => void;
    /// This window now shows another repo.
    onswitch: (repo: Repo) => void;
    /// Show the Connect screen (for another repo).
    onconnect: () => void;
    /// This window's repo was removed from Ariekei.
    onremoved: () => void;
    /// The New task form is open (Ctrl+N opens it too, see App).
    creating?: boolean;
  } = $props();

  // Each item is in one section, the first that fits (lib/inboxSections.ts).
  const sections = $derived(bySection(rows));
  const DECISION: Record<string, [string, string]> = {
    APPROVED: ["approved", "ok"],
    CHANGES_REQUESTED: ["changes req.", "warn"],
    REVIEW_REQUIRED: ["review req.", ""],
  };

  // Sections the user folded (remembered across launches).
  let folded: string[] = $state([]);
  api.uiSetting("inboxFolded").then((v) => (folded = v ? JSON.parse(v) : []));
  function fold(key: string) {
    folded = folded.includes(key) ? folded.filter((k) => k !== key) : [...folded, key];
    api.setUiSetting("inboxFolded", JSON.stringify(folded));
  }

  // The collapsed strip: one pip per item that wants something, most urgent first.
  const pips = $derived(
    rows
      .map((row) => ({ row, kind: attentionKind(row.attention) }))
      .filter((p) => p.kind !== "none")
      .sort((a, b) => URGENCY[a.kind] - URGENCY[b.kind]),
  );

  // A task's PRs and issues. More than three fold, except those that want something (and the
  // selected one), under "+N more" until the user unfolds them.
  const FOLD_LINKS_OVER = 3;
  let unfoldedTasks: number[] = $state([]);
  function linksOf(task: Row) {
    const all = rows.filter((r) => r.taskId === task.id);
    if (all.length <= FOLD_LINKS_OVER || unfoldedTasks.includes(task.id)) return { shown: all, hidden: 0, all: all.length };
    const shown = all.filter((r) => attentionKind(r.attention) !== "none" || r.id === selected);
    return { shown, hidden: all.length - shown.length, all: all.length };
  }
  function toggleLinks(task: Row) {
    unfoldedTasks = unfoldedTasks.includes(task.id) ? unfoldedTasks.filter((id) => id !== task.id) : [...unfoldedTasks, task.id];
  }

  // --- browsing the repo's open PRs and issues (Browse.svelte), starring, removing ---

  function browse() {
    browser.text = "";
    browser.open = true;
  }

  let actionError: string | null = $state(null);

  async function star(row: Row) {
    try {
      await api.setStarred(row.id, !row.starred);
      actionError = null;
      onadded();
    } catch (e) {
      actionError = String(e);
    }
  }

  async function remove(row: Row) {
    try {
      await api.untrack(row.id);
      actionError = null;
      onadded();
    } catch (e) {
      actionError = String(e);
    }
  }

  // --- New task: work with no PR or issue yet ---
  let taskTitle = $state("");
  let taskNotes = $state("");
  // An existing branch to work on (e.g. owner:branch on a fork); empty for a new one.
  let taskBranch = $state("");
  let starting = $state(false);
  let taskError: string | null = $state(null);
  let newTaskButton: HTMLElement | undefined = $state();

  function newTask() {
    if (creating) closeForm();
    else creating = true;
    taskError = null;
  }

  // Closing the form leaves no pressed or focused look on the + that opened it.
  function closeForm() {
    creating = false;
    newTaskButton?.querySelector("button")?.blur();
  }

  async function startTask(e?: SubmitEvent) {
    e?.preventDefault();
    if (!(taskTitle.trim() || taskBranch.trim()) || starting) return;
    starting = true;
    taskError = null;
    try {
      const id = await api.createTask(taskTitle, taskNotes, taskBranch);
      closeForm();
      taskTitle = taskNotes = taskBranch = "";
      if (folded.includes("tasks")) fold("tasks");
      onadded();
      onselect(id);
    } catch (err) {
      taskError = String(err);
    } finally {
      starting = false;
    }
  }

  // Esc closes the form; Ctrl/Cmd+Enter starts the task from any field.
  function onTaskKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !starting) closeForm();
    else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      startTask();
    }
  }
</script>

{#snippet item(row: Row, section: string, nested: boolean)}
  {@const kind = attentionKind(row.attention)}
  {@const decision = row.reviewDecision ? DECISION[row.reviewDecision] : null}
  {@const links = row.kind === "task" ? rows.filter((r) => r.taskId === row.id).length : 0}
  <li class:closed={!row.open} class:nested>
    <button class="row" class:active={row.id === selected} aria-current={row.id === selected} onclick={() => onselect(row.id)}
      title={row.title} data-number={row.number} data-task={row.kind === "task" ? -row.number : undefined}>
      <span class="dot">
        {#if nested}
          <!-- A task's PR or issue has no agent of its own: the slot shows what it is. -->
          <span class="kind"><Icon name={row.kind === "pr" ? "pull-request" : "issue"} size={12} label={row.kind === "pr" ? "Pull request" : "Issue"} /></span>
        {:else}
          <StatusDot status={row.agent} />
        {/if}
      </span>
      <span class="lines">
        <span class="line1">
          <span class="num">{shortName(row)}</span>
          <span class="title" class:strong={kind === "needs-you" || row.newActivity}>{row.title}</span>
          {#if row.newActivity}
            <svg class="new" width="8" height="8" viewBox="0 0 8 8" role="img" aria-label="New reviews or comments"><path d="M4 0.5L7.5 4 4 7.5 0.5 4z" /></svg>
          {/if}
        </span>
        <span class="line2">
          {#if row.kind === "task"}
            {#if links}<span class="faint shrink" title={row.branch ?? undefined}>task{row.branch ? ` · ${row.branch}` : ""}</span>{:else}<span class="faint">no PR or issue yet</span>{/if}
          {:else}
            {#if row.kind === "issue" && !nested}<span class="faint issue"><Icon name="issue" size={12} />issue</span>{/if}
            {#if section === "approved"}<span class="faint">ready to merge</span>{/if}
            {#if section !== "mine" && section !== "approved" && section !== "tasks"}<span>{row.author}</span>{/if}
            {#if row.issueNumber}<span>from #{row.issueNumber}</span>{/if}
            {#if ciState(row.ci)}
              <span class="ci" data-ci={ciState(row.ci)} title="CI: {row.ci?.toLowerCase()}{row.failingChecks.length ? ` (${row.failingChecks.join(', ')})` : ''}">
                <CiMark state={ciState(row.ci)} />
              </span>
            {/if}
            {#if row.isDraft}<span class="faint">draft</span>{/if}
            {#if !row.open}<span class="faint">closed</span>{/if}
            {#if decision && section !== "approved"}<span class={decision[1]}>{decision[0]}</span>{/if}
            {#if row.unresolvedThreads}
              <span class="threads" title="Unresolved review threads"><Icon name="comment" size={12} />{row.unresolvedThreads}</span>
            {/if}
          {/if}
          <span class="spacer"></span>
          {#if kind !== "none"}<span class="attention {kind}">{ATTENTION_LABEL[kind]}</span>{/if}
        </span>
      </span>
    </button>
    {#if !nested}
      <!-- Shown on hover and focus only: Starred already says which items are starred. -->
      <span class="acts">
        {#if row.removable}
          <IconButton icon="inbox-remove" label="Remove {shortName(row)} from the Inbox" size={22} onclick={() => remove(row)} data-remove={row.number} />
        {/if}
        <StarToggle starred={row.starred} name={shortName(row)} onclick={() => star(row)} data-star={row.number} />
      </span>
    {/if}
  </li>
{/snippet}

{#if collapsed}
  <aside class="inbox collapsed" aria-label="Inbox, collapsed">
    <IconButton icon="sidebar-expand" label="Expand Inbox (Ctrl+B)" size={28} onclick={ontoggle} />
    <span class="divider"></span>
    {#each pips as { row, kind } (row.id)}
      {@const label = `${shortName(row)} ${row.title} — ${ATTENTION_LABEL[kind]}`}
      <button class="pip" class:active={row.id === selected} onclick={() => onselect(row.id)} title={label} aria-label={label} data-pip={row.number}>
        <AttentionPip {kind} label={ATTENTION_LABEL[kind]} />
      </button>
    {/each}
  </aside>
{:else}
<aside class="inbox" aria-label="Inbox">
  <header>
    <RepoMenu {repo} {onswitch} {onconnect} {onremoved} />
    <span class="new-task-button" bind:this={newTaskButton}>
      <IconButton icon="plus" label="New task — work with no PR or issue yet (Ctrl+N)" size={28} onclick={newTask}
        pressed={creating} aria-expanded={creating} data-new-task />
    </span>
    <IconButton icon="refresh" label={refreshing ? "Refreshing from GitHub…" : "Refresh from GitHub"} size={28} onclick={onrefresh}
      class={refreshing ? "spinning" : ""} aria-busy={refreshing || undefined} data-refresh data-refreshing={refreshing || undefined} />
    <IconButton icon="sidebar-collapse" label="Collapse Inbox (Ctrl+B)" size={28} onclick={ontoggle} />
  </header>
  <div class="search">
    <!-- Opens Browse (a dialog), so it is a button that looks like the field. -->
    <button class="browse-field" onclick={browse} aria-haspopup="dialog" data-browse-field>
      <Icon name="search" size={14} />
      <span class="placeholder">Browse PRs and issues…</span>
      <Kbd key="Ctrl K" />
    </button>
  </div>
  {#if creating}
    <form class="new-task" class:starting onsubmit={startTask} data-task-form>
      <div class="form-head">
        <Icon name="task" size={14} />
        <span class="form-title">New task</span>
        <Kbd key="Esc" />
      </div>
      <label>
        <span class="field-label">Title</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value={taskTitle} placeholder="e.g. Peers drop after restart" autofocus disabled={starting} onkeydown={onTaskKey} />
      </label>
      <label>
        <span class="field-label">Existing branch <span class="optional">— optional</span></span>
        <input class="mono" bind:value={taskBranch} placeholder="owner:branch"
          title="Work on this branch instead of a new one, e.g. on a fork: owner:branch, owner/repo:branch, a GitHub branch URL or a branch name. A plain git push updates it." disabled={starting} onkeydown={onTaskKey} data-task-branch />
      </label>
      <label>
        <span class="field-label">What to look into</span>
        <textarea bind:value={taskNotes} rows="3" placeholder="Optional; the agent gets it as context" disabled={starting} onkeydown={onTaskKey}></textarea>
      </label>
      {#if taskError}<Banner tone="bad">{taskError}</Banner>{/if}
      <div class="form-actions">
        {#if starting}<span class="muted">{taskBranch.trim() ? "Fetching the branch…" : "Creating the worktree…"}</span>{/if}
        <Button size="sm" variant="quiet" type="button" onclick={closeForm} disabled={starting}>Cancel</Button>
        <Button size="sm" variant="primary" icon="play" type="submit" loading={starting} disabled={!(taskTitle.trim() || taskBranch.trim())}
          title="Start task (Ctrl+Enter)">Start task</Button>
      </div>
    </form>
  {/if}
  {#if actionError}<p class="action-error" role="alert">{actionError}</p>{/if}
  {#if error}
    <div class="poll-error" role="status" title={error}>
      <Icon name="warning" size={14} />
      <span><strong>Could not refresh from GitHub.</strong> <span class="muted">{error}</span></span>
    </div>
  {/if}

  {#if !loaded}
    <div class="skeleton" aria-label="Loading the Inbox">
      {#each [70, 58, 64] as w}
        <div class="sk-row"><span class="sk-dot"></span><span class="sk-bar" style:width="{w}%"></span></div>
        <div class="sk-sub"><span class="sk-bar small" style:width="{w - 25}%"></span></div>
      {/each}
    </div>
  {:else}
    <nav class="sections" aria-label="Work items">
      {#each SECTIONS as section (section.key)}
        {@const list = sections[section.key]}
        {@const open = !folded.includes(section.key)}
        {#if list.length || section.key === "open"}
          <section data-section={section.key}>
            <button class="section-head" onclick={() => fold(section.key)} aria-expanded={open}>
              <span class="chev" class:folded={!open}><Icon name="chevron-down" size={12} /></span>
              <span class="label">{section.title}</span>
              <span class="count">{list.length}</span>
            </button>
            {#if open}
              <ul>
                {#each list as row (row.id)}
                  {@render item(row, section.key, false)}
                  {#if row.kind === "task"}
                    {@const links = linksOf(row)}
                    {#if links.all}
                      <li class="links">
                        <ul>
                          {#each links.shown as link (link.id)}
                            {@render item(link, section.key, true)}
                          {/each}
                          {#if links.hidden || links.all > FOLD_LINKS_OVER && unfoldedTasks.includes(row.id)}
                            <li>
                              <button class="more" onclick={() => toggleLinks(row)} data-more-links={row.number}>
                                {links.hidden ? `+${links.hidden} more` : "Show fewer"}
                              </button>
                            </li>
                          {/if}
                        </ul>
                      </li>
                    {/if}
                  {/if}
                {/each}
                {#if section.key === "open"}
                  <li>
                    <button class="browse-row" onclick={browse} data-browse-open-section>
                      <Icon name="search" size={14} />
                      <span>Browse…</span>
                      <Kbd key="Ctrl K" />
                    </button>
                  </li>
                {/if}
              </ul>
            {/if}
          </section>
        {/if}
      {/each}
    </nav>
  {/if}
</aside>
{/if}

<style>
  .inbox {
    flex: none; width: var(--w-inbox); display: flex; flex-direction: column; min-height: 0;
    background: var(--surface-1); border-right: 1px solid var(--border);
  }
  .inbox.collapsed { width: var(--w-inbox-strip); align-items: center; gap: var(--sp-2); padding: var(--sp-3) 0; box-sizing: border-box; }
  .divider { width: 20px; height: 1px; margin: var(--sp-2) 0; background: var(--border); }
  .pip {
    position: relative; width: 28px; height: 28px; padding: 0; display: inline-grid; place-items: center;
    border: 0; border-radius: var(--r-md); background: transparent;
  }
  .pip:hover { background: var(--surface-hover); }
  .pip.active { background: var(--surface-selected); }
  .pip.active::before {
    content: ""; position: absolute; left: -4px; top: 6px; width: 3px; height: 16px;
    border-radius: 0 2px 2px 0; background: var(--you);
  }

  /* The refresh arrow turns while a refresh runs (as the Spinner does; still with reduced
     motion, where the button only dims). */
  header :global(.spinning svg) { animation: spin var(--dur-spin) linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    header :global(.spinning svg) { animation: none; opacity: 0.5; }
  }
  header { display: flex; align-items: center; gap: var(--sp-1); height: var(--h-inbox-header); padding: 0 var(--sp-3) 0 var(--sp-4); flex: none; }
  .new-task-button { display: contents; }
  .search { padding: 0 var(--sp-4) var(--sp-4); flex: none; }
  .browse-field {
    width: 100%; display: flex; align-items: center; gap: var(--sp-3); height: 28px; padding: 0 var(--sp-3) 0 var(--sp-4); box-sizing: border-box;
    border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-raised); color: var(--text-faint);
    font-size: var(--fs-12); text-align: left;
  }
  .browse-field:hover { border-color: var(--border-strong); }
  .browse-field .placeholder { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .poll-error {
    margin: 0 var(--sp-4) var(--sp-4); display: flex; gap: var(--sp-4); padding: var(--sp-4) var(--sp-5);
    border-radius: var(--r-md); background: var(--warn-bg); font-size: var(--fs-12); line-height: var(--lh-12);
    overflow: hidden; max-height: 54px;
  }
  .poll-error :global(svg) { color: var(--warn); margin-top: 2px; }
  .poll-error strong { font-weight: var(--fw-semibold); }
  .muted { color: var(--text-muted); }

  .sections, .skeleton { flex: 1; min-height: 0; overflow-y: auto; padding-bottom: var(--sp-6); }
  section { display: flex; flex-direction: column; }
  section + section { margin-top: var(--sp-2); }
  /* Section headers stay at the top while their section scrolls. */
  .section-head {
    position: sticky; top: 0; z-index: 1; background: var(--surface-1);
    display: flex; align-items: center; gap: var(--sp-2); height: var(--h-section); padding: 0 var(--sp-5) 0 var(--sp-4);
    border: 0; border-radius: 0; text-align: left;
    font-size: var(--fs-11); line-height: var(--lh-11); font-weight: var(--fw-semibold); letter-spacing: 0.04em;
    text-transform: uppercase; color: var(--text-muted);
  }
  .section-head:focus-visible { outline-offset: -2px; }
  .chev { display: flex; color: var(--text-faint); transition: transform var(--dur-fast) var(--ease); }
  .chev.folded { transform: rotate(-90deg); }
  .count { margin-left: var(--sp-2); font-family: var(--font-mono); font-size: var(--fs-11); font-weight: var(--fw-regular); letter-spacing: 0; color: var(--text-faint); }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); }
  li { position: relative; }
  li.closed { opacity: 0.6; }

  /* Star and Remove: on hover and focus, over the end of the title line, on the row's own
     background so the title ends cleanly under them. */
  .acts {
    position: absolute; top: var(--sp-2); right: var(--sp-3); display: none; gap: 1px; padding-left: var(--sp-3);
    border-radius: var(--r-md); background: color-mix(in srgb, var(--text) 5%, var(--surface-1));
  }
  li:hover > .acts, li:focus-within > .acts { display: flex; }
  li:has(> .row.active) > .acts { background: var(--surface-selected); }

  /* A task's PRs and issues hang under it on a guide line from the task's dot. */
  .links { margin-left: 20px; }
  .links::before {
    content: ""; position: absolute; left: 0; top: 0; bottom: 6px; width: 1px; background: var(--border-strong);
  }
  .links .row { margin-left: var(--sp-2); width: calc(100% - 10px); }
  .kind { display: flex; width: 14px; height: 14px; align-items: center; justify-content: center; color: var(--text-faint); }
  .more {
    margin: 0 var(--sp-3) 0 var(--sp-2); height: var(--h-row); padding: 0 var(--sp-4); width: calc(100% - 10px); box-sizing: border-box;
    border: 0; border-radius: var(--r-md); background: transparent; text-align: left;
    font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted);
  }
  .more:hover { background: var(--surface-hover); color: var(--text); }
  .more:focus-visible { outline-offset: -2px; }

  .browse-row {
    display: flex; align-items: center; gap: var(--sp-3); height: var(--h-row); margin: 2px var(--sp-3) 0; padding: 0 var(--sp-4);
    width: calc(100% - 12px); box-sizing: border-box; border: 0; border-radius: var(--r-md); background: transparent;
    font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted); text-align: left;
  }
  .browse-row span { flex: 1; }
  .browse-row:hover { background: var(--surface-hover); color: var(--text); }
  .browse-row:focus-visible { outline-offset: -2px; }

  .new-task {
    flex: none; display: flex; flex-direction: column; gap: var(--sp-4); margin: 0 var(--sp-4) var(--sp-4);
    padding: 10px; border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: inset 0 0 0 1px var(--border);
  }
  .form-head { display: flex; align-items: center; gap: var(--sp-3); color: var(--text-muted); }
  .form-title { flex: 1; font-size: var(--fs-12); line-height: var(--lh-12); font-weight: var(--fw-semibold); color: var(--text); }
  .new-task label { display: flex; flex-direction: column; gap: 3px; }
  .field-label { font-size: var(--fs-11); line-height: var(--lh-11); font-weight: var(--fw-medium); color: var(--text-muted); }
  .optional { font-weight: var(--fw-regular); color: var(--text-faint); }
  .new-task input { height: 28px; }
  .new-task input.mono { font-family: var(--font-mono); font-size: var(--fs-code-inline); }
  .new-task textarea { height: 56px; font-size: var(--fs-12); line-height: var(--lh-12); resize: vertical; }
  .new-task.starting input, .new-task.starting textarea { opacity: 0.5; }
  .form-actions { display: flex; align-items: center; justify-content: flex-end; gap: var(--sp-3); font-size: var(--fs-12); line-height: var(--lh-12); }
  .form-actions .muted { flex: 1; }

  .row {
    display: flex; gap: var(--sp-4); margin: 0 var(--sp-3); padding: var(--sp-3) var(--sp-4) 7px var(--sp-3);
    width: calc(100% - 12px); box-sizing: border-box; border: 0; border-radius: var(--r-md);
    background: transparent; color: var(--text); text-align: left; font-size: var(--fs-13);
  }
  .row:hover { background: var(--surface-hover); }
  .row.active { background: var(--surface-selected); box-shadow: inset 0 0 0 1px var(--you-line); }
  .row:focus-visible { outline-offset: -2px; }
  .dot { flex: none; width: 14px; padding-top: 3px; }
  .lines { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .line1 { display: flex; align-items: baseline; gap: var(--sp-3); min-width: 0; }
  .num { flex: none; font-family: var(--font-mono); font-size: var(--fs-code-inline); color: var(--text-muted); }
  .title { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-weight: var(--fw-medium); }
  .title.strong { font-weight: var(--fw-semibold); }
  .new { flex: none; align-self: center; fill: var(--gh); }
  .line2 {
    display: flex; align-items: center; gap: var(--sp-4); min-width: 0; overflow: hidden; white-space: nowrap;
    font-size: var(--fs-code-inline); line-height: 16px; color: var(--text-muted);
  }
  .line2 > span { flex: none; }
  .line2 .spacer { flex: 1; }
  /* The attention label wins the room; a task's branch gives way. */
  .line2 .attention { flex: none; font-weight: var(--fw-medium); }
  .line2 > .shrink { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .ci { display: inline-flex; }
  .issue { display: inline-flex; align-items: center; gap: var(--sp-1); }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .faint { color: var(--text-faint); }
  .threads { display: inline-flex; align-items: center; gap: var(--sp-1); }
  .attention.needs-you { color: var(--agent-strong); }
  .attention.blocked, .attention.ci-failing { color: var(--bad); }
  .attention.activity { color: var(--gh-strong); }
  .attention.review { color: var(--you-strong); }
  .attention.all-closed { color: var(--info); }
  .attention.working { color: var(--text-muted); }
  .action-error { margin: 0 var(--sp-4) var(--sp-4); font-size: var(--fs-12); color: var(--bad); }

  .skeleton { padding: var(--sp-6) var(--sp-7); display: flex; flex-direction: column; gap: var(--sp-3); }
  .sk-row { display: flex; gap: var(--sp-4); align-items: center; }
  .sk-dot { width: 12px; height: 12px; border-radius: var(--r-pill); background: var(--border); }
  .sk-bar { display: block; height: 10px; border-radius: var(--r-xs); background: var(--border); }
  .sk-bar.small { height: 8px; }
  .sk-sub { padding-left: 20px; margin-bottom: var(--sp-4); }
</style>
