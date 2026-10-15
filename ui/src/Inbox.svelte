<script lang="ts">
  import RepoMenu from "./RepoMenu.svelte";
  import AttentionPip, { URGENCY } from "./controls/AttentionPip.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Button from "./controls/Button.svelte";
  import Pill from "./controls/Pill.svelte";
  import StatusDot from "./controls/StatusDot.svelte";
  import { ATTENTION_LABEL, attentionKind } from "./lib/attention";
  import { api, shortName, type Repo, type Row, type SearchHit } from "./lib/api";

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
    onadded,
    onswitch,
    onconnect,
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
    /// A PR or task was added to the Inbox (from search, or New task).
    onadded: () => void;
    /// This window now shows another repo.
    onswitch: (repo: Repo) => void;
    /// Show the Connect screen (for another repo).
    onconnect: () => void;
    /// The New task form is open (Ctrl+N opens it too, see App).
    creating?: boolean;
  } = $props();

  // A task's PRs and issues are listed under it, not in their own sections.
  const own = (r: Row) => r.taskId == null;
  const SECTIONS: { key: string; title: string; empty: string; has: (r: Row) => boolean }[] = [
    { key: "tasks", title: "Tasks", empty: "", has: (r) => r.kind === "task" },
    { key: "mine", title: "Mine", empty: "You have no open pull requests here.", has: (r) => own(r) && r.kind === "pr" && r.role === "author" },
    { key: "requested", title: "Review requested", empty: "Nothing here.", has: (r) => own(r) && r.kind === "pr" && r.role === "requested" },
    { key: "issues", title: "Issues", empty: "Nothing here.", has: (r) => own(r) && r.kind === "issue" },
    { key: "opened", title: "Opened", empty: "PRs you open from search appear here.", has: (r) => own(r) && r.kind === "pr" && r.role === "other" },
  ];
  const DECISION: Record<string, [string, string]> = {
    APPROVED: ["approved", "ok"],
    CHANGES_REQUESTED: ["changes req.", "warn"],
    REVIEW_REQUIRED: ["review req.", ""],
  };
  const CI_FAILING = new Set(["FAILURE", "ERROR"]);

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

  // --- search over all open PRs of the repo ---
  let query = $state("");
  let hits: SearchHit[] = $state([]);
  let searching = $state(false);
  let searchError: string | null = $state(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const text = query.trim();
    clearTimeout(timer);
    if (!text) {
      hits = [];
      searchError = null;
      return;
    }
    timer = setTimeout(async () => {
      searching = true;
      try {
        const found = await api.searchPrs(text);
        if (query.trim() === text) {
          hits = found;
          searchError = null;
        }
      } catch (e) {
        searchError = String(e);
      } finally {
        searching = false;
      }
    }, 350);
  });

  async function pick(hit: SearchHit) {
    try {
      const id = hit.itemId ?? (await api.trackPr(hit.number));
      query = "";
      onadded();
      onselect(id);
    } catch (e) {
      searchError = String(e);
    }
  }

  // --- New task: work with no PR or issue yet ---
  let taskTitle = $state("");
  let taskNotes = $state("");
  let starting = $state(false);
  let taskError: string | null = $state(null);

  function newTask() {
    creating = !creating;
    taskError = null;
  }

  async function startTask(e: SubmitEvent) {
    e.preventDefault();
    if (!taskTitle.trim() || starting) return;
    starting = true;
    taskError = null;
    try {
      const id = await api.createTask(taskTitle, taskNotes);
      creating = false;
      taskTitle = taskNotes = "";
      if (folded.includes("tasks")) fold("tasks");
      onadded();
      onselect(id);
    } catch (err) {
      taskError = String(err);
    } finally {
      starting = false;
    }
  }

  function onTaskKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !starting) creating = false;
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Escape") query = "";
    if (e.key === "Enter" && hits[0]) pick(hits[0]);
  }
</script>

