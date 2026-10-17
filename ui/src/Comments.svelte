<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import Pill from "./controls/Pill.svelte";
  import { api, type Conversation, type Thread } from "./lib/api";
  import { hunkTail } from "./lib/hunk";
  import { externalLinks, sanitize } from "./lib/html";

  let {
    id,
    conv,
    viewer,
    onchanged,
    onshow,
  }: { id: number; conv: Conversation; viewer: string; onchanged: () => void; onshow?: (path: string) => void } = $props();

  let replyTo: string | null = $state(null);
  let replyText = $state("");
  let newComment = $state("");
  let busy = $state(false);
  let error: string | null = $state(null);
  // Resolved threads the user unfolded.
  let opened: string[] = $state([]);

  const ago = (iso: string) => {
    if (!iso) return "";
    const days = Math.floor((Date.now() - new Date(iso).getTime()) / 86_400_000);
    if (days < 1) return new Date(iso).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    if (days === 1) return "yesterday";
    if (days < 7) return `${days} days ago`;
    return new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  };
  const REVIEW_STATE: Record<string, [string, "ok" | "bad" | "neutral" | null]> = {
    APPROVED: ["Approved", "ok"],
    CHANGES_REQUESTED: ["Changes requested", "bad"],
    COMMENTED: ["", null],
    DISMISSED: ["Dismissed", "neutral"],
  };
  const short = (path: string) => (path.split("/").length > 3 ? `…/${path.split("/").slice(-3).join("/")}` : path);
  const text = (html: string) => new DOMParser().parseFromString(html, "text/html").body.textContent?.trim() ?? "";

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
  const me = (who: string) => who === viewer;

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

