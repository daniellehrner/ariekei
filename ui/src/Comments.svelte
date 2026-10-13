<script lang="ts">
  import { api, type Conversation, type Thread } from "./lib/api";
  import { externalLinks, sanitize } from "./lib/html";

  let { id, conv, onchanged }: { id: number; conv: Conversation; onchanged: () => void } = $props();

  let replyTo: string | null = $state(null);
  let replyText = $state("");
  let newComment = $state("");
  let busy = $state(false);
  let error: string | null = $state(null);
  let showResolved = $state(false);

  const date = (iso: string) => (iso ? new Date(iso).toLocaleString() : "");
  const REVIEW_STATE: Record<string, string> = {
    APPROVED: "approved",
    CHANGES_REQUESTED: "requested changes",
    COMMENTED: "reviewed",
    DISMISSED: "review dismissed",
  };

  // Conversation comments and reviews (with text or a decision), oldest first.
  const timeline = $derived.by(() => {
    const items = [
      ...conv.comments.map((c) => ({ kind: "comment" as const, at: c.createdAt, author: c.author, html: c.bodyHtml, state: "" })),
      ...conv.reviews
        .filter((r) => r.bodyHtml.trim() || r.state === "APPROVED" || r.state === "CHANGES_REQUESTED")
        .map((r) => ({ kind: "review" as const, at: r.submittedAt, author: r.author, html: r.bodyHtml, state: r.state })),
    ];
    return items.sort((a, b) => a.at.localeCompare(b.at));
  });

  const unresolved = $derived(conv.threads.filter((t) => !t.resolved));
  const resolved = $derived(conv.threads.filter((t) => t.resolved));

  async function act(f: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await f();
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const sendReply = (t: Thread) =>
    act(async () => {
      await api.replyComment(id, t.comments[0].id, replyText);
      replyText = "";
      replyTo = null;
    });
  const toggleResolved = (t: Thread) => act(() => api.resolveThread(t.id, !t.resolved));
  const sendComment = () =>
    act(async () => {
      await api.postComment(id, newComment);
      newComment = "";
    });
</script>

{#snippet thread(t: Thread)}
  <article class="thread" class:resolved={t.resolved} data-thread={t.id}>
    <header>
      <code>{t.path}{t.line ? `:${t.line}` : ""}</code>
      {#if t.outdated}<span class="tag">outdated</span>{/if}
      {#if t.resolved}<span class="tag">resolved</span>{/if}
      <span class="spacer"></span>
      <button onclick={() => toggleResolved(t)} disabled={busy}>{t.resolved ? "Unresolve" : "Resolve"}</button>
    </header>
    {#if t.diffHunk}<pre class="hunk">{t.diffHunk.split("\n").slice(-6).join("\n")}</pre>{/if}
    {#each t.comments as c (c.id)}
      <div class="comment">
        <div class="who"><strong>{c.author}</strong> <span class="muted">{date(c.createdAt)}</span></div>
        <div class="markdown">{@html sanitize(c.bodyHtml)}</div>
      </div>
    {/each}
    {#if replyTo === t.id}
      <div class="compose">
        <textarea bind:value={replyText} rows="3" placeholder="Reply (markdown)"></textarea>
        <div class="actions">
          <button onclick={() => (replyTo = null)}>Cancel</button>
          <button class="primary" onclick={() => sendReply(t)} disabled={busy || !replyText.trim()}>Reply on GitHub</button>
        </div>
      </div>
    {:else}
      <button class="link" onclick={() => { replyTo = t.id; replyText = ""; }}>Reply</button>
    {/if}
  </article>
{/snippet}

<section class="comments" use:externalLinks>
  {#if error}<p class="error">{error}</p>{/if}

  <h3>Review threads <span class="muted">{unresolved.length} unresolved, {resolved.length} resolved</span></h3>
  {#each unresolved as t (t.id)}{@render thread(t)}{:else}<p class="muted">No unresolved review threads.</p>{/each}
  {#if resolved.length}
    <button class="link" onclick={() => (showResolved = !showResolved)}>{showResolved ? "Hide" : "Show"} {resolved.length} resolved</button>
    {#if showResolved}{#each resolved as t (t.id)}{@render thread(t)}{/each}{/if}
  {/if}

  <h3>Conversation</h3>
  {#each timeline as item}
    <div class="comment entry">
      <div class="who">
        <strong>{item.author}</strong>
        {#if item.kind === "review"}<span class="rv {item.state}">{REVIEW_STATE[item.state] ?? item.state.toLowerCase()}</span>{/if}
        <span class="muted">{date(item.at)}</span>
      </div>
      {#if item.html.trim()}<div class="markdown">{@html sanitize(item.html)}</div>{/if}
    </div>
  {:else}
    <p class="muted">No comments yet.</p>
  {/each}

  <div class="compose">
    <textarea bind:value={newComment} rows="4" placeholder="Leave a comment on the pull request (markdown)"></textarea>
    <div class="actions">
      <button class="primary" onclick={sendComment} disabled={busy || !newComment.trim()}>Comment on GitHub</button>
    </div>
  </div>
</section>

<style>
  .comments { padding: 12px 18px; overflow-y: auto; height: 100%; box-sizing: border-box; }
  h3 { font-size: 0.8em; text-transform: uppercase; letter-spacing: 0.05em; margin: 16px 0 8px; }
  h3 .muted { text-transform: none; letter-spacing: 0; font-weight: 400; }
  .thread { border: 1px solid var(--border); border-radius: 6px; margin: 0 0 10px; padding: 8px 10px; }
  .thread.resolved { opacity: 0.75; }
  .thread header { display: flex; gap: 8px; align-items: center; }
  .spacer { flex: 1; }
  .hunk { font-family: var(--mono); font-size: 0.8em; background: var(--panel); padding: 6px 8px; margin: 6px 0; overflow-x: auto; border-radius: 4px; }
  .comment { padding: 6px 0; border-top: 1px solid var(--border); }
  .comment:first-of-type { border-top: 0; }
  .entry { border-top: 1px solid var(--border); }
  .who { font-size: 0.9em; display: flex; gap: 8px; align-items: baseline; }
  .rv { font-size: 0.9em; }
  .rv.APPROVED { color: var(--ok); }
  .rv.CHANGES_REQUESTED { color: var(--bad); }
  .tag { border: 1px solid var(--border); border-radius: 3px; padding: 0 4px; font-size: 0.8em; color: var(--muted); }
  .muted { color: var(--muted); }
  .compose { margin-top: 8px; }
  .compose textarea { width: 100%; box-sizing: border-box; font: inherit; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 4px; }
  .primary { border-color: var(--accent); }
  .link { border: 0; background: none; color: var(--accent); padding: 2px 0; }
  .error { color: var(--bad); }
  .markdown :global(pre) { background: var(--panel); padding: 8px; overflow-x: auto; border-radius: 4px; }
  .markdown :global(code) { font-family: var(--mono); font-size: 0.9em; }
  .markdown :global(a) { color: var(--accent); }
  .markdown :global(img) { max-width: 100%; }
  .markdown :global(p) { margin: 4px 0; }
</style>
