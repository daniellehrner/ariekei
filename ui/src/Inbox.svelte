<script lang="ts">
  import RepoMenu from "./RepoMenu.svelte";
  import { api, type Repo, type Row, type SearchHit } from "./lib/api";

  let {
    repo,
    rows,
    selected,
    error,
    collapsed,
    ontoggle,
    onselect,
    onrefresh,
    onadded,
    onswitch,
    onconnect,
  }: {
    repo: Repo;
    rows: Row[];
    selected: number | null;
    error: string | null;
    collapsed: boolean;
    ontoggle: () => void;
    onselect: (id: number) => void;
    onrefresh: () => void;
    /// A PR was added to the Inbox (from search).
    onadded: () => void;
    /// This window now shows another repo.
    onswitch: (repo: Repo) => void;
    /// Show the Connect screen (for another repo).
    onconnect: () => void;
  } = $props();

  const SECTIONS: { key: string; title: string; has: (r: Row) => boolean }[] = [
    { key: "mine", title: "Mine", has: (r) => r.kind === "pr" && r.role === "author" },
    { key: "requested", title: "Review requested", has: (r) => r.kind === "pr" && r.role === "requested" },
    { key: "issues", title: "Issues", has: (r) => r.kind === "issue" },
    { key: "opened", title: "Opened", has: (r) => r.kind === "pr" && r.role === "other" },
  ];
  const CI: Record<string, [string, string]> = {
    SUCCESS: ["✓", "ok"],
    FAILURE: ["✗", "bad"],
    ERROR: ["✗", "bad"],
    PENDING: ["●", "warn"],
    EXPECTED: ["●", "warn"],
  };
  const DECISION: Record<string, string> = {
    APPROVED: "approved",
    CHANGES_REQUESTED: "changes requested",
    REVIEW_REQUIRED: "review required",
  };
  const AGENT: Record<string, string> = {
    starting: "starting",
    ready: "ready",
    working: "working",
    waitingForPermission: "needs you",
    finished: "finished",
    error: "error",
    stopped: "stopped",
  };

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

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Escape") query = "";
    if (e.key === "Enter" && hits[0]) pick(hits[0]);
  }
</script>