{#snippet avatar(who: string)}
  <span class="avatar" class:mine={me(who)}>{who.slice(0, 1).toUpperCase()}</span>
{/snippet}

{#snippet comments(t: Thread)}
  {#each t.comments as c (c.id)}
    <div class="comment">
      {@render avatar(c.author)}
      <div class="said">
        <span class="who"><strong>{c.author}</strong> <span class="muted">· {ago(c.createdAt)}</span></span>
        <div class="markdown">{@html sanitize(c.bodyHtml)}</div>
      </div>
    </div>
  {/each}
{/snippet}

{#snippet thread(t: Thread)}
  {@const lines = hunkTail(t.diffHunk, 4)}
  <section class="thread" data-thread={t.id}>
    <header>
      <Icon name="comment" size={14} />
      <code title={t.path}>{short(t.path)}{t.line ? `:${t.line}` : ""}</code>
      {#if t.outdated}<Pill size="sm">outdated</Pill>{/if}
      {#if onshow}<Button variant="quiet" size="sm" class="show" onclick={() => onshow(t.path)} data-show-in-review>Show in Review</Button>{/if}
    </header>
    {#if lines.length}
      <pre class="hunk">{#each lines as l}<span class="l {l.kind}"><span class="n">{l.number ?? ""}</span>{l.text}</span>{/each}</pre>
    {/if}
    {@render comments(t)}
    {#if replyTo === t.id}
      <div class="compose reply">
        <textarea bind:value={replyText} rows="3" placeholder="Reply — Markdown supported"></textarea>
        <div class="buttons">
          <Button variant="quiet" onclick={() => (replyTo = null)}>Cancel</Button>
          <Button variant="gh" icon="submit-github" onclick={() => sendReply(t)} disabled={busy || !replyText.trim()} data-send-reply>Reply on GitHub</Button>
        </div>
      </div>
    {:else}
      <div class="thread-actions">
        <Button icon="reply" size="sm" onclick={() => { replyTo = t.id; replyText = ""; }} data-reply>Reply</Button>
        <Button variant="quiet" size="sm" icon="check" onclick={() => toggleResolved(t)} disabled={busy} data-resolve>Resolve</Button>
      </div>
    {/if}
  </section>
{/snippet}

<div class="comments" role="tabpanel" aria-label="Comments" use:externalLinks>
  <div class="column">
    {#if error}<p class="error">{error}</p>{/if}

    {#if conv.threads.length}
      <h2>Review threads · {unresolved.length} unresolved</h2>
      {#each unresolved as t (t.id)}{@render thread(t)}{/each}
      {#each resolved as t (t.id)}
        {@const open = opened.includes(t.id)}
        <div class="folded" class:open data-thread={t.id}>
          <button class="fold" onclick={() => (opened = open ? opened.filter((o) => o !== t.id) : [...opened, t.id])} aria-expanded={open}>
            <span class="chev" class:open><Icon name="chevron-right" size={14} /></span>
            <code>{short(t.path)}{t.line ? `:${t.line}` : ""}</code>
            {#if t.outdated}<Pill size="sm">outdated</Pill>{/if}
            <Pill size="sm" tone="ok">resolved</Pill>
            <span class="summary">{t.comments[0] ? `${t.comments[0].author}: ${text(t.comments[0].bodyHtml)}` : ""}</span>
          </button>
          {#if open}
            <div class="unfolded">
              {@render comments(t)}
              <Button variant="quiet" size="sm" onclick={() => toggleResolved(t)} disabled={busy} data-unresolve>Unresolve</Button>
            </div>
          {/if}
        </div>
      {/each}
    {:else if conv.headRef}
      <div class="none"><strong>No review threads yet</strong><span>Line comments from GitHub show up here, unresolved first.</span></div>
    {/if}

    <h2 class="conversation">Conversation</h2>
    {#if timeline.length}
      <ol class="timeline">
        {#each timeline as item}
          {@const look = item.kind === "review" ? REVIEW_STATE[item.state] : null}
          <li>
            {@render avatar(item.author)}
            <div class="said">
              <span class="who">
                <strong>{item.author}</strong>
                <span class="muted">{item.kind === "review" ? "reviewed" : "commented"} · {ago(item.at)}</span>
                {#if look?.[1]}<Pill size="sm" tone={look[1]}>{look[0]}</Pill>{/if}
              </span>
              {#if item.html.trim()}<div class="markdown">{@html sanitize(item.html)}</div>{/if}
            </div>
          </li>
        {/each}
      </ol>
    {:else}
      <p class="muted">No comments yet.</p>
    {/if}

    <label class="compose">
      <span class="label">New comment</span>
      <textarea bind:value={newComment} rows="3" placeholder="Comment on the {conv.headRef ? 'pull request' : 'issue'} — Markdown supported"></textarea>
      <span class="buttons">
        <Button icon="submit-github" onclick={sendComment} disabled={busy || !newComment.trim()} data-send-comment>Comment on GitHub</Button>
      </span>
    </label>
  </div>
</div>

<style>
  .comments { flex: 1; min-height: 0; height: 100%; box-sizing: border-box; overflow: auto; padding: var(--sp-8) var(--sp-9) 40px; }
  .column { max-width: 820px; display: flex; flex-direction: column; gap: 14px; }
  h2 {
    margin: 0; font-size: var(--fs-11); line-height: var(--lh-11); font-weight: var(--fw-semibold);
    letter-spacing: 0.04em; text-transform: uppercase; color: var(--text-muted);
  }
  h2.conversation { margin-top: var(--sp-6); }
  .thread { border: 1px solid var(--border); border-radius: var(--r-lg); overflow: hidden; background: var(--surface-raised); }
  .thread header {
    display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-3) var(--sp-5);
    background: var(--surface-2); border-bottom: 1px solid var(--border); font-size: var(--fs-12); color: var(--text-muted);
  }
  .thread header code { flex: 1; min-width: 0; font-size: 11.5px; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .thread header :global(.show) { color: var(--you); }
  .hunk {
    margin: 0; padding: var(--sp-3) 0; font-size: 11.5px; line-height: 18px; background: var(--surface-editor);
    border-bottom: 1px solid var(--border); overflow-x: auto;
  }
  .hunk .l { display: block; padding: 0 var(--sp-5); white-space: pre; }
  .hunk .add { background: var(--diff-add-line); }
  .hunk .del { background: var(--diff-del-line); }
  .hunk .n { display: inline-block; width: 4ch; margin-right: 1ch; text-align: right; color: var(--diff-gutter-text); }
  .comment { display: flex; gap: var(--sp-5); padding: var(--sp-5) var(--sp-6) 0; }
  .comment:last-of-type { padding-bottom: var(--sp-5); }
  .avatar {
    flex: none; width: 22px; height: 22px; border-radius: 11px; background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--border);
    display: inline-grid; place-items: center; font-size: 10px; font-weight: var(--fw-semibold); color: var(--text-muted);
  }
  .avatar.mine { background: var(--you-bg); box-shadow: none; color: var(--you-strong); }
  .said { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: var(--sp-1); }
  .who { display: flex; align-items: center; gap: var(--sp-3); font-size: var(--fs-12); }
  .who strong { font-weight: var(--fw-semibold); }
  .muted { color: var(--text-muted); }
  .thread-actions { display: flex; gap: var(--sp-3); padding: var(--sp-4) var(--sp-6); border-top: 1px solid var(--border); }
  .compose.reply { display: flex; flex-direction: column; gap: var(--sp-3); padding: var(--sp-4) var(--sp-6); border-top: 1px solid var(--border); }
  .folded { border: 1px solid var(--border); border-radius: var(--r-lg); }
  .fold {
    width: 100%; display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-4) var(--sp-5);
    border: 0; border-radius: var(--r-lg); background: transparent; color: var(--text-muted); font-size: var(--fs-12); text-align: left;
  }
  .fold code { font-size: 11.5px; }
  .fold .summary { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .chev { display: flex; transition: transform var(--dur-fast) var(--ease); }
  .chev.open { transform: rotate(90deg); }
  .unfolded { padding: 0 var(--sp-6) var(--sp-5) var(--sp-4); display: flex; flex-direction: column; gap: var(--sp-4); align-items: flex-start; }
  .unfolded .comment { padding: 0; }
  .none { padding: var(--sp-6) 0; display: flex; flex-direction: column; gap: var(--sp-2); color: var(--text-muted); }
  .none strong { color: var(--text); font-weight: var(--fw-semibold); }
  .timeline {
    list-style: none; margin: 0; padding: 0 0 0 11px; display: flex; flex-direction: column; gap: 14px;
    border-left: 1px solid var(--border);
  }
  .timeline li { display: flex; gap: var(--sp-5); margin-left: -11px; }
  .timeline .avatar { background: var(--surface-0); }
  .timeline .avatar.mine { background: var(--you-bg); }
  .compose { display: flex; flex-direction: column; gap: var(--sp-3); }
  label.compose { margin-top: var(--sp-4); }
  .compose .label { font-size: var(--fs-12); font-weight: var(--fw-semibold); }
  .compose textarea { width: 100%; font-size: var(--fs-13); line-height: var(--lh-13); padding: var(--sp-4) var(--sp-5); }
  .buttons { display: flex; gap: var(--sp-3); justify-content: flex-end; }
  .error { color: var(--bad); margin: 0; }
  .markdown { font-size: var(--fs-13); line-height: var(--lh-13); overflow-wrap: anywhere; }
  .markdown :global(p) { margin: 0 0 var(--sp-2); text-wrap: pretty; }
  .markdown :global(p:last-child) { margin-bottom: 0; }
  .markdown :global(pre) { background: var(--surface-2); padding: var(--sp-4); overflow-x: auto; border-radius: var(--r-md); font-size: 12px; }
  .markdown :global(code) { font-family: var(--font-mono); font-size: 12px; padding: 1px 4px; border-radius: var(--r-sm); background: var(--surface-2); }
  .markdown :global(pre code) { padding: 0; background: none; }
  .markdown :global(a) { color: var(--you); }
  .markdown :global(img) { max-width: 100%; }
</style>