{#snippet item(row: Row, section: string, nested: boolean)}
  {@const kind = attentionKind(row.attention)}
  {@const decision = row.reviewDecision ? DECISION[row.reviewDecision] : null}
  <li class:closed={!row.open} class:nested>
    <button class="row" class:active={row.id === selected} aria-current={row.id === selected} onclick={() => onselect(row.id)}
      title={row.title} data-number={row.number} data-task={row.kind === "task" ? -row.number : undefined}>
      <span class="dot"><StatusDot status={row.agent} /></span>
      <span class="lines">
        <span class="line1">
          {#if nested}<span class="kind"><Icon name={row.kind === "pr" ? "pull-request" : "issue"} size={12} label={row.kind === "pr" ? "Pull request" : "Issue"} /></span>{/if}
          <span class="num">{shortName(row)}</span>
          <span class="title" class:strong={kind === "needs-you" || row.newActivity}>{row.title}</span>
          {#if row.newActivity}
            <svg class="new" width="8" height="8" viewBox="0 0 8 8" role="img" aria-label="New reviews or comments"><path d="M4 0.5L7.5 4 4 7.5 0.5 4z" /></svg>
          {/if}
        </span>
        <span class="line2">
          {#if section !== "mine" && section !== "tasks"}<span>{row.author}</span>{/if}
          {#if row.kind === "task" && !rows.some((r) => r.taskId === row.id)}<span class="faint">no PR or issue yet</span>{/if}
          {#if row.issueNumber}<span>from #{row.issueNumber}</span>{/if}
          {#if row.ci}
            {@const ci = row.ci === "SUCCESS" ? "ok" : CI_FAILING.has(row.ci) ? "bad" : "warn"}
            <span class="ci {ci}" title="CI: {row.ci.toLowerCase()}{row.failingChecks.length ? ` (${row.failingChecks.join(', ')})` : ''}">
              <Icon name={ci === "ok" ? "check" : ci === "bad" ? "x" : "pending"} size={12} label="CI {row.ci.toLowerCase()}" />
            </span>
          {/if}
          {#if row.isDraft}<span class="faint">draft</span>{/if}
          {#if !row.open}<span class="faint">closed</span>{/if}
          {#if decision}<span class={decision[1]}>{decision[0]}</span>{/if}
          {#if row.unresolvedThreads}
            <span class="threads" title="Unresolved review threads"><Icon name="comment" size={12} />{row.unresolvedThreads}</span>
          {/if}
          <span class="spacer"></span>
          {#if kind !== "none"}<span class="attention {kind}">{ATTENTION_LABEL[kind]}</span>{/if}
        </span>
      </span>
    </button>
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
    <RepoMenu {repo} {onswitch} {onconnect} />
    <IconButton icon="plus" label="New task — work with no PR or issue yet (Ctrl+N)" size={28} onclick={newTask} data-new-task />
    <IconButton icon="refresh" label="Refresh from GitHub" size={28} onclick={onrefresh} />
    <IconButton icon="sidebar-collapse" label="Collapse Inbox (Ctrl+B)" size={28} onclick={ontoggle} />
  </header>
  <div class="search">
    <label>
      <Icon name="search" size={14} />
      <input
        bind:value={query}
        onkeydown={onSearchKey}
        placeholder="Search PRs — words, #123, author:name"
        aria-label="Search open pull requests"
      />
    </label>
  </div>
  {#if creating}
    <form class="new-task" onsubmit={startTask} data-task-form>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={taskTitle} placeholder="Task title, e.g. Peers drop after restart" aria-label="Task title" autofocus disabled={starting} onkeydown={onTaskKey} />
      <textarea bind:value={taskNotes} rows="3" placeholder="What to look into (optional; the agent gets it as context)" aria-label="What to look into" disabled={starting} onkeydown={onTaskKey}></textarea>
      {#if taskError}<p class="task-error" role="alert">{taskError}</p>{/if}
      <div class="form-actions">
        {#if starting}<span class="muted">Creating the worktree…</span>{/if}
        <Button size="sm" variant="quiet" type="button" onclick={() => (creating = false)} disabled={starting}>Cancel</Button>
        <Button size="sm" variant="primary" icon="play" type="submit" disabled={!taskTitle.trim() || starting}>Start task</Button>
      </div>
    </form>
  {/if}
  {#if error}
    <div class="poll-error" role="status" title={error}>
      <Icon name="warning" size={14} />
      <span><strong>Could not refresh from GitHub.</strong> <span class="muted">{error}</span></span>
    </div>
  {/if}

  {#if query.trim()}
    <ul class="hits" aria-label="Search results">
      {#if searchError}<li class="empty error">{searchError}</li>{/if}
      {#each hits as hit (hit.number)}
        <li>
          <button class="row" onclick={() => pick(hit)} data-hit={hit.number}>
            <span class="lines">
              <span class="line1"><span class="num">#{hit.number}</span><span class="title">{hit.title}</span></span>
              <span class="line2">
                <span>{hit.author}</span>
                {#if hit.isDraft}<Pill size="sm">draft</Pill>{/if}
                {#if hit.itemId}<Pill size="sm" tone="you">in inbox</Pill>{/if}
              </span>
            </span>
          </button>
        </li>
      {:else}
        <li class="empty">{searching ? "Searching…" : `No open pull request matches “${query.trim()}”.`}</li>
      {/each}
    </ul>
  {:else if !loaded}
    <div class="skeleton" aria-label="Loading the Inbox">
      {#each [70, 58, 64] as w}
        <div class="sk-row"><span class="sk-dot"></span><span class="sk-bar" style:width="{w}%"></span></div>
        <div class="sk-sub"><span class="sk-bar small" style:width="{w - 25}%"></span></div>
      {/each}
    </div>
  {:else}
    <nav class="sections" aria-label="Work items">
      {#each SECTIONS as section (section.key)}
        {@const list = rows.filter(section.has)}
        {@const open = !folded.includes(section.key)}
        {#if list.length || section.key === "mine" || section.key === "opened"}
          <section data-section={section.key}>
            <button class="section-head" onclick={() => fold(section.key)} aria-expanded={open}>
              <span class="chev" class:folded={!open}><Icon name="chevron-down" size={14} /></span>
              <span class="label">{section.title}</span>
              <span class="count">{list.length}</span>
            </button>
            {#if open}
              <ul>
                {#each list as row (row.id)}
                  {@render item(row, section.key, false)}
                  {#if row.kind === "task"}
                    {#each rows.filter((r) => r.taskId === row.id) as link (link.id)}
                      {@render item(link, section.key, true)}
                    {/each}
                  {/if}
                {:else}
                  <li class="empty">{section.empty}</li>
                {/each}
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
    background: var(--surface-1); border-right: 1px solid var(--border); position: relative;
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

  header { display: flex; align-items: center; gap: var(--sp-1); height: 40px; padding: 0 var(--sp-3) 0 var(--sp-4); flex: none; }
  .search { padding: 0 var(--sp-4) var(--sp-4); flex: none; }
  .search label {
    display: flex; align-items: center; gap: var(--sp-3); height: 28px; padding: 0 var(--sp-4); box-sizing: border-box;
    border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-raised); color: var(--text-faint);
  }
  .search label:focus-within { border-color: var(--you); outline: 2px solid color-mix(in srgb, var(--focus) 25%, transparent); }
  .search input { flex: 1; min-width: 0; height: 26px; border: 0; outline: none; padding: 0; background: transparent; }
  .search input:focus { outline: none; }
  .poll-error {
    margin: 0 var(--sp-4) var(--sp-4); display: flex; gap: var(--sp-4); padding: var(--sp-4) var(--sp-5);
    border-radius: var(--r-md); background: var(--warn-bg); font-size: var(--fs-12); line-height: 16px;
    overflow: hidden; max-height: 48px;
  }
  .poll-error :global(svg) { color: var(--warn); margin-top: 1px; }
  .poll-error strong { font-weight: var(--fw-semibold); }
  .muted { color: var(--text-muted); }

  .sections, .hits, .skeleton { flex: 1; min-height: 0; overflow-y: auto; padding-bottom: var(--sp-6); }
  section { display: flex; flex-direction: column; }
  .section-head {
    display: flex; align-items: center; gap: var(--sp-2); height: 28px; padding: 0 var(--sp-6) 0 var(--sp-4);
    border: 0; border-radius: 0; background: transparent; color: var(--text-muted); text-align: left;
    font-size: var(--fs-11); font-weight: var(--fw-semibold); letter-spacing: 0.04em; text-transform: uppercase;
  }
  .section-head:focus-visible { outline-offset: -2px; }
  .section-head .label { flex: 1; }
  .chev { display: flex; transition: transform var(--dur-fast) var(--ease); }
  .chev.folded { transform: rotate(-90deg); }
  .count { font-family: var(--font-mono); font-weight: var(--fw-medium); letter-spacing: 0; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); }
  li.closed { opacity: 0.6; }
  li.nested .row { padding-left: 26px; }
  .kind { flex: none; display: inline-flex; align-self: center; color: var(--text-faint); }
  .new-task {
    flex: none; display: flex; flex-direction: column; gap: var(--sp-3); margin: 0 var(--sp-4) var(--sp-4);
    padding: var(--sp-4); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-raised);
  }
  .new-task input, .new-task textarea { font: inherit; font-size: var(--fs-12); resize: vertical; }
  .form-actions { display: flex; align-items: center; justify-content: flex-end; gap: var(--sp-3); font-size: var(--fs-12); }
  .form-actions .muted { flex: 1; }
  .task-error { margin: 0; font-size: var(--fs-12); color: var(--bad); }

  .row {
    display: flex; gap: var(--sp-4); margin: 0 var(--sp-3); padding: var(--sp-3) var(--sp-4) 7px var(--sp-3);
    width: calc(100% - 12px); box-sizing: border-box; border: 0; border-radius: var(--r-md);
    background: transparent; color: var(--text); text-align: left; font-size: var(--fs-13);
  }
  .row:hover { background: var(--surface-hover); }
  .row.active { background: var(--surface-selected); box-shadow: inset 0 0 0 1px var(--you-line); }
  .row:focus-visible { outline-offset: -2px; }
  .dot { padding-top: 3px; }
  .lines { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .line1 { display: flex; align-items: baseline; gap: var(--sp-3); min-width: 0; }
  .num { flex: none; font-family: var(--font-mono); font-size: 11.5px; color: var(--text-muted); }
  .title { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-weight: var(--fw-medium); }
  .title.strong { font-weight: var(--fw-semibold); }
  .new { flex: none; align-self: center; fill: var(--gh); }
  .line2 {
    display: flex; align-items: center; gap: var(--sp-4); min-width: 0; overflow: hidden; white-space: nowrap;
    font-size: 11.5px; line-height: 16px; color: var(--text-muted);
  }
  .line2 > span { flex: none; }
  .line2 .spacer { flex: 1; }
  .line2 .attention { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; font-weight: var(--fw-medium); }
  .ci { display: inline-flex; }
  .ok { color: var(--ok); }
  .bad { color: var(--bad); }
  .warn { color: var(--warn); }
  .faint { color: var(--text-faint); }
  .threads { display: inline-flex; align-items: center; gap: 3px; }
  .attention.needs-you { color: var(--agent-strong); }
  .attention.blocked, .attention.ci-failing { color: var(--bad); }
  .attention.activity { color: var(--gh-strong); }
  .attention.review, .attention.all-closed { color: var(--you-strong); }
  .attention.working { color: var(--text-muted); }
  .hits .line2 { gap: var(--sp-3); }
  .empty { padding: var(--sp-2) var(--sp-6) var(--sp-5) var(--sp-9); font-size: var(--fs-12); line-height: 16px; color: var(--text-faint); }
  .empty.error { color: var(--bad); }

  .skeleton { padding: var(--sp-6) var(--sp-7); display: flex; flex-direction: column; gap: var(--sp-3); }
  .sk-row { display: flex; gap: var(--sp-4); align-items: center; }
  .sk-dot { width: 12px; height: 12px; border-radius: 6px; background: var(--border); }
  .sk-bar { display: block; height: 10px; border-radius: 3px; background: var(--border); }
  .sk-bar.small { height: 8px; }
  .sk-sub { padding-left: 20px; margin-bottom: var(--sp-4); }
</style>
