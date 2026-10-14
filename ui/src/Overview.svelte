<script lang="ts">
  import Icon from "./controls/Icon.svelte";
  import Pill from "./controls/Pill.svelte";
  import type { Conversation } from "./lib/api";
  import { externalLinks, sanitize } from "./lib/html";

  let { conv }: { conv: Conversation } = $props();

  const FAILED = ["FAILURE", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"];
  const outcome = (o: string) => (FAILED.includes(o) ? "bad" : o === "SUCCESS" ? "ok" : o === "SKIPPED" || o === "NEUTRAL" ? "muted" : "warn");
  const OUTCOME_TEXT: Record<string, string> = { bad: "failed", ok: "passed", muted: "skipped", warn: "running" };
  const date = (iso: string) => (iso ? new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" }) : "");
  const REVIEW: Record<string, ["ok" | "warn" | "neutral", string]> = {
    APPROVED: ["ok", "Approved"],
    CHANGES_REQUESTED: ["warn", "Changes requested"],
    DISMISSED: ["neutral", "Dismissed"],
  };
  // Latest review per reviewer that expresses a decision, then the reviewers still asked.
  const reviewers = $derived.by(() => {
    const by = new Map<string, string>();
    for (const r of conv.reviews) if (r.state !== "COMMENTED" && r.state !== "PENDING") by.set(r.author, r.state);
    const decided = [...by.entries()].map(([who, state]) => ({ who, state }));
    return [...decided, ...conv.requestedReviewers.filter((w) => !by.has(w)).map((who) => ({ who, state: "REQUESTED" }))];
  });
  const state = $derived.by((): ["ok" | "gh" | "bad" | "neutral", string] => {
    if (conv.state === "MERGED") return ["gh", "Merged"];
    if (conv.state === "CLOSED") return ["bad", "Closed"];
    return conv.isDraft ? ["neutral", "Draft"] : ["ok", "Open"];
  });
  const unresolved = $derived(conv.threads.filter((t) => !t.resolved).length);
</script>

<div class="overview" role="tabpanel" aria-label="Overview" use:externalLinks>
  <article>
    <div class="meta">
      <Pill tone={state[0]} data-state={conv.state}>{state[1]}</Pill>
      {#if conv.headRef}
        <span><strong>{conv.author}</strong> wants to merge <code>{conv.headRef}</code> into <code>{conv.baseRef}</code></span>
      {:else}
        <span><strong>{conv.author}</strong> opened this issue</span>
      {/if}
      <span>· opened {date(conv.createdAt)}</span>
      <a class="gh-link" href={conv.url}>GitHub<Icon name="external" size={12} /></a>
    </div>
    {#if conv.labels.length}
      <div class="labels">{#each conv.labels as l}<span class="label">{l}</span>{/each}</div>
    {/if}
    <hr />
    <h2 class="sr-only">{conv.title} #{conv.number}</h2>
    <div class="body markdown">
      {#if conv.bodyHtml.trim()}
        {@html sanitize(conv.bodyHtml)}
      {:else}
        <p class="muted">No description.</p>
      {/if}
    </div>
  </article>
  <aside>
    {#if conv.headRef}
      <section>
        <h3>Reviews</h3>
        {#each reviewers as r}
          {@const look = REVIEW[r.state] ?? ["neutral", r.state === "REQUESTED" ? "Requested" : r.state.toLowerCase()]}
          <div class="reviewer">
            <span class="avatar">{r.who.slice(0, 1).toUpperCase()}</span>
            <span class="who">{r.who}</span>
            <Pill tone={look[0]}>{look[1]}</Pill>
          </div>
        {:else}
          <p class="muted">{conv.reviewDecision ? conv.reviewDecision.toLowerCase().replaceAll("_", " ") : "No reviews yet."}</p>
        {/each}
      </section>
      <section>
        <h3>Checks</h3>
        {#each conv.checks as c}
          {@const o = outcome(c.outcome)}
          <svelte:element this={c.url ? "a" : "div"} class="check" href={c.url || undefined}>
            <span class="mark {o}"><Icon name={o === "ok" ? "check" : o === "bad" ? "x" : "pending"} size={14} /></span>
            <span class="name">{c.name}</span>
            <span class="muted">{OUTCOME_TEXT[o]}</span>
          </svelte:element>
        {:else}
          <p class="muted">No checks.</p>
        {/each}
      </section>
    {/if}
    <section>
      <h3>Discussion</h3>
      <p class="muted">
        {conv.comments.length} comment{conv.comments.length === 1 ? "" : "s"}{#if conv.headRef}, {unresolved} unresolved of {conv.threads.length} review thread{conv.threads.length === 1 ? "" : "s"}{/if}.
      </p>
    </section>
  </aside>
</div>

<style>
  .overview {
    flex: 1; min-height: 0; height: 100%; box-sizing: border-box; overflow: auto;
    display: flex; flex-wrap: wrap; align-items: flex-start; gap: var(--sp-10); padding: var(--sp-8) var(--sp-9) 40px;
  }
  article { flex: 999 1 520px; min-width: 0; max-width: 760px; display: flex; flex-direction: column; gap: 14px; }
  .meta { display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-4); font-size: var(--fs-12); color: var(--text-muted); }
  .meta strong { font-weight: var(--fw-semibold); color: var(--text); }
  .meta code { font-size: 11.5px; padding: 1px 5px; border-radius: var(--r-sm); background: var(--surface-2); color: var(--text); }
  .gh-link { display: inline-flex; align-items: center; gap: var(--sp-2); color: var(--text-muted); }
  .labels { display: flex; flex-wrap: wrap; gap: var(--sp-3); }
  .label {
    height: 20px; padding: 0 var(--sp-4); border-radius: 10px; background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--border);
    font-size: 11.5px; line-height: 20px; color: var(--text-muted);
  }
  hr { width: 100%; height: 1px; margin: 0; border: 0; background: var(--border); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  aside { flex: 1 1 260px; max-width: 320px; display: flex; flex-direction: column; gap: var(--sp-8); font-size: var(--fs-12); line-height: var(--lh-12); }
  section { display: flex; flex-direction: column; gap: var(--sp-3); }
  h3 {
    margin: 0 0 var(--sp-1); font-size: var(--fs-11); font-weight: var(--fw-semibold); letter-spacing: 0.04em;
    text-transform: uppercase; color: var(--text-muted);
  }
  .reviewer { display: flex; align-items: center; gap: var(--sp-4); }
  .avatar {
    width: 20px; height: 20px; border-radius: 10px; background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--border);
    display: inline-grid; place-items: center; font-size: 10px; font-weight: var(--fw-semibold); color: var(--text-muted);
  }
  .who { flex: 1; font-weight: var(--fw-medium); }
  .check { display: flex; align-items: center; gap: var(--sp-4); min-height: 24px; color: var(--text); text-decoration: none; }
  a.check:hover .name { text-decoration: underline; }
  .check .name { flex: 1; min-width: 0; font-family: var(--font-mono); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mark { display: inline-flex; }
  .mark.ok { color: var(--ok); }
  .mark.bad { color: var(--bad); }
  .mark.warn { color: var(--warn); }
  .mark.muted, .muted { color: var(--text-muted); }
  p { margin: 0; }
  .markdown { font-size: 13.5px; line-height: 22px; overflow-wrap: anywhere; }
  .markdown :global(h1), .markdown :global(h2), .markdown :global(h3) { font-size: var(--fs-15); line-height: var(--lh-15); font-weight: var(--fw-semibold); margin: 6px 0 10px; }
  .markdown :global(p) { margin: 0 0 10px; text-wrap: pretty; }
  .markdown :global(ul), .markdown :global(ol) { margin: 0 0 10px; padding-left: 20px; }
  .markdown :global(pre) { background: var(--surface-2); padding: var(--sp-4) var(--sp-5); overflow-x: auto; border-radius: var(--r-md); font-size: 12px; line-height: 18px; }
  .markdown :global(code) { font-family: var(--font-mono); font-size: 12px; padding: 1px 5px; border-radius: var(--r-sm); background: var(--surface-2); }
  .markdown :global(pre code) { padding: 0; background: none; }
  .markdown :global(img) { max-width: 100%; }
  .markdown :global(a) { color: var(--you); }
  .markdown :global(table) { border-collapse: collapse; margin-bottom: 10px; }
  .markdown :global(td), .markdown :global(th) { border: 1px solid var(--border); padding: 3px 6px; }
  .markdown :global(blockquote) { border-left: 3px solid var(--border); margin: 0 0 10px; padding-left: 10px; color: var(--text-muted); }
</style>
