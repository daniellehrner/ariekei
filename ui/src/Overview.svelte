<script lang="ts">
  import type { Conversation } from "./lib/api";
  import { externalLinks, sanitize } from "./lib/html";

  let { conv }: { conv: Conversation } = $props();

  const FAILED = ["FAILURE", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"];
  const outcomeClass = (o: string) => (FAILED.includes(o) ? "bad" : o === "SUCCESS" ? "ok" : o === "SKIPPED" || o === "NEUTRAL" ? "muted" : "warn");
  const date = (iso: string) => (iso ? new Date(iso).toLocaleString() : "");
  const latestReviews = $derived.by(() => {
    // Latest review per reviewer that expresses a decision.
    const by = new Map<string, string>();
    for (const r of conv.reviews) if (r.state !== "COMMENTED" && r.state !== "PENDING") by.set(r.author, r.state);
    return [...by.entries()];
  });
</script>

<section class="overview" use:externalLinks>
  <h2>{conv.title} <span class="num">#{conv.number}</span></h2>
  <p class="meta">
    <span class="state {conv.state}">{conv.isDraft && conv.state === "OPEN" ? "draft" : conv.state.toLowerCase()}</span>
    {#if conv.headRef}
      <span><strong>{conv.author}</strong> wants to merge <code>{conv.headRef}</code> into <code>{conv.baseRef}</code></span>
    {:else}
      <span>opened by <strong>{conv.author}</strong></span>
    {/if}
    <span>opened {date(conv.createdAt)}</span>
    <a href={conv.url}>on GitHub</a>
  </p>
  {#if conv.labels.length}
    <p class="labels">{#each conv.labels as l}<span class="label">{l}</span>{/each}</p>
  {/if}

  <div class="columns">
    <div class="body markdown">
      {#if conv.bodyHtml.trim()}
        {@html sanitize(conv.bodyHtml)}
      {:else}
        <p class="muted">No description.</p>
      {/if}
    </div>
    <aside class="side">
      {#if conv.headRef}
      <h3>Reviews</h3>
      <p>{conv.reviewDecision ? conv.reviewDecision.toLowerCase().replaceAll("_", " ") : "no decision"}</p>
      <ul>
        {#each latestReviews as [who, state]}<li><span class="rv {state}">{state.toLowerCase().replaceAll("_", " ")}</span> {who}</li>{/each}
        {#each conv.requestedReviewers as who}<li><span class="rv">requested</span> {who}</li>{/each}
      </ul>
      <h3>Checks</h3>
      <ul class="checks">
        {#each conv.checks as c}
          <li><span class="dot {outcomeClass(c.outcome)}">●</span> {#if c.url}<a href={c.url}>{c.name}</a>{:else}{c.name}{/if} <span class="muted">{c.outcome.toLowerCase()}</span></li>
        {:else}
          <li class="muted">No checks.</li>
        {/each}
      </ul>
      {/if}
      <h3>Discussion</h3>
      <p>{conv.comments.length} comment{conv.comments.length === 1 ? "" : "s"}, {conv.threads.filter((t) => !t.resolved).length} unresolved of {conv.threads.length} review thread{conv.threads.length === 1 ? "" : "s"}</p>
    </aside>
  </div>
</section>

<style>
  .overview { padding: 14px 18px; overflow-y: auto; height: 100%; box-sizing: border-box; }
  h2 { margin: 0 0 6px; font-size: 18px; }
  .num { color: var(--muted); font-weight: 400; }
  .meta { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; color: var(--muted); margin: 0 0 8px; }
  .meta strong { color: var(--text); }
  .state { border-radius: 10px; padding: 1px 8px; border: 1px solid var(--border); color: var(--text); }
  .state.OPEN { border-color: var(--ok); }
  .state.MERGED { border-color: var(--accent); }
  .state.CLOSED { border-color: var(--bad); }
  .labels { display: flex; gap: 6px; flex-wrap: wrap; margin: 0 0 10px; }
  .label { border: 1px solid var(--border); border-radius: 10px; padding: 0 8px; font-size: 0.85em; }
  .columns { display: flex; gap: 24px; align-items: flex-start; }
  .body { flex: 1; min-width: 0; }
  .side { width: 260px; flex-shrink: 0; font-size: 0.9em; }
  .side h3 { font-size: 0.75em; text-transform: uppercase; letter-spacing: 0.05em; color: var(--muted); margin: 14px 0 4px; }
  .side ul { list-style: none; margin: 0; padding: 0; }
  .side li { margin: 2px 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rv { color: var(--muted); }
  .rv.APPROVED { color: var(--ok); }
  .rv.CHANGES_REQUESTED { color: var(--bad); }
  .dot.ok { color: var(--ok); }
  .dot.bad { color: var(--bad); }
  .dot.warn { color: var(--warn); }
  .dot.muted, .muted { color: var(--muted); }
  a { color: var(--accent); }
  .markdown :global(pre) { background: var(--panel); padding: 8px; overflow-x: auto; border-radius: 4px; }
  .markdown :global(code) { font-family: var(--mono); font-size: 0.9em; }
  .markdown :global(img) { max-width: 100%; }
  .markdown :global(a) { color: var(--accent); }
  .markdown :global(table) { border-collapse: collapse; }
  .markdown :global(td), .markdown :global(th) { border: 1px solid var(--border); padding: 3px 6px; }
  .markdown :global(blockquote) { border-left: 3px solid var(--border); margin: 0; padding-left: 10px; color: var(--muted); }
</style>
