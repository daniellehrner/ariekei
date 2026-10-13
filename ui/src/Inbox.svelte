<script lang="ts">
  import { api, type Repo, type Row, type SearchHit } from "./lib/api";

  let {
    repo,
    rows,
    selected,
    error,
    onselect,
    onrefresh,
    onadded,
  }: {
    repo: Repo;
    rows: Row[];
    selected: number | null;
    error: string | null;
    onselect: (id: number) => void;
    onrefresh: () => void;
    /// A PR was added to the Inbox (from search).
    onadded: () => void;
  } = $props();

  const SECTIONS: { role: Row["role"]; title: string }[] = [
    { role: "author", title: "Mine" },
    { role: "requested", title: "Review requested" },
    { role: "other", title: "Opened" },
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

<aside class="inbox">
  <header>
    <span class="repo" title={repo.path}>{repo.owner}/{repo.name}</span>
    <button onclick={onrefresh} title="Refresh from GitHub" aria-label="Refresh">↻</button>
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
      {#each SECTIONS as section (section.role)}
        {@const list = rows.filter((r) => r.role === section.role)}
        {#if list.length || section.role === "author"}
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
                    {#if section.role !== "author"}<span class="author">{row.author}</span>{/if}
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

<style>
  .inbox { width: 340px; flex-shrink: 0; border-right: 1px solid var(--border); display: flex; flex-direction: column; background: var(--sidebar); min-height: 0; }
  header { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--border); }
  .repo { flex: 1; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
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