{#if collapsed}
  <aside class="inbox collapsed">
    <button class="expand" onclick={ontoggle} title="Show the Inbox (Ctrl+B)" aria-label="Show the Inbox">»</button>
    <!-- Items that need attention stay visible as dots. -->
    {#each rows.filter((r) => r.attention) as row (row.id)}
      <button class="pip {row.agent ?? ''}" class:active={row.id === selected} class:bad={row.ci === "FAILURE" || row.ci === "ERROR"}
        onclick={() => onselect(row.id)} title="#{row.number} {row.title} — {row.attention}" aria-label="#{row.number}: {row.attention}"
        data-pip={row.number}>●</button>
    {/each}
  </aside>
{:else}
<aside class="inbox">
  <header>
    <RepoMenu {repo} {onswitch} {onconnect} />
    <button onclick={onrefresh} title="Refresh from GitHub" aria-label="Refresh">↻</button>
    <button onclick={ontoggle} title="Hide the Inbox (Ctrl+B)" aria-label="Hide the Inbox">«</button>
  </header>
  <div class="search">
    <input
      bind:value={query}
      onkeydown={onSearchKey}
      placeholder="Search open PRs (words, #123, author:name)"
      aria-label="Search open pull requests"
    />
  </div>
  {#if error}<p class="error" title={error}>GitHub: {error}</p>{/if}

  {#if query.trim()}
    <ul class="hits">
      {#if searchError}<li class="error">{searchError}</li>{/if}
      {#each hits as hit (hit.number)}
        <li>
          <button class="row" onclick={() => pick(hit)} data-hit={hit.number}>
            <span class="line1"><span class="num">#{hit.number}</span><span class="title">{hit.title}</span></span>
            <span class="line2">
              <span>{hit.author}</span>
              {#if hit.isDraft}<span class="tag">draft</span>{/if}
              {#if hit.itemId}<span class="tag">in inbox</span>{/if}
            </span>
          </button>
        </li>
      {:else}
        <li class="empty">{searching ? "Searching…" : "No matching open PRs."}</li>
      {/each}
    </ul>
  {:else}
    <div class="sections">
      {#each SECTIONS as section (section.key)}
        {@const list = rows.filter(section.has)}
        {#if list.length || section.key === "mine"}
          <h3>{section.title} <span class="count">{list.length}</span></h3>
          <ul>
            {#each list as row (row.id)}
              {@const ci = row.ci ? CI[row.ci] : null}
              <li class:active={row.id === selected} class:closed={!row.open}>
                <button class="row" onclick={() => onselect(row.id)} title={row.title} data-number={row.number}>
                  <span class="line1">
                    <span class="agent {row.agent ?? 'none'}" title={row.agent ? `agent: ${AGENT[row.agent]}` : "no agent running"}></span>
                    <span class="num">#{row.number}</span>
                    <span class="title">{row.title}</span>
                    {#if row.newActivity}<span class="new" title="new reviews or comments">●</span>{/if}
                  </span>
                  <span class="line2">
                    {#if section.key !== "mine"}<span class="author">{row.author}</span>{/if}
                    {#if row.issueNumber}<span class="tag">from #{row.issueNumber}</span>{/if}
                    {#if ci}<span class="ci {ci[1]}" title="CI: {row.ci}{row.failingChecks.length ? ` (${row.failingChecks.join(', ')})` : ''}">{ci[0]}</span>{/if}
                    {#if row.isDraft}<span class="tag">draft</span>{/if}
                    {#if !row.open}<span class="tag">closed</span>{/if}
                    {#if row.reviewDecision}<span class="decision {row.reviewDecision}">{DECISION[row.reviewDecision] ?? row.reviewDecision}</span>{/if}
                    {#if row.unresolvedThreads}<span class="threads" title="unresolved review threads">💬 {row.unresolvedThreads}</span>{/if}
                    {#if row.attention}<span class="attention">{row.attention}</span>{/if}
                  </span>
                </button>
              </li>
            {:else}
              <li class="empty">No open pull requests.</li>
            {/each}
          </ul>
        {/if}
      {/each}
    </div>
  {/if}
</aside>
{/if}

<style>
  .inbox.collapsed { width: 36px; align-items: center; padding-top: 8px; gap: 6px; }
  .expand { padding: 2px 8px; margin-bottom: 6px; }
  .pip { border: 0; background: none; padding: 2px; font-size: 12px; color: var(--accent); }
  .pip.waitingForPermission { color: var(--warn); }
  .pip.bad { color: var(--bad); }
  .pip.active { outline: 1px solid var(--border); border-radius: 4px; }
  .inbox { width: 340px; flex-shrink: 0; border-right: 1px solid var(--border); display: flex; flex-direction: column; background: var(--sidebar); min-height: 0; }
  header { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--border); }
  header button { padding: 2px 8px; }
  .search { padding: 8px 12px; border-bottom: 1px solid var(--border); }
  .search input { width: 100%; box-sizing: border-box; }
  .sections, .hits { overflow-y: auto; flex: 1; }
  h3 { font-size: 0.75em; text-transform: uppercase; letter-spacing: 0.05em; color: var(--muted); margin: 12px 12px 4px; }
  .count { font-weight: 400; }
  ul { list-style: none; margin: 0; padding: 0; }
  li.active { background: var(--panel); }
  li.closed { opacity: 0.6; }
  .row { display: flex; flex-direction: column; gap: 3px; width: 100%; border: 0; border-radius: 0; background: none; text-align: left; padding: 8px 12px; }
  .line1, .line2 { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .line2 { font-size: 0.8em; color: var(--muted); padding-left: 14px; flex-wrap: wrap; }
  .hits .line2 { padding-left: 0; }
  .num { color: var(--muted); font-variant-numeric: tabular-nums; }
  .title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .new { color: var(--accent); font-size: 0.8em; }
  .agent { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; background: transparent; border: 1px solid var(--border); }
  .agent.working, .agent.starting { background: var(--accent); border-color: var(--accent); }
  .agent.waitingForPermission { background: var(--warn); border-color: var(--warn); }
  .agent.finished, .agent.ready { background: var(--ok); border-color: var(--ok); }
  .agent.error { background: var(--bad); border-color: var(--bad); }
  .author { color: var(--text); }
  .ci.ok { color: var(--ok); }
  .ci.bad { color: var(--bad); }
  .ci.warn { color: var(--warn); }
  .tag { border: 1px solid var(--border); border-radius: 3px; padding: 0 4px; }
  .decision.CHANGES_REQUESTED { color: var(--bad); }
  .decision.APPROVED { color: var(--ok); }
  .attention { color: var(--text); font-weight: 600; margin-left: auto; }
  .error { color: var(--bad); margin: 8px 12px; font-size: 0.85em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { padding: 8px 12px; color: var(--muted); font-size: 0.9em; }
</style>
