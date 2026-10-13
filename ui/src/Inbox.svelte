<script lang="ts">
  import type { Repo, Row } from "./lib/api";

  let {
    repo,
    rows,
    selected,
    error,
    onselect,
    onrefresh,
  }: {
    repo: Repo;
    rows: Row[];
    selected: number | null;
    error: string | null;
    onselect: (id: number) => void;
    onrefresh: () => void;
  } = $props();

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
</script>

<aside class="inbox">
  <header>
    <span class="repo" title={repo.path}>{repo.owner}/{repo.name}</span>
    <button onclick={onrefresh} title="Refresh from GitHub" aria-label="Refresh">↻</button>
  </header>
  {#if error}<p class="error" title={error}>GitHub: {error}</p>{/if}
  <ul>
    {#each rows as row (row.id)}
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
</aside>

<style>
  .inbox { width: 340px; flex-shrink: 0; border-right: 1px solid var(--border); display: flex; flex-direction: column; background: var(--sidebar); min-height: 0; }
  header { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--border); }
  .repo { flex: 1; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  header button { padding: 2px 8px; }
  ul { list-style: none; margin: 0; padding: 0; overflow-y: auto; flex: 1; }
  li.active { background: var(--panel); }
  li.closed { opacity: 0.6; }
  .row { display: flex; flex-direction: column; gap: 3px; width: 100%; border: 0; border-radius: 0; background: none; text-align: left; padding: 8px 12px; }
  .line1, .line2 { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .line2 { font-size: 0.8em; color: var(--muted); padding-left: 14px; flex-wrap: wrap; }
  .num { color: var(--muted); font-variant-numeric: tabular-nums; }
  .title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .new { color: var(--accent); font-size: 0.8em; }
  .agent { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; background: transparent; border: 1px solid var(--border); }
  .agent.working, .agent.starting { background: var(--accent); border-color: var(--accent); }
  .agent.waitingForPermission { background: var(--warn); border-color: var(--warn); }
  .agent.finished, .agent.ready { background: var(--ok); border-color: var(--ok); }
  .agent.error { background: var(--bad); border-color: var(--bad); }
  .ci.ok { color: var(--ok); }
  .ci.bad { color: var(--bad); }
  .ci.warn { color: var(--warn); }
  .tag { border: 1px solid var(--border); border-radius: 3px; padding: 0 4px; }
  .decision.CHANGES_REQUESTED { color: var(--bad); }
  .decision.APPROVED { color: var(--ok); }
  .attention { color: var(--text); font-weight: 600; margin-left: auto; }
  .error { color: var(--bad); margin: 8px 12px; font-size: 0.85em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { padding: 12px; color: var(--muted); }
</style>
